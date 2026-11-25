//! Fork: stale-while-revalidate for a stale ratings index
//! (`LOUK_RATINGS_SWR`, off by default; `docs/louk/speed.md`).
//!
//! Upstream rebuilds an image's cache key from the `available_ratings` row
//! (which rating sources a title has). Once that row is older than the
//! title's stale interval, the fast path gives up and the request waits for
//! the slow path: id resolve plus a full ratings fetch (MDBList: up to
//! seconds), and then usually serves the old cached image anyway while a
//! background re-render runs.
//!
//! With the setting on, a stale row is still used to build the key. On a
//! cache hit the old image is served at once, and the request's own slow path
//! (the unchanged upstream code) runs detached in the background: one
//! revalidation per cache key at a time (`refresh_locks`), at most
//! [`MAX_CONCURRENT`] at once (extra ones are skipped, the next request tries
//! again), and inside a task-local scope that makes the fast path behave
//! exactly as upstream, so it fetches ratings once, upserts the row and
//! re-renders as today. Ratings calls: the same fetch a request makes today,
//! moved off the request path; never an extra one. With `RATINGS_DISABLED`
//! nothing here runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use sea_orm::EntityTrait;
use tokio::sync::Semaphore;

use crate::AppState;
use crate::entity::available_ratings;
use crate::image::serve::{self, LogoBackdropKind};
use crate::services::db::{ImageSize, RenderSettings};

/// Revalidations allowed to run at once (they also queue for the shared
/// render permits like any request).
pub const MAX_CONCURRENT: usize = 2;

/// Lives in `LoukState`.
pub struct SwrState {
    permits: Arc<Semaphore>,
    /// Revalidations started (tests and diagnostics).
    pub started: AtomicU64,
    /// Replays dropped because the index was fresh again by the time they ran.
    pub skipped_fresh: AtomicU64,
    /// Stale hits that found every revalidation slot busy (served stale, no
    /// revalidation; a later request tries again).
    pub skipped_busy: AtomicU64,
}

impl SwrState {
    pub fn new() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            started: AtomicU64::new(0),
            skipped_fresh: AtomicU64::new(0),
            skipped_busy: AtomicU64::new(0),
        }
    }
}

impl Default for SwrState {
    fn default() -> Self {
        Self::new()
    }
}

tokio::task_local! {
    static REVALIDATING: ();
}

/// True inside a revalidation: the fast path must behave exactly as upstream.
pub fn in_revalidation() -> bool {
    REVALIDATING.try_with(|_| ()).is_ok()
}

/// Whether the stale-row lookup applies at all.
pub fn applies(
    louk: &crate::louk::config::LoukConfig,
    available: &Option<String>,
    revalidating: bool,
) -> bool {
    louk.ratings_swr && !louk.ratings_disabled && available.is_none() && !revalidating
}

/// Fast-path hook, right after upstream's `available_ratings` lookup: when
/// that found nothing usable and the setting is on, fall back to the stored
/// row whatever its age. Returns the sources to build the key with, and
/// whether they came from a stale row.
pub async fn fast_path_sources(
    state: &AppState,
    id_key: &str,
    available: Option<String>,
) -> (Option<String>, bool) {
    if !applies(&state.config.louk, &available, in_revalidation()) {
        return (available, false);
    }
    match available_ratings::Entity::find_by_id(id_key.to_owned())
        .one(&state.db)
        .await
    {
        Ok(Some(row)) => (Some(row.sources), true),
        _ => (None, false),
    }
}

/// The request to replay in the background, as the handler received it.
pub enum Replay {
    Poster {
        id_type: String,
        id_value: String,
        settings: RenderSettings,
        image_size: Option<ImageSize>,
    },
    Episode {
        id_type: String,
        id_value: String,
        settings: RenderSettings,
        image_size: Option<ImageSize>,
    },
    LogoBackdrop {
        id_type: String,
        id_value: String,
        settings: RenderSettings,
        kind: LogoBackdropKind,
        image_size: Option<ImageSize>,
    },
}

/// Fast-path hook on a cache hit built from a stale row: start the request's
/// slow path in the background, deduplicated per cache key and bounded.
pub fn revalidate(state: &AppState, id_key: &str, cache_key: &str, replay: Replay) {
    let swr = &state.louk.swr;
    let lock_key = format!("louk-swr:{cache_key}");
    // Cheap early exit before taking a slot: this key is already replaying.
    if state.refresh_locks.contains_key(&lock_key) {
        return;
    }
    let Ok(permit) = swr.permits.clone().try_acquire_owned() else {
        swr.skipped_busy.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let entry = state.refresh_locks.entry(lock_key.clone()).or_insert(());
    if !entry.is_fresh() {
        return; // one is already running for this key; the permit drops here
    }
    let state = state.clone();
    let key = cache_key.to_owned();
    let id_key = id_key.to_owned();
    tokio::spawn(REVALIDATING.scope((), async move {
        // A request that read the stale row just before an earlier replay
        // refreshed it lands here after that replay released the lock: the
        // index is fresh again, nothing to do.
        let fresh = crate::cache::read_available_ratings(
            &state.db,
            &id_key,
            state.config.ratings_min_stale_secs,
            state.config.ratings_max_age_secs,
        )
        .await
        .is_some();
        if fresh {
            state.louk.swr.skipped_fresh.fetch_add(1, Ordering::Relaxed);
            state.refresh_locks.invalidate(&lock_key);
            return;
        }
        state.louk.swr.started.fetch_add(1, Ordering::Relaxed);
        tracing::info!(key = %key, "louk-swr: served stale index, revalidating");
        let start = Instant::now();
        let result = match replay {
            Replay::Poster {
                id_type,
                id_value,
                settings,
                image_size,
            } => serve::handle_inner(&state, &id_type, &id_value, settings, image_size)
                .await
                .map(|_| ()),
            Replay::Episode {
                id_type,
                id_value,
                settings,
                image_size,
            } => serve::handle_episode_inner(&state, &id_type, &id_value, settings, image_size)
                .await
                .map(|_| ()),
            Replay::LogoBackdrop {
                id_type,
                id_value,
                settings,
                kind,
                image_size,
            } => serve::handle_logo_backdrop_inner(
                &state, &id_type, &id_value, &settings, kind, image_size,
            )
            .await
            .map(|_| ()),
        };
        let elapsed_ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(()) => tracing::info!(key = %key, elapsed_ms, "louk-swr: revalidated"),
            Err(e) => {
                tracing::warn!(key = %key, elapsed_ms, error = %e, "louk-swr: revalidation failed")
            }
        }
        state.refresh_locks.invalidate(&lock_key);
        drop(permit);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::louk::config::LoukConfig;

    #[test]
    fn applies_decision_table() {
        let on = LoukConfig {
            ratings_swr: true,
            ..LoukConfig::default()
        };
        let off = LoukConfig::default();
        let disabled = LoukConfig {
            ratings_swr: true,
            ratings_disabled: true,
            ..LoukConfig::default()
        };
        let none: Option<String> = None;
        let fresh = Some("ilm".to_owned());
        assert!(applies(&on, &none, false), "on + stale/missing index");
        assert!(!applies(&on, &fresh, false), "fresh index: upstream path");
        assert!(
            !applies(&on, &none, true),
            "inside a revalidation: upstream path"
        );
        assert!(!applies(&off, &none, false), "setting off");
        assert!(!applies(&disabled, &none, false), "RATINGS_DISABLED");
    }

    #[tokio::test]
    async fn revalidation_scope_is_visible_only_inside() {
        assert!(!in_revalidation());
        REVALIDATING
            .scope((), async { assert!(in_revalidation()) })
            .await;
        assert!(!in_revalidation());
    }
}
