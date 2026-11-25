//! Fork: stale-while-revalidate for a stale ratings index (`api/src/louk/swr.rs`).
//!
//! No TMDB or ratings mock exists in this crate: resolutions are seeded into
//! `id_cache`, ratings into `ratings_cache` (so no rating API is ever called),
//! and the art is a per-title override upload. What these tests cannot show:
//! the latency saved against a real MDBList, and a revalidation whose ratings
//! fetch goes to the network.

mod common;

use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ConnectionTrait, Statement};
use tower::ServiceExt;

use common::TestAppOptions;
use openposterdb_api::AppState;
use openposterdb_api::id::{MediaType, ResolvedId};
use openposterdb_api::louk::config::LoukConfig;
use openposterdb_api::services::ratings::{RatingBadge, RatingSource, RatingsResult};

struct App {
    app: axum::Router,
    state: std::sync::Arc<AppState>,
    key: String,
    uri: String,
    id_key: String,
    _dir: tempfile::TempDir,
}

async fn json_req(app: &axum::Router, method: &str, uri: &str, token: &str, body: Body, json: bool) -> serde_json::Value {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if json {
        req = req.header("content-type", "application/json");
    }
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK, "{method} {uri}");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn png() -> Vec<u8> {
    let seed: u32 = rand::random();
    let img = image::RgbaImage::from_fn(64, 32, |x, y| {
        let v = seed.wrapping_add(x * 7).wrapping_add(y * 13);
        image::Rgba([v as u8, (v >> 8) as u8, 200, 255])
    });
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

async fn seed_ratings(state: &AppState, tmdb_id: u64) {
    state
        .ratings_cache
        .insert(
            format!("{tmdb_id}/movie"),
            RatingsResult {
                badges: vec![RatingBadge { source: RatingSource::Imdb, value: "7.5".into() }],
                ..RatingsResult::default()
            },
        )
        .await;
}

/// App with a logo override for movie `tmdb_id` (no release date, so the
/// index goes stale after the 1-day minimum), seeded ratings, and one request
/// already served (which wrote the index row and the cache file).
async fn app_with(louk: LoukConfig, tmdb_id: u64) -> App {
    let dir = tempfile::tempdir().unwrap();
    let (app, state) = common::setup_test_app_with_options(TestAppOptions {
        cache_dir_override: Some(dir.path().to_str().unwrap().to_owned()),
        disable_fanart: true,
        louk,
        ..TestAppOptions::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    let key = json_req(&app, "POST", "/api/keys", &token, Body::from(r#"{"name":"swr"}"#), true).await["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let sha = json_req(&app, "POST", "/api/admin/louk/uploads", &token, Body::from(png()), false).await["sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = serde_json::json!({"source": "upload", "value": sha}).to_string();
    json_req(&app, "PUT", &format!("/api/admin/louk/overrides/movie/{tmdb_id}/logo"), &token, Body::from(body), true).await;
    let m = ResolvedId {
        imdb_id: Some(format!("tt{tmdb_id}")),
        tmdb_id,
        tvdb_id: None,
        media_type: MediaType::Movie,
        poster_path: None,
        release_date: None,
        episode: None,
    };
    let id_key = format!("tmdb/movie-{tmdb_id}");
    state.id_cache.insert(id_key.clone(), m).await;
    seed_ratings(&state, tmdb_id).await;
    let uri = format!("/{key}/tmdb/logo-default/movie-{tmdb_id}.png");
    App { app, state, key, uri, id_key, _dir: dir }
}

async fn get(a: &App) -> (StatusCode, Vec<u8>) {
    let res = a
        .app
        .clone()
        .oneshot(Request::builder().uri(&a.uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    (status, res.into_body().collect().await.unwrap().to_bytes().to_vec())
}

async fn row_updated_at(state: &AppState, id_key: &str) -> Option<i64> {
    let row = state
        .db
        .query_one(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!("SELECT updated_at FROM available_ratings WHERE id_key = '{id_key}'"),
        ))
        .await
        .unwrap()?;
    Some(row.try_get::<i64>("", "updated_at").unwrap())
}

/// Age the index row past its stale interval and drop the in-memory copy.
async fn make_index_stale(state: &AppState, id_key: &str) {
    state
        .db
        .execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!("UPDATE available_ratings SET updated_at = 1 WHERE id_key = '{id_key}'"),
        ))
        .await
        .unwrap();
    state.available_ratings_cache.invalidate(id_key).await;
    state.available_ratings_cache.run_pending_tasks().await;
    assert_eq!(row_updated_at(state, id_key).await, Some(1));
}

async fn wait_for_row_refresh(state: &AppState, id_key: &str) -> bool {
    for _ in 0..100 {
        if row_updated_at(state, id_key).await.is_some_and(|t| t > 1) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

fn swr_on() -> LoukConfig {
    LoukConfig { ratings_swr: true, ..LoukConfig::default() }
}

#[tokio::test]
async fn swr_serves_cached_image_and_revalidates_once() {
    let a = app_with(swr_on(), 9_207_001).await;
    let (status, first) = get(&a).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&first));
    assert!(row_updated_at(&a.state, &a.id_key).await.is_some(), "first request writes the index");
    make_index_stale(&a.state, &a.id_key).await;

    // Five requests at once on a stale index: all served the cached image,
    // one background revalidation.
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..5 {
        let app = a.app.clone();
        let uri = a.uri.clone();
        set.spawn(async move {
            let res = app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
            let status = res.status();
            (status, res.into_body().collect().await.unwrap().to_bytes().to_vec())
        });
    }
    while let Some(r) = set.join_next().await {
        let (status, body) = r.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, first, "stale hit serves the cached image");
    }
    assert_eq!(a.state.louk.swr.started.load(Ordering::Relaxed), 1, "deduplicated");

    // The revalidation ran the upstream slow path: index row refreshed.
    assert!(wait_for_row_refresh(&a.state, &a.id_key).await, "revalidation refreshed the index");
    for _ in 0..100 {
        if a.state.refresh_locks.iter().all(|(k, _)| !k.starts_with("louk-swr:")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(a.state.refresh_locks.iter().all(|(k, _)| !k.starts_with("louk-swr:")), "lock released");

    // Fresh index again: the normal fast path, no new revalidation.
    let (status, body) = get(&a).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, first);
    assert_eq!(a.state.louk.swr.started.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn swr_off_keeps_upstream_behaviour() {
    let a = app_with(LoukConfig::default(), 9_207_002).await;
    let (status, first) = get(&a).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&first));
    make_index_stale(&a.state, &a.id_key).await;
    let (status, body) = get(&a).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, first);
    // Upstream: the request itself took the slow path and rewrote the index
    // before answering.
    assert!(row_updated_at(&a.state, &a.id_key).await.is_some_and(|t| t > 1));
    assert_eq!(a.state.louk.swr.started.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn swr_does_nothing_with_ratings_disabled() {
    let a = app_with(LoukConfig { ratings_swr: true, ratings_disabled: true, ..LoukConfig::default() }, 9_207_003).await;
    let (status, _) = get(&a).await;
    assert_eq!(status, StatusCode::OK);
    // Ratings disabled never writes the index; plant a stale row anyway.
    a.state
        .db
        .execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!("INSERT OR REPLACE INTO available_ratings (id_key, sources, updated_at, release_date) VALUES ('{}', 'i', 1, NULL)", a.id_key),
        ))
        .await
        .unwrap();
    let (status, _) = get(&a).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(a.state.louk.swr.started.load(Ordering::Relaxed), 0);
    assert_eq!(row_updated_at(&a.state, &a.id_key).await, Some(1), "row untouched");
    let _ = &a.key;
}
