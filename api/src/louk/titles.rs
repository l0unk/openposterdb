//! Title index: which titles have been requested, under which ids, and what
//! they are called.
//!
//! The image request path only calls [`touch`] and [`note_resolved`] (plus
//! [`note_cross_ids`] after an episode uplift). Those are synchronous map
//! inserts: no await, no DB, no network, no panics, and they never change a
//! response. Everything else runs in the background workers started by
//! [`spawn_workers`]:
//! - flush (every 30 s): pending maps → `louk_title_aliases` / `louk_titles`;
//! - details: name/year/poster for new titles from TMDB (one call at a time,
//!   at least 250 ms apart);
//! - backfill (every 10 min): up to 50 cached `image_meta` keys without an
//!   alias are resolved through `id::resolve` (TMDB only).
//!
//! No worker ever calls OMDb, MDBList or Trakt.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, QueryResult, Statement,
    TransactionTrait, Value,
};
use serde::Deserialize;

use crate::AppState;
use crate::cache::ImageType;
use crate::error::AppError;
use crate::id::{self, IdType, MediaType, ResolvedId};
use crate::louk::state::{AliasRow, MAX_PENDING_RESOLVED, MAX_PENDING_TOUCHES};

/// Minimum spacing between two fork TMDB calls.
pub const TMDB_GAP: Duration = Duration::from_millis(250);
const FLUSH_EVERY: Duration = Duration::from_secs(30);
const BACKFILL_FIRST_DELAY: Duration = Duration::from_secs(300);
const BACKFILL_EVERY: Duration = Duration::from_secs(600);
pub const BACKFILL_BATCH: usize = 50;
const BACKFILL_PAGE: i64 = 500;
const BACKFILL_MAX_PAGES: usize = 40;
const BACKFILL_FAILED_CAP: usize = 10_000;
const DETAILS_BATCH: i64 = 20;
const DETAILS_IDLE: Duration = Duration::from_secs(300);
/// A failed details fetch is retried after a day.
const DETAILS_RETRY_SECS: i64 = 86_400;
/// Season episode names are refreshed after a week.
pub const EPISODES_TTL_SECS: i64 = 7 * 86_400;
/// Longer id values are ignored by the hooks (real ids are far shorter).
const MAX_ID_VALUE_LEN: usize = 200;
const FLUSH_CHUNK: usize = 500;

pub fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

pub(crate) fn stmt(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

// ---------------------------------------------------------------------------
// Request-path hooks (sync, O(1), never fail)
// ---------------------------------------------------------------------------

/// Record that `{id_type}/{id_value}` was requested now. Only updates
/// `last_requested` of an alias that is already known (flush time).
pub fn touch(state: &AppState, id_type: &str, id_value: &str) {
    if id_value.is_empty() || id_value.len() > MAX_ID_VALUE_LEN {
        return;
    }
    let louk = &state.louk;
    let key = (id_type.to_owned(), id_value.to_owned());
    if louk.touches.len() >= MAX_PENDING_TOUCHES && !louk.touches.contains_key(&key) {
        louk.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    louk.touches.insert(key, now_secs());
}

/// Record what `{id_type}/{id_value}` resolved to (call with the resolution
/// BEFORE any episode uplift), plus the cross-id aliases upstream writes
/// copies under (`spawn_cross_id_cache`).
pub fn note_resolved(state: &AppState, id_type: &str, id_value: &str, r: &ResolvedId) {
    if id_value.is_empty() || id_value.len() > MAX_ID_VALUE_LEN {
        return;
    }
    let row = alias_row(r, now_secs());
    for (key, value) in cross_aliases(id_type, r, &row) {
        push_resolved(state, key, value);
    }
    push_resolved(state, (id_type.to_owned(), id_value.to_owned()), row);
}

/// After an episode was uplifted to its show: record the show's cross-id
/// aliases (upstream writes the rendered copies under those keys).
pub fn note_cross_ids(state: &AppState, id_type: &str, r: &ResolvedId) {
    let row = alias_row(r, now_secs());
    for (key, value) in cross_aliases(id_type, r, &row) {
        push_resolved(state, key, value);
    }
}

fn push_resolved(state: &AppState, key: (String, String), row: AliasRow) {
    let louk = &state.louk;
    if louk.resolved.len() >= MAX_PENDING_RESOLVED && !louk.resolved.contains_key(&key) {
        louk.dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    louk.resolved.insert(key, row);
}

/// The alias row for a resolution. Episodes belong to their show (`tv`) and
/// carry season/episode; their imdb/tvdb ids and still are the episode's,
/// so they are not used as the show's.
pub fn alias_row(r: &ResolvedId, seen_at: i64) -> AliasRow {
    match r.media_type {
        MediaType::Movie | MediaType::Tv => AliasRow {
            media_type: if r.media_type == MediaType::Movie {
                "movie"
            } else {
                "tv"
            },
            tmdb_id: r.tmdb_id as i64,
            season: None,
            episode: None,
            imdb_id: r.imdb_id.clone().filter(|s| !s.is_empty()),
            tvdb_id: r.tvdb_id.map(|v| v as i64),
            poster_path: r.poster_path.clone().filter(|s| !s.is_empty()),
            year: year_of(r.release_date.as_deref()),
            seen_at,
        },
        MediaType::Episode => {
            let (show, season, episode) = match &r.episode {
                Some(ep) => (
                    ep.show_tmdb_id,
                    Some(ep.season_number as i64),
                    Some(ep.episode_number as i64),
                ),
                None => (r.tmdb_id, None, None),
            };
            AliasRow {
                media_type: "tv",
                tmdb_id: show as i64,
                season,
                episode,
                imdb_id: None,
                tvdb_id: None,
                poster_path: None,
                year: None,
                seen_at,
            }
        }
    }
}

/// The alternate `(id_type, id_value)` keys upstream's `spawn_cross_id_cache`
/// writes for a resolution requested as `id_type` (same formats, same skip of
/// the requested id type). Ids that only the ratings response adds are not
/// visible here; the backfill picks those up.
pub fn cross_aliases(
    id_type: &str,
    r: &ResolvedId,
    row: &AliasRow,
) -> Vec<((String, String), AliasRow)> {
    let mut out = Vec::with_capacity(3);
    if id_type != "imdb"
        && let Some(imdb) = r.imdb_id.as_ref().filter(|s| !s.is_empty())
    {
        out.push((("imdb".to_owned(), imdb.clone()), row.clone()));
    }
    if id_type != "tmdb" {
        let value = id::format_tmdb_id_value(r.tmdb_id, &r.media_type, r.episode.as_ref());
        out.push((("tmdb".to_owned(), value), row.clone()));
    }
    if id_type != "tvdb"
        && let Some(tvdb) = r.tvdb_id
    {
        out.push((("tvdb".to_owned(), tvdb.to_string()), row.clone()));
    }
    out
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<i64>().ok())
}

/// `(id_type, id_value)` of an `image_meta` cache key: id_type is the part
/// before the first `/`, id_value runs up to the first `_` or `@` (the same
/// delimiters `purge_title` anchors on).
pub fn parse_cache_key_prefix(key: &str) -> Option<(String, String)> {
    let (id_type, rest) = key.split_once('/')?;
    if !matches!(id_type, "imdb" | "tmdb" | "tvdb") {
        return None;
    }
    let end = rest.find(['_', '@']).unwrap_or(rest.len());
    let value = rest.get(..end)?;
    if value.is_empty() || value.len() > MAX_ID_VALUE_LEN || value.contains([':', '/']) {
        return None;
    }
    Some((id_type.to_owned(), value.to_owned()))
}

// ---------------------------------------------------------------------------
// Flush
// ---------------------------------------------------------------------------

fn drain<V>(map: &DashMap<(String, String), V>) -> Vec<((String, String), V)> {
    let keys: Vec<(String, String)> = map.iter().map(|e| e.key().clone()).collect();
    keys.into_iter().filter_map(|k| map.remove(&k)).collect()
}

/// Write everything the hooks collected since the last flush.
pub async fn flush_once(state: &AppState) -> Result<(), AppError> {
    let resolved = drain(&state.louk.resolved);
    let touches = drain(&state.louk.touches);
    if resolved.is_empty() && touches.is_empty() {
        return Ok(());
    }
    for chunk in resolved.chunks(FLUSH_CHUNK) {
        let txn = state.db.begin().await?;
        for ((id_type, id_value), row) in chunk {
            upsert_alias(&txn, id_type, id_value, row).await?;
        }
        txn.commit().await?;
    }
    for chunk in touches.chunks(FLUSH_CHUNK) {
        let txn = state.db.begin().await?;
        for ((id_type, id_value), at) in chunk {
            apply_touch(&txn, id_type, id_value, *at).await?;
        }
        txn.commit().await?;
    }
    if !resolved.is_empty() {
        state.louk.title_queue.notify_one();
    }
    Ok(())
}

/// Upsert the title row and the alias row for one resolution.
pub async fn upsert_alias(
    db: &impl ConnectionTrait,
    id_type: &str,
    id_value: &str,
    row: &AliasRow,
) -> Result<(), DbErr> {
    db.execute(stmt(
        "INSERT INTO louk_titles
            (media_type, tmdb_id, imdb_id, tvdb_id, poster_path, year, first_seen, last_requested)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(media_type, tmdb_id) DO UPDATE SET
            imdb_id = COALESCE(excluded.imdb_id, louk_titles.imdb_id),
            tvdb_id = COALESCE(excluded.tvdb_id, louk_titles.tvdb_id),
            poster_path = COALESCE(louk_titles.poster_path, excluded.poster_path),
            year = COALESCE(louk_titles.year, excluded.year),
            first_seen = MIN(louk_titles.first_seen, excluded.first_seen),
            last_requested = MAX(louk_titles.last_requested, excluded.last_requested)",
        vec![
            row.media_type.into(),
            row.tmdb_id.into(),
            row.imdb_id.clone().into(),
            row.tvdb_id.into(),
            row.poster_path.clone().into(),
            row.year.into(),
            row.seen_at.into(),
            row.seen_at.into(),
        ],
    ))
    .await?;
    db.execute(stmt(
        "INSERT INTO louk_title_aliases
            (id_type, id_value, media_type, tmdb_id, season, episode, last_requested)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id_type, id_value) DO UPDATE SET
            media_type = excluded.media_type,
            tmdb_id = excluded.tmdb_id,
            season = excluded.season,
            episode = excluded.episode,
            last_requested = MAX(louk_title_aliases.last_requested, excluded.last_requested)",
        vec![
            id_type.into(),
            id_value.into(),
            row.media_type.into(),
            row.tmdb_id.into(),
            row.season.into(),
            row.episode.into(),
            row.seen_at.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn apply_touch(
    db: &impl ConnectionTrait,
    id_type: &str,
    id_value: &str,
    at: i64,
) -> Result<(), DbErr> {
    let Some(alias) = db
        .query_one(stmt(
            "SELECT media_type, tmdb_id FROM louk_title_aliases WHERE id_type = ? AND id_value = ?",
            vec![id_type.into(), id_value.into()],
        ))
        .await?
    else {
        // Not indexed yet: the slow path (note_resolved) or the backfill adds it.
        return Ok(());
    };
    let media_type: String = alias.try_get("", "media_type")?;
    let tmdb_id: i64 = alias.try_get("", "tmdb_id")?;
    db.execute(stmt(
        "UPDATE louk_title_aliases SET last_requested = MAX(last_requested, ?)
         WHERE id_type = ? AND id_value = ?",
        vec![at.into(), id_type.into(), id_value.into()],
    ))
    .await?;
    db.execute(stmt(
        "UPDATE louk_titles SET last_requested = MAX(last_requested, ?)
         WHERE media_type = ? AND tmdb_id = ?",
        vec![at.into(), media_type.into(), tmdb_id.into()],
    ))
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries shared with the handlers (and P3)
// ---------------------------------------------------------------------------

/// Every `(id_type, id_value)` recorded for a title (P3 purges each one).
pub async fn aliases_for_title(
    db: &DatabaseConnection,
    media_type: &str,
    tmdb_id: u64,
) -> Result<Vec<(String, String)>, AppError> {
    Ok(alias_rows(db, media_type, tmdb_id as i64)
        .await?
        .into_iter()
        .map(|a| (a.id_type, a.id_value))
        .collect())
}

#[derive(Clone, Debug)]
pub struct AliasInfo {
    pub id_type: String,
    pub id_value: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub last_requested: i64,
}

pub async fn alias_rows(
    db: &impl ConnectionTrait,
    media_type: &str,
    tmdb_id: i64,
) -> Result<Vec<AliasInfo>, AppError> {
    let rows = db
        .query_all(stmt(
            "SELECT id_type, id_value, season, episode, last_requested FROM louk_title_aliases
             WHERE media_type = ? AND tmdb_id = ?
             ORDER BY season IS NOT NULL, last_requested DESC, id_type, id_value",
            vec![media_type.into(), tmdb_id.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(AliasInfo {
            id_type: r.try_get("", "id_type")?,
            id_value: r.try_get("", "id_value")?,
            season: r.try_get("", "season")?,
            episode: r.try_get("", "episode")?,
            last_requested: r.try_get("", "last_requested")?,
        });
    }
    Ok(out)
}

/// One `image_meta` row of a title, with the alias it was found under.
#[derive(Clone, Debug)]
pub struct TitleImageRow {
    pub cache_key: String,
    pub image_type: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub alias_type: String,
    pub alias_value: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

/// Join of a title's aliases with `image_meta`: a cache key belongs to alias
/// `{t}/{v}` when it equals it or continues with `@` or `_` (the delimiter
/// rule of upstream's `purge_title` / `cache::title_file_match`). SQLite runs
/// each OR arm as a range search on the `image_meta` primary key.
const TITLE_IMAGES_JOIN: &str = "FROM louk_title_aliases a JOIN image_meta m
      ON m.cache_key = a.id_type || '/' || a.id_value
      OR (m.cache_key >= a.id_type || '/' || a.id_value || '@'
          AND m.cache_key < a.id_type || '/' || a.id_value || 'A')
      OR (m.cache_key >= a.id_type || '/' || a.id_value || '_'
          AND m.cache_key < a.id_type || '/' || a.id_value || '`')
    WHERE a.media_type = ? AND a.tmdb_id = ?";

/// Every cached image of a title across all its aliases (newest first, at
/// most `limit` rows). A key can only match one alias of a title.
pub async fn images_for_title(
    db: &impl ConnectionTrait,
    media_type: &str,
    tmdb_id: i64,
    limit: i64,
) -> Result<Vec<TitleImageRow>, AppError> {
    let sql = format!(
        "SELECT m.cache_key AS cache_key, m.image_type AS image_type,
                m.created_at AS created_at, m.updated_at AS updated_at,
                a.id_type AS alias_type, a.id_value AS alias_value,
                a.season AS season, a.episode AS episode
         {TITLE_IMAGES_JOIN}
         ORDER BY m.created_at DESC, m.cache_key
         LIMIT ?"
    );
    let rows = db
        .query_all(stmt(
            &sql,
            vec![media_type.into(), tmdb_id.into(), limit.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        out.push(TitleImageRow {
            cache_key: r.try_get("", "cache_key")?,
            image_type: r.try_get("", "image_type")?,
            created_at: r.try_get("", "created_at")?,
            updated_at: r.try_get("", "updated_at")?,
            alias_type: r.try_get("", "alias_type")?,
            alias_value: r.try_get("", "alias_value")?,
            season: r.try_get("", "season")?,
            episode: r.try_get("", "episode")?,
        });
    }
    Ok(out)
}

/// Number of cached images per kind for a title.
pub async fn image_counts_for_title(
    db: &impl ConnectionTrait,
    media_type: &str,
    tmdb_id: i64,
) -> Result<[u64; 4], AppError> {
    let sql = format!(
        "SELECT m.image_type AS image_type, COUNT(DISTINCT m.cache_key) AS n
         {TITLE_IMAGES_JOIN}
         GROUP BY m.image_type"
    );
    let rows = db
        .query_all(stmt(&sql, vec![media_type.into(), tmdb_id.into()]))
        .await?;
    let mut counts = [0u64; 4];
    for r in &rows {
        let image_type: String = r.try_get("", "image_type")?;
        let n: i64 = r.try_get("", "n")?;
        if let Some(kind) = image_type_of(&image_type) {
            counts[kind_index(kind)] += n.max(0) as u64;
        }
    }
    Ok(counts)
}

/// Index of a kind in `[poster, logo, backdrop, episode]` arrays.
pub fn kind_index(kind: ImageType) -> usize {
    match kind {
        ImageType::Poster => 0,
        ImageType::Logo => 1,
        ImageType::Backdrop => 2,
        ImageType::Episode => 3,
    }
}

/// Image kind of an `image_meta.image_type` value (`p`/`l`/`b`/`e`, or the
/// long names rows from before upstream's v002 upgrade may still carry).
pub fn image_type_of(image_type: &str) -> Option<ImageType> {
    match image_type {
        "p" | "poster" => Some(ImageType::Poster),
        "l" | "logo" => Some(ImageType::Logo),
        "b" | "backdrop" => Some(ImageType::Backdrop),
        "e" | "episode" => Some(ImageType::Episode),
        _ => None,
    }
}

/// Kind for a `kind` query/name (`poster`, `logo`, `backdrop`, `episode`).
pub fn image_type_from_name(name: &str) -> Option<ImageType> {
    match name {
        "poster" => Some(ImageType::Poster),
        "logo" => Some(ImageType::Logo),
        "backdrop" => Some(ImageType::Backdrop),
        "episode" => Some(ImageType::Episode),
        _ => None,
    }
}

/// Percent-encode one URL path segment (keeps RFC 3986 unreserved and `@`).
fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'@') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Upstream admin URL that serves the cached file of `{id_type}/{cache_value}`.
pub fn admin_image_url(kind: ImageType, id_type: &str, cache_value: &str) -> String {
    let value = encode_segment(cache_value);
    match kind {
        ImageType::Poster => format!("/api/admin/posters/{id_type}/{value}/image"),
        ImageType::Logo => format!("/api/admin/logos/{id_type}/{value}"),
        ImageType::Backdrop => format!("/api/admin/backdrops/{id_type}/{value}"),
        ImageType::Episode => format!("/api/admin/episodes/{id_type}/{value}/image"),
    }
}

/// The canonical TMDB alias value of a title (`movie-603`, `series-1399`).
pub fn canonical_tmdb_value(media_type: &str, tmdb_id: i64) -> String {
    if media_type == "movie" {
        format!("movie-{tmdb_id}")
    } else {
        format!("series-{tmdb_id}")
    }
}

// ---------------------------------------------------------------------------
// TMDB (paced) and the background workers
// ---------------------------------------------------------------------------

/// Run one fork TMDB call: one at a time, at least [`TMDB_GAP`] after the
/// previous one ended.
pub async fn paced<T>(
    state: &AppState,
    call: impl Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let mut last = state.louk.tmdb_gate.lock().await;
    if let Some(t) = *last {
        let elapsed = t.elapsed();
        if elapsed < TMDB_GAP {
            tokio::time::sleep(TMDB_GAP - elapsed).await;
        }
    }
    let out = call.await;
    *last = Some(Instant::now());
    out
}

/// Start the background workers (main.rs only; tests call the `*_once`
/// functions directly, so no test starts network activity by itself).
pub fn spawn_workers(state: Arc<AppState>) {
    let s = state.clone();
    supervise("flush", move || flush_loop(s.clone()));
    let s = state.clone();
    supervise("details", move || details_loop(s.clone()));
    supervise("backfill", move || backfill_loop(state.clone()));
}

/// Run a worker; if it panics, log it and start it again a minute later. A
/// worker never takes the server down.
fn supervise<F, Fut>(name: &'static str, make: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(make()).await {
                Ok(()) => return,
                Err(e) => {
                    tracing::warn!(worker = name, error = %e, "louk: title index worker stopped; restarting in 60 s");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            }
        }
    });
}

async fn flush_loop(s: Arc<AppState>) {
    let mut interval = tokio::time::interval(FLUSH_EVERY);
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(e) = flush_once(&s).await {
            tracing::warn!(error = %e, "louk: title index flush failed");
        }
        let dropped = s.louk.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            tracing::debug!(
                dropped,
                "louk: title index hook entries dropped (pending map full)"
            );
        }
    }
}

async fn details_loop(s: Arc<AppState>) {
    loop {
        let done = match details_once(&s, DETAILS_BATCH).await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "louk: title details batch failed");
                0
            }
        };
        if done == 0 {
            let _ = tokio::time::timeout(DETAILS_IDLE, s.louk.title_queue.notified()).await;
        }
    }
}

async fn backfill_loop(s: Arc<AppState>) {
    tokio::time::sleep(BACKFILL_FIRST_DELAY).await;
    let mut cursor = String::new();
    let mut failed: HashSet<(String, String)> = HashSet::new();
    let mut interval = tokio::time::interval(BACKFILL_EVERY);
    loop {
        interval.tick().await;
        match backfill_once(&s, &mut cursor, &mut failed, BACKFILL_BATCH).await {
            Ok(n) if n > 0 => tracing::info!(indexed = n, "louk: title index backfill"),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "louk: title index backfill failed"),
        }
    }
}

#[derive(Deserialize)]
struct TitleDetails {
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    imdb_id: Option<String>,
    #[serde(default)]
    external_ids: Option<TitleExternalIds>,
}

#[derive(Deserialize)]
struct TitleExternalIds {
    imdb_id: Option<String>,
    tvdb_id: Option<u64>,
}

/// Fetch details for up to `limit` titles that have none yet (or whose last
/// attempt failed more than a day ago). Returns how many were attempted;
/// every attempt is recorded (a failure as a negative `resolved_at`), so the
/// worker never loops on the same title.
pub async fn details_once(state: &AppState, limit: i64) -> Result<usize, AppError> {
    let now = now_secs();
    let rows = state
        .db
        .query_all(stmt(
            "SELECT media_type, tmdb_id FROM louk_titles
             WHERE resolved_at IS NULL OR (resolved_at < 0 AND resolved_at > ?)
             ORDER BY last_requested DESC LIMIT ?",
            vec![(-(now - DETAILS_RETRY_SECS)).into(), limit.into()],
        ))
        .await?;
    let mut todo = Vec::with_capacity(rows.len());
    for r in &rows {
        let media_type: String = r.try_get("", "media_type")?;
        let tmdb_id: i64 = r.try_get("", "tmdb_id")?;
        todo.push((media_type, tmdb_id));
    }
    for (media_type, tmdb_id) in &todo {
        let result = if media_type == "tv" {
            let path = format!("/tv/{tmdb_id}");
            paced(
                state,
                state
                    .tmdb
                    .get::<TitleDetails>(&path, &[("append_to_response", "external_ids")]),
            )
            .await
        } else {
            let path = format!("/movie/{tmdb_id}");
            paced(state, state.tmdb.get::<TitleDetails>(&path, &[])).await
        };
        match result {
            Ok(d) => {
                let name = d.title.or(d.name).filter(|s| !s.is_empty());
                let year = year_of(d.release_date.as_deref().or(d.first_air_date.as_deref()));
                let imdb_id = d
                    .imdb_id
                    .or_else(|| d.external_ids.as_ref().and_then(|e| e.imdb_id.clone()))
                    .filter(|s| !s.is_empty());
                let tvdb_id = d
                    .external_ids
                    .as_ref()
                    .and_then(|e| e.tvdb_id)
                    .map(|v| v as i64);
                let poster_path = d.poster_path.filter(|s| !s.is_empty());
                state
                    .db
                    .execute(stmt(
                        "UPDATE louk_titles SET name = ?, year = COALESCE(?, year),
                            poster_path = COALESCE(?, poster_path),
                            imdb_id = COALESCE(imdb_id, ?), tvdb_id = COALESCE(tvdb_id, ?),
                            resolved_at = ?
                         WHERE media_type = ? AND tmdb_id = ?",
                        vec![
                            name.into(),
                            year.into(),
                            poster_path.into(),
                            imdb_id.into(),
                            tvdb_id.into(),
                            now_secs().into(),
                            media_type.clone().into(),
                            (*tmdb_id).into(),
                        ],
                    ))
                    .await?;
            }
            Err(e) => {
                tracing::debug!(error = %e, media_type = %media_type, tmdb_id = *tmdb_id, "louk: title details fetch failed");
                state
                    .db
                    .execute(stmt(
                        "UPDATE louk_titles SET resolved_at = ? WHERE media_type = ? AND tmdb_id = ?",
                        vec![
                            (-now_secs()).into(),
                            media_type.clone().into(),
                            (*tmdb_id).into(),
                        ],
                    ))
                    .await?;
            }
        }
    }
    Ok(todo.len())
}

/// One backfill round: scan `image_meta` from `cursor` (keyset, wraps around
/// once) for up to `limit` id prefixes without an alias row, resolve each via
/// `id::resolve` (TMDB only), and record title + aliases. Prefixes that fail
/// to resolve go into `failed` (bounded) and are skipped for the process
/// lifetime. Returns how many prefixes were indexed.
pub async fn backfill_once(
    state: &AppState,
    cursor: &mut String,
    failed: &mut HashSet<(String, String)>,
    limit: usize,
) -> Result<usize, AppError> {
    let mut candidates: Vec<((String, String), i64)> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut wrapped = cursor.is_empty();
    let mut pages = 0;
    'scan: while pages < BACKFILL_MAX_PAGES {
        pages += 1;
        let rows = state
            .db
            .query_all(stmt(
                "SELECT cache_key, updated_at FROM image_meta WHERE cache_key > ?
                 ORDER BY cache_key LIMIT ?",
                vec![cursor.clone().into(), BACKFILL_PAGE.into()],
            ))
            .await?;
        if rows.is_empty() {
            if wrapped {
                break;
            }
            cursor.clear();
            wrapped = true;
            continue;
        }
        for r in &rows {
            let key: String = r.try_get("", "cache_key")?;
            let updated_at: i64 = r.try_get("", "updated_at")?;
            *cursor = key.clone();
            let Some(prefix) = parse_cache_key_prefix(&key) else {
                continue;
            };
            if failed.contains(&prefix) || !seen.insert(prefix.clone()) {
                continue;
            }
            let known = state
                .db
                .query_one(stmt(
                    "SELECT 1 AS one FROM louk_title_aliases WHERE id_type = ? AND id_value = ?",
                    vec![prefix.0.clone().into(), prefix.1.clone().into()],
                ))
                .await?
                .is_some();
            if !known {
                candidates.push((prefix, updated_at));
                if candidates.len() >= limit {
                    break 'scan;
                }
            }
        }
    }

    let mut indexed = 0;
    for ((id_type_str, id_value), updated_at) in candidates {
        let Ok(id_type) = IdType::parse(&id_type_str) else {
            continue;
        };
        let result = paced(
            state,
            id::resolve(id_type, &id_value, &state.tmdb, &state.id_cache),
        )
        .await;
        match result {
            Ok(r) => {
                let row = alias_row(&r, updated_at);
                let txn = state.db.begin().await?;
                for ((t, v), alias) in cross_aliases(&id_type_str, &r, &row) {
                    upsert_alias(&txn, &t, &v, &alias).await?;
                }
                upsert_alias(&txn, &id_type_str, &id_value, &row).await?;
                txn.commit().await?;
                indexed += 1;
            }
            Err(e) => {
                tracing::debug!(error = %e, id_type = %id_type_str, id_value = %id_value, "louk: backfill resolve failed");
                if failed.len() < BACKFILL_FAILED_CAP {
                    failed.insert((id_type_str, id_value));
                }
            }
        }
    }
    if indexed > 0 {
        state.louk.title_queue.notify_one();
    }
    Ok(indexed)
}

// ---------------------------------------------------------------------------
// Season episode names (lazy, for the title page)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EpisodeRow {
    pub episode: i64,
    pub name: Option<String>,
    pub still_path: Option<String>,
    pub air_date: Option<String>,
}

#[derive(Deserialize)]
struct SeasonDetails {
    #[serde(default)]
    episodes: Vec<SeasonEpisode>,
}

#[derive(Deserialize)]
struct SeasonEpisode {
    episode_number: u32,
    name: Option<String>,
    still_path: Option<String>,
    air_date: Option<String>,
}

pub async fn stored_episodes(
    db: &impl ConnectionTrait,
    show: i64,
    season: i64,
) -> Result<(Vec<EpisodeRow>, i64), AppError> {
    let rows = db
        .query_all(stmt(
            "SELECT episode, name, still_path, air_date, fetched_at FROM louk_episodes
             WHERE show_tmdb_id = ? AND season = ? ORDER BY episode",
            vec![show.into(), season.into()],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    let mut oldest = i64::MAX;
    for r in &rows {
        let fetched_at: i64 = r.try_get("", "fetched_at")?;
        oldest = oldest.min(fetched_at);
        out.push(episode_from_row(r)?);
    }
    Ok((out, oldest))
}

fn episode_from_row(r: &QueryResult) -> Result<EpisodeRow, DbErr> {
    Ok(EpisodeRow {
        episode: r.try_get("", "episode")?,
        name: r.try_get("", "name")?,
        still_path: r.try_get("", "still_path")?,
        air_date: r.try_get("", "air_date")?,
    })
}

/// Episodes of one season: from `louk_episodes` when fetched within the last
/// 7 days, otherwise from TMDB `/tv/{id}/season/{n}` (paced) and stored. A
/// failed fetch falls back to whatever is stored (possibly nothing).
pub async fn season_episodes(
    state: &AppState,
    show: i64,
    season: i64,
) -> Result<Vec<EpisodeRow>, AppError> {
    let (stored, oldest) = stored_episodes(&state.db, show, season).await?;
    if !stored.is_empty() && oldest > now_secs() - EPISODES_TTL_SECS {
        return Ok(stored);
    }
    let path = format!("/tv/{show}/season/{season}");
    match paced(state, state.tmdb.get::<SeasonDetails>(&path, &[])).await {
        Ok(details) => {
            let now = now_secs();
            let txn = state.db.begin().await?;
            for ep in &details.episodes {
                txn.execute(stmt(
                    "INSERT INTO louk_episodes
                        (show_tmdb_id, season, episode, name, still_path, air_date, fetched_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(show_tmdb_id, season, episode) DO UPDATE SET
                        name = excluded.name, still_path = excluded.still_path,
                        air_date = excluded.air_date, fetched_at = excluded.fetched_at",
                    vec![
                        show.into(),
                        season.into(),
                        (ep.episode_number as i64).into(),
                        ep.name.clone().into(),
                        ep.still_path.clone().into(),
                        ep.air_date.clone().into(),
                        now.into(),
                    ],
                ))
                .await?;
            }
            txn.commit().await?;
            Ok(stored_episodes(&state.db, show, season).await?.0)
        }
        Err(e) => {
            tracing::debug!(error = %e, show, season, "louk: season fetch failed");
            Ok(stored)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::EpisodeInfo;

    #[test]
    fn parse_cache_key_prefix_cases() {
        assert_eq!(
            parse_cache_key_prefix("imdb/tt123@ir.pbc.zm"),
            Some(("imdb".into(), "tt123".into()))
        );
        assert_eq!(
            parse_cache_key_prefix("tmdb/movie-603_b_t@ir.zl"),
            Some(("tmdb".into(), "movie-603".into()))
        );
        assert_eq!(
            parse_cache_key_prefix("imdb/episode-tt0944947-S1E2@i.zm"),
            Some(("imdb".into(), "episode-tt0944947-S1E2".into()))
        );
        assert_eq!(
            parse_cache_key_prefix("tvdb/81189"),
            Some(("tvdb".into(), "81189".into()))
        );
        assert_eq!(parse_cache_key_prefix("bogus/tt1@x"), None);
        assert_eq!(parse_cache_key_prefix("imdb/@x"), None);
        assert_eq!(parse_cache_key_prefix("imdb/tt1:logo:x"), None);
        assert_eq!(parse_cache_key_prefix("no-slash"), None);
    }

    fn resolved(media_type: MediaType) -> ResolvedId {
        ResolvedId {
            imdb_id: Some("tt0944947".into()),
            tmdb_id: 1399,
            tvdb_id: Some(121361),
            media_type,
            poster_path: Some("/p.jpg".into()),
            release_date: Some("2011-04-17".into()),
            episode: None,
        }
    }

    #[test]
    fn alias_row_for_show_keeps_title_facts() {
        let row = alias_row(&resolved(MediaType::Tv), 5);
        assert_eq!(row.media_type, "tv");
        assert_eq!(row.tmdb_id, 1399);
        assert_eq!(row.year, Some(2011));
        assert_eq!(row.imdb_id.as_deref(), Some("tt0944947"));
        assert_eq!((row.season, row.episode), (None, None));
    }

    #[test]
    fn alias_row_for_episode_maps_to_show_without_episode_ids() {
        let mut r = resolved(MediaType::Episode);
        r.imdb_id = Some("tt1480055".into());
        r.episode = Some(EpisodeInfo {
            show_tmdb_id: 1399,
            season_number: 1,
            episode_number: 2,
            still_path: None,
        });
        let row = alias_row(&r, 5);
        assert_eq!(row.media_type, "tv");
        assert_eq!(row.tmdb_id, 1399);
        assert_eq!((row.season, row.episode), (Some(1), Some(2)));
        assert_eq!(row.imdb_id, None);
        assert_eq!(row.poster_path, None);
    }

    #[test]
    fn cross_aliases_mirror_upstream_formats() {
        let r = resolved(MediaType::Tv);
        let row = alias_row(&r, 5);
        let keys: Vec<(String, String)> = cross_aliases("imdb", &r, &row)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            keys,
            vec![
                ("tmdb".into(), "series-1399".into()),
                ("tvdb".into(), "121361".into())
            ]
        );
        let mut m = resolved(MediaType::Movie);
        m.tvdb_id = None;
        let row = alias_row(&m, 5);
        let keys: Vec<(String, String)> = cross_aliases("tmdb", &m, &row)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, vec![("imdb".into(), "tt0944947".into())]);
    }

    #[test]
    fn admin_image_urls_match_upstream_routes() {
        assert_eq!(
            admin_image_url(ImageType::Poster, "imdb", "tt1@ir.zm"),
            "/api/admin/posters/imdb/tt1@ir.zm/image"
        );
        assert_eq!(
            admin_image_url(ImageType::Logo, "tmdb", "movie-1_l_t@.zm"),
            "/api/admin/logos/tmdb/movie-1_l_t@.zm"
        );
        assert_eq!(
            admin_image_url(ImageType::Backdrop, "tvdb", "1_b@.zl"),
            "/api/admin/backdrops/tvdb/1_b@.zl"
        );
        assert_eq!(
            admin_image_url(ImageType::Episode, "imdb", "a b#c?d%/e"),
            "/api/admin/episodes/imdb/a%20b%23c%3Fd%25%2Fe/image"
        );
    }

    #[test]
    fn kinds_round_trip() {
        for kind in [
            ImageType::Poster,
            ImageType::Logo,
            ImageType::Backdrop,
            ImageType::Episode,
        ] {
            assert_eq!(image_type_of(kind.db_value()), Some(kind));
            assert_eq!(image_type_of(kind.label()), Some(kind));
            assert_eq!(image_type_from_name(kind.label()), Some(kind));
        }
        assert_eq!(image_type_of("x"), None);
        assert_eq!(canonical_tmdb_value("movie", 603), "movie-603");
        assert_eq!(canonical_tmdb_value("tv", 1399), "series-1399");
    }

    #[test]
    fn year_of_handles_short_and_odd_dates() {
        assert_eq!(year_of(Some("1999-03-31")), Some(1999));
        assert_eq!(year_of(Some("")), None);
        assert_eq!(year_of(Some("19")), None);
        assert_eq!(year_of(Some("éééé")), None);
        assert_eq!(year_of(None), None);
    }
}
