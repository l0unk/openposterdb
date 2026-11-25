//! Fork: per-title overrides on the image render paths and the purge on
//! change (`api/src/louk/overrides/{apply,purge}.rs` + the `serve.rs` hooks).
//!
//! No TMDB mock exists in this crate, so resolutions are seeded into
//! `id_cache` and ratings into `ratings_cache`; the override sources are
//! uploads (local files). Titles are seeded WITHOUT TMDB art (no
//! poster_path/still), so a 200 image proves the override was used. The
//! only network call is in `clearing_restores_automatic_pick`, which
//! expects the automatic path to fail with the dummy TMDB key.

mod common;

use std::io::Cursor;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sea_orm::{ConnectionTrait, Statement};
use tower::ServiceExt;

use common::TestAppOptions;
use openposterdb_api::AppState;
use openposterdb_api::cache::{self, ImageType, MemCacheEntry};
use openposterdb_api::id::{EpisodeInfo, MediaType, ResolvedId};
use openposterdb_api::louk::config::LoukConfig;
use openposterdb_api::louk::overrides::{apply, store};
use openposterdb_api::louk::titles;
use openposterdb_api::services::ratings::{RatingBadge, RatingSource, RatingsResult};

// --- helpers --------------------------------------------------------------

struct App {
    app: axum::Router,
    state: std::sync::Arc<AppState>,
    token: String,
    key: String,
    _dir: tempfile::TempDir,
}

async fn app_with(ratings_disabled: bool, external_cache_only: bool) -> App {
    let dir = tempfile::tempdir().unwrap();
    let (app, state) = common::setup_test_app_with_options(TestAppOptions {
        cache_dir_override: Some(dir.path().to_str().unwrap().to_owned()),
        disable_fanart: true,
        external_cache_only,
        louk: LoukConfig {
            ratings_disabled,
            ..LoukConfig::default()
        },
        ..TestAppOptions::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    let (status, json) = send(
        &app,
        "POST",
        "/api/keys",
        Some(&token),
        Some(serde_json::json!({"name": "overrides"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let key = json["key"].as_str().unwrap().to_owned();
    App {
        app,
        state,
        token,
        key,
        _dir: dir,
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    json: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = match json {
        Some(j) => {
            req = req.header("content-type", "application/json");
            Body::from(j.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// GET an image route: (status, content type, bytes).
async fn get_image(app: &axum::Router, uri: &str) -> (StatusCode, String, Vec<u8>) {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    (
        status,
        ct,
        res.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
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

fn dims(bytes: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(bytes).expect("response decodes");
    (img.width(), img.height())
}

async fn upload(a: &App, bytes: Vec<u8>) -> String {
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
    json["sha256"].as_str().unwrap().to_owned()
}

async fn put_override(a: &App, path: &str, body: serde_json::Value) -> serde_json::Value {
    let (status, json) = send(
        &a.app,
        "PUT",
        &format!("/api/admin/louk/overrides/{path}"),
        Some(&a.token),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    json
}

fn show(tmdb_id: u64, imdb: &str, tvdb: u64) -> ResolvedId {
    ResolvedId {
        imdb_id: Some(imdb.to_owned()),
        tmdb_id,
        tvdb_id: Some(tvdb),
        media_type: MediaType::Tv,
        poster_path: None,
        release_date: Some("2015-01-01".to_owned()),
        episode: None,
    }
}

fn movie(tmdb_id: u64, imdb: &str) -> ResolvedId {
    ResolvedId {
        imdb_id: Some(imdb.to_owned()),
        tmdb_id,
        tvdb_id: None,
        media_type: MediaType::Movie,
        poster_path: None,
        release_date: Some("2015-01-01".to_owned()),
        episode: None,
    }
}

async fn seed_ids(state: &AppState, ids: &[(&str, &ResolvedId)]) {
    for (k, r) in ids {
        state.id_cache.insert((*k).to_owned(), (*r).clone()).await;
    }
}

async fn meta_keys(state: &AppState, like: &str) -> Vec<String> {
    let rows = state
        .db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "SELECT cache_key FROM image_meta WHERE cache_key LIKE '{like}' ORDER BY cache_key"
            ),
        ))
        .await
        .unwrap();
    rows.iter()
        .map(|r| r.try_get::<String>("", "cache_key").unwrap())
        .collect()
}

fn assert_aspect(bytes: &[u8], ratio: f64) {
    let (w, h) = dims(bytes);
    let got = w as f64 / h as f64;
    assert!((got - ratio).abs() < 0.05, "{w}x{h} is not {ratio}:1");
}

// --- render paths ---------------------------------------------------------

#[tokio::test]
async fn override_applies_for_imdb_tmdb_and_tvdb_requests() {
    let a = app_with(true, false).await;
    let sha = upload(&a, png(64, 32)).await;
    put_override(
        &a,
        "tv/9200001/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let s = show(9_200_001, "tt9200001", 92_001);
    seed_ids(
        &a.state,
        &[
            ("imdb/tt9200001", &s),
            ("tmdb/series-9200001", &s),
            ("tvdb/92001", &s),
        ],
    )
    .await;

    for path in [
        "imdb/logo-default/tt9200001.png",
        "tmdb/logo-default/series-9200001.png",
        "tvdb/logo-default/92001.png",
    ] {
        let (status, ct, body) = get_image(&a.app, &format!("/{}/{path}", a.key)).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{path}: {}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(ct, "image/png", "{path}");
        assert_aspect(&body, 2.0);
    }
}

#[tokio::test]
async fn override_applies_on_poster_path() {
    let a = app_with(true, false).await;
    let sha = upload(&a, png(40, 60)).await;
    put_override(
        &a,
        "movie/9200006/poster",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let m = movie(9_200_006, "tt9200006");
    seed_ids(&a.state, &[("imdb/tt9200006", &m)]).await;
    // The title has no TMDB poster at all: upstream would 404.
    let (status, ct, body) = get_image(
        &a.app,
        &format!("/{}/imdb/poster-default/tt9200006.jpg", a.key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(ct, "image/jpeg");
    assert_aspect(&body, 2.0 / 3.0);
}

#[tokio::test]
async fn override_applies_on_episode_path() {
    let a = app_with(true, false).await;
    let sha = upload(&a, png(64, 36)).await;
    put_override(
        &a,
        "tv/9200007/episode?season=1&episode=2",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let ep = ResolvedId {
        imdb_id: Some("tt9200708".to_owned()),
        tmdb_id: 9_200_708,
        tvdb_id: None,
        media_type: MediaType::Episode,
        poster_path: None,
        release_date: None,
        episode: Some(EpisodeInfo {
            show_tmdb_id: 9_200_007,
            season_number: 1,
            episode_number: 2,
            still_path: None,
        }),
    };
    seed_ids(&a.state, &[("imdb/episode-tt9200007-S1E2", &ep)]).await;
    let (status, ct, body) = get_image(
        &a.app,
        &format!("/{}/imdb/episode-default/episode-tt9200007-S1E2.jpg", a.key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(ct, "image/jpeg");
    assert_aspect(&body, 64.0 / 36.0);
    // Another episode of the same show is not affected (no still → 404).
    let mut other = ep.clone();
    other.episode = Some(EpisodeInfo {
        show_tmdb_id: 9_200_007,
        season_number: 1,
        episode_number: 3,
        still_path: None,
    });
    seed_ids(&a.state, &[("imdb/episode-tt9200007-S1E3", &other)]).await;
    let (status, _, _) = get_image(
        &a.app,
        &format!("/{}/imdb/episode-default/episode-tt9200007-S1E3.jpg", a.key),
    )
    .await;
    assert_ne!(status, StatusCode::OK);
}

#[tokio::test]
async fn override_applies_with_external_cache_only() {
    let a = app_with(true, true).await;
    let sha = upload(&a, png(96, 54)).await;
    put_override(
        &a,
        "movie/9200008/backdrop",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let m = movie(9_200_008, "tt9200008");
    seed_ids(&a.state, &[("tmdb/movie-9200008", &m)]).await;
    let (status, ct, body) = get_image(
        &a.app,
        &format!("/{}/tmdb/backdrop-default/movie-9200008.jpg", a.key),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(ct, "image/jpeg");
    assert_aspect(&body, 96.0 / 54.0);
}

// --- purge and clear ------------------------------------------------------

#[tokio::test]
async fn override_purges_all_aliases() {
    let a = app_with(true, false).await;
    let st = &a.state;
    let dir = st.config.cache_dir.clone();
    // The title is known under imdb + tmdb; the imdb alias is still pending
    // in the hook maps (not flushed yet).
    let m = movie(9_200_010, "tt9200010");
    titles::note_resolved(st, "imdb", "tt9200010", &m);

    let suffix = "_l_t_en@.sb.lt.zm";
    let mut seeded = Vec::new();
    for (id_type, value) in [
        ("imdb", "tt9200010"),
        ("tmdb", "movie-9200010"),
        ("imdb", "tt92000100"),
    ] {
        let cache_value = format!("{value}{suffix}");
        let path = cache::typed_cache_path(&dir, ImageType::Logo, id_type, &cache_value).unwrap();
        cache::write(&path, b"old logo").await.unwrap();
        let key = format!("{id_type}/{cache_value}");
        cache::upsert_meta_db(&st.db, &key, None, ImageType::Logo)
            .await
            .unwrap();
        st.image_mem_cache
            .insert(
                key.clone(),
                MemCacheEntry {
                    bytes: bytes::Bytes::from_static(b"old logo"),
                    last_checked: std::time::Instant::now(),
                },
            )
            .await;
        seeded.push((path, key));
    }
    // A poster of the same title: another kind, must stay.
    let poster_path =
        cache::typed_cache_path(&dir, ImageType::Poster, "imdb", "tt9200010@.zm").unwrap();
    cache::write(&poster_path, b"poster").await.unwrap();
    cache::upsert_meta_db(&st.db, "imdb/tt9200010@.zm", None, ImageType::Poster)
        .await
        .unwrap();

    let sha = upload(&a, png(20, 10)).await;
    let json = put_override(
        &a,
        "movie/9200010/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let ids: Vec<&str> = json["purge"]["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"imdb/tt9200010") && ids.contains(&"tmdb/movie-9200010"),
        "{ids:?}"
    );
    assert!(!ids.contains(&"imdb/tt92000100"));
    assert_eq!(json["purge"]["files_deleted"], 2, "{json}");
    assert_eq!(json["purge"]["meta_deleted"], 2, "{json}");
    assert!(json["purge"]["errors"].as_array().unwrap().is_empty());

    for (path, key) in &seeded[..2] {
        assert!(!path.exists(), "{key} file purged");
        assert!(
            st.image_mem_cache.get(key).await.is_none(),
            "{key} mem purged"
        );
    }
    assert!(seeded[2].0.exists(), "decoy tt92000100 kept");
    assert!(
        st.image_mem_cache.get(&seeded[2].1).await.is_some(),
        "decoy mem kept"
    );
    assert!(poster_path.exists(), "other kind kept");
    assert_eq!(
        meta_keys(st, "imdb/tt9200010@%").await,
        ["imdb/tt9200010@.zm"]
    );
    assert_eq!(meta_keys(st, "imdb/tt92000100%").await.len(), 1);
    assert!(meta_keys(st, "tmdb/movie-9200010%").await.is_empty());
}

#[tokio::test]
async fn clearing_restores_automatic_pick() {
    let a = app_with(true, false).await;
    let sha = upload(&a, png(64, 32)).await;
    put_override(
        &a,
        "tv/9200011/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let s = show(9_200_011, "tt9200011", 92_011);
    let ids = [
        ("imdb/tt9200011", &s),
        ("tmdb/series-9200011", &s),
        ("tvdb/92011", &s),
    ];
    seed_ids(&a.state, &ids).await;
    for path in [
        "imdb/logo-default/tt9200011.png",
        "tvdb/logo-default/92011.png",
    ] {
        let (status, _, _) = get_image(&a.app, &format!("/{}/{path}", a.key)).await;
        assert_eq!(status, StatusCode::OK, "{path}");
    }
    assert!(!meta_keys(&a.state, "imdb/tt9200011%").await.is_empty());
    // Let the background cross-id copies of those renders land first.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let (status, json) = send(
        &a.app,
        "DELETE",
        "/api/admin/louk/overrides/tv/9200011/logo",
        Some(&a.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["deleted"], true);
    let purged: Vec<&str> = json["purge"]["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for id in ["imdb/tt9200011", "tmdb/series-9200011", "tvdb/92011"] {
        assert!(purged.contains(&id), "{id} in {purged:?}");
    }
    assert!(meta_keys(&a.state, "imdb/tt9200011%").await.is_empty());
    assert!(meta_keys(&a.state, "tvdb/92011%").await.is_empty());

    // Back to the automatic pick: TMDB (dummy key) has nothing, fanart is
    // off, so the request no longer returns the override.
    seed_ids(&a.state, &ids).await;
    let (status, _, _) = get_image(
        &a.app,
        &format!("/{}/imdb/logo-default/tt9200011.png", a.key),
    )
    .await;
    assert_ne!(status, StatusCode::OK);
}

// --- no badges ----------------------------------------------------------------

async fn seed_9200020(state: &AppState, m: &ResolvedId) {
    seed_ids(state, &[("imdb/tt9200020", m)]).await;
    state
        .ratings_cache
        .insert(
            "9200020/movie".to_owned(),
            RatingsResult {
                badges: vec![RatingBadge {
                    source: RatingSource::Imdb,
                    value: "7.5".into(),
                }],
                ..RatingsResult::default()
            },
        )
        .await;
}

#[tokio::test]
async fn no_badges_changes_cache_key() {
    // Ratings enabled; the rating is seeded, so nothing calls a rating API.
    let a = app_with(false, false).await;
    let m = movie(9_200_020, "tt9200020");
    let sha = upload(&a, png(64, 32)).await;
    put_override(
        &a,
        "movie/9200020/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    seed_9200020(&a.state, &m).await;
    let uri = format!("/{}/imdb/logo-default/tt9200020.png", a.key);
    let (status, _, with_badges) = get_image(&a.app, &uri).await;
    assert_eq!(status, StatusCode::OK);
    let keys = meta_keys(&a.state, "imdb/tt9200020%").await;
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert!(keys[0].contains("@i."), "badged key {keys:?}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let json = put_override(
        &a,
        "movie/9200020/logo",
        serde_json::json!({"source": "upload", "value": sha, "no_badges": true}),
    )
    .await;
    assert_eq!(json["override"]["no_badges"], true);
    assert!(
        meta_keys(&a.state, "imdb/tt9200020%").await.is_empty(),
        "purged on change"
    );
    seed_9200020(&a.state, &m).await;
    let (status, _, without) = get_image(&a.app, &uri).await;
    assert_eq!(status, StatusCode::OK);
    let keys = meta_keys(&a.state, "imdb/tt9200020%").await;
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert!(
        keys[0].contains("@.") && !keys[0].contains("@i"),
        "badge-free key {keys:?}"
    );
    assert_ne!(with_badges, without);
    // The second request takes the fast path with the badge-free key.
    let (status, _, again) = get_image(&a.app, &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, without);
}

// --- defaults -------------------------------------------------------------------

#[tokio::test]
async fn defaults_unchanged_without_override() {
    let (_app, state) = common::setup_test_app().await;
    let m = movie(9_200_030, "tt9200030");
    state
        .id_cache
        .insert("imdb/tt9200030".into(), m.clone())
        .await;
    let badges = vec![RatingBadge {
        source: RatingSource::Imdb,
        value: "7.5".into(),
    }];
    for kind in [ImageType::Poster, ImageType::Logo, ImageType::Backdrop] {
        assert_eq!(
            apply::badge_limit_hint(&state, "imdb", "tt9200030", kind, "en", 5).await,
            5
        );
        assert_eq!(apply::badge_limit(&state, &m, kind, "en", 5).await, 5);
        assert_eq!(apply::badge_limit(&state, &m, kind, "en", 0).await, 0);
        assert_eq!(
            apply::strip_badges(&state, &m, kind, "en", badges.clone())
                .await
                .len(),
            1
        );
        assert!(
            apply::source_bytes(&state, &m, kind, "en", None)
                .await
                .is_none()
        );
        assert!(!apply::has_art(&state, &m, kind, "en").await);
    }
    assert!(store(&state).is_empty());

    // The map is loaded once; later lookups do not query the DB: a row
    // written behind the store's back is invisible until a reload.
    state
        .db
        .execute_unprepared(
            "INSERT INTO louk_overrides (media_type, tmdb_id, kind, source, value, no_badges) VALUES ('movie', 9200030, 'logo', 'auto', '', 1)",
        )
        .await
        .unwrap();
    assert_eq!(
        apply::badge_limit(&state, &m, ImageType::Logo, "en", 5).await,
        5
    );
    store(&state).reload(&state.db).await.unwrap();
    assert_eq!(
        apply::badge_limit(&state, &m, ImageType::Logo, "en", 5).await,
        0
    );
    assert_eq!(
        apply::badge_limit_hint(&state, "imdb", "tt9200030", ImageType::Logo, "en", 5).await,
        0
    );
    // Only that kind; and an id the cache does not know keeps its limit.
    assert_eq!(
        apply::badge_limit(&state, &m, ImageType::Poster, "en", 5).await,
        5
    );
    assert_eq!(
        apply::badge_limit_hint(&state, "imdb", "tt0000404", ImageType::Logo, "en", 5).await,
        5
    );
    // `auto` keeps the automatic art.
    assert!(!apply::has_art(&state, &m, ImageType::Logo, "en").await);
    assert!(
        apply::strip_badges(&state, &m, ImageType::Logo, "en", badges.clone())
            .await
            .is_empty()
    );
}
