//! Fork: title index and the title dashboard admin API
//! (`api/src/louk/{titles,handlers}.rs`). No test here reaches TMDB: the
//! workers are not started in tests, movies need no season data, and the tv
//! detail test seeds fresh `louk_episodes` rows.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ConnectionTrait, DatabaseConnection, SqlxSqliteConnector, Statement};
use tower::ServiceExt;

use openposterdb_api::AppState;
use openposterdb_api::cache::{self, ImageType};
use openposterdb_api::id::{EpisodeInfo, MediaType, ResolvedId};
use openposterdb_api::louk::state::MAX_PENDING_TOUCHES;
use openposterdb_api::louk::titles;
use openposterdb_api::services::db;

// --- helpers --------------------------------------------------------------

async fn exec(state: &AppState, sql: &str) {
    state.db.execute_unprepared(sql).await.expect(sql);
}

async fn scalar_i64(state: &AppState, sql: &str) -> Option<i64> {
    let row = state
        .db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            sql.to_owned(),
        ))
        .await
        .expect(sql)?;
    row.try_get::<Option<i64>>("", "v").expect("column v")
}

async fn count(state: &AppState, sql: &str) -> i64 {
    scalar_i64(state, sql).await.unwrap_or(0)
}

fn movie(imdb: &str, tmdb_id: u64, date: &str) -> ResolvedId {
    ResolvedId {
        imdb_id: Some(imdb.to_owned()),
        tmdb_id,
        tvdb_id: None,
        media_type: MediaType::Movie,
        poster_path: Some("/poster.jpg".to_owned()),
        release_date: Some(date.to_owned()),
        episode: None,
    }
}

async fn seed_title(state: &AppState, media_type: &str, tmdb_id: i64, name: &str, last: i64) {
    exec(
        state,
        &format!(
            "INSERT INTO louk_titles (media_type, tmdb_id, name, year, first_seen, last_requested, resolved_at)
             VALUES ('{media_type}', {tmdb_id}, '{name}', 2000, {last}, {last}, {last})"
        ),
    )
    .await;
}

async fn seed_alias(
    state: &AppState,
    id_type: &str,
    id_value: &str,
    media_type: &str,
    tmdb_id: i64,
    se: Option<(i64, i64)>,
) {
    let (s, e) = match se {
        Some((s, e)) => (s.to_string(), e.to_string()),
        None => ("NULL".to_owned(), "NULL".to_owned()),
    };
    exec(
        state,
        &format!(
            "INSERT INTO louk_title_aliases (id_type, id_value, media_type, tmdb_id, season, episode, last_requested)
             VALUES ('{id_type}', '{id_value}', '{media_type}', {tmdb_id}, {s}, {e}, 100)"
        ),
    )
    .await;
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn names(json: &serde_json::Value) -> Vec<String> {
    json["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["name"].as_str().unwrap_or("").to_owned())
        .collect()
}

// --- hooks + flush ----------------------------------------------------------

#[tokio::test]
async fn touch_then_flush_updates_last_requested() {
    let (_app, state) = common::setup_test_app().await;
    seed_title(&state, "movie", 9_900_001, "Touched", 100).await;
    seed_alias(&state, "imdb", "tt9900001", "movie", 9_900_001, None).await;

    titles::touch(&state, "imdb", "tt9900001");
    // Unknown ids are ignored at flush time, not inserted.
    titles::touch(&state, "imdb", "tt9900999");
    assert_eq!(state.louk.touches.len(), 2);
    titles::flush_once(&state).await.unwrap();
    assert!(state.louk.touches.is_empty());

    let alias = count(
        &state,
        "SELECT last_requested AS v FROM louk_title_aliases WHERE id_value = 'tt9900001'",
    )
    .await;
    let title = count(
        &state,
        "SELECT last_requested AS v FROM louk_titles WHERE tmdb_id = 9900001",
    )
    .await;
    assert!(alias > 100, "alias last_requested {alias}");
    assert!(title > 100, "title last_requested {title}");
    assert_eq!(
        count(
            &state,
            "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_value = 'tt9900999'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn touch_map_is_bounded() {
    let (_app, state) = common::setup_test_app().await;
    for i in 0..MAX_PENDING_TOUCHES {
        titles::touch(&state, "imdb", &format!("tt{i}"));
    }
    assert_eq!(state.louk.touches.len(), MAX_PENDING_TOUCHES);
    titles::touch(&state, "imdb", "tt-one-too-many");
    assert_eq!(state.louk.touches.len(), MAX_PENDING_TOUCHES);
    assert_eq!(
        state
            .louk
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    // A key that is already pending is still refreshed, not dropped.
    titles::touch(&state, "imdb", "tt0");
    assert_eq!(
        state
            .louk
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    // Oversized values are ignored.
    titles::touch(&state, "imdb", &"x".repeat(500));
    assert_eq!(state.louk.touches.len(), MAX_PENDING_TOUCHES);
}

#[tokio::test]
async fn note_resolved_creates_title_and_alias() {
    let (_app, state) = common::setup_test_app().await;
    titles::note_resolved(
        &state,
        "imdb",
        "tt9900133",
        &movie("tt9900133", 9_900_603, "1999-03-30"),
    );
    // requested id + the tmdb cross id upstream also caches under
    assert_eq!(state.louk.resolved.len(), 2);
    titles::flush_once(&state).await.unwrap();
    assert!(state.louk.resolved.is_empty());

    assert_eq!(
        count(
            &state,
            "SELECT year AS v FROM louk_titles WHERE media_type = 'movie' AND tmdb_id = 9900603"
        )
        .await,
        1999
    );
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_titles WHERE tmdb_id = 9900603 AND imdb_id = 'tt9900133' AND resolved_at IS NULL AND poster_path = '/poster.jpg'").await,
        1
    );
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE media_type = 'movie' AND tmdb_id = 9900603").await,
        2
    );
    let aliases = titles::aliases_for_title(&state.db, "movie", 9_900_603)
        .await
        .unwrap();
    assert!(aliases.contains(&("imdb".to_owned(), "tt9900133".to_owned())));
    assert!(aliases.contains(&("tmdb".to_owned(), "movie-9900603".to_owned())));

    // An episode id maps to its show, with season/episode, and does not
    // overwrite the show's own facts.
    let ep = ResolvedId {
        imdb_id: Some("tt9900155".to_owned()),
        tmdb_id: 9_901_399,
        tvdb_id: Some(9_912_345),
        media_type: MediaType::Episode,
        poster_path: Some("/still.jpg".to_owned()),
        release_date: Some("2011-04-24".to_owned()),
        episode: Some(EpisodeInfo {
            show_tmdb_id: 9_901_399,
            season_number: 1,
            episode_number: 2,
            still_path: None,
        }),
    };
    titles::note_resolved(&state, "imdb", "episode-tt9900947-S1E2", &ep);
    titles::flush_once(&state).await.unwrap();
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_value = 'episode-tt9900947-S1E2' AND media_type = 'tv' AND tmdb_id = 9901399 AND season = 1 AND episode = 2").await,
        1
    );
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_type = 'tmdb' AND id_value = 'episode-9901399-S1E2' AND season = 1").await,
        1
    );
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_titles WHERE media_type = 'tv' AND tmdb_id = 9901399 AND imdb_id IS NULL AND poster_path IS NULL").await,
        1
    );
}

// --- REST -------------------------------------------------------------------

#[tokio::test]
async fn titles_list_search_and_filter() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    seed_title(&state, "movie", 603, "The Matrix", 300).await;
    seed_title(&state, "movie", 550, "Fight Club", 100).await;
    seed_title(&state, "tv", 1399, "Game of Thrones", 200).await;

    let (status, json) = send(&app, "GET", "/api/admin/louk/titles", Some(&token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["total"], 3);
    assert_eq!(json["page"], 1);
    assert_eq!(json["per_page"], 48);
    assert_eq!(
        names(&json),
        ["The Matrix", "Game of Thrones", "Fight Club"]
    );
    let first = &json["items"][0];
    assert_eq!(first["media_type"], "movie");
    assert_eq!(first["tmdb_id"], 603);
    assert_eq!(first["year"], 2000);
    assert_eq!(first["last_requested"], 300);
    assert!(first["imdb_id"].is_null());
    assert!(first["poster_thumb_url"].is_null());
    assert_eq!(
        first["image_counts"],
        serde_json::json!({"poster": 0, "logo": 0, "backdrop": 0, "episode": 0})
    );

    let (_, json) = send(&app, "GET", "/api/admin/louk/titles?q=matrix", Some(&token)).await;
    assert_eq!(json["total"], 1);
    assert_eq!(names(&json), ["The Matrix"]);

    // `_` and `%` are literal in q
    let (_, json) = send(&app, "GET", "/api/admin/louk/titles?q=%25", Some(&token)).await;
    assert_eq!(json["total"], 0);

    let (_, json) = send(&app, "GET", "/api/admin/louk/titles?type=tv", Some(&token)).await;
    assert_eq!(names(&json), ["Game of Thrones"]);

    let (_, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles?sort=name",
        Some(&token),
    )
    .await;
    assert_eq!(
        names(&json),
        ["Fight Club", "Game of Thrones", "The Matrix"]
    );

    let (_, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles?sort=last&per_page=2&page=2",
        Some(&token),
    )
    .await;
    assert_eq!(json["total"], 3);
    assert_eq!(names(&json), ["Fight Club"]);

    for bad in ["type=episode", "sort=year", "page=0", "per_page=x"] {
        let (status, json) = send(
            &app,
            "GET",
            &format!("/api/admin/louk/titles?{bad}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        assert!(json["error"].is_string(), "{bad}");
    }
}

#[tokio::test]
async fn title_detail_lists_images_by_alias() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    seed_title(&state, "movie", 9_900_011, "One", 100).await;
    seed_alias(&state, "imdb", "tt9900011", "movie", 9_900_011, None).await;
    seed_alias(&state, "tmdb", "movie-9900011", "movie", 9_900_011, None).await;
    cache::upsert_meta_db(
        &state.db,
        "imdb/tt9900011@ir.pbc.zm",
        None,
        ImageType::Poster,
    )
    .await
    .unwrap();
    cache::upsert_meta_db(
        &state.db,
        "tmdb/movie-9900011_b_t@.zl",
        None,
        ImageType::Backdrop,
    )
    .await
    .unwrap();
    // decoy: another title whose id starts with this one's
    cache::upsert_meta_db(&state.db, "imdb/tt99000110@ir.zm", None, ImageType::Poster)
        .await
        .unwrap();

    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/movie/9900011",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["name"], "One");
    assert!(json.get("seasons").is_none(), "movies have no seasons");
    assert_eq!(json["aliases"].as_array().unwrap().len(), 2);
    let images = json["images"].as_array().unwrap();
    let mut values: Vec<&str> = images
        .iter()
        .map(|i| i["cache_value"].as_str().unwrap())
        .collect();
    values.sort();
    assert_eq!(values, ["movie-9900011_b_t@.zl", "tt9900011@ir.pbc.zm"]);
    let poster = images.iter().find(|i| i["kind"] == "poster").unwrap();
    assert_eq!(poster["id_type"], "imdb");
    assert_eq!(
        poster["admin_image_url"],
        "/api/admin/posters/imdb/tt9900011@ir.pbc.zm/image"
    );
    let backdrop = images.iter().find(|i| i["kind"] == "backdrop").unwrap();
    assert_eq!(
        backdrop["admin_image_url"],
        "/api/admin/backdrops/tmdb/movie-9900011_b_t@.zl"
    );
    assert_eq!(
        json["image_counts"],
        serde_json::json!({"poster": 1, "logo": 0, "backdrop": 1, "episode": 0})
    );

    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/movie/9900012",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json["error"].is_string());
    let (status, _) = send(&app, "GET", "/api/admin/louk/titles/film/1", Some(&token)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/movie/abc",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn title_detail_groups_episodes_by_season() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let now = chrono::Utc::now().timestamp();
    seed_title(&state, "tv", 9_901_399, "Show", 100).await;
    seed_alias(&state, "imdb", "tt9900944", "tv", 9_901_399, None).await;
    seed_alias(
        &state,
        "imdb",
        "episode-tt9900944-S1E2",
        "tv",
        9_901_399,
        Some((1, 2)),
    )
    .await;
    exec(
        &state,
        &format!(
            "INSERT INTO louk_episodes (show_tmdb_id, season, episode, name, still_path, air_date, fetched_at) VALUES
             (9901399, 1, 1, 'Pilot', '/s1.jpg', '2011-04-17', {now}),
             (9901399, 1, 2, 'Second', NULL, '2011-04-24', {now})"
        ),
    )
    .await;
    cache::upsert_meta_db(
        &state.db,
        "imdb/episode-tt9900944-S1E2@ir.zm",
        None,
        ImageType::Episode,
    )
    .await
    .unwrap();
    cache::upsert_meta_db(&state.db, "imdb/tt9900944@ir.zm", None, ImageType::Poster)
        .await
        .unwrap();

    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/tv/9901399",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["images"].as_array().unwrap().len(), 2);
    let seasons = json["seasons"].as_array().unwrap();
    assert_eq!(seasons.len(), 1);
    assert_eq!(seasons[0]["season"], 1);
    let eps = seasons[0]["episodes"].as_array().unwrap();
    assert_eq!(eps.len(), 2);
    assert_eq!(eps[0]["episode"], 1);
    assert_eq!(eps[0]["name"], "Pilot");
    assert_eq!(
        eps[0]["still_thumb_url"],
        "https://image.tmdb.org/t/p/w300/s1.jpg"
    );
    assert_eq!(eps[0]["cached"].as_array().unwrap().len(), 0);
    assert_eq!(eps[1]["episode"], 2);
    assert!(eps[1]["still_thumb_url"].is_null());
    let cached = eps[1]["cached"].as_array().unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0]["kind"], "episode");
    assert_eq!(cached[0]["cache_value"], "episode-tt9900944-S1E2@ir.zm");
    assert_eq!(
        cached[0]["admin_image_url"],
        "/api/admin/episodes/imdb/episode-tt9900944-S1E2@ir.zm/image"
    );
}

#[tokio::test]
async fn title_by_alias_finds_title() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    seed_title(&state, "movie", 9_900_021, "Aliased", 100).await;
    seed_alias(&state, "imdb", "tt9900021", "movie", 9_900_021, None).await;

    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/by-alias/imdb/tt9900021",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json,
        serde_json::json!({"media_type": "movie", "tmdb_id": 9900021})
    );
    // a full cache value is cut to its id
    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/by-alias/imdb/tt9900021@ir.zm",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["tmdb_id"], 9900021);

    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/by-alias/imdb/tt9900022",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json["error"].is_string());
    let (status, _) = send(
        &app,
        "GET",
        "/api/admin/louk/titles/by-alias/bogus/tt9900021",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn title_purge_removes_every_alias() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    seed_title(&state, "movie", 9_900_031, "Purged", 100).await;
    seed_alias(&state, "imdb", "tt9900031", "movie", 9_900_031, None).await;
    seed_alias(&state, "tmdb", "movie-9900031", "movie", 9_900_031, None).await;
    cache::upsert_meta_db(&state.db, "imdb/tt9900031@ir.zm", None, ImageType::Poster)
        .await
        .unwrap();
    cache::upsert_meta_db(
        &state.db,
        "tmdb/movie-9900031_l_t@.zm",
        None,
        ImageType::Logo,
    )
    .await
    .unwrap();
    cache::upsert_meta_db(&state.db, "imdb/tt99000310@ir.zm", None, ImageType::Poster)
        .await
        .unwrap();

    let (status, json) = send(
        &app,
        "POST",
        "/api/admin/louk/titles/movie/9900031/purge",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["ok"], true);
    assert_eq!(json["meta_deleted"], 2);
    assert_eq!(json["errors"], serde_json::json!([]));
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM image_meta WHERE cache_key IN ('imdb/tt9900031@ir.zm', 'tmdb/movie-9900031_l_t@.zm')").await,
        0
    );
    assert_eq!(
        count(
            &state,
            "SELECT COUNT(*) AS v FROM image_meta WHERE cache_key = 'imdb/tt99000310@ir.zm'"
        )
        .await,
        1
    );
    // the index stays (P3 needs the aliases)
    assert_eq!(
        titles::aliases_for_title(&state.db, "movie", 9_900_031)
            .await
            .unwrap()
            .len(),
        2
    );

    let (status, _) = send(
        &app,
        "POST",
        "/api/admin/louk/titles/movie/9900032/purge",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn title_refetch_rejects_bad_kind() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    seed_title(&state, "movie", 9_900_041, "Refetch", 100).await;
    for kind in ["episode", "banner"] {
        let (status, json) = send(
            &app,
            "POST",
            &format!("/api/admin/louk/titles/movie/9900041/refetch?kind={kind}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{kind}");
        assert!(json["error"].is_string());
    }
    let (status, _) = send(
        &app,
        "POST",
        "/api/admin/louk/titles/movie/9900042/refetch?kind=poster",
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn louk_routes_require_admin() {
    let (app, _state) = common::setup_test_app().await;
    for (method, uri) in [
        ("GET", "/api/admin/louk/titles"),
        ("GET", "/api/admin/louk/titles/movie/603"),
        ("GET", "/api/admin/louk/titles/by-alias/imdb/tt0133093"),
        ("POST", "/api/admin/louk/titles/movie/603/purge"),
        ("POST", "/api/admin/louk/titles/movie/603/refetch"),
    ] {
        let (status, _) = send(&app, method, uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

// --- rollback -----------------------------------------------------------------

async fn memory_db() -> DatabaseConnection {
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(":memory:")
        .create_if_missing(true)
        .pragma("foreign_keys", "ON");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    SqlxSqliteConnector::from_sqlx_sqlite_pool(pool)
}

async fn upstream_startup(db: &DatabaseConnection) {
    for sql in openposterdb_api::SCHEMA_SQL {
        db.execute_unprepared(sql).await.expect("upstream schema");
    }
    for (sql, expected_err) in openposterdb_api::MIGRATIONS {
        match db.execute_unprepared(sql).await {
            Ok(_) => {}
            Err(e) if e.to_string().to_lowercase().contains(expected_err) => {}
            Err(e) => panic!("upstream migration failed on a fork DB: {e}\n  SQL: {sql}"),
        }
    }
}

/// Rollback: the upstream binary runs its startup schema + migrations and its
/// queries against a fork-migrated DB (fork tables present and filled).
#[tokio::test]
async fn upstream_schema_opens_fork_db() {
    let db = memory_db().await;
    upstream_startup(&db).await;
    openposterdb_api::louk::schema::apply(&db).await.unwrap();
    openposterdb_api::louk::schema::apply(&db)
        .await
        .expect("fork schema is idempotent");
    for sql in [
        "INSERT INTO louk_titles (media_type, tmdb_id, first_seen, last_requested) VALUES ('movie', 1, 1, 1)",
        "INSERT INTO louk_title_aliases (id_type, id_value, media_type, tmdb_id, last_requested) VALUES ('imdb', 'tt1', 'movie', 1, 1)",
        "INSERT INTO louk_episodes (show_tmdb_id, season, episode, fetched_at) VALUES (2, 1, 1, 1)",
        // Fork columns all have defaults: minimal inserts work too.
        "INSERT INTO louk_titles (media_type, tmdb_id) VALUES ('tv', 2)",
        "INSERT INTO louk_title_aliases (id_type, id_value, media_type, tmdb_id) VALUES ('tmdb', 'series-2', 'tv', 2)",
        "INSERT INTO louk_episodes (show_tmdb_id, season, episode) VALUES (2, 1, 2)",
        "INSERT INTO louk_overrides (media_type, tmdb_id, kind, source) VALUES ('tv', 2, 'logo', 'auto')",
    ] {
        db.execute_unprepared(sql).await.expect(sql);
    }

    // Upstream starts again on the same DB.
    upstream_startup(&db).await;
    db::get_global_settings(&db).await.expect("global settings");
    db::list_api_keys(&db).await.expect("api keys");
    assert_eq!(
        db::count_image_meta(&db).await.expect("image_meta count"),
        0
    );
    cache::upsert_meta_db(&db, "imdb/tt1@ir.zm", None, ImageType::Poster)
        .await
        .expect("image_meta write");
    assert_eq!(db::count_image_meta(&db).await.unwrap(), 1);
}

// --- review follow-ups: hook wiring, backfill parsing, lossless flush --------

fn no_art(
    media_type: MediaType,
    tmdb_id: u64,
    imdb: &str,
    episode: Option<EpisodeInfo>,
) -> ResolvedId {
    ResolvedId {
        imdb_id: Some(imdb.to_owned()),
        tmdb_id,
        tvdb_id: None,
        media_type,
        poster_path: None,
        release_date: None,
        episode,
    }
}

async fn api_key(app: &axum::Router, token: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/keys")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::json!({"name": "hooks"}).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<serde_json::Value>(&body).unwrap()["key"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn pending(map: &dashmap::DashMap<(String, String), impl Sized>, t: &str, v: &str) -> bool {
    map.contains_key(&(t.to_owned(), v.to_owned()))
}

/// The `serve.rs` hooks fire on the real image routes (deleting one fails
/// this test). No network: resolutions come from `id_cache`, ratings are
/// off, and the titles have no TMDB art, so each request stops before any
/// TMDB fetch.
#[tokio::test]
async fn serve_hooks_fire_on_image_routes() {
    let (app, state) = common::setup_test_app_with_options(common::TestAppOptions {
        disable_fanart: true,
        louk: openposterdb_api::louk::config::LoukConfig {
            ratings_disabled: true,
            ..Default::default()
        },
        ..Default::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    let key = api_key(&app, &token).await;
    state
        .id_cache
        .insert(
            "imdb/tt9900700".into(),
            no_art(MediaType::Movie, 9_900_700, "tt9900700", None),
        )
        .await;
    let ep = EpisodeInfo {
        show_tmdb_id: 9_900_702,
        season_number: 1,
        episode_number: 2,
        still_path: None,
    };
    state
        .id_cache
        .insert(
            "imdb/episode-tt9900702-S1E2".into(),
            no_art(MediaType::Episode, 9_900_799, "tt9900799", Some(ep)),
        )
        .await;

    for uri in [
        format!("/{key}/imdb/poster-default/tt9900700.jpg"),
        format!("/{key}/imdb/episode-default/episode-tt9900702-S1E2.jpg"),
        // Not a valid tmdb id: touched, then rejected before any TMDB call.
        format!("/{key}/tmdb/logo-default/movie-x9900703.png"),
    ] {
        let res = app
            .clone()
            .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_ne!(res.status(), StatusCode::UNAUTHORIZED, "{uri}");
    }

    // touch (handle_inner, handle_episode_inner, handle_logo_backdrop_inner)
    assert!(pending(&state.louk.touches, "imdb", "tt9900700"));
    assert!(pending(
        &state.louk.touches,
        "imdb",
        "episode-tt9900702-S1E2"
    ));
    assert!(pending(&state.louk.touches, "tmdb", "movie-x9900703"));
    // note_resolved (resolve_with_ratings): the requested id and its cross ids
    assert!(pending(&state.louk.resolved, "imdb", "tt9900700"));
    assert!(pending(&state.louk.resolved, "tmdb", "movie-9900700"));
    assert!(pending(
        &state.louk.resolved,
        "imdb",
        "episode-tt9900702-S1E2"
    ));

    titles::flush_once(&state).await.unwrap();
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_value IN ('tt9900700', 'movie-9900700')").await,
        2
    );
    assert_eq!(
        count(
            &state,
            "SELECT COUNT(*) AS v FROM louk_title_aliases
             WHERE id_value = 'episode-tt9900702-S1E2' AND media_type = 'tv' AND tmdb_id = 9900702 AND season = 1 AND episode = 2"
        )
        .await,
        1
    );
}

/// The backfill parses cached keys, including the fork's `.mw<N>` backdrop
/// token, resolves them from `id_cache` and records the aliases.
#[tokio::test]
async fn backfill_indexes_cached_keys_with_mw_token() {
    let (_app, state) = common::setup_test_app().await;
    state
        .id_cache
        .insert(
            "imdb/tt9900800".into(),
            movie("tt9900800", 9_900_800, "2001-01-01"),
        )
        .await;
    state
        .id_cache
        .insert(
            "tmdb/movie-9900801".into(),
            movie("tt9900801", 9_900_801, "2002-01-01"),
        )
        .await;
    cache::upsert_meta_db(&state.db, "imdb/tt9900800@.pbc.zm", None, ImageType::Poster)
        .await
        .unwrap();
    cache::upsert_meta_db(
        &state.db,
        "tmdb/movie-9900801_b_t@ir.ptr.sh.li.dv.bl.shr.bgd.mw1920.zl",
        None,
        ImageType::Backdrop,
    )
    .await
    .unwrap();

    let mut cursor = String::new();
    let mut failed = std::collections::HashSet::new();
    let n = titles::backfill_once(&state, &mut cursor, &mut failed, 50)
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert!(failed.is_empty(), "{failed:?}");
    for (t, v, id) in [
        ("imdb", "tt9900800", 9_900_800),
        ("tmdb", "movie-9900800", 9_900_800),
        ("tmdb", "movie-9900801", 9_900_801),
    ] {
        assert_eq!(
            count(
                &state,
                &format!("SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_type = '{t}' AND id_value = '{v}' AND tmdb_id = {id}")
            )
            .await,
            1,
            "{t}/{v}"
        );
    }
    // A second round finds nothing new.
    assert_eq!(
        titles::backfill_once(&state, &mut cursor, &mut failed, 50)
            .await
            .unwrap(),
        0
    );
}

/// A flush that cannot write keeps every pending entry for the next flush.
#[tokio::test]
async fn failed_flush_loses_nothing() {
    let (_app, state) = common::setup_test_app().await;
    titles::note_resolved(
        &state,
        "imdb",
        "tt9900900",
        &movie("tt9900900", 9_900_900, "2003-01-01"),
    );
    titles::touch(&state, "imdb", "tt9900900");
    let before = (state.louk.resolved.len(), state.louk.touches.len());

    exec(
        &state,
        "ALTER TABLE louk_title_aliases RENAME TO louk_title_aliases_away",
    )
    .await;
    assert!(titles::flush_once(&state).await.is_err());
    assert_eq!(
        (state.louk.resolved.len(), state.louk.touches.len()),
        before,
        "entries put back"
    );

    exec(
        &state,
        "ALTER TABLE louk_title_aliases_away RENAME TO louk_title_aliases",
    )
    .await;
    titles::flush_once(&state).await.unwrap();
    assert!(state.louk.resolved.is_empty() && state.louk.touches.is_empty());
    assert_eq!(
        count(&state, "SELECT COUNT(*) AS v FROM louk_title_aliases WHERE id_value IN ('tt9900900', 'movie-9900900')").await,
        2
    );
    assert!(
        count(
            &state,
            "SELECT last_requested AS v FROM louk_title_aliases WHERE id_value = 'tt9900900'"
        )
        .await
            > 0
    );
}
