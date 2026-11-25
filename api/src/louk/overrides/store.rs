//! The `louk_overrides` table and its in-memory mirror.
//!
//! The whole table (admin-sized: one row per overridden title + kind) is
//! loaded into a map on first use and kept in sync by [`OverrideStore::set`]
//! and [`OverrideStore::clear`], which write the DB first and the map second.
//! A lookup is a hash probe under a read lock: no DB query, no network, no
//! error path. While the first load fails (DB unavailable) lookups return
//! `None` and the next call retries, so a render is never failed by it.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};

use super::{NO_EPISODE, OverrideKey, OverrideSource, TitleOverride, kind_from_name};
use crate::cache::ImageType;
use crate::error::AppError;
use crate::services::lang::lang_base;

/// Additive fork table (rollback-safe: upstream never reads it). Every column
/// without a natural value has a DEFAULT.
pub const OVERRIDES_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS louk_overrides (
        media_type TEXT NOT NULL,
        tmdb_id    INTEGER NOT NULL,
        kind       TEXT NOT NULL,
        season     INTEGER NOT NULL DEFAULT -1,
        episode    INTEGER NOT NULL DEFAULT -1,
        lang       TEXT NOT NULL DEFAULT '',
        source     TEXT NOT NULL,
        value      TEXT NOT NULL DEFAULT '',
        fanart_id  TEXT,
        no_badges  INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (media_type, tmdb_id, kind, season, episode, lang)
    )";

#[derive(Clone, Debug)]
struct Row {
    kind: ImageType,
    season: i32,
    episode: i32,
    lang: String,
    ov: TitleOverride,
}

type Map = HashMap<(&'static str, u64), Vec<Row>>;

pub struct OverrideStore {
    map: tokio::sync::OnceCell<RwLock<Map>>,
    /// Serialises writers so the DB and the map change in the same order.
    writes: tokio::sync::Mutex<()>,
    /// Rows in the map, and rows with `no_badges` (cheap pre-checks).
    rows: AtomicUsize,
    no_badges_rows: AtomicUsize,
}

impl Default for OverrideStore {
    fn default() -> Self {
        Self::new()
    }
}

fn stmt(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn media_type_static(s: &str) -> Option<&'static str> {
    match s {
        "movie" => Some("movie"),
        "tv" => Some("tv"),
        _ => None,
    }
}

impl OverrideStore {
    pub fn new() -> Self {
        Self {
            map: tokio::sync::OnceCell::new(),
            writes: tokio::sync::Mutex::new(()),
            rows: AtomicUsize::new(0),
            no_badges_rows: AtomicUsize::new(0),
        }
    }

    async fn load(&self, db: &DatabaseConnection) -> Result<&RwLock<Map>, AppError> {
        self.map
            .get_or_try_init(|| async {
                let map = load_all(db).await?;
                self.recount(&map);
                Ok::<_, AppError>(RwLock::new(map))
            })
            .await
    }

    /// The map, loading it on first use; `None` (logged) while the DB fails.
    async fn loaded(&self, db: &DatabaseConnection) -> Option<&RwLock<Map>> {
        match self.load(db).await {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!(error = %e, "louk overrides: loading the table failed; serving without overrides");
                None
            }
        }
    }

    fn recount(&self, map: &Map) {
        let rows = map.values().map(Vec::len).sum();
        let nb = map.values().flatten().filter(|r| r.ov.no_badges).count();
        self.rows.store(rows, Ordering::Relaxed);
        self.no_badges_rows.store(nb, Ordering::Relaxed);
    }

    /// Load the table now (e.g. after rows were written behind the store's
    /// back, in tests). Replaces the map.
    pub async fn reload(&self, db: &DatabaseConnection) -> Result<(), AppError> {
        let fresh = load_all(db).await?;
        let lock = self.load(db).await?;
        let mut g = lock.write().unwrap_or_else(|p| p.into_inner());
        *g = fresh;
        self.recount(&g);
        Ok(())
    }

    /// Number of override rows (0 before the first load).
    pub fn len(&self) -> usize {
        self.rows.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when some row has `no_badges` (loads the table on first use).
    pub async fn any_no_badges(&self, db: &DatabaseConnection) -> bool {
        if self.loaded(db).await.is_none() {
            return false;
        }
        self.no_badges_rows.load(Ordering::Relaxed) > 0
    }

    /// The override a render of this title should use: exact `lang`, then its
    /// base (`pt-BR` → `pt`), then `""` (any). Language only matters for
    /// posters and logos; backdrops and stills only have `""` rows.
    pub async fn lookup(
        &self,
        db: &DatabaseConnection,
        media_type: &'static str,
        tmdb_id: u64,
        kind: ImageType,
        season: i32,
        episode: i32,
        lang: &str,
    ) -> Option<TitleOverride> {
        let lock = self.loaded(db).await?;
        if self.is_empty() {
            return None;
        }
        let g = lock.read().unwrap_or_else(|p| p.into_inner());
        let rows = g.get(&(media_type, tmdb_id))?;
        let matching = |l: &str| {
            rows.iter()
                .find(|r| {
                    r.kind == kind && r.season == season && r.episode == episode && r.lang == l
                })
                .map(|r| r.ov.clone())
        };
        if matches!(kind, ImageType::Poster | ImageType::Logo) && !lang.is_empty() {
            if let Some(ov) = matching(lang) {
                return Some(ov);
            }
            let base = lang_base(lang);
            if base != lang {
                if let Some(ov) = matching(base) {
                    return Some(ov);
                }
            }
        }
        matching("")
    }

    /// The row stored under exactly this key (no language fallback).
    pub async fn get_exact(
        &self,
        db: &DatabaseConnection,
        key: &OverrideKey,
    ) -> Result<Option<TitleOverride>, AppError> {
        let lock = self.load(db).await?;
        let g = lock.read().unwrap_or_else(|p| p.into_inner());
        Ok(g.get(&(key.media_type, key.tmdb_id)).and_then(|rows| {
            rows.iter()
                .find(|r| {
                    r.kind == key.kind
                        && r.season == key.season
                        && r.episode == key.episode
                        && r.lang == key.lang
                })
                .map(|r| r.ov.clone())
        }))
    }

    /// Every override of a title, ordered by kind, season, episode, lang.
    pub async fn list_title(
        &self,
        db: &DatabaseConnection,
        media_type: &'static str,
        tmdb_id: u64,
    ) -> Result<Vec<(OverrideKey, TitleOverride)>, AppError> {
        let lock = self.load(db).await?;
        let g = lock.read().unwrap_or_else(|p| p.into_inner());
        let mut out: Vec<(OverrideKey, TitleOverride)> = g
            .get(&(media_type, tmdb_id))
            .map(|rows| {
                rows.iter()
                    .map(|r| {
                        (
                            OverrideKey {
                                media_type,
                                tmdb_id,
                                kind: r.kind,
                                season: r.season,
                                episode: r.episode,
                                lang: r.lang.clone(),
                            },
                            r.ov.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by(|(a, _), (b, _)| {
            (kind_order(a.kind), a.season, a.episode, &a.lang).cmp(&(
                kind_order(b.kind),
                b.season,
                b.episode,
                &b.lang,
            ))
        });
        Ok(out)
    }

    /// Insert or replace an override (DB first, then the map).
    pub async fn set(
        &self,
        db: &DatabaseConnection,
        key: &OverrideKey,
        ov: &TitleOverride,
    ) -> Result<(), AppError> {
        let _w = self.writes.lock().await;
        let lock = self.load(db).await?;
        db.execute(stmt(
            "INSERT INTO louk_overrides
                (media_type, tmdb_id, kind, season, episode, lang, source, value, fanart_id, no_badges, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(media_type, tmdb_id, kind, season, episode, lang) DO UPDATE SET
                source = excluded.source,
                value = excluded.value,
                fanart_id = excluded.fanart_id,
                no_badges = excluded.no_badges,
                updated_at = excluded.updated_at",
            vec![
                key.media_type.into(),
                (key.tmdb_id as i64).into(),
                key.kind.label().into(),
                key.season.into(),
                key.episode.into(),
                key.lang.clone().into(),
                ov.source.name().into(),
                ov.source.value().to_owned().into(),
                ov.source.fanart_id().map(str::to_owned).into(),
                i64::from(ov.no_badges).into(),
                ov.updated_at.into(),
            ],
        ))
        .await?;
        let mut g = lock.write().unwrap_or_else(|p| p.into_inner());
        let rows = g.entry((key.media_type, key.tmdb_id)).or_default();
        let row = Row {
            kind: key.kind,
            season: key.season,
            episode: key.episode,
            lang: key.lang.clone(),
            ov: ov.clone(),
        };
        match rows.iter_mut().find(|r| {
            r.kind == key.kind
                && r.season == key.season
                && r.episode == key.episode
                && r.lang == key.lang
        }) {
            Some(existing) => *existing = row,
            None => rows.push(row),
        }
        self.recount(&g);
        Ok(())
    }

    /// Delete an override; `false` when there was none.
    pub async fn clear(
        &self,
        db: &DatabaseConnection,
        key: &OverrideKey,
    ) -> Result<bool, AppError> {
        let _w = self.writes.lock().await;
        let lock = self.load(db).await?;
        let res = db
            .execute(stmt(
                "DELETE FROM louk_overrides
                 WHERE media_type = ? AND tmdb_id = ? AND kind = ? AND season = ? AND episode = ? AND lang = ?",
                vec![
                    key.media_type.into(),
                    (key.tmdb_id as i64).into(),
                    key.kind.label().into(),
                    key.season.into(),
                    key.episode.into(),
                    key.lang.clone().into(),
                ],
            ))
            .await?;
        let mut g = lock.write().unwrap_or_else(|p| p.into_inner());
        let mut removed = false;
        if let Some(rows) = g.get_mut(&(key.media_type, key.tmdb_id)) {
            let before = rows.len();
            rows.retain(|r| {
                !(r.kind == key.kind
                    && r.season == key.season
                    && r.episode == key.episode
                    && r.lang == key.lang)
            });
            removed = rows.len() != before;
            if rows.is_empty() {
                g.remove(&(key.media_type, key.tmdb_id));
            }
        }
        self.recount(&g);
        Ok(removed || res.rows_affected() > 0)
    }
}

fn kind_order(k: ImageType) -> u8 {
    match k {
        ImageType::Poster => 0,
        ImageType::Logo => 1,
        ImageType::Backdrop => 2,
        ImageType::Episode => 3,
    }
}

async fn load_all(db: &DatabaseConnection) -> Result<Map, AppError> {
    let rows = db
        .query_all(stmt(
            "SELECT media_type, tmdb_id, kind, season, episode, lang, source, value, fanart_id, no_badges, updated_at
             FROM louk_overrides",
            vec![],
        ))
        .await?;
    let mut map: Map = HashMap::new();
    for r in &rows {
        let media_type: String = r.try_get("", "media_type")?;
        let tmdb_id: i64 = r.try_get("", "tmdb_id")?;
        let kind: String = r.try_get("", "kind")?;
        let season: i64 = r.try_get("", "season")?;
        let episode: i64 = r.try_get("", "episode")?;
        let lang: String = r.try_get("", "lang")?;
        let source: String = r.try_get("", "source")?;
        let value: String = r.try_get("", "value")?;
        let fanart_id: Option<String> = r.try_get("", "fanart_id")?;
        let no_badges: i64 = r.try_get("", "no_badges")?;
        let updated_at: i64 = r.try_get("", "updated_at")?;
        let (Some(mt), Some(kind), Ok(source)) = (
            media_type_static(&media_type),
            kind_from_name(&kind),
            OverrideSource::from_parts(&source, &value, fanart_id.as_deref()),
        ) else {
            tracing::warn!(media_type = %media_type, tmdb_id, kind = %kind, source = %source, "louk overrides: skipping an invalid row");
            continue;
        };
        let (Ok(season), Ok(episode)) = (i32::try_from(season), i32::try_from(episode)) else {
            continue;
        };
        let (season, episode) = if kind == ImageType::Episode {
            (season, episode)
        } else {
            (NO_EPISODE, NO_EPISODE)
        };
        map.entry((mt, tmdb_id as u64)).or_default().push(Row {
            kind,
            season,
            episode,
            lang,
            ov: TitleOverride {
                source,
                no_badges: no_badges != 0,
                updated_at,
            },
        });
    }
    Ok(map)
}
