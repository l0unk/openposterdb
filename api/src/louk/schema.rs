//! Fork-only schema. Additive: new `louk_` tables only, nothing in upstream's
//! tables changes, so the upstream binary still opens a fork-migrated DB
//! (pinned by `upstream_schema_opens_fork_db`).

use sea_orm::{ConnectionTrait, DbErr};

pub const LOUK_SCHEMA_SQL: &[&str] = &[
    // One row per title (a movie, or a show; episodes hang off the show).
    "CREATE TABLE IF NOT EXISTS louk_titles (
        media_type     TEXT NOT NULL,
        tmdb_id        INTEGER NOT NULL,
        name           TEXT,
        year           INTEGER,
        imdb_id        TEXT,
        tvdb_id        INTEGER,
        poster_path    TEXT,
        first_seen     INTEGER NOT NULL DEFAULT 0,
        last_requested INTEGER NOT NULL DEFAULT 0,
        resolved_at    INTEGER,
        PRIMARY KEY (media_type, tmdb_id)
    )",
    "CREATE INDEX IF NOT EXISTS idx_louk_titles_last ON louk_titles(last_requested DESC)",
    // Every `(id_type, id_value)` seen for a title: requested ones and the
    // cross-id copies upstream writes (`spawn_cross_id_cache`).
    "CREATE TABLE IF NOT EXISTS louk_title_aliases (
        id_type        TEXT NOT NULL,
        id_value       TEXT NOT NULL,
        media_type     TEXT NOT NULL,
        tmdb_id        INTEGER NOT NULL,
        season         INTEGER,
        episode        INTEGER,
        last_requested INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (id_type, id_value)
    )",
    "CREATE INDEX IF NOT EXISTS idx_louk_alias_title ON louk_title_aliases(media_type, tmdb_id)",
    "CREATE TABLE IF NOT EXISTS louk_episodes (
        show_tmdb_id INTEGER NOT NULL,
        season       INTEGER NOT NULL,
        episode      INTEGER NOT NULL,
        name         TEXT,
        still_path   TEXT,
        air_date     TEXT,
        fetched_at   INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (show_tmdb_id, season, episode)
    )",
];

/// Fork migrations `(sql, expected error substring)`, same contract as
/// upstream's `MIGRATIONS`. Empty for now.
pub const LOUK_MIGRATIONS: &[(&str, &str)] = &[];

/// Run the fork schema and migrations. Called right after upstream's
/// `SCHEMA_SQL`/`MIGRATIONS` loops (main.rs and tests/common).
pub async fn apply(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    for sql in LOUK_SCHEMA_SQL {
        db.execute_unprepared(sql).await?;
    }
    for (sql, expected_err) in LOUK_MIGRATIONS {
        match db.execute_unprepared(sql).await {
            Ok(_) => {}
            Err(e) if e.to_string().to_lowercase().contains(expected_err) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
