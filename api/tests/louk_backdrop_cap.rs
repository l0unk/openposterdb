//! Fork: `BACKDROP_MAX_WIDTH` (see `api/src/louk/backdrop_cap.rs`).
//!
//! Pins Review Focus 1 and 2 of the fork plan: with the cap unset every
//! backdrop key, hash and render equals upstream; a cap that does not reduce
//! the native width (small/medium with 1920) adds nothing; a reducing cap adds
//! `.mw<N>` right before the image-size token and renders at the cap.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use openposterdb_api::cache::{self, ImageType};
use openposterdb_api::image::{generate, serve};
use openposterdb_api::louk::backdrop_cap::{self, backdrop_suffix};
use openposterdb_api::louk::config::LoukConfig;
use openposterdb_api::services::db::{self, ImageSize, RenderSettings};
use openposterdb_api::services::ratings::{self, RatingBadge, RatingSource};

const ALL: [ImageSize; 4] = [ImageSize::Small, ImageSize::Medium, ImageSize::Large, ImageSize::VeryLarge];

fn capped(cap: u32) -> LoukConfig {
    LoukConfig { backdrop_max_width: Some(cap), ..LoukConfig::default() }
}

/// Upstream's backdrop suffix for default settings and a typical badge set.
fn upstream_suffix(settings: &RenderSettings, size: Option<ImageSize>, ratings_suffix: &str) -> String {
    serve::settings_cache_suffix_with_ratings(settings, ImageType::Backdrop, size, ratings_suffix)
}

// --- cache keys -----------------------------------------------------------

#[test]
fn backdrop_cap_unset_keeps_upstream_cache_key() {
    let settings = RenderSettings::default();
    for rs in ["@", "@ir", "@irl"] {
        for size in ALL.map(Some).into_iter().chain([None]) {
            let s = upstream_suffix(&settings, size, rs);
            assert_eq!(backdrop_suffix(&LoukConfig::default(), ImageType::Backdrop, size, s.clone()), s, "{size:?} {rs}");
            // Other kinds are never touched, capped or not.
            for kind in [ImageType::Poster, ImageType::Logo, ImageType::Episode] {
                let k = serve::settings_cache_suffix_with_ratings(&settings, kind, size, rs);
                assert_eq!(backdrop_suffix(&capped(1920), kind, size, k.clone()), k, "{kind:?} {size:?}");
            }
        }
    }
    // CDN hash unchanged too.
    for size in ALL.map(Some).into_iter().chain([None]) {
        let h = serve::settings_hash(&settings, ImageType::Backdrop, size);
        assert_eq!(backdrop_cap::cdn_hash(&LoukConfig::default(), ImageType::Backdrop, size, h.clone()), h);
    }
}

#[test]
fn backdrop_cap_small_medium_1920_no_token_same_key() {
    let settings = RenderSettings::default();
    for size in [Some(ImageSize::Small), Some(ImageSize::Medium), None] {
        let s = upstream_suffix(&settings, size, "@ir");
        assert_eq!(backdrop_suffix(&capped(1920), ImageType::Backdrop, size, s.clone()), s, "{size:?}");
        let h = serve::settings_hash(&settings, ImageType::Backdrop, size);
        assert_eq!(backdrop_cap::cdn_hash(&capped(1920), ImageType::Backdrop, size, h.clone()), h, "{size:?}");
    }
}

#[test]
fn backdrop_cap_large_token_sits_before_size_token() {
    let mut settings = RenderSettings::default();
    for (size, tail) in [(ImageSize::Large, ".zl"), (ImageSize::VeryLarge, ".zvl")] {
        for inset in [0, 5] {
            settings.backdrop_edge_inset_y = inset;
            let s = upstream_suffix(&settings, Some(size), "@ir");
            let got = backdrop_suffix(&capped(1920), ImageType::Backdrop, Some(size), s.clone());
            let head = s.strip_suffix(tail).expect("upstream suffix ends with the size token");
            assert_eq!(got, format!("{head}.mw1920{tail}"));
            assert!(got.ends_with(&format!(".mw1920{tail}")));
            let h = serve::settings_hash(&settings, ImageType::Backdrop, Some(size));
            assert_ne!(backdrop_cap::cdn_hash(&capped(1920), ImageType::Backdrop, Some(size), h.clone()), h);
        }
    }
    // A cap at or above the native width changes nothing.
    let s = upstream_suffix(&RenderSettings::default(), Some(ImageSize::Large), "@ir");
    assert_eq!(backdrop_suffix(&capped(3840), ImageType::Backdrop, Some(ImageSize::Large), s.clone()), s);
}

// --- rendering ------------------------------------------------------------

/// A wide, short synthetic source (4:1) keeps debug-build renders cheap while
/// still checking the aspect ratio.
fn source_png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 251) as u8, (y % 241) as u8, ((x + y) % 239) as u8, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

fn badges() -> Vec<RatingBadge> {
    vec![
        RatingBadge { source: RatingSource::Imdb, value: "8.7".into() },
        RatingBadge { source: RatingSource::Tmdb, value: "82%".into() },
        RatingBadge { source: RatingSource::Rt, value: "91%".into() },
    ]
}

/// Render a backdrop the way `serve.rs` does, with `(target_width, badge_scale)`.
fn render(src: &[u8], dims: (u32, f32)) -> Vec<u8> {
    let s = RenderSettings::default();
    let font = ab_glyph::FontArc::try_from_slice(openposterdb_api::FONT_BYTES).unwrap();
    let position = s.backdrop_position;
    let direction = s.backdrop_badge_direction.resolve(position);
    generate::render_backdrop_sync(
        src, &badges(), &font, 85, position, s.backdrop_badge_style, s.backdrop_label_style,
        s.backdrop_appearance(), direction, dims.0, dims.1, s.backdrop_badge_size, 0, 0,
    )
    .unwrap()
}

fn dims_of(jpeg: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(jpeg).unwrap();
    (img.width(), img.height())
}

#[test]
fn backdrop_cap_large_renders_1920_like_upstream_medium() {
    let src = source_png(1000, 250);
    let factor = RenderSettings::default().backdrop_badge_size.scale_factor();
    let up_medium = (ImageSize::Medium.backdrop_target_width(), ImageSize::Medium.badge_scale(ImageType::Backdrop) * factor);
    let up_render = render(&src, up_medium);
    for size in [ImageSize::Large, ImageSize::VeryLarge] {
        let dims = backdrop_cap::render_dims(&capped(1920), size, factor);
        let out = render(&src, dims);
        assert_eq!(dims_of(&out), (1920, 480), "{size:?}: 1920 wide, 4:1 kept");
        // Same canvas and badge scale as upstream's 1920 render: badges keep
        // the size relative to the image that upstream gives every size.
        assert!(out == up_render, "{size:?}: capped render differs from upstream medium");
    }
}

#[test]
fn backdrop_cap_unset_renders_like_upstream() {
    let src = source_png(1000, 250);
    let factor = RenderSettings::default().backdrop_badge_size.scale_factor();
    for size in [ImageSize::Small, ImageSize::Large] {
        let upstream = (size.backdrop_target_width(), size.badge_scale(ImageType::Backdrop) * factor);
        let ours = backdrop_cap::render_dims(&LoukConfig::default(), size, factor);
        assert_eq!(ours.0, upstream.0);
        assert_eq!(ours.1.to_bits(), upstream.1.to_bits());
        let out = render(&src, ours);
        assert!(out == render(&src, upstream), "{size:?}");
        assert_eq!(dims_of(&out), (size.backdrop_target_width(), size.backdrop_target_width() / 4));
    }
}

// --- routes ---------------------------------------------------------------

async fn create_api_key(app: &axum::Router, token: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/keys")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(serde_json::json!({"name": "louk-cap"}).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["key"].as_str().unwrap().to_string()
}

/// The settings the image handler uses for `raw_key` (same calls as
/// `handlers::image::resolve_settings`), with `?ratings_limit=0` applied.
async fn effective_settings(state: &openposterdb_api::AppState, raw_key: &str) -> RenderSettings {
    let hash = openposterdb_api::handlers::auth::hash_api_key(raw_key);
    let key = db::find_api_key_by_hash(&state.db, &hash).await.unwrap().expect("key exists");
    let globals = db::parse_global_render_settings(&db::get_global_settings(&state.db).await.unwrap());
    let mut s = db::get_effective_render_settings(&state.db, key.id, Some(&globals)).await;
    s.backdrop_ratings_limit = 0;
    s
}

/// The on-disk file name (`{id_value}{variant}{suffix}`) the fast path looks up.
fn cache_value(cfg: &LoukConfig, s: &RenderSettings, size: ImageSize) -> String {
    let rs = ratings::badges_suffix_from_available("", &s.ratings_order, &s.ratings_exclude, 0);
    let suffix = backdrop_suffix(cfg, ImageType::Backdrop, Some(size), upstream_suffix(s, Some(size), &rs));
    format!("movie-603_b_t{suffix}")
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Vec<u8>, Option<String>) {
    let res = app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let location = res.headers().get("location").map(|v| v.to_str().unwrap().to_string());
    (status, res.into_body().collect().await.unwrap().to_bytes().to_vec(), location)
}

#[tokio::test]
async fn backdrop_cap_serves_capped_key_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().to_string_lossy().to_string();

    for (cfg, want_large) in [(capped(1920), b"capped".as_slice()), (LoukConfig::default(), b"upstream".as_slice())] {
        let (app, state) = common::setup_test_app_with_options(common::TestAppOptions {
            cache_dir_override: Some(cache_dir.clone()),
            louk: cfg.clone(),
            ..Default::default()
        })
        .await;
        let token = common::setup_admin(&app).await;
        let key = create_api_key(&app, &token).await;
        let s = effective_settings(&state, &key).await;

        // Seed upstream (uncapped) keys for large and medium, and the capped
        // large key, each with distinct bytes.
        let up_large = cache_value(&LoukConfig::default(), &s, ImageSize::Large);
        let cap_large = cache_value(&capped(1920), &s, ImageSize::Large);
        let medium = cache_value(&LoukConfig::default(), &s, ImageSize::Medium);
        assert_ne!(up_large, cap_large);
        assert!(cap_large.ends_with(".mw1920.zl"), "{cap_large}");
        assert_eq!(cache_value(&capped(1920), &s, ImageSize::Medium), medium, "medium has no token");
        for (value, bytes) in [(&up_large, b"upstream".as_slice()), (&cap_large, b"capped"), (&medium, b"medium")] {
            let p = cache::typed_cache_path(&cache_dir, ImageType::Backdrop, "tmdb", value).unwrap();
            cache::write(&p, bytes).await.unwrap();
        }

        let (status, body, _) = get(&app, &format!("/{key}/tmdb/backdrop-default/movie-603.jpg?imageSize=large&ratings_limit=0")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, want_large, "large with {cfg:?}");

        let (status, body, _) = get(&app, &format!("/{key}/tmdb/backdrop-default/movie-603.jpg?imageSize=medium&ratings_limit=0")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, b"medium", "medium with {cfg:?}");
    }
}

#[tokio::test]
async fn backdrop_cap_cdn_redirect_hash() {
    let mut locations = Vec::new();
    for cfg in [LoukConfig::default(), capped(1920)] {
        let (app, _state) = common::setup_test_app_with_options(common::TestAppOptions {
            enable_cdn_redirects: true,
            louk: cfg,
            ..Default::default()
        })
        .await;
        let token = common::setup_admin(&app).await;
        let key = create_api_key(&app, &token).await;
        let mut per_size = Vec::new();
        for size in ["large", "medium"] {
            let (status, _, location) = get(&app, &format!("/{key}/tmdb/backdrop-default/movie-603.jpg?imageSize={size}")).await;
            assert_eq!(status, StatusCode::FOUND, "{size}");
            per_size.push(location.unwrap());
        }
        locations.push(per_size);
    }
    let (up, cap) = (&locations[0], &locations[1]);
    assert_ne!(up[0], cap[0], "large: capped renders get their own CDN URL");
    assert_eq!(up[1], cap[1], "medium: same CDN URL as upstream");
}

#[tokio::test]
async fn backdrop_cap_preview_renders_capped_width() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().to_string_lossy().to_string();
    let (app, _state) = common::setup_test_app_with_options(common::TestAppOptions {
        cache_dir_override: Some(cache_dir),
        louk: capped(1920),
        ..Default::default()
    })
    .await;
    let token = common::setup_admin(&app).await;
    for (size, want) in [("large", (1920, 1080)), ("medium", (1920, 1080)), ("small", (1280, 720))] {
        let req = Request::builder()
            .uri(format!("/api/admin/preview/backdrop?imageSize={size}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "{size}");
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(dims_of(&body), want, "{size}");
    }
}
