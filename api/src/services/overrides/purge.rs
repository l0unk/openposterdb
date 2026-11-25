//! Purge every cached render of a title + kind when its override changes.
//!
//! Renders are cached per REQUESTED id (`imdb/tt…`, `tmdb/movie-…`,
//! `tvdb/…`, plus cross-id copies) and every settings suffix, so the purge
//! runs the admin per-title purge (`handlers::admin::purge_title`: disk
//! files of all suffixes, `image_meta`, `available_ratings`, the
//! `image_mem_cache` / `image_inflight` entries and `id_cache`) once per id
//! the title is known under ([`alias_targets`]).

use std::time::Duration;

use axum::Json;
use serde::Serialize;

use super::{OverrideKey, apply};
use crate::AppState;
use crate::cache::ImageType;
use crate::id::{self, IdType};

#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct PurgeOut {
    /// Every `{id_type}/{id_value}` purged.
    pub ids: Vec<String>,
    pub files_deleted: u64,
    pub meta_deleted: u64,
    /// `"{id_type}/{id_value}: message"` for ids whose purge failed.
    pub errors: Vec<String>,
    /// With `EXTERNAL_CACHE_ONLY` / CDN copies: those cannot be purged here.
    pub external_cache_only: bool,
}

/// Purge the cached renders an override of `key` affects. Never fails:
/// errors per id are collected (and logged).
pub async fn purge_for(state: &AppState, key: &OverrideKey) -> PurgeOut {
    let mut out = PurgeOut {
        external_cache_only: state.config.external_cache_only,
        ..PurgeOut::default()
    };
    for (id_type, id_value) in alias_targets(state, key).await {
        match crate::handlers::admin::purge_title(state, key.kind, &id_type, &id_value).await {
            Ok(Json(r)) => {
                out.files_deleted += r.files_deleted;
                out.meta_deleted += r.meta_deleted;
            }
            Err(e) => {
                tracing::warn!(error = %e, id_type = %id_type, id_value = %id_value, "override: purge failed for one id");
                out.errors.push(format!("{id_type}/{id_value}: {e}"));
            }
        }
        out.ids.push(format!("{id_type}/{id_value}"));
    }
    out
}

/// Re-purge this long after an override is saved or cleared: longer than
/// a render plus its 30 s `image_inflight` retention and the cross-id copies
/// it spawns, so a render that was in flight during the change cannot leave
/// the old art cached.
pub const REPURGE_AFTER_CHANGE: Duration = Duration::from_secs(60);
/// Re-purge this long after an override's image failed to load (the
/// automatic art got cached meanwhile); once per override version.
pub const REPURGE_AFTER_SOURCE_FAILURE: Duration = Duration::from_secs(600);
/// Most delayed re-purges waiting at once; beyond this, new ones are dropped
/// (logged) so a storm of saves or failures cannot queue unbounded work.
pub const MAX_PENDING_REPURGES: usize = 256;
/// Most remembered source failures (cleared when full).
pub const MAX_SOURCE_FAILURES: usize = 1024;

/// Identity of a key in the re-purge bookkeeping.
pub fn job_key(key: &OverrideKey) -> String {
    format!(
        "{}/{}/{}/{}/{}/{}",
        key.media_type,
        key.tmdb_id,
        key.kind.label(),
        key.season,
        key.episode,
        key.lang
    )
}

/// When the delayed re-purge of `key` will run, if one is pending.
pub fn pending_repurge(state: &AppState, key: &OverrideKey) -> Option<tokio::time::Instant> {
    super::store(state).repurges.get(&job_key(key)).map(|d| *d)
}

/// Number of pending delayed re-purges.
pub fn pending_repurges(state: &AppState) -> usize {
    super::store(state).repurges.len()
}

/// Purge `key` again after `delay`. One job per key: scheduling a key that
/// is already pending only moves its run time later (never earlier). Returns
/// false when the job was dropped because [`MAX_PENDING_REPURGES`] are
/// pending.
pub fn schedule_repurge(state: &AppState, key: &OverrideKey, delay: Duration) -> bool {
    let jobs = &super::store(state).repurges;
    let k = job_key(key);
    let due = tokio::time::Instant::now() + delay;
    if let Some(mut existing) = jobs.get_mut(&k) {
        if *existing < due {
            *existing = due;
        }
        return true;
    }
    if jobs.len() >= MAX_PENDING_REPURGES {
        tracing::warn!(key = %k, "override: too many pending re-purges; skipping this one");
        return false;
    }
    if jobs.insert(k.clone(), due).is_some() {
        // Raced with another scheduler: that job runs (at least as late).
        return true;
    }
    let st = state.clone();
    let key = key.clone();
    tokio::spawn(async move {
        let job = {
            let (st, key, k) = (st.clone(), key.clone(), k.clone());
            tokio::spawn(async move { run_repurge(&st, &key, &k).await })
        };
        if job.await.is_err() {
            // A panic must not leave the key blocked.
            super::store(&st).repurges.remove(&k);
        }
    });
    true
}

async fn run_repurge(state: &AppState, key: &OverrideKey, k: &str) {
    loop {
        let Some(due) = pending_repurge(state, key) else {
            return;
        };
        if tokio::time::Instant::now() < due {
            tokio::time::sleep_until(due).await;
            continue;
        }
        let started = tokio::time::Instant::now();
        let out = purge_for(state, key).await;
        tracing::info!(key = %k, ids = ?out.ids, files_deleted = out.files_deleted, "override: delayed re-purge");
        // Done, unless someone scheduled it again while the purge ran.
        if super::store(state)
            .repurges
            .remove_if(k, |_, d| *d <= started)
            .is_some()
        {
            return;
        }
    }
}

/// An override's image could not be fetched or decoded, so the automatic
/// art was rendered (and cached) instead: purge again later, once per
/// override version (`updated_at`), so a lasting failure is not retried
/// forever.
pub fn note_source_failure(state: &AppState, key: &OverrideKey, updated_at: i64) {
    let failures = &super::store(state).source_failures;
    let tag = format!("{}#{updated_at}", job_key(key));
    if failures.contains_key(&tag) {
        return;
    }
    if failures.len() >= MAX_SOURCE_FAILURES {
        failures.clear();
    }
    if failures.insert(tag, ()).is_none() {
        schedule_repurge(state, key, REPURGE_AFTER_SOURCE_FAILURE);
    }
}

/// Whether a failure of this override version was already noted.
pub fn source_failure_noted(state: &AppState, key: &OverrideKey, updated_at: i64) -> bool {
    super::store(state)
        .source_failures
        .contains_key(&format!("{}#{updated_at}", job_key(key)))
}

/// Every `(id_type, id_value)` renders of `key`'s title + kind may be cached
/// under. Renders are cached per REQUESTED id (plus cross-id copies), so
/// this collects:
/// - the canonical TMDB id (`movie-{id}` / `series-{id}`; for stills
///   `episode-{id}-S{s}E{e}`);
/// - the title's imdb and tvdb ids from `id::resolve` of the canonical id
///   (through `id_cache`; skipped with a warning when TMDB cannot be reached),
///   in their `episode-{id}-S{s}E{e}` form for stills;
/// - every id in `id_cache` that resolves to this title (and, for stills, this
///   episode): the ids it was requested under recently, e.g. an episode's own
///   imdb id or a lowercase `s1e2` form;
/// - for posters, logos and backdrops of a show: every `episode-{id}-…` id
///   with a cached render of this kind in `image_meta` (an episode id
///   requested on a show route is uplifted to the show and cached under the
///   episode id).
pub async fn alias_targets(state: &AppState, key: &OverrideKey) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |t: &str, v: String| {
        if !v.is_empty() && !out.iter().any(|(a, b)| a == t && *b == v) {
            out.push((t.to_owned(), v));
        }
    };
    let episode = key.kind == ImageType::Episode;
    let ep_suffix = format!("-S{}E{}", key.season, key.episode);
    let canonical = if key.media_type == "movie" {
        format!("movie-{}", key.tmdb_id)
    } else {
        format!("series-{}", key.tmdb_id)
    };

    let (imdb_id, tvdb_id) = match id::resolve(
        IdType::Tmdb,
        &canonical,
        &state.tmdb,
        &state.id_cache,
    )
    .await
    {
        Ok(r) => (r.imdb_id.filter(|s| !s.is_empty()), r.tvdb_id),
        Err(e) => {
            tracing::warn!(error = %e, id = %canonical, "override: resolving the title's imdb/tvdb ids failed; purging the ids known locally");
            (None, None)
        }
    };
    let tmdb_base = key.tmdb_id.to_string();
    let tvdb_base = tvdb_id.map(|v| v.to_string());
    if episode {
        push("tmdb", format!("episode-{tmdb_base}{ep_suffix}"));
        if let Some(imdb) = &imdb_id {
            push("imdb", format!("episode-{imdb}{ep_suffix}"));
        }
        if let Some(tvdb) = &tvdb_base {
            push("tvdb", format!("episode-{tvdb}{ep_suffix}"));
        }
    } else {
        push("tmdb", canonical.clone());
        if let Some(imdb) = &imdb_id {
            push("imdb", imdb.clone());
        }
        if let Some(tvdb) = &tvdb_base {
            push("tvdb", tvdb.clone());
        }
    }

    // Ids requested recently that resolve to this title.
    let want = Some((key.media_type, key.tmdb_id, key.season, key.episode));
    for (k, resolved) in state.id_cache.iter() {
        if apply::title_of(&resolved, key.kind) != want {
            continue;
        }
        if let Some((t, v)) = k.split_once('/') {
            push(t, v.to_owned());
        }
    }

    // Show art cached under episode ids.
    if !episode && key.media_type == "tv" {
        let bases = [
            ("tmdb", Some(tmdb_base)),
            ("imdb", imdb_id),
            ("tvdb", tvdb_base),
        ];
        for (t, base) in bases {
            let Some(base) = base else { continue };
            for v in episode_ids_with_renders(state, key.kind, t, &base).await {
                push(t, v);
            }
        }
    }
    out
}

/// Distinct `episode-{base}-…` id values of `id_type` with a cached render of
/// `kind` (from `image_meta`).
async fn episode_ids_with_renders(
    state: &AppState,
    kind: ImageType,
    id_type: &str,
    base: &str,
) -> Vec<String> {
    use crate::entity::image_meta;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let prefix = format!("{id_type}/episode-{base}-");
    let rows = image_meta::Entity::find()
        .filter(image_meta::Column::ImageType.eq(kind.db_value()))
        .filter(image_meta::Column::CacheKey.starts_with(prefix.as_str()))
        .all(&state.db)
        .await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "override: reading cached episode-id renders failed");
            return Vec::new();
        }
    };
    let mut out: Vec<String> = Vec::new();
    for r in rows {
        let Some(value) = r.cache_key.strip_prefix(&format!("{id_type}/")) else {
            continue;
        };
        // The id value ends where the variant/settings suffix starts.
        let id_value = value.split(['_', '@']).next().unwrap_or("");
        if !id_value.is_empty() && !out.iter().any(|v| v == id_value) {
            out.push(id_value.to_owned());
        }
    }
    out
}
