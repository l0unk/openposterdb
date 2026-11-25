//! Fork: badge looks `s` / `m` / `a` (see `api/src/louk/badge_looks.rs`).
//!
//! Route-level checks: the new `badge_background` values pass through the
//! admin preview endpoints, the image query param, global settings and
//! per-key settings, and unknown values are still rejected as upstream does.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const LOOKS: [&str; 3] = ["s", "m", "a"];

fn authed(method: &str, uri: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    match body {
        Some(b) => builder.body(Body::from(b.to_string())).unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn get_bytes(app: &axum::Router, uri: &str, token: &str) -> (StatusCode, Vec<u8>) {
    let res = app
        .clone()
        .oneshot(authed("GET", uri, token, None))
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body)
}

#[tokio::test]
async fn preview_draws_every_fork_look_for_every_kind() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;

    for (kind, magic) in [
        ("poster", &[0xFF_u8, 0xD8][..]),
        ("backdrop", &[0xFF, 0xD8]),
        ("episode", &[0xFF, 0xD8]),
        ("logo", &[0x89, b'P']),
    ] {
        let (status, upstream) = get_bytes(
            &app,
            &format!("/api/admin/preview/{kind}?badge_background=d"),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind} default");
        let mut seen = vec![upstream];
        for look in LOOKS {
            let (status, body) = get_bytes(
                &app,
                &format!("/api/admin/preview/{kind}?badge_background={look}"),
                &token,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{kind} {look}");
            assert_eq!(&body[..2], magic, "{kind} {look}: image format");
            assert!(
                !seen.contains(&body),
                "{kind} {look}: differs from upstream and the other looks"
            );
            seen.push(body);
        }
    }
}

#[tokio::test]
async fn preview_fork_looks_take_the_usual_params() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let uris = [
        "/api/admin/preview/poster?badge_background=s&badge_shape=p&label_style=t&ratings_limit=8&position=tl&badge_direction=v",
        "/api/admin/preview/poster?badge_background=a&ratings_limit=5&imageSize=very-large",
        "/api/admin/preview/poster?badge_background=m&badge_size=xs&split=true",
        "/api/admin/preview/backdrop?badge_background=a&position=tc&badge_direction=h&imageSize=large",
        "/api/admin/preview/backdrop?badge_background=s&label_style=i&edge_inset_x=2&edge_inset_y=2",
        "/api/admin/preview/episode?badge_background=m&blur=true",
        "/api/admin/preview/logo?badge_background=a&badge_style=v",
    ];
    for uri in uris {
        let (status, body) = get_bytes(&app, uri, &token).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(body.len() > 1000, "{uri}: an image");
    }
}

#[tokio::test]
async fn unknown_background_is_still_rejected() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let (status, _) = get_bytes(&app, "/api/admin/preview/poster?badge_background=g", &token).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn image_route_accepts_fork_looks() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let res = app
        .clone()
        .oneshot(authed(
            "POST",
            "/api/keys",
            &token,
            Some(json!({"name": "looks"})),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let key: Value = serde_json::from_slice(&body).unwrap();
    let api_key = key["key"].as_str().unwrap().to_string();

    for look in LOOKS {
        for path in [
            "imdb/poster-default/tt0000001.jpg",
            "imdb/backdrop-default/tt0000001.jpg",
            "imdb/logo-default/tt0000001.png",
        ] {
            let req = Request::builder()
                .uri(format!("/{api_key}/{path}?badge_background={look}"))
                .body(Body::empty())
                .unwrap();
            let res = app.clone().oneshot(req).await.unwrap();
            assert_ne!(res.status(), StatusCode::BAD_REQUEST, "{path} {look}");
        }
    }
}

#[tokio::test]
async fn global_and_per_key_settings_store_fork_looks() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;

    let update = json!({
        "image_source": "t",
        "poster_badge_background": "s",
        "logo_badge_background": "m",
        "backdrop_badge_background": "a",
        "episode_badge_background": "s",
    });
    let res = app
        .clone()
        .oneshot(authed("PUT", "/api/admin/settings", &token, Some(update)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = app
        .clone()
        .oneshot(authed("GET", "/api/admin/settings", &token, None))
        .await
        .unwrap();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let got: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(got["poster_badge_background"], "s");
    assert_eq!(got["logo_badge_background"], "m");
    assert_eq!(got["backdrop_badge_background"], "a");
    assert_eq!(got["episode_badge_background"], "s");

    let res = app
        .clone()
        .oneshot(authed(
            "POST",
            "/api/keys",
            &token,
            Some(json!({"name": "looks"})),
        ))
        .await
        .unwrap();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let key_id = serde_json::from_slice::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let update = json!({ "image_source": "t", "poster_badge_background": "m", "backdrop_badge_background": "s" });
    let res = app
        .clone()
        .oneshot(authed(
            "PUT",
            &format!("/api/keys/{key_id}/settings"),
            &token,
            Some(update),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = app
        .clone()
        .oneshot(authed(
            "GET",
            &format!("/api/keys/{key_id}/settings"),
            &token,
            None,
        ))
        .await
        .unwrap();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let got: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(got["poster_badge_background"], "m");
    assert_eq!(got["backdrop_badge_background"], "s");

    // Unknown values are refused, as upstream does for any bad token.
    let bad = json!({ "image_source": "t", "poster_badge_background": "g" });
    let res = app
        .clone()
        .oneshot(authed("PUT", "/api/admin/settings", &token, Some(bad)))
        .await
        .unwrap();
    assert!(res.status().is_client_error(), "got {}", res.status());
}

// --- ?art= (real artwork in admin previews, `louk::preview_art`) ---

/// One request on a fresh app with its own cache dir, so nothing comes
/// from the memory or disk preview caches.
async fn fresh_preview(uri: &str) -> (StatusCode, Vec<u8>) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("louk-badge-looks-{}-{n}", std::process::id()));
    let (app, _state) = common::setup_test_app_with_options(common::TestAppOptions {
        cache_dir_override: Some(dir.to_string_lossy().into_owned()),
        ..Default::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    get_bytes(&app, uri, &token).await
}

#[tokio::test]
async fn preview_without_art_is_unchanged_by_season_and_episode() {
    for kind in ["poster", "backdrop", "logo", "episode"] {
        for bg in ["d", "s"] {
            let (s1, plain) = fresh_preview(&format!("/api/admin/preview/{kind}?badge_background={bg}")).await;
            // Without art, season/episode are ignored even when malformed (as upstream ignores them).
            let (s2, with) = fresh_preview(&format!("/api/admin/preview/{kind}?badge_background={bg}&season=x&episode=1&art=")).await;
            assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK), "{kind} {bg}");
            assert_eq!(plain, with, "{kind} {bg}: no art means the usual sample render");
        }
    }
}

#[tokio::test]
async fn preview_rejects_bad_art() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    for uri in [
        "/api/admin/preview/poster?art=film-603",
        "/api/admin/preview/poster?art=movie-abc",
        "/api/admin/preview/backdrop?art=movie-0",
        "/api/admin/preview/episode?art=series-1399",
        "/api/admin/preview/episode?art=movie-603&season=1&episode=1",
        "/api/admin/preview/poster?art=movie-603&season=x",
    ] {
        let (status, body) = get_bytes(&app, uri, &token).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].is_string(), "{uri}: JSON error");
    }
}

#[tokio::test]
async fn self_service_preview_refuses_art() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_api_key_session(&app).await;
    let (status, _) = get_bytes(&app, "/api/key/me/preview/poster?badge_background=s", &token).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get_bytes(&app, "/api/key/me/preview/poster?art=movie-603", &token).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn preview_art_failure_is_a_json_error_not_a_panic() {
    // The test app's TMDB key is a dummy, so TMDB refuses (or is unreachable):
    // the preview answers 404/502 with a JSON error.
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let (status, body) = get_bytes(&app, "/api/admin/preview/poster?art=movie-603&badge_background=a", &token).await;
    assert!(status == StatusCode::BAD_GATEWAY || status == StatusCode::NOT_FOUND, "got {status}");
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json["error"].as_str().unwrap().contains("TMDB"));
}
