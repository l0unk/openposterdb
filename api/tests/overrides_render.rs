//! Per-title overrides on the image render paths and the purge on change
//! (`services/overrides/{apply,purge}.rs` + the `image/serve.rs` hooks).
//!
//! No TMDB mock exists in this crate, so resolutions are seeded into
//! `id_cache` and ratings into `ratings_cache`; the override sources are
//! uploads (local files). Titles are seeded WITHOUT TMDB art (no
//! poster_path/still), so a 200 image proves the override was used. Calls
//! that do reach TMDB (an id that is not seeded) fail with the dummy key,
//! which is what those tests expect.

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
use openposterdb_api::services::overrides::{OverrideKey, apply, purge, store, uploads};
use openposterdb_api::services::ratings::{RatingBadge, RatingSource, RatingsResult};

// --- helpers --------------------------------------------------------------

struct App {
    app: axum::Router,
    state: std::sync::Arc<AppState>,
    token: String,
    key: String,
    _dir: tempfile::TempDir,
}

async fn app_with(external_cache_only: bool) -> App {
    let dir = tempfile::tempdir().unwrap();
    let (app, state) = common::setup_test_app_with_options(TestAppOptions {
        cache_dir_override: Some(dir.path().to_str().unwrap().to_owned()),
        disable_fanart: true,
        external_cache_only,
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
        .uri("/api/admin/uploads")
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
        &format!("/api/admin/overrides/{path}"),
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

/// Seed resolutions, and empty ratings for them so no rating API is called.
async fn seed_ids(state: &AppState, ids: &[(&str, &ResolvedId)]) {
    for (k, r) in ids {
        state.id_cache.insert((*k).to_owned(), (*r).clone()).await;
        let ratings_key = match (&r.media_type, &r.episode) {
            (MediaType::Movie, _) => format!("{}/movie", r.tmdb_id),
            (MediaType::Tv, _) => format!("{}/tv", r.tmdb_id),
            (MediaType::Episode, Some(e)) => format!(
                "{}/episode/S{}E{}",
                e.show_tmdb_id, e.season_number, e.episode_number
            ),
            (MediaType::Episode, None) => continue,
        };
        if state.ratings_cache.get(&ratings_key).await.is_none() {
            state
                .ratings_cache
                .insert(ratings_key, RatingsResult::default())
                .await;
        }
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
    let a = app_with(false).await;
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
    let a = app_with(false).await;
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
    let a = app_with(false).await;
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
    let a = app_with(true).await;
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
    let a = app_with(false).await;
    let st = &a.state;
    let dir = st.config.cache_dir.clone();
    // The title was requested by imdb id (in `id_cache`); its canonical tmdb
    // id is not cached, so resolving it fails (dummy TMDB key) and the purge
    // relies on the tmdb id + the `id_cache` scan.
    let m = movie(9_200_010, "tt9200010");
    st.id_cache.insert("imdb/tt9200010".into(), m.clone()).await;

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
    let a = app_with(false).await;
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
        "/api/admin/overrides/tv/9200011/logo",
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
    let a = app_with(false).await;
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
            "INSERT INTO image_overrides (media_type, tmdb_id, kind, source, value, no_badges) VALUES ('movie', 9200030, 'logo', 'auto', '', 1)",
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

// --- delayed re-purge, failed sources ---------------------------------------

async fn seed_logo_file(
    state: &AppState,
    id_type: &str,
    value: &str,
    suffix: &str,
) -> std::path::PathBuf {
    let cache_value = format!("{value}{suffix}");
    let path = cache::typed_cache_path(
        &state.config.cache_dir,
        ImageType::Logo,
        id_type,
        &cache_value,
    )
    .unwrap();
    cache::write(&path, b"old logo").await.unwrap();
    cache::upsert_meta_db(
        &state.db,
        &format!("{id_type}/{cache_value}"),
        None,
        ImageType::Logo,
    )
    .await
    .unwrap();
    path
}

#[tokio::test]
async fn repurge_runs_after_the_delay_once_per_key() {
    let a = app_with(false).await;
    let st = &a.state;
    let m = movie(9_200_050, "tt9200050");
    seed_ids(st, &[("tmdb/movie-9200050", &m)]).await;
    let key = OverrideKey::parse("movie", "9200050", "logo", None, None, None).unwrap();

    let delay = std::time::Duration::from_millis(300);
    assert!(purge::schedule_repurge(st, &key, delay));
    assert!(
        purge::schedule_repurge(st, &key, delay),
        "a second schedule joins the pending job"
    );
    assert_eq!(purge::pending_repurges(st), 1);
    // A render that lands after the first purge (the race) is removed by the
    // delayed one.
    let late = seed_logo_file(st, "imdb", "tt9200050", "_l_t_en@.late.zm").await;
    assert!(late.exists());
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    assert!(!late.exists(), "late render purged");
    assert_eq!(purge::pending_repurges(st), 0);
    assert!(meta_keys(st, "imdb/tt9200050%").await.is_empty());
}

#[tokio::test]
async fn repurges_are_bounded() {
    let a = app_with(false).await;
    let hour = std::time::Duration::from_secs(3600);
    for id in 1..=purge::MAX_PENDING_REPURGES {
        let key = OverrideKey::parse(
            "movie",
            &(9_300_000 + id).to_string(),
            "logo",
            None,
            None,
            None,
        )
        .unwrap();
        assert!(purge::schedule_repurge(&a.state, &key, hour));
    }
    let extra = OverrideKey::parse("movie", "9399999", "logo", None, None, None).unwrap();
    assert!(
        !purge::schedule_repurge(&a.state, &extra, hour),
        "beyond the cap new jobs are dropped"
    );
    assert_eq!(
        purge::pending_repurges(&a.state),
        purge::MAX_PENDING_REPURGES
    );
    // An already pending key is still accepted (it only moves its time).
    let first = OverrideKey::parse("movie", "9300001", "logo", None, None, None).unwrap();
    assert!(purge::schedule_repurge(&a.state, &first, hour));
}

#[tokio::test]
async fn saving_and_clearing_schedule_a_repurge() {
    let a = app_with(false).await;
    let key = OverrideKey::parse("movie", "9200051", "logo", None, None, None).unwrap();
    let before = tokio::time::Instant::now();
    put_override(
        &a,
        "movie/9200051/logo",
        serde_json::json!({"source": "tmdb", "value": "/x.png"}),
    )
    .await;
    let due = purge::pending_repurge(&a.state, &key).expect("scheduled");
    assert!(
        due >= before + purge::REPURGE_AFTER_CHANGE,
        "about 60 s later"
    );
    let (status, _) = send(
        &a.app,
        "DELETE",
        "/api/admin/overrides/movie/9200051/logo",
        Some(&a.token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(purge::pending_repurges(&a.state), 1, "deduplicated");
    assert!(purge::pending_repurge(&a.state, &key).unwrap() >= due);
}

#[tokio::test]
async fn failed_override_source_schedules_one_repurge() {
    let a = app_with(false).await;
    let sha = upload(&a, png(64, 32)).await;
    put_override(
        &a,
        "tv/9200052/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let key = OverrideKey::parse("tv", "9200052", "logo", None, None, None).unwrap();
    let ov = store(&a.state)
        .get_exact(&a.state.db, &key)
        .await
        .unwrap()
        .unwrap();
    // The upload disappears (or a TMDB/fanart fetch fails): automatic art.
    let (path, _) = uploads::find_upload(&a.state.config.db_dir, &sha)
        .await
        .unwrap();
    std::fs::remove_file(path).unwrap();
    let s = show(9_200_052, "tt9200052", 92_052);
    seed_ids(&a.state, &[("imdb/tt9200052", &s)]).await;
    let (status, _, _) = get_image(
        &a.app,
        &format!("/{}/imdb/logo-default/tt9200052.png", a.key),
    )
    .await;
    assert_ne!(status, StatusCode::OK, "no override art, no network art");
    assert!(purge::source_failure_noted(&a.state, &key, ov.updated_at));
    let due = purge::pending_repurge(&a.state, &key).expect("re-purge scheduled");
    assert!(
        due >= tokio::time::Instant::now() + purge::REPURGE_AFTER_CHANGE,
        "the 10 min one"
    );
}

#[tokio::test]
async fn override_purges_show_art_cached_under_episode_ids() {
    let a = app_with(false).await;
    let st = &a.state;
    // Canonical id seeded: resolving it gives the imdb/tvdb ids, no network.
    let s = show(9_200_070, "tt9200070", 92_070);
    seed_ids(st, &[("tmdb/series-9200070", &s)]).await;
    // Show logos requested with episode ids are cached under those ids.
    let suffix = "_l_t_en@.sb.lt.zm";
    let mut purged = Vec::new();
    for (id_type, value) in [
        ("imdb", "episode-tt9200070-S1E2"),
        ("tmdb", "episode-9200070-s2e3"),
        ("tvdb", "episode-92070-S1E1"),
    ] {
        purged.push(seed_logo_file(st, id_type, value, suffix).await);
    }
    let decoy = seed_logo_file(st, "imdb", "episode-tt92000700-S1E2", suffix).await;
    // A still of the show (another kind) stays.
    let still = cache::typed_cache_path(
        &st.config.cache_dir,
        ImageType::Episode,
        "imdb",
        "episode-tt9200070-S1E2_e@.zm",
    )
    .unwrap();
    cache::write(&still, b"still").await.unwrap();

    let sha = upload(&a, png(20, 10)).await;
    let json = put_override(
        &a,
        "tv/9200070/logo",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let ids: Vec<&str> = json["purge"]["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for id in [
        "tmdb/series-9200070",
        "imdb/tt9200070",
        "tvdb/92070",
        "imdb/episode-tt9200070-S1E2",
        "tmdb/episode-9200070-s2e3",
        "tvdb/episode-92070-S1E1",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    for p in &purged {
        assert!(!p.exists(), "{} purged", p.display());
    }
    assert!(decoy.exists(), "another show's episode kept");
    assert!(still.exists(), "other kind kept");
}

#[tokio::test]
async fn still_override_purges_the_episode_ids_only() {
    let a = app_with(false).await;
    let st = &a.state;
    let s = show(9_200_080, "tt9200080", 92_080);
    seed_ids(st, &[("tmdb/series-9200080", &s)]).await;
    // The episode was also requested by its own imdb id (in `id_cache`).
    let ep = ResolvedId {
        imdb_id: Some("tt9200899".to_owned()),
        tmdb_id: 9_200_899,
        tvdb_id: None,
        media_type: MediaType::Episode,
        poster_path: None,
        release_date: None,
        episode: Some(EpisodeInfo {
            show_tmdb_id: 9_200_080,
            season_number: 1,
            episode_number: 2,
            still_path: None,
        }),
    };
    seed_ids(st, &[("imdb/tt9200899", &ep)]).await;
    let sha = upload(&a, png(64, 36)).await;
    let json = put_override(
        &a,
        "tv/9200080/episode?season=1&episode=2",
        serde_json::json!({"source": "upload", "value": sha}),
    )
    .await;
    let ids: Vec<&str> = json["purge"]["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for id in [
        "tmdb/episode-9200080-S1E2",
        "imdb/episode-tt9200080-S1E2",
        "tvdb/episode-92080-S1E2",
        "imdb/tt9200899",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    assert!(!ids.contains(&"tmdb/series-9200080"), "{ids:?}");
}
