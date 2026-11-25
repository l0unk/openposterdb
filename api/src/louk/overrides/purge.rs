//! Purge every cached render of a title + kind when its override changes.
//!
//! Upstream caches renders per REQUESTED id (`imdb/tt…`, `tmdb/movie-…`,
//! `tvdb/…`, plus cross-id copies) and every settings suffix, so the purge
//! runs upstream's per-title purge (`handlers::admin::purge_title`: disk
//! files of all suffixes, `image_meta`, `available_ratings`, the
//! `image_mem_cache` / `image_inflight` entries and `id_cache`) once per id
//! the title is known under. [`alias_targets`] is the ONLY function of the
//! override feature that reads the fork's title index (`louk::titles`); a
//! port without that index replaces just this function.

use axum::Json;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::Serialize;

use super::OverrideKey;
use crate::AppState;
use crate::cache::ImageType;

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
                tracing::warn!(error = %e, id_type = %id_type, id_value = %id_value, "louk override: purge failed for one id");
                out.errors.push(format!("{id_type}/{id_value}: {e}"));
            }
        }
        out.ids.push(format!("{id_type}/{id_value}"));
    }
    out
}

/// Every `(id_type, id_value)` renders of `key`'s title + kind may be cached
/// under: the canonical TMDB id, the imdb/tvdb ids the index knows, every
/// alias (requested ids and cross-id copies) and every alias that has
/// `image_meta` rows of this kind. Stills: only ids of that season/episode.
pub async fn alias_targets(state: &AppState, key: &OverrideKey) -> Vec<(String, String)> {
    use crate::louk::titles;

    // Ids requested in the last flush interval are still in the pending maps.
    if let Err(e) = titles::flush_once(state).await {
        tracing::warn!(error = %e, "louk override: title index flush before purge failed");
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |t: &str, v: String| {
        if !v.is_empty() && !out.iter().any(|(a, b)| a == t && *b == v) {
            out.push((t.to_owned(), v));
        }
    };
    let tmdb_id = key.tmdb_id as i64;
    let episode = key.kind == ImageType::Episode;
    let ep_suffix = format!("-S{}E{}", key.season, key.episode);

    let (imdb_id, tvdb_id) = title_ids(state, key.media_type, tmdb_id).await;
    if episode {
        push("tmdb", format!("episode-{}{ep_suffix}", key.tmdb_id));
        if let Some(imdb) = &imdb_id {
            push("imdb", format!("episode-{imdb}{ep_suffix}"));
        }
        if let Some(tvdb) = tvdb_id {
            push("tvdb", format!("episode-{tvdb}{ep_suffix}"));
        }
    } else {
        push(
            "tmdb",
            titles::canonical_tmdb_value(key.media_type, tmdb_id),
        );
        if let Some(imdb) = imdb_id {
            push("imdb", imdb);
        }
        if let Some(tvdb) = tvdb_id {
            push("tvdb", tvdb.to_string());
        }
    }

    let same_episode = |s: Option<i64>, e: Option<i64>| {
        s == Some(i64::from(key.season)) && e == Some(i64::from(key.episode))
    };
    match titles::alias_rows(&state.db, key.media_type, tmdb_id).await {
        Ok(rows) => {
            for a in rows {
                let take = if episode {
                    same_episode(a.season, a.episode)
                } else {
                    a.season.is_none()
                };
                if take {
                    push(&a.id_type, a.id_value);
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "louk override: reading title aliases failed"),
    }
    // Renders of this kind under any alias, e.g. a show poster requested
    // with an episode id (upstream uplifts it and caches it under that id).
    match titles::images_for_title(&state.db, key.media_type, tmdb_id, i64::MAX).await {
        Ok(rows) => {
            for r in rows {
                if titles::image_type_of(&r.image_type) != Some(key.kind) {
                    continue;
                }
                if episode && !same_episode(r.season, r.episode) {
                    continue;
                }
                push(&r.alias_type, r.alias_value);
            }
        }
        Err(e) => tracing::warn!(error = %e, "louk override: reading title images failed"),
    }
    out
}

async fn title_ids(
    state: &AppState,
    media_type: &str,
    tmdb_id: i64,
) -> (Option<String>, Option<i64>) {
    let row = state
        .db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "SELECT imdb_id, tvdb_id FROM louk_titles WHERE media_type = ? AND tmdb_id = ?",
            vec![media_type.into(), tmdb_id.into()],
        ))
        .await;
    match row {
        Ok(Some(r)) => (
            r.try_get::<Option<String>>("", "imdb_id")
                .ok()
                .flatten()
                .filter(|s| !s.is_empty()),
            r.try_get::<Option<i64>>("", "tvdb_id").ok().flatten(),
        ),
        Ok(None) => (None, None),
        Err(e) => {
            tracing::warn!(error = %e, "louk override: reading the title row failed");
            (None, None)
        }
    }
}
