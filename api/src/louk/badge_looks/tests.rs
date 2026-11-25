use super::*;
use crate::cache::ImageType;
use crate::image::{badge, generate, serve};
use crate::services::db::{
    BadgeDirection, BadgePosition, BadgeSize, BadgeStyle, PosterFit, RenderSettings,
};

fn font() -> FontArc {
    FontArc::try_from_slice(crate::FONT_BYTES).unwrap()
}

fn rb(source: RatingSource, value: &str) -> RatingBadge {
    RatingBadge {
        source,
        value: value.to_string(),
    }
}

/// 0, 1, 5 and 8 badges, with long ("100%", "10.00"), empty and "N/A" values.
fn badge_sets() -> Vec<Vec<RatingBadge>> {
    let all = vec![
        rb(RatingSource::Mal, "10.00"),
        rb(RatingSource::Imdb, "8.5"),
        rb(RatingSource::Letterboxd, "3.4"),
        rb(RatingSource::Rt, "100%"),
        rb(RatingSource::Metacritic, "47"),
        rb(RatingSource::RtAudience, "N/A"),
        rb(RatingSource::Tmdb, ""),
        rb(RatingSource::Ebert, "4.0"),
    ];
    vec![vec![], all[..1].to_vec(), all[..5].to_vec(), all.clone()]
}

const LABELS: [LabelStyle; 3] = [LabelStyle::Official, LabelStyle::Icon, LabelStyle::Text];
const SHAPES: [BadgeShape; 2] = [BadgeShape::Rounded, BadgeShape::Pill];

fn appearance(background: BadgeBackground, shape: BadgeShape) -> BadgeAppearance {
    BadgeAppearance { shape, background }
}

fn solid_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = RgbaImage::from_pixel(w, h, Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut buf = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut buf),
        img.as_raw(),
        w,
        h,
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
    buf
}

/// Bounding box (x0, y0, x1, y1 inclusive) of pixels that differ by more
/// than `tol` in any channel between two decoded images of equal size.
fn diff_bbox(a: &[u8], b: &[u8], tol: i32) -> Option<(u32, u32, u32, u32)> {
    let a = image::load_from_memory(a).unwrap().to_rgba8();
    let b = image::load_from_memory(b).unwrap().to_rgba8();
    assert_eq!(a.dimensions(), b.dimensions());
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for (x, y, pa) in a.enumerate_pixels() {
        let pb = b.get_pixel(x, y);
        if (0..4).any(|i| (pa.0[i] as i32 - pb.0[i] as i32).abs() > tol) {
            bbox = Some(match bbox {
                None => (x, y, x, y),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
            });
        }
    }
    bbox
}

// --- Defaults and tokens ---

#[test]
fn louk_defaults_do_not_change_cache_keys() {
    // Literals are v1.3.0's default suffixes (same shape as tests/upgrade.rs).
    let s = RenderSettings::default();
    assert_eq!(
        serve::settings_cache_suffix(&s, ImageType::Poster, None),
        "@mil.pbc.sd.lo.dd.bm.shr.bgd.zm"
    );
    assert_eq!(
        serve::settings_cache_suffix(&s, ImageType::Logo, None),
        "@milrc.sv.lo.bm.shr.bgd.zm"
    );
    assert_eq!(
        serve::settings_cache_suffix(&s, ImageType::Backdrop, None),
        "@milrc.ptr.sv.lo.dd.bm.shr.bgd.zm"
    );
    assert_eq!(
        serve::settings_cache_suffix(&s, ImageType::Episode, None),
        "@m.ptr.sv.lo.dv.bl.shr.bgd.zm"
    );
    // The default background stays upstream's and is not a fork look.
    assert_eq!(
        crate::services::db::default_badge_background(),
        BadgeBackground::Default
    );
    assert!(!is_fork_look(BadgeBackground::Default));
    // Upstream looks never reach the fork renderer.
    let f = font();
    let badges = vec![rb(RatingSource::Imdb, "8.5"), rb(RatingSource::Rt, "91%")];
    for bg in [
        BadgeBackground::Default,
        BadgeBackground::Dark,
        BadgeBackground::Transparent,
        BadgeBackground::None,
    ] {
        let a = appearance(bg, BadgeShape::Rounded);
        assert!(render_row(&badges, &f, LabelStyle::Official, a, 1.0).is_none());
        assert!(render_vertical(&badges[0], &f, LabelStyle::Official, a, 1.0).is_none());
        assert!(preview_samples(bg).is_none());
        assert!(matches!(collapse(&badges, a), Cow::Borrowed(_)));
    }
}

#[test]
fn fork_tokens_are_distinct_stable_and_roundtrip() {
    let upstream = ["d", "k", "t", "n"];
    let mut seen = Vec::new();
    for (bg, token) in FORK_LOOKS.iter().zip(["s", "m", "a"]) {
        assert_eq!(
            bg.as_str(),
            token,
            "token for {bg:?} must stay stable (it is in cache keys)"
        );
        assert_eq!(token.len(), 1);
        assert!(!upstream.contains(&token));
        assert!(!seen.contains(&token));
        seen.push(token);
        assert_eq!(BadgeBackground::parse(token).unwrap(), *bg);
        let json = serde_json::to_string(bg).unwrap();
        assert_eq!(json, format!("\"{token}\""));
        assert_eq!(serde_json::from_str::<BadgeBackground>(&json).unwrap(), *bg);
        assert!(is_fork_look(*bg));
    }
    assert!(BadgeBackground::parse("z").is_err());
    assert!(BadgeBackground::parse("").is_err());
}

#[test]
fn fork_looks_get_their_own_cache_token_per_kind() {
    for bg in FORK_LOOKS {
        let mut s = RenderSettings::default();
        s.poster_badge_background = bg;
        s.logo_badge_background = bg;
        s.backdrop_badge_background = bg;
        s.episode_badge_background = bg;
        let token = format!(".bg{}.", bg.as_str());
        for kind in [
            ImageType::Poster,
            ImageType::Logo,
            ImageType::Backdrop,
            ImageType::Episode,
        ] {
            let suffix = serve::settings_cache_suffix(&s, kind, None);
            assert!(suffix.contains(&token), "{kind:?} {suffix}");
            assert!(suffix.ends_with(".zm"), "size token stays last: {suffix}");
            let default = serve::settings_cache_suffix(&RenderSettings::default(), kind, None);
            assert_ne!(suffix, default);
            assert_eq!(
                suffix.replace(&token, ".bgd."),
                default,
                "only the background token differs"
            );
            assert_ne!(
                serve::settings_hash(&s, kind, None),
                serve::settings_hash(&RenderSettings::default(), kind, None)
            );
        }
    }
}

// --- Scores ---

#[test]
fn normalized_score_uses_each_sources_scale() {
    let n = |s, v| normalized_score(&rb(s, v));
    let close = |a: Option<f32>, b: f32| (a.unwrap() - b).abs() < 1e-4;
    assert!(close(n(RatingSource::Imdb, "8.5"), 0.85));
    assert!(close(n(RatingSource::Mal, "8.50"), 0.85));
    assert!(close(n(RatingSource::Letterboxd, "4.0"), 0.80));
    assert!(close(n(RatingSource::Ebert, "4.0"), 1.0));
    assert!(close(n(RatingSource::Ebert, "3.0"), 0.75));
    assert!(close(n(RatingSource::Metacritic, "72"), 0.72));
    assert!(close(n(RatingSource::Mdblist, "58"), 0.58));
    assert!(close(n(RatingSource::Rt, "100%"), 1.0));
    assert!(close(n(RatingSource::Tmdb, "64%"), 0.64));
    assert!(close(n(RatingSource::Trakt, " 77 % "), 0.77));
    assert!(close(n(RatingSource::Imdb, "12"), 1.0), "clamped");
    assert_eq!(n(RatingSource::Imdb, ""), None);
    assert_eq!(n(RatingSource::Imdb, "N/A"), None);
    assert_eq!(n(RatingSource::Rt, "%"), None);
    assert_eq!(n(RatingSource::Imdb, "NaN"), None);
}

#[test]
fn bands_follow_the_spec_thresholds() {
    assert_eq!(band_color(0.70), HIGH);
    assert_eq!(band_color(0.95), HIGH);
    assert_eq!(band_color(0.69), MID);
    assert_eq!(band_color(0.50), MID);
    assert_eq!(band_color(0.49), LOW);
    assert_eq!(band_color(0.0), LOW);
}

#[test]
fn collapse_averages_the_shown_sources() {
    let a = appearance(BadgeBackground::Average, BadgeShape::Rounded);
    let badges = vec![
        rb(RatingSource::Imdb, "8.0"),
        rb(RatingSource::Rt, "60%"),
        rb(RatingSource::Letterboxd, "N/A"),
    ];
    let out = collapse(&badges, a);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].value, "7.0");
    assert!((normalized_score(&out[0]).unwrap() - 0.70).abs() < 1e-4);
    assert!(collapse(&[], a).is_empty());
    assert!(collapse(&[rb(RatingSource::Imdb, "N/A")], a).is_empty());
    assert_eq!(average_text(1.0), "10");
    assert_eq!(average_text(0.996), "10");
    assert_eq!(average_text(0.78), "7.8");
    assert_eq!(average_text(0.0), "0.0");
    // Score and Mono keep every badge.
    for bg in [BadgeBackground::Score, BadgeBackground::Mono] {
        assert_eq!(
            collapse(&badges, appearance(bg, BadgeShape::Rounded)).len(),
            3
        );
    }
}

#[test]
fn preview_samples_cover_every_source_and_all_bands() {
    for bg in FORK_LOOKS {
        let samples = preview_samples(bg).unwrap();
        assert_eq!(samples.len(), RatingSource::all_keys().len());
        for key in RatingSource::all_keys() {
            let src = RatingSource::from_key(key).unwrap();
            assert!(samples.iter().any(|b| b.source == src), "missing {key}");
        }
        // The poster preview (default order, limit 3) shows one of each band.
        let order = crate::services::db::default_ratings_order();
        let shown = crate::services::ratings::apply_rating_preferences(samples, &order, "", 3);
        let bands: Vec<_> = shown
            .iter()
            .map(|b| band_color(normalized_score(b).unwrap()))
            .collect();
        assert_eq!(bands, vec![HIGH, MID, LOW]);
    }
}

// --- Badge sprites ---

#[test]
fn every_look_label_shape_and_count_renders() {
    let f = font();
    for bg in FORK_LOOKS {
        for label in LABELS {
            for shape in SHAPES {
                for set in badge_sets() {
                    for scale in [0.1_f32, 0.16, 1.0, 2.0] {
                        let a = appearance(bg, shape);
                        let row = render_row(&set, &f, label, a, scale).unwrap();
                        let expected = if bg == BadgeBackground::Average {
                            usize::from(!set.is_empty())
                        } else {
                            set.len()
                        };
                        assert_eq!(
                            row.len(),
                            expected,
                            "{bg:?} {label:?} {shape:?} n={} scale={scale}",
                            set.len()
                        );
                        for img in &row {
                            assert!(img.width() > 0 && img.height() > 0);
                            assert!(img.pixels().any(|p| p.0[3] > 0), "{bg:?} draws something");
                        }
                        if bg != BadgeBackground::Average && row.len() > 1 {
                            // Uniform widths and one height per row, like upstream.
                            assert!(
                                row.iter().all(|i| i.width() == row[0].width()
                                    && i.height() == row[0].height())
                            );
                        }
                        for b in &set {
                            let v = render_vertical(b, &f, label, a, scale).unwrap();
                            assert!(v.width() > 0 && v.height() > 0);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn score_and_mono_keep_upstreams_badge_size() {
    let f = font();
    let badge = rb(RatingSource::Imdb, "8.5");
    for bg in [BadgeBackground::Score, BadgeBackground::Mono] {
        let h_row = render_row(
            std::slice::from_ref(&badge),
            &f,
            LabelStyle::Text,
            appearance(bg, BadgeShape::Rounded),
            1.0,
        )
        .unwrap();
        let h = &h_row[0];
        let up = badge::render_badges_uniform(
            std::slice::from_ref(&badge),
            &f,
            LabelStyle::Text,
            BadgeAppearance::default(),
            1.0,
        );
        assert_eq!(h.dimensions(), up[0].dimensions(), "{bg:?} horizontal");
        let v = render_vertical(
            &badge,
            &f,
            LabelStyle::Text,
            appearance(bg, BadgeShape::Rounded),
            1.0,
        )
        .unwrap();
        assert_eq!(v.width(), 88, "{bg:?} vertical width");
    }
}

#[test]
fn score_meter_fills_with_the_band_colour() {
    let f = font();
    let a = appearance(BadgeBackground::Score, BadgeShape::Rounded);
    let meter_px = |value: &str, x_share: f32| {
        let img_row = render_row(
            &[rb(RatingSource::Imdb, value)],
            &f,
            LabelStyle::Text,
            a,
            1.0,
        )
        .unwrap();
        let img = &img_row[0];
        let x = (img.width() as f32 * x_share) as u32;
        // Two rows above the bottom edge: inside the meter, clear of the hairline.
        *img.get_pixel(x, img.height() - 3)
    };
    let near = |p: Rgba<u8>, c: Rgba<u8>| {
        (0..3).all(|i| (p.0[i] as i32 - c.0[i] as i32).abs() <= 40) && p.0[3] > 200
    };
    assert!(near(meter_px("8.5", 0.5), HIGH));
    assert!(near(meter_px("5.5", 0.3), MID));
    assert!(near(meter_px("3.0", 0.2), LOW));
    // Past the fill the track shows, not the band colour.
    assert!(!near(meter_px("3.0", 0.6), LOW));
    // The corner is transparent (rounded, anti-aliased).
    let img_row = render_row(
        &[rb(RatingSource::Imdb, "8.5")],
        &f,
        LabelStyle::Text,
        a,
        1.0,
    )
    .unwrap();
    let img = &img_row[0];
    assert_eq!(img.get_pixel(0, 0).0[3], 0);
}

#[test]
fn mono_has_no_colour() {
    let f = font();
    let badges = vec![
        rb(RatingSource::Imdb, "8.5"),
        rb(RatingSource::Rt, "91%"),
        rb(RatingSource::Tmdb, "40%"),
    ];
    for label in LABELS {
        let a = appearance(BadgeBackground::Mono, BadgeShape::Rounded);
        let mut imgs = render_row(&badges, &f, label, a, 1.0).unwrap();
        imgs.extend(
            badges
                .iter()
                .map(|b| render_vertical(b, &f, label, a, 1.0).unwrap()),
        );
        for img in imgs {
            for p in img.pixels().filter(|p| p.0[3] > 128) {
                let (mx, mn) = (
                    p.0[..3].iter().max().unwrap(),
                    p.0[..3].iter().min().unwrap(),
                );
                assert!(
                    mx - mn <= 40,
                    "{label:?}: saturated pixel {p:?} in a mono badge"
                );
            }
        }
    }
}

#[test]
fn average_disc_centres_its_number() {
    let f = font();
    for (value, scale) in [("7.8", 1.0_f32), ("10.0", 1.0), ("2.4", 2.0), ("9.1", 0.5)] {
        let disc_row = render_row(
            &[rb(RatingSource::Imdb, value)],
            &f,
            LabelStyle::Official,
            appearance(BadgeBackground::Average, BadgeShape::Rounded),
            scale,
        )
        .unwrap();
        let disc = &disc_row[0];
        assert_eq!(disc.width(), disc.height());
        assert_eq!(disc.width(), (58.0 * scale * DISC_SHARE).round() as u32);
        // Only the white number has all three channels above 200.
        let bright: Vec<(u32, u32)> = disc
            .enumerate_pixels()
            .filter(|(_, _, p)| p.0[..3].iter().all(|c| *c > 200) && p.0[3] > 200)
            .map(|(x, y, _)| (x, y))
            .collect();
        assert!(!bright.is_empty(), "{value}: number drawn");
        let (x0, x1) = (
            bright.iter().map(|p| p.0).min().unwrap(),
            bright.iter().map(|p| p.0).max().unwrap(),
        );
        let (y0, y1) = (
            bright.iter().map(|p| p.1).min().unwrap(),
            bright.iter().map(|p| p.1).max().unwrap(),
        );
        let d = disc.width() as f32;
        let (cx, cy) = ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0);
        assert!(
            (cx - d / 2.0).abs() <= d * 0.08,
            "{value}: number centred horizontally ({cx} of {d})"
        );
        assert!(
            (cy - d / 2.0).abs() <= d * 0.10,
            "{value}: number centred vertically ({cy} of {d})"
        );
        // The number stays inside the ring.
        assert!(
            x0 as f32 > d * 0.15 && (x1 as f32) < d * 0.85,
            "{value}: number inside the ring"
        );
        // Corners are outside the disc.
        assert_eq!(disc.get_pixel(0, 0).0[3], 0);
    }
}

// --- Whole renders (synthetic canvases) ---

#[test]
fn posters_render_every_look_at_card_and_tiny_sizes() {
    let f = font();
    let badges = vec![
        rb(RatingSource::Mal, "8.12"),
        rb(RatingSource::Imdb, "6.4"),
        rb(RatingSource::Letterboxd, "2.1"),
    ];
    for (width, scale) in [(580_u32, 1.0_f32), (92, 92.0 / 580.0)] {
        let png = solid_png(width, width * 3 / 2, [128, 128, 128]);
        let bare = generate::render_poster_sync(
            &png,
            &[],
            &f,
            90,
            BadgePosition::BottomCenter,
            BadgeStyle::Horizontal,
            LabelStyle::Official,
            BadgeAppearance::default(),
            BadgeDirection::Horizontal,
            width,
            scale,
            BadgeSize::Medium,
            false,
            PosterFit::Native,
        )
        .unwrap();
        for bg in FORK_LOOKS {
            for shape in SHAPES {
                let out = generate::render_poster_sync(
                    &png,
                    &badges,
                    &f,
                    90,
                    BadgePosition::BottomCenter,
                    BadgeStyle::Default.resolve(BadgeDirection::Horizontal),
                    LabelStyle::Official,
                    appearance(bg, shape),
                    BadgeDirection::Horizontal,
                    width,
                    scale,
                    BadgeSize::Medium,
                    false,
                    PosterFit::Native,
                )
                .unwrap();
                let (x0, y0, x1, y1) = diff_bbox(&bare, &out, 24)
                    .unwrap_or_else(|| panic!("{bg:?} {width}: badges drawn"));
                let h = width * 3 / 2;
                assert!(
                    y0 >= h / 2,
                    "{bg:?} {width}: bottom-centre badges stay in the bottom half (y0={y0})"
                );
                assert!(x1 < width && y1 < h);
                // Centred: the left and right margins match within 3 px + 2 %.
                let (left, right) = (x0 as i32, (width - 1 - x1) as i32);
                assert!(
                    (left - right).abs() <= 8 + (width as i32) / 50,
                    "{bg:?} {width}: centred ({left} vs {right})"
                );
                if bg == BadgeBackground::Average {
                    let d = (58.0 * scale * DISC_SHARE).round() as u32;
                    assert!(x1 - x0 < d + 16, "{bg:?} {width}: one disc, not a row");
                }
            }
        }
    }
}

#[test]
fn backdrops_put_every_look_in_the_top_right_column() {
    let f = font();
    let badges = preview_samples(BadgeBackground::Score).unwrap()[..5].to_vec();
    let png = solid_png(1920, 1080, [200, 200, 200]);
    let render = |bs: &[RatingBadge], a: BadgeAppearance| {
        generate::render_backdrop_sync(
            &png,
            bs,
            &f,
            90,
            BadgePosition::TopRight,
            BadgeStyle::Vertical,
            LabelStyle::Official,
            a,
            BadgeDirection::Vertical,
            1920,
            1.0,
            BadgeSize::Medium,
            0,
            0,
        )
        .unwrap()
    };
    let bare = render(&[], BadgeAppearance::default());
    for bg in FORK_LOOKS {
        let out = render(&badges, appearance(bg, BadgeShape::Rounded));
        let (x0, y0, x1, y1) = diff_bbox(&bare, &out, 24).unwrap();
        // Upstream's column: 20 px side margin, 20 px top margin, 88 px wide.
        assert!(
            x0 >= 1920 - 20 - 88 - 30 && x1 <= 1920 - 20 + 8,
            "{bg:?}: right column x {x0}..{x1}"
        );
        assert!((12..=28).contains(&y0), "{bg:?}: top margin {y0}");
        if bg == BadgeBackground::Average {
            assert!(y1 < 20 + 78 + 12, "{bg:?}: a single disc ({y1})");
        } else {
            assert!(y1 > 400 && y1 < 1080 / 2 + 60, "{bg:?}: five badges ({y1})");
        }
    }
}

#[test]
fn logos_and_episodes_render_every_look() {
    let f = font();
    let badges = vec![
        rb(RatingSource::Imdb, "8.5"),
        rb(RatingSource::Rt, "100%"),
        rb(RatingSource::Metacritic, "47"),
    ];
    let logo = solid_png(400, 120, [230, 230, 230]);
    let still = solid_png(780, 439, [40, 40, 60]);
    for bg in FORK_LOOKS {
        for style in [BadgeStyle::Horizontal, BadgeStyle::Vertical] {
            let a = appearance(bg, BadgeShape::Rounded);
            let png = generate::render_logo_sync(
                &logo,
                &badges,
                &f,
                style,
                LabelStyle::Official,
                a,
                780,
                1.0,
            )
            .unwrap();
            let img = image::load_from_memory(&png).unwrap();
            assert!(
                img.height() > 234,
                "{bg:?} {style:?}: badges below the logo"
            );
            let jpg = generate::render_episode_sync(
                &still,
                &badges[..1],
                &f,
                90,
                BadgePosition::TopRight,
                style,
                LabelStyle::Official,
                a,
                BadgeDirection::Vertical,
                780,
                1.25,
                BadgeSize::Large,
                false,
            )
            .unwrap();
            assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        }
    }
}
