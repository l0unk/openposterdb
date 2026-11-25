//! Fork: `ETag` and `304 Not Modified` on image routes (`api/src/louk/http_cache.rs`).
//!
//! No TMDB mock exists, so images are rendered from per-title override
//! uploads (local files) with resolutions seeded into `id_cache` and ratings
//! disabled, as in `louk_overrides_render.rs`.

mod common;

use std::io::Cursor;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use common::TestAppOptions;
use openposterdb_api::AppState;
use openposterdb_api::id::{MediaType, ResolvedId};
use openposterdb_api::louk::config::LoukConfig;

struct App {
    app: axum::Router,
    state: std::sync::Arc<AppState>,
    token: String,
    key: String,
    _dir: tempfile::TempDir,
}

async fn app_with(louk: LoukConfig) -> App {
    let dir = tempfile::tempdir().unwrap();
    let (app, state) = common::setup_test_app_with_options(TestAppOptions {
        cache_dir_override: Some(dir.path().to_str().unwrap().to_owned()),
        disable_fanart: true,
        louk,
        ..TestAppOptions::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/keys")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::json!({"name": "etag"}).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let key = json["key"].as_str().unwrap().to_owned();
    App {
        app,
        state,
        token,
        key,
        _dir: dir,
    }
}

fn enabled() -> LoukConfig {
    LoukConfig {
        ratings_disabled: true,
        ..LoukConfig::default()
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let seed: u32 = rand::random();
    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        let v = seed.wrapping_add(x * 7).wrapping_add(y * 13);
        image::Rgba([v as u8, (v >> 8) as u8, 200, 255])
    });
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn set_logo_override(a: &App, tmdb_id: u64, bytes: Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri("/api/admin/louk/uploads")
        .header("authorization", format!("Bearer {}", a.token))
        .body(Body::from(bytes))
        .unwrap();
    let res = a.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let sha = json["sha256"].as_str().unwrap().to_owned();

    let req = Request::builder()
        .method("PUT")
        .uri(format!("/api/admin/louk/overrides/movie/{tmdb_id}/logo"))
        .header("authorization", format!("Bearer {}", a.token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"source": "upload", "value": sha}).to_string(),
        ))
        .unwrap();
    let res = a.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

async fn seed_movie(state: &AppState, tmdb_id: u64) {
    let m = ResolvedId {
        imdb_id: Some(format!("tt{tmdb_id}")),
        tmdb_id,
        tvdb_id: None,
        media_type: MediaType::Movie,
        poster_path: None,
        release_date: Some("2015-01-01".to_owned()),
        episode: None,
    };
    state
        .id_cache
        .insert(format!("tmdb/movie-{tmdb_id}"), m)
        .await;
}

async fn get(
    a: &App,
    method: &str,
    uri: &str,
    inm: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(v) = inm {
        req = req.header("if-none-match", v);
    }
    let res = a
        .app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    (
        status,
        headers,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn etag(h: &HeaderMap) -> Option<String> {
    h.get("etag").map(|v| v.to_str().unwrap().to_owned())
}

/// Logo of `tmdb_id` rendered from an override; returns (uri, etag, body).
async fn rendered_logo(a: &App, tmdb_id: u64) -> (String, String, Vec<u8>) {
    seed_movie(&a.state, tmdb_id).await;
    set_logo_override(a, tmdb_id, png(64, 32)).await;
    let uri = format!("/{}/tmdb/logo-default/movie-{tmdb_id}.png", a.key);
    let (status, h, body) = get(a, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let tag = etag(&h).expect("200 carries an ETag");
    assert!(
        tag.starts_with('"') && tag.ends_with('"') && tag.len() == 34,
        "{tag}"
    );
    assert_eq!(
        tag,
        openposterdb_api::louk::http_cache::etag_for(&body),
        "tag is the body's"
    );
    (uri, tag, body)
}

#[tokio::test]
async fn etag_roundtrip_304() {
    let a = app_with(enabled()).await;
    let (uri, tag, body) = rendered_logo(&a, 9_206_001).await;

    // Memory hit: same tag, same bytes.
    let (status, h, again) = get(&a, "GET", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(etag(&h).as_deref(), Some(tag.as_str()));
    assert_eq!(again, body);
    let cache_control = h.get("cache-control").cloned();

    for inm in [
        tag.clone(),
        format!("W/{tag}"),
        "*".to_owned(),
        format!("\"nope\", {tag}"),
    ] {
        let (status, h, b) = get(&a, "GET", &uri, Some(&inm)).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED, "If-None-Match: {inm}");
        assert!(b.is_empty(), "304 has no body");
        assert_eq!(etag(&h).as_deref(), Some(tag.as_str()));
        assert_eq!(
            h.get("cache-control").cloned(),
            cache_control,
            "304 keeps Cache-Control"
        );
        assert!(h.get("content-type").is_none());
    }

    let (status, h, b) = get(&a, "GET", &uri, Some("\"0000\", \"1111\"")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b, body);
    assert_eq!(etag(&h).as_deref(), Some(tag.as_str()));
}

#[tokio::test]
async fn disk_hit_keeps_the_tag() {
    let a = app_with(enabled()).await;
    let (uri, tag, body) = rendered_logo(&a, 9_206_002).await;
    a.state.image_mem_cache.invalidate_all();
    a.state.image_mem_cache.run_pending_tasks().await;
    let (status, h, again) = get(&a, "GET", &uri, Some(&tag)).await;
    assert_eq!(
        status,
        StatusCode::NOT_MODIFIED,
        "disk hit, same bytes: {}",
        String::from_utf8_lossy(&again)
    );
    assert_eq!(etag(&h).as_deref(), Some(tag.as_str()));
    let (_, _, b) = get(&a, "GET", &uri, None).await;
    assert_eq!(b, body);
}

#[tokio::test]
async fn head_carries_etag_and_honours_if_none_match() {
    let a = app_with(enabled()).await;
    let (uri, tag, body) = rendered_logo(&a, 9_206_003).await;
    let (status, h, b) = get(&a, "HEAD", &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(b.is_empty());
    assert_eq!(etag(&h).as_deref(), Some(tag.as_str()));
    assert_eq!(
        h.get("content-length").unwrap().to_str().unwrap(),
        body.len().to_string()
    );
    let (status, _, b) = get(&a, "HEAD", &uri, Some(&tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(b.is_empty());
}

#[tokio::test]
async fn changed_render_gets_a_new_etag() {
    let a = app_with(enabled()).await;
    let (uri, old_tag, old_body) = rendered_logo(&a, 9_206_004).await;
    // A new override purges every cached variant; the next request re-renders.
    set_logo_override(&a, 9_206_004, png(64, 32)).await;
    let (status, h, body) = get(&a, "GET", &uri, Some(&old_tag)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "old tag must not validate the new image"
    );
    assert_ne!(body, old_body);
    let new_tag = etag(&h).expect("new render carries a tag");
    assert_ne!(new_tag, old_tag);
    let (status, _, _) = get(&a, "GET", &uri, Some(&new_tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
}

#[tokio::test]
async fn error_responses_are_untouched() {
    let a = app_with(enabled()).await;
    // Unknown API key → 4xx, no validator, never a 304.
    let (status, h, _) = get(
        &a,
        "GET",
        "/0000000000000000/tmdb/logo-default/movie-1.png",
        Some("*"),
    )
    .await;
    assert!(status.is_client_error(), "{status}");
    assert!(etag(&h).is_none());
    // A title that cannot render (no override, no network) → error, no validator.
    let uri = format!("/{}/tmdb/logo-default/movie-9206099.png", a.key);
    let (status, h, _) = get(&a, "GET", &uri, Some("*")).await;
    assert!(
        !status.is_success() && status != StatusCode::NOT_MODIFIED,
        "{status}"
    );
    assert!(etag(&h).is_none());
}

#[tokio::test]
async fn no_headers_change_when_disabled() {
    let off = app_with(LoukConfig {
        ratings_disabled: true,
        http_etag_disabled: true,
        ..LoukConfig::default()
    })
    .await;
    let on = app_with(enabled()).await;
    seed_movie(&off.state, 9_206_005).await;
    set_logo_override(&off, 9_206_005, png(64, 32)).await;
    let uri_off = format!("/{}/tmdb/logo-default/movie-9206005.png", off.key);
    let (_, _, _) = get(&off, "GET", &uri_off, None).await;
    let (status, h_off, b_off) = get(&off, "GET", &uri_off, Some("*")).await;
    assert_eq!(status, StatusCode::OK, "no 304 when disabled");
    assert!(etag(&h_off).is_none());
    assert!(!b_off.is_empty());

    let (uri_on, _, _) = rendered_logo(&on, 9_206_006).await;
    let (_, h_on, _) = get(&on, "GET", &uri_on, None).await;
    let names = |h: &HeaderMap| {
        let mut v: Vec<String> = h
            .keys()
            .map(|k| k.as_str().to_owned())
            .filter(|k| k != "etag")
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&h_off), names(&h_on), "only ETag differs");
    for name in ["content-type", "cache-control", "vary"] {
        assert_eq!(h_off.get(name), h_on.get(name), "{name}");
    }
}
