//! Fork runtime state: the one `AppState.louk` field.

use std::sync::atomic::AtomicU64;
use std::time::Instant;

use dashmap::DashMap;

/// Caps on the pending maps the request-path hooks fill. Beyond these, new
/// entries are dropped (and counted) until the next flush empties the maps,
/// so a crawl cannot grow memory without limit.
pub const MAX_PENDING_TOUCHES: usize = 20_000;
pub const MAX_PENDING_RESOLVED: usize = 5_000;

/// What a resolution told us about one `(id_type, id_value)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasRow {
    /// `"movie"` or `"tv"` (episodes belong to their show).
    pub media_type: &'static str,
    pub tmdb_id: i64,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    /// Title-level facts; only set when the resolution was for the title
    /// itself (an episode's imdb/tvdb ids and still are not the show's).
    pub imdb_id: Option<String>,
    pub tvdb_id: Option<i64>,
    pub poster_path: Option<String>,
    pub year: Option<i64>,
    /// Unix seconds when the hook saw it.
    pub seen_at: i64,
}

pub struct LoukState {
    /// `(id_type, id_value)` → last request time (unix seconds).
    pub touches: DashMap<(String, String), i64>,
    /// `(id_type, id_value)` → what resolution found, waiting for a flush.
    pub resolved: DashMap<(String, String), AliasRow>,
    /// Wakes the details worker when a flush created titles without details.
    pub title_queue: tokio::sync::Notify,
    /// Serialises every fork TMDB call (workers and the title page) and
    /// spaces them at least `TMDB_GAP` apart.
    pub tmdb_gate: tokio::sync::Mutex<Option<Instant>>,
    /// Hook entries dropped because a pending map was full.
    pub dropped: AtomicU64,
    /// Shows whose season episode names are being fetched in the background
    /// (a title page ran out of its fetch budget).
    pub season_jobs: DashMap<i64, ()>,
    /// Per-title art overrides (in-memory mirror of `louk_overrides`).
    pub overrides: crate::louk::overrides::store::OverrideStore,
}

impl LoukState {
    pub fn new() -> Self {
        Self {
            touches: DashMap::new(),
            resolved: DashMap::new(),
            title_queue: tokio::sync::Notify::new(),
            tmdb_gate: tokio::sync::Mutex::new(None),
            dropped: AtomicU64::new(0),
            season_jobs: DashMap::new(),
            overrides: crate::louk::overrides::store::OverrideStore::new(),
        }
    }
}

impl Default for LoukState {
    fn default() -> Self {
        Self::new()
    }
}
