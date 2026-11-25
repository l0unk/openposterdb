//! Fork: per-title override store, uploads and candidates API
//! (`api/src/louk/overrides/`). No test here reaches TMDB or fanart.tv.

mod common;

use std::io::Cursor;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use openposterdb_api::cache::ImageType;
use openposterdb_api::louk::overrides::{OverrideKey, OverrideSource, TitleOverride, store};

// --- helpers --------------------------------------------------------------

async fn send_raw(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    content_type: Option<&str>,
    body: Body,
) -> (StatusCode, Vec<u8>, Option<String>) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    if let Some(ct) = content_type {
        req = req.header("content-type", ct);
    }
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let ct = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body, ct)
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    json: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let (ct, body) = match json {
        Some(j) => (Some("application/json"), Body::from(j.to_string())),
        None => (None, Body::empty()),
    };
    let (status, bytes, _) = send_raw(app, method, uri, token, ct, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}

/// A PNG with pseudo-random pixels, so every call gives new bytes.
fn png(width: u32, height: u32) -> Vec<u8> {
    let seed: u32 = rand::random();
    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        let v = seed
            .wrapping_add(x.wrapping_mul(31))
            .wrapping_add(y.wrapping_mul(17));
        image::Rgba([v as u8, (v >> 8) as u8, (v >> 16) as u8, 255])
    });
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

async fn upload(
    app: &axum::Router,
    token: &str,
    bytes: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let (status, body, _) = send_raw(
        app,
        "POST",
        "/api/admin/louk/uploads",
        Some(token),
        Some("image/png"),
        Body::from(bytes),
    )
    .await;
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

// --- uploads ----------------------------------------------------------------

#[tokio::test]
async fn upload_rejects_non_image() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    // Text with an image content type: the bytes decide, not the header.
    let (status, json) = upload(&app, &token, b"definitely not an image".to_vec()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].as_str().unwrap().contains("PNG"), "{json}");
    // GIF and SVG are not accepted either.
    let (status, _) = upload(
        &app,
        &token,
        b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = upload(
        &app,
        &token,
        b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A multipart body gets a hint.
    let (status, body, _) = send_raw(
        &app,
        "POST",
        "/api/admin/louk/uploads",
        Some(&token),
        Some("multipart/form-data; boundary=x"),
        Body::from("--x\r\n\r\nhello\r\n--x--"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("multipart"));
}

#[tokio::test]
async fn upload_rejects_over_15mb() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    // Streamed body without Content-Length: cut off while reading.
    let mut big = png(8, 8);
    big.resize(15 * 1024 * 1024 + 1, 0);
    let (status, json) = upload(&app, &token, big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(json["error"].as_str().unwrap().contains("15 MiB"), "{json}");
    // Declared Content-Length over the limit: refused before reading.
    let req = Request::builder()
        .method("POST")
        .uri("/api/admin/louk/uploads")
        .header("authorization", format!("Bearer {token}"))
        .header("content-length", (20 * 1024 * 1024).to_string())
        .body(Body::from(png(4, 4)))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    // Exactly at the limit is not "too large" (it is just not an image here).
    let (status, _) = upload(&app, &token, vec![0u8; 15 * 1024 * 1024]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_rejects_9000px() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let (status, json) = upload(&app, &token, png(9000, 10)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["error"].as_str().unwrap().contains("9000x10"),
        "{json}"
    );
}

#[tokio::test]
async fn upload_rejects_undecodable() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let mut broken = png(64, 64);
    broken.truncate(60);
    let (status, json) = upload(&app, &token, broken).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["error"].as_str().unwrap().contains("decoded"),
        "{json}"
    );
    let (status, _) = upload(
        &app,
        &token,
        b"\xFF\xD8\xFF\xE0garbage that is no jpeg".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_accepts_png_and_dedups() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let bytes = png(64, 32);
    let (status, first) = upload(&app, &token, bytes.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let sha = first["sha256"].as_str().unwrap().to_owned();
    assert_eq!(sha.len(), 64);
    assert_eq!(first["format"], "png");
    assert_eq!(first["width"], 64);
    assert_eq!(first["height"], 32);
    assert_eq!(first["bytes"], bytes.len());
    assert_eq!(first["created"], true);
    assert_eq!(first["url"], format!("/api/admin/louk/uploads/{sha}"));

    let (status, second) = upload(&app, &token, bytes.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["sha256"], sha.as_str());
    assert_eq!(second["created"], false);

    // Served back (admin only) with the sniffed content type.
    let (status, body, ct) = send_raw(
        &app,
        "GET",
        &format!("/api/admin/louk/uploads/{sha}"),
        Some(&token),
        None,
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
    assert_eq!(ct.as_deref(), Some("image/png"));
    let (status, _) = send(
        &app,
        "GET",
        &format!("/api/admin/louk/uploads/{}", "0".repeat(64)),
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    for bad in ["abc", "..%2F..%2Fetc%2Fpasswd", &"A".repeat(64)] {
        let (status, _) = send(
            &app,
            "GET",
            &format!("/api/admin/louk/uploads/{bad}"),
            Some(&token),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

// --- overrides ----------------------------------------------------------------

#[tokio::test]
async fn override_roundtrip_put_get_delete() {
    let (app, state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let uri = "/api/admin/louk/overrides/movie/9100001/logo";

    let (status, json) = send(&app, "GET", uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json["override"].is_null());

    let (status, json) = send(
        &app,
        "PUT",
        uri,
        Some(&token),
        Some(serde_json::json!({"source": "tmdb", "value": "/first.png", "no_badges": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    let ov = &json["override"];
    assert_eq!(ov["media_type"], "movie");
    assert_eq!(ov["tmdb_id"], 9100001);
    assert_eq!(ov["kind"], "logo");
    assert!(ov["season"].is_null() && ov["episode"].is_null() && ov["lang"].is_null());
    assert_eq!(ov["source"], "tmdb");
    assert_eq!(ov["value"], "/first.png");
    assert!(ov["fanart_id"].is_null());
    assert_eq!(ov["no_badges"], true);
    assert!(ov["updated_at"].as_i64().unwrap() > 0);
    assert_eq!(ov["thumb_url"], "https://image.tmdb.org/t/p/w300/first.png");

    let (status, json) = send(&app, "GET", uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["override"]["value"], "/first.png");

    // Replace with an upload.
    let (_, up) = upload(&app, &token, png(20, 10)).await;
    let sha = up["sha256"].as_str().unwrap().to_owned();
    let (status, json) = send(
        &app,
        "PUT",
        uri,
        Some(&token),
        Some(serde_json::json!({"source": "upload", "value": sha})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["override"]["no_badges"], false);
    assert_eq!(
        json["override"]["thumb_url"],
        format!("/api/admin/louk/uploads/{sha}")
    );

    // Persisted in the DB: a fresh load sees it.
    state.louk.overrides.reload(&state.db).await.unwrap();
    let key = OverrideKey::parse("movie", "9100001", "logo", None, None, None).unwrap();
    let got = store(&state)
        .get_exact(&state.db, &key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        got.source,
        OverrideSource::Upload {
            sha256: sha.clone()
        }
    );

    // Title listing.
    let (status, json) = send(
        &app,
        "GET",
        "/api/admin/louk/overrides/movie/9100001",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["overrides"].as_array().unwrap().len(), 1);

    let (status, json) = send(&app, "DELETE", uri, Some(&token), None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["deleted"], true);
    let (_, json) = send(&app, "DELETE", uri, Some(&token), None).await;
    assert_eq!(json["deleted"], false);
    let (_, json) = send(&app, "GET", uri, Some(&token), None).await;
    assert!(json["override"].is_null());
    state.louk.overrides.reload(&state.db).await.unwrap();
    assert!(
        store(&state)
            .get_exact(&state.db, &key)
            .await
            .unwrap()
            .is_none()
    );

    // Episode stills carry season/episode.
    let ep_uri = "/api/admin/louk/overrides/tv/9100002/episode?season=1&episode=2";
    let (status, json) = send(
        &app,
        "PUT",
        ep_uri,
        Some(&token),
        Some(serde_json::json!({"source": "fanart", "value": "https://assets.fanart.tv/fanart/tv/1/x.jpg", "fanart_id": "77"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["override"]["season"], 1);
    assert_eq!(json["override"]["episode"], 2);
    assert_eq!(json["override"]["fanart_id"], "77");
    assert_eq!(
        json["override"]["thumb_url"],
        "https://assets.fanart.tv/preview/tv/1/x.jpg"
    );
}

#[tokio::test]
async fn override_put_validation() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let uri = "/api/admin/louk/overrides/movie/9100003/poster";
    for body in [
        serde_json::json!({"source": "tmdb", "value": "../x.png"}),
        serde_json::json!({"source": "tmdb", "value": "/x.svg"}),
        serde_json::json!({"source": "fanart", "value": "https://example.com/x.png", "fanart_id": "1"}),
        serde_json::json!({"source": "upload", "value": "0".repeat(64)}),
        serde_json::json!({"source": "bogus", "value": "x"}),
        serde_json::json!({"value": "/x.png"}),
    ] {
        let (status, json) = send(&app, "PUT", uri, Some(&token), Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} → {json}");
        assert!(json["error"].is_string());
    }
    for bad in [
        "/api/admin/louk/overrides/show/1/poster",
        "/api/admin/louk/overrides/movie/x/poster",
        "/api/admin/louk/overrides/movie/1/thumb",
        "/api/admin/louk/overrides/movie/1/episode?season=1&episode=1",
        "/api/admin/louk/overrides/tv/1/episode?season=1",
        "/api/admin/louk/overrides/movie/1/backdrop?lang=en",
    ] {
        let (status, _) = send(
            &app,
            "PUT",
            bad,
            Some(&token),
            Some(serde_json::json!({"source": "auto"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    // `auto` + no_badges: keep the automatic art, drop the badges.
    let (status, json) = send(
        &app,
        "PUT",
        uri,
        Some(&token),
        Some(serde_json::json!({"source": "auto", "no_badges": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["override"]["source"], "auto");
    assert_eq!(json["override"]["value"], "");
    assert!(json["override"]["thumb_url"].is_null());
}

async fn look(
    st: &openposterdb_api::louk::overrides::store::OverrideStore,
    db: &sea_orm::DatabaseConnection,
    lang: &str,
) -> Option<String> {
    match st
        .lookup(db, "movie", 9_100_004, ImageType::Logo, -1, -1, lang)
        .await
        .map(|o| o.source)
    {
        Some(OverrideSource::Tmdb { file_path }) => Some(file_path),
        _ => None,
    }
}

#[tokio::test]
async fn override_lang_precedence() {
    let (_app, state) = common::setup_test_app().await;
    let st = store(&state);
    let set = |lang: &str, path: &str| {
        let key = OverrideKey::parse("movie", "9100004", "logo", None, None, Some(lang)).unwrap();
        let ov = TitleOverride {
            source: OverrideSource::Tmdb {
                file_path: path.into(),
            },
            no_badges: false,
            updated_at: 1,
        };
        (key, ov)
    };
    assert_eq!(look(st, &state.db, "en").await, None);
    let (k_any, o_any) = set("", "/any.png");
    st.set(&state.db, &k_any, &o_any).await.unwrap();
    assert_eq!(look(st, &state.db, "en").await.as_deref(), Some("/any.png"));
    let (k_de, o_de) = set("de", "/de.png");
    st.set(&state.db, &k_de, &o_de).await.unwrap();
    assert_eq!(
        look(st, &state.db, "de").await.as_deref(),
        Some("/de.png"),
        "exact lang beats ''"
    );
    assert_eq!(
        look(st, &state.db, "de-AT").await.as_deref(),
        Some("/de.png"),
        "base lang beats ''"
    );
    assert_eq!(look(st, &state.db, "en").await.as_deref(), Some("/any.png"));
    let (k_deat, o_deat) = set("de-AT", "/de-at.png");
    st.set(&state.db, &k_deat, &o_deat).await.unwrap();
    assert_eq!(
        look(st, &state.db, "de-AT").await.as_deref(),
        Some("/de-at.png")
    );
    // Other kinds and titles are not affected.
    assert!(
        st.lookup(
            &state.db,
            "movie",
            9_100_004,
            ImageType::Poster,
            -1,
            -1,
            "de"
        )
        .await
        .is_none()
    );
    assert!(
        st.lookup(&state.db, "tv", 9_100_004, ImageType::Logo, -1, -1, "de")
            .await
            .is_none()
    );
    // Backdrops ignore the request language.
    let bk = OverrideKey::parse("movie", "9100004", "backdrop", None, None, None).unwrap();
    st.set(&state.db, &bk, &o_any).await.unwrap();
    assert!(
        st.lookup(
            &state.db,
            "movie",
            9_100_004,
            ImageType::Backdrop,
            -1,
            -1,
            "de"
        )
        .await
        .is_some()
    );
    assert!(st.clear(&state.db, &k_de).await.unwrap());
    assert_eq!(look(st, &state.db, "de").await.as_deref(), Some("/any.png"));
    assert_eq!(st.len(), 3);
}

#[tokio::test]
async fn override_rows_load_from_db() {
    let (_app, state) = common::setup_test_app().await;
    use sea_orm::ConnectionTrait;
    // Minimal insert: every other column takes its DEFAULT.
    state
        .db
        .execute_unprepared(
            "INSERT INTO louk_overrides (media_type, tmdb_id, kind, source, value) VALUES ('tv', 9100005, 'poster', 'tmdb', '/p.jpg')",
        )
        .await
        .unwrap();
    state.louk.overrides.reload(&state.db).await.unwrap();
    let got = store(&state)
        .lookup(&state.db, "tv", 9_100_005, ImageType::Poster, -1, -1, "en")
        .await
        .unwrap();
    assert_eq!(
        got.source,
        OverrideSource::Tmdb {
            file_path: "/p.jpg".into()
        }
    );
    assert!(!got.no_badges);
    assert!(!store(&state).any_no_badges(&state.db).await);
}

#[tokio::test]
async fn override_routes_require_admin() {
    let (app, _state) = common::setup_test_app().await;
    let _token = common::setup_admin(&app).await;
    for (method, uri) in [
        ("GET", "/api/admin/louk/overrides/movie/1"),
        ("GET", "/api/admin/louk/overrides/movie/1/logo"),
        ("PUT", "/api/admin/louk/overrides/movie/1/logo"),
        ("DELETE", "/api/admin/louk/overrides/movie/1/logo"),
        ("POST", "/api/admin/louk/uploads"),
        (
            "GET",
            "/api/admin/louk/uploads/0000000000000000000000000000000000000000000000000000000000000000",
        ),
        ("GET", "/api/admin/louk/candidates/movie/1/logo"),
    ] {
        let (status, _, _) = send_raw(&app, method, uri, None, None, Body::empty()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        let (status, _, _) =
            send_raw(&app, method, uri, Some("not-a-token"), None, Body::empty()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} bad token");
    }
}

#[tokio::test]
async fn candidates_reject_bad_parameters() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    // All rejected before any TMDB call.
    for bad in [
        "/api/admin/louk/candidates/show/1/logo",
        "/api/admin/louk/candidates/movie/0/logo",
        "/api/admin/louk/candidates/movie/1/thumb",
        "/api/admin/louk/candidates/movie/1/logo?lang=english",
        "/api/admin/louk/candidates/movie/1/episode?season=1&episode=1",
        "/api/admin/louk/candidates/tv/1/episode",
        "/api/admin/louk/candidates/tv/1/poster?season=1",
    ] {
        let (status, json) = send(&app, "GET", bad, Some(&token), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} → {json}");
    }
}

// --- fix round: language shadowing, load-failure backoff -------------------

#[tokio::test]
async fn put_reports_language_rows_that_still_win() {
    let (app, _state) = common::setup_test_app().await;
    let token = common::setup_admin(&app).await;
    let put = |lang: &'static str, path: &'static str| {
        let app = app.clone();
        let token = token.clone();
        async move {
            let q = if lang.is_empty() {
                String::new()
            } else {
                format!("?lang={lang}")
            };
            let (status, json) = send(
                &app,
                "PUT",
                &format!("/api/admin/louk/overrides/movie/9100010/logo{q}"),
                Some(&token),
                Some(serde_json::json!({"source": "tmdb", "value": path})),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{json}");
            json
        }
    };
    let json = put("en", "/en.png").await;
    assert_eq!(json["shadowed_by"].as_array().unwrap().len(), 0);
    // An any-language row is shadowed by the English one for English requests.
    let json = put("", "/any.png").await;
    let shadow = json["shadowed_by"].as_array().unwrap();
    assert_eq!(shadow.len(), 1, "{json}");
    assert_eq!(shadow[0]["lang"], "en");
    assert_eq!(shadow[0]["value"], "/en.png");
    // A base-language row is shadowed only by its regional rows.
    put("pt-BR", "/pt-br.png").await;
    let json = put("pt", "/pt.png").await;
    let shadow = json["shadowed_by"].as_array().unwrap();
    assert_eq!(shadow.len(), 1, "{json}");
    assert_eq!(shadow[0]["lang"], "pt-BR");
    let json = put("pt-BR", "/pt-br2.png").await;
    assert_eq!(json["shadowed_by"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn store_load_failure_backs_off() {
    let (_app, state) = common::setup_test_app().await;
    use sea_orm::ConnectionTrait;
    state
        .db
        .execute_unprepared("ALTER TABLE louk_overrides RENAME TO louk_overrides_away")
        .await
        .unwrap();
    // The first load fails: no override, no error.
    assert!(
        store(&state)
            .lookup(&state.db, "movie", 9_100_011, ImageType::Logo, -1, -1, "en")
            .await
            .is_none()
    );
    state
        .db
        .execute_unprepared("ALTER TABLE louk_overrides_away RENAME TO louk_overrides")
        .await
        .unwrap();
    state
        .db
        .execute_unprepared(
            "INSERT INTO louk_overrides (media_type, tmdb_id, kind, source, value) VALUES ('movie', 9100011, 'logo', 'tmdb', '/x.png')",
        )
        .await
        .unwrap();
    // Within the retry pause the table is not queried again.
    assert!(
        store(&state)
            .lookup(&state.db, "movie", 9_100_011, ImageType::Logo, -1, -1, "en")
            .await
            .is_none()
    );
    // An explicit reload (or the next retry after 30 s) sees it.
    store(&state).reload(&state.db).await.unwrap();
    assert!(
        store(&state)
            .lookup(&state.db, "movie", 9_100_011, ImageType::Logo, -1, -1, "en")
            .await
            .is_some()
    );
}
