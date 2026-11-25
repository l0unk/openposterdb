//! Admin endpoints of the title dashboard (fork-only, under
//! `/api/admin/louk/titles`). Errors use upstream's `{"error": "..."}` shape.
//! See docs/api.md "Title dashboard (fork-only)" for the contract.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use sea_orm::{ConnectionTrait, QueryResult};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::cache::ImageType;
use crate::error::AppError;
use crate::image::serve::{self, LogoBackdropKind};
use crate::louk::titles::{self, stmt};
use crate::services::db;

pub const DEFAULT_PER_PAGE: u64 = 48;
pub const MAX_PER_PAGE: u64 = 200;
const MAX_QUERY_LEN: usize = 200;
/// Most image rows one title page lists.
const MAX_DETAIL_IMAGES: i64 = 5_000;
/// Seasons whose episode names may be fetched from TMDB for one page view;
/// further seasons show what is stored.
const MAX_SEASONS_FETCHED: usize = 40;

const TMDB_POSTER_THUMB: &str = "https://image.tmdb.org/t/p/w185";
const TMDB_STILL_THUMB: &str = "https://image.tmdb.org/t/p/w300";

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Serialize, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageCounts {
    pub poster: u64,
    pub logo: u64,
    pub backdrop: u64,
    pub episode: u64,
}

impl From<[u64; 4]> for ImageCounts {
    fn from(c: [u64; 4]) -> Self {
        Self {
            poster: c[0],
            logo: c[1],
            backdrop: c[2],
            episode: c[3],
        }
    }
}

#[derive(Serialize, Debug)]
pub struct TitleSummary {
    pub media_type: String,
    pub tmdb_id: i64,
    pub name: Option<String>,
    pub year: Option<i64>,
    pub imdb_id: Option<String>,
    pub poster_thumb_url: Option<String>,
    pub last_requested: i64,
    pub image_counts: ImageCounts,
}

#[derive(Serialize, Debug)]
pub struct TitleList {
    pub items: Vec<TitleSummary>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

#[derive(Serialize, Debug)]
pub struct AliasOut {
    pub id_type: String,
    pub id_value: String,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub last_requested: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ImageOut {
    pub kind: &'static str,
    pub id_type: String,
    pub cache_value: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub admin_image_url: String,
}

#[derive(Serialize, Debug)]
pub struct EpisodeOut {
    pub episode: i64,
    pub name: Option<String>,
    pub air_date: Option<String>,
    pub still_thumb_url: Option<String>,
    pub cached: Vec<ImageOut>,
}

#[derive(Serialize, Debug)]
pub struct SeasonOut {
    pub season: i64,
    pub episodes: Vec<EpisodeOut>,
}

#[derive(Serialize, Debug)]
pub struct TitleDetail {
    #[serde(flatten)]
    pub summary: TitleSummary,
    pub tvdb_id: Option<i64>,
    pub first_seen: i64,
    pub resolved_at: Option<i64>,
    pub aliases: Vec<AliasOut>,
    pub images: Vec<ImageOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasons: Option<Vec<SeasonOut>>,
}

#[derive(Serialize, Debug)]
pub struct AliasTarget {
    pub media_type: String,
    pub tmdb_id: i64,
}

#[derive(Serialize, Debug)]
pub struct PurgeOut {
    pub ok: bool,
    pub external_cache_only: bool,
    /// How many `(id_type, id_value)` ids were purged.
    pub ids: u64,
    pub files_deleted: u64,
    pub meta_deleted: u64,
    pub ratings_deleted: u64,
    pub errors: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct RefetchResult {
    pub kind: &'static str,
    pub id_type: String,
    pub id_value: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct RefetchOut {
    pub ok: bool,
    pub results: Vec<RefetchResult>,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_media_type(s: &str) -> Result<&'static str, AppError> {
    match s {
        "movie" => Ok("movie"),
        "tv" => Ok("tv"),
        _ => Err(AppError::BadRequest(
            "media_type must be movie or tv".into(),
        )),
    }
}

fn parse_title_path(media_type: &str, tmdb_id: &str) -> Result<(&'static str, i64), AppError> {
    let media_type = parse_media_type(media_type)?;
    let tmdb_id = tmdb_id
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| AppError::BadRequest("tmdb_id must be a positive integer".into()))?;
    Ok((media_type, tmdb_id))
}

fn parse_positive(name: &str, v: Option<&str>, default: u64) -> Result<u64, AppError> {
    match v.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(default),
        Some(s) => s
            .parse::<u64>()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| AppError::BadRequest(format!("{name} must be a positive integer"))),
    }
}

fn poster_thumb_url(poster_path: Option<&str>) -> Option<String> {
    poster_path
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("{TMDB_POSTER_THUMB}{p}"))
}

fn still_thumb_url(still_path: Option<&str>) -> Option<String> {
    still_path
        .filter(|p| p.starts_with('/'))
        .map(|p| format!("{TMDB_STILL_THUMB}{p}"))
}

fn not_found(media_type: &str, tmdb_id: i64) -> AppError {
    AppError::IdNotFound(format!("title {media_type}/{tmdb_id} is not in the index"))
}

struct TitleRow {
    media_type: String,
    tmdb_id: i64,
    name: Option<String>,
    year: Option<i64>,
    imdb_id: Option<String>,
    tvdb_id: Option<i64>,
    poster_path: Option<String>,
    first_seen: i64,
    last_requested: i64,
    resolved_at: Option<i64>,
}

const TITLE_COLUMNS: &str = "media_type, tmdb_id, name, year, imdb_id, tvdb_id, poster_path, \
                             first_seen, last_requested, resolved_at";

fn title_from_row(r: &QueryResult) -> Result<TitleRow, AppError> {
    Ok(TitleRow {
        media_type: r.try_get("", "media_type")?,
        tmdb_id: r.try_get("", "tmdb_id")?,
        name: r.try_get("", "name")?,
        year: r.try_get("", "year")?,
        imdb_id: r.try_get("", "imdb_id")?,
        tvdb_id: r.try_get("", "tvdb_id")?,
        poster_path: r.try_get("", "poster_path")?,
        first_seen: r.try_get("", "first_seen")?,
        last_requested: r.try_get("", "last_requested")?,
        resolved_at: r.try_get("", "resolved_at")?,
    })
}

async fn load_title(
    db: &impl ConnectionTrait,
    media_type: &str,
    tmdb_id: i64,
) -> Result<Option<TitleRow>, AppError> {
    let row = db
        .query_one(stmt(
            &format!(
                "SELECT {TITLE_COLUMNS} FROM louk_titles WHERE media_type = ? AND tmdb_id = ?"
            ),
            vec![media_type.into(), tmdb_id.into()],
        ))
        .await?;
    row.as_ref().map(title_from_row).transpose()
}

async fn summary_of(db: &impl ConnectionTrait, t: &TitleRow) -> Result<TitleSummary, AppError> {
    let counts = titles::image_counts_for_title(db, &t.media_type, t.tmdb_id).await?;
    Ok(TitleSummary {
        media_type: t.media_type.clone(),
        tmdb_id: t.tmdb_id,
        name: t.name.clone(),
        year: t.year,
        imdb_id: t.imdb_id.clone(),
        poster_thumb_url: poster_thumb_url(t.poster_path.as_deref()),
        last_requested: t.last_requested,
        image_counts: counts.into(),
    })
}

fn image_out(row: &titles::TitleImageRow) -> Option<ImageOut> {
    let kind = titles::image_type_of(&row.image_type)?;
    let (id_type, cache_value) = row.cache_key.split_once('/')?;
    Some(ImageOut {
        kind: kind.label(),
        id_type: id_type.to_owned(),
        cache_value: cache_value.to_owned(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        admin_image_url: titles::admin_image_url(kind, id_type, cache_value),
    })
}

// ---------------------------------------------------------------------------
// GET /api/admin/louk/titles
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub media_type: Option<String>,
    pub sort: Option<String>,
    pub page: Option<String>,
    pub per_page: Option<String>,
}

pub async fn list_titles(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<TitleList>, AppError> {
    let page = parse_positive("page", query.page.as_deref(), 1)?;
    let per_page =
        parse_positive("per_page", query.per_page.as_deref(), DEFAULT_PER_PAGE)?.min(MAX_PER_PAGE);

    let mut filters: Vec<&str> = Vec::new();
    let mut values: Vec<sea_orm::Value> = Vec::new();
    match query
        .media_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None | Some("all") => {}
        Some(t) => {
            filters.push("media_type = ?");
            values.push(parse_media_type(t)?.into());
        }
    }
    if let Some(q) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if q.chars().count() > MAX_QUERY_LEN {
            return Err(AppError::BadRequest("q is too long".into()));
        }
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        filters.push("(name LIKE ? ESCAPE '\\' OR imdb_id = ? OR CAST(tmdb_id AS TEXT) = ?)");
        values.push(format!("%{escaped}%").into());
        values.push(q.to_owned().into());
        values.push(q.to_owned().into());
    }
    let order = match query
        .sort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None | Some("last") => "last_requested DESC, media_type, tmdb_id",
        Some("name") => "name IS NULL, name COLLATE NOCASE, last_requested DESC, tmdb_id",
        Some(_) => return Err(AppError::BadRequest("sort must be last or name".into())),
    };
    let where_sql = if filters.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", filters.join(" AND "))
    };

    let total_row = state
        .db
        .query_one(stmt(
            &format!("SELECT COUNT(*) AS n FROM louk_titles {where_sql}"),
            values.clone(),
        ))
        .await?;
    let total: i64 = match &total_row {
        Some(r) => r.try_get("", "n")?,
        None => 0,
    };

    let offset = (page - 1).saturating_mul(per_page).min(i64::MAX as u64) as i64;
    let mut page_values = values;
    page_values.push((per_page as i64).into());
    page_values.push(offset.into());
    let rows = state
        .db
        .query_all(stmt(
            &format!(
                "SELECT {TITLE_COLUMNS} FROM louk_titles {where_sql} ORDER BY {order} LIMIT ? OFFSET ?"
            ),
            page_values,
        ))
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let t = title_from_row(r)?;
        items.push(summary_of(&state.db, &t).await?);
    }
    Ok(Json(TitleList {
        items,
        total: total.max(0) as u64,
        page,
        per_page,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/admin/louk/titles/by-alias/{id_type}/{id_value}
// ---------------------------------------------------------------------------

async fn lookup_alias(
    db: &impl ConnectionTrait,
    id_type: &str,
    id_value: &str,
) -> Result<Option<AliasTarget>, AppError> {
    let row = db
        .query_one(stmt(
            "SELECT media_type, tmdb_id FROM louk_title_aliases WHERE id_type = ? AND id_value = ?",
            vec![id_type.into(), id_value.into()],
        ))
        .await?;
    match row {
        Some(r) => Ok(Some(AliasTarget {
            media_type: r.try_get("", "media_type")?,
            tmdb_id: r.try_get("", "tmdb_id")?,
        })),
        None => Ok(None),
    }
}

/// Which title an id belongs to. Also accepts a full cache value
/// (`tt123@ir.zm`, `movie-603_b_t@….zl`): it is cut to the id the same way
/// the backfill parses cache keys.
pub async fn title_by_alias(
    State(state): State<Arc<AppState>>,
    Path((id_type, id_value)): Path<(String, String)>,
) -> Result<Json<AliasTarget>, AppError> {
    crate::id::IdType::parse(&id_type)?;
    let id_value = id_value.trim();
    if id_value.is_empty() {
        return Err(AppError::BadRequest("empty id value".into()));
    }
    if let Some(t) = lookup_alias(&state.db, &id_type, id_value).await? {
        return Ok(Json(t));
    }
    if let Some((_, short)) = titles::parse_cache_key_prefix(&format!("{id_type}/{id_value}"))
        && short != id_value
        && let Some(t) = lookup_alias(&state.db, &id_type, &short).await?
    {
        return Ok(Json(t));
    }
    Err(AppError::IdNotFound(format!(
        "{id_type}/{id_value} is not in the index"
    )))
}

// ---------------------------------------------------------------------------
// GET /api/admin/louk/titles/{media_type}/{tmdb_id}
// ---------------------------------------------------------------------------

pub async fn title_detail(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id)): Path<(String, String)>,
) -> Result<Json<TitleDetail>, AppError> {
    let (media_type, tmdb_id) = parse_title_path(&media_type, &tmdb_id)?;
    let t = load_title(&state.db, media_type, tmdb_id)
        .await?
        .ok_or_else(|| not_found(media_type, tmdb_id))?;
    let summary = summary_of(&state.db, &t).await?;

    let alias_rows = titles::alias_rows(&state.db, media_type, tmdb_id).await?;
    let image_rows =
        titles::images_for_title(&state.db, media_type, tmdb_id, MAX_DETAIL_IMAGES).await?;

    let mut images = Vec::with_capacity(image_rows.len());
    // (season, episode) -> cached episode stills of that episode.
    let mut cached_eps: HashMap<(i64, i64), Vec<ImageOut>> = HashMap::new();
    for row in &image_rows {
        let Some(img) = image_out(row) else { continue };
        if img.kind == "episode"
            && let (Some(s), Some(e)) = (row.season, row.episode)
        {
            cached_eps.entry((s, e)).or_default().push(img.clone());
        }
        images.push(img);
    }

    let seasons = if media_type == "tv" {
        Some(build_seasons(&state, tmdb_id, &alias_rows, cached_eps).await?)
    } else {
        None
    };

    let aliases = alias_rows
        .into_iter()
        .map(|a| AliasOut {
            id_type: a.id_type,
            id_value: a.id_value,
            season: a.season,
            episode: a.episode,
            last_requested: a.last_requested,
        })
        .collect();

    Ok(Json(TitleDetail {
        summary,
        tvdb_id: t.tvdb_id,
        first_seen: t.first_seen,
        resolved_at: t.resolved_at,
        aliases,
        images,
        seasons,
    }))
}

/// Seasons of a show that were requested (an episode alias exists) or whose
/// episode names are already stored. Episode names come from `louk_episodes`
/// (fetched from TMDB when missing or older than 7 days).
async fn build_seasons(
    state: &AppState,
    show: i64,
    aliases: &[titles::AliasInfo],
    mut cached_eps: HashMap<(i64, i64), Vec<ImageOut>>,
) -> Result<Vec<SeasonOut>, AppError> {
    let mut seasons: BTreeSet<i64> = aliases.iter().filter_map(|a| a.season).collect();
    let stored = state
        .db
        .query_all(stmt(
            "SELECT DISTINCT season FROM louk_episodes WHERE show_tmdb_id = ?",
            vec![show.into()],
        ))
        .await?;
    for r in &stored {
        let s: i64 = r.try_get("", "season")?;
        seasons.insert(s);
    }
    // Requested episodes without a cached still still get a row.
    let mut requested: BTreeSet<(i64, i64)> = aliases
        .iter()
        .filter_map(|a| Some((a.season?, a.episode?)))
        .collect();
    requested.extend(cached_eps.keys().copied());

    let mut out = Vec::with_capacity(seasons.len());
    for (i, season) in seasons.into_iter().enumerate() {
        let known = if i < MAX_SEASONS_FETCHED {
            titles::season_episodes(state, show, season).await?
        } else {
            titles::stored_episodes(&state.db, show, season).await?.0
        };
        let mut episodes: Vec<EpisodeOut> = known
            .into_iter()
            .map(|e| EpisodeOut {
                episode: e.episode,
                cached: cached_eps.remove(&(season, e.episode)).unwrap_or_default(),
                name: e.name,
                air_date: e.air_date,
                still_thumb_url: still_thumb_url(e.still_path.as_deref()),
            })
            .collect();
        for &(s, e) in requested.range((season, i64::MIN)..=(season, i64::MAX)) {
            debug_assert_eq!(s, season);
            if !episodes.iter().any(|x| x.episode == e) {
                episodes.push(EpisodeOut {
                    episode: e,
                    name: None,
                    air_date: None,
                    still_thumb_url: None,
                    cached: cached_eps.remove(&(s, e)).unwrap_or_default(),
                });
            }
        }
        episodes.sort_by_key(|e| e.episode);
        out.push(SeasonOut { season, episodes });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// POST /api/admin/louk/titles/{media_type}/{tmdb_id}/purge
// ---------------------------------------------------------------------------

const TITLE_KINDS: [ImageType; 3] = [ImageType::Poster, ImageType::Logo, ImageType::Backdrop];
const ALL_KINDS: [ImageType; 4] = [
    ImageType::Poster,
    ImageType::Logo,
    ImageType::Backdrop,
    ImageType::Episode,
];

/// Purge every cached image of a title: upstream's per-title purge for every
/// id it is known under (its aliases, plus `tmdb/movie-N`/`tmdb/series-N` and
/// its imdb id). Title-level ids are purged for poster, logo and backdrop;
/// every id also for each kind it has `image_meta` rows of (so episode ids
/// only cost a directory scan for kinds actually cached under them).
/// Index rows are kept.
pub async fn purge_title(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id)): Path<(String, String)>,
) -> Result<Json<PurgeOut>, AppError> {
    let (media_type, tmdb_id) = parse_title_path(&media_type, &tmdb_id)?;
    let t = load_title(&state.db, media_type, tmdb_id)
        .await?
        .ok_or_else(|| not_found(media_type, tmdb_id))?;

    // (id_type, id_value) -> kinds to purge, in a stable order.
    let mut targets: Vec<((String, String), [bool; 4])> = Vec::new();
    let mut add = |id_type: &str, id_value: &str, kind: ImageType| {
        let idx = titles::kind_index(kind);
        if let Some((_, kinds)) = targets
            .iter_mut()
            .find(|((t, v), _)| t == id_type && v == id_value)
        {
            kinds[idx] = true;
        } else {
            let mut kinds = [false; 4];
            kinds[idx] = true;
            targets.push(((id_type.to_owned(), id_value.to_owned()), kinds));
        }
    };
    let canonical = titles::canonical_tmdb_value(media_type, tmdb_id);
    for kind in TITLE_KINDS {
        add("tmdb", &canonical, kind);
        if let Some(imdb) = t.imdb_id.as_deref().filter(|s| !s.is_empty()) {
            add("imdb", imdb, kind);
        }
    }
    for a in titles::alias_rows(&state.db, media_type, tmdb_id).await? {
        if a.season.is_none() {
            for kind in TITLE_KINDS {
                add(&a.id_type, &a.id_value, kind);
            }
        }
    }
    for row in titles::images_for_title(&state.db, media_type, tmdb_id, i64::MAX).await? {
        if let Some(kind) = titles::image_type_of(&row.image_type) {
            add(&row.alias_type, &row.alias_value, kind);
        }
    }

    let mut out = PurgeOut {
        ok: true,
        external_cache_only: state.config.external_cache_only,
        ids: targets.len() as u64,
        files_deleted: 0,
        meta_deleted: 0,
        ratings_deleted: 0,
        errors: Vec::new(),
    };
    for ((id_type, id_value), kinds) in &targets {
        for kind in ALL_KINDS {
            if !kinds[titles::kind_index(kind)] {
                continue;
            }
            match crate::handlers::admin::purge_title(&state, kind, id_type, id_value).await {
                Ok(Json(r)) => {
                    out.files_deleted += r.files_deleted;
                    out.meta_deleted += r.meta_deleted;
                    out.ratings_deleted += r.ratings_deleted;
                }
                Err(e) => {
                    tracing::warn!(error = %e, id_type = %id_type, id_value = %id_value, "louk: title purge failed for one id");
                    out.errors
                        .push(format!("{}/{} {}: {e}", id_type, id_value, kind.label()));
                }
            }
        }
    }
    out.ok = out.errors.is_empty();
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// POST /api/admin/louk/titles/{media_type}/{tmdb_id}/refetch?kind=
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct RefetchQuery {
    pub kind: Option<String>,
}

/// Same as upstream's `POST /api/admin/{kind}/{id_type}/{id_value}/fetch`
/// (render with the global settings, served from cache when present) for the
/// title's most recently requested title-level id, falling back to
/// `tmdb/movie-N` / `tmdb/series-N`. Without `kind`: poster, logo, backdrop.
pub async fn refetch_title(
    State(state): State<Arc<AppState>>,
    Path((media_type, tmdb_id)): Path<(String, String)>,
    Query(q): Query<RefetchQuery>,
) -> Result<Json<RefetchOut>, AppError> {
    let (media_type, tmdb_id) = parse_title_path(&media_type, &tmdb_id)?;
    let kinds: Vec<ImageType> = match q.kind.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => TITLE_KINDS.to_vec(),
        Some("episode") => {
            return Err(AppError::BadRequest(
                "episode stills are re-fetched per episode: POST /api/admin/episodes/{id_type}/{id_value}/fetch".into(),
            ));
        }
        Some(k) => vec![titles::image_type_from_name(k).ok_or_else(|| {
            AppError::BadRequest("kind must be poster, logo, backdrop or episode".into())
        })?],
    };
    load_title(&state.db, media_type, tmdb_id)
        .await?
        .ok_or_else(|| not_found(media_type, tmdb_id))?;

    let (id_type, id_value) = titles::alias_rows(&state.db, media_type, tmdb_id)
        .await?
        .into_iter()
        .find(|a| a.season.is_none())
        .map(|a| (a.id_type, a.id_value))
        .unwrap_or_else(|| {
            (
                "tmdb".to_owned(),
                titles::canonical_tmdb_value(media_type, tmdb_id),
            )
        });

    let db_ref = state.db.clone();
    let settings = state
        .global_settings_cache
        .try_get_with((), async move {
            let globals = db::get_global_settings(&db_ref).await?;
            Ok::<_, AppError>(Arc::new(db::parse_global_render_settings(&globals)))
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut results = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let res = match kind {
            ImageType::Poster => {
                serve::handle_inner(&state, &id_type, &id_value, (*settings).clone(), None)
                    .await
                    .map(|_| ())
            }
            ImageType::Logo => serve::handle_logo_backdrop_inner(
                &state,
                &id_type,
                &id_value,
                &settings,
                LogoBackdropKind::Logo,
                None,
            )
            .await
            .map(|_| ()),
            ImageType::Backdrop => serve::handle_logo_backdrop_inner(
                &state,
                &id_type,
                &id_value,
                &settings,
                LogoBackdropKind::Backdrop,
                None,
            )
            .await
            .map(|_| ()),
            ImageType::Episode => Ok(()),
        };
        results.push(RefetchResult {
            kind: kind.label(),
            id_type: id_type.clone(),
            id_value: id_value.clone(),
            ok: res.is_ok(),
            error: res.err().map(|e| e.to_string()),
        });
    }
    Ok(Json(RefetchOut {
        ok: results.iter().all(|r| r.ok),
        results,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_path_parsing() {
        assert_eq!(parse_title_path("movie", "603").unwrap(), ("movie", 603));
        assert_eq!(parse_title_path("tv", "1399").unwrap(), ("tv", 1399));
        assert!(parse_title_path("episode", "1").is_err());
        assert!(parse_title_path("movie", "0").is_err());
        assert!(parse_title_path("movie", "-3").is_err());
        assert!(parse_title_path("movie", "abc").is_err());
    }

    #[test]
    fn positive_params() {
        assert_eq!(parse_positive("page", None, 1).unwrap(), 1);
        assert_eq!(parse_positive("page", Some(""), 7).unwrap(), 7);
        assert_eq!(parse_positive("page", Some("3"), 1).unwrap(), 3);
        assert!(parse_positive("page", Some("0"), 1).is_err());
        assert!(parse_positive("page", Some("x"), 1).is_err());
    }

    #[test]
    fn thumb_urls() {
        assert_eq!(
            poster_thumb_url(Some("/abc.jpg")).as_deref(),
            Some("https://image.tmdb.org/t/p/w185/abc.jpg")
        );
        assert_eq!(poster_thumb_url(Some("abc.jpg")), None);
        assert_eq!(poster_thumb_url(None), None);
        assert_eq!(
            still_thumb_url(Some("/s.jpg")).as_deref(),
            Some("https://image.tmdb.org/t/p/w300/s.jpg")
        );
    }
}
