//! Fork badge looks: three extra `BadgeBackground` variants drawn here instead
//! of in `image::badge`.
//!
//! - `s` Score: one dark "off-air" plate, upstream's label (icon or text) and a
//!   white number, plus a meter along the bottom edge that fills with the
//!   score in a colour-blind-checked teal / amber / red scale.
//! - `m` Mono: the same plate, lighter, no colour at all: grey text labels or
//!   white source glyphs (official logos are swapped for the glyphs).
//! - `a` Average: one ring dial with the mean of the shown ratings on 0–10.
//!
//! Geometry mirrors upstream's badge metrics (height 58, value font 34,
//! label font 26, icon 48, vertical width 88, all times `badge_scale`) so the
//! layout code in `image::generate` places these badges exactly like upstream
//! ones. Upstream looks never reach this module: every entry point returns
//! `None` (or borrows the input) for them, so their bytes are unchanged.
//! Design rationale: `docs/louk/badge-styles.md`.

use std::borrow::Cow;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::{Rgba, RgbaImage, imageops};
use imageproc::drawing::draw_text_mut;

use crate::image::icons;
use crate::services::db::{BadgeAppearance, BadgeBackground, BadgeShape, LabelStyle};
use crate::services::ratings::{RatingBadge, RatingSource};

// --- Palette (hex values and contrast maths in docs/louk/badge-styles.md) ---

/// "Off-air" #0E1116: the plate colour, a cool near-black that separates
/// from warm or neutral black artwork.
const OFF_AIR: [u8; 3] = [0x0E, 0x11, 0x16];
/// Score plate opacity 86 %: white text stays >= 12.8:1 even over pure white art.
const SCORE_PLATE_ALPHA: u8 = 219;
/// Mono plate opacity 72 %: lighter, white text stays >= 7.6:1 over pure white.
const MONO_PLATE_ALPHA: u8 = 184;
/// Average disc opacity 88 %.
const AVERAGE_PLATE_ALPHA: u8 = 224;
/// Inner hairline: white at 14 %, so a dark plate still has an edge on dark art.
const HAIRLINE_ALPHA: f32 = 0.14;
/// Unfilled meter / ring track: white at 16 %.
const TRACK: Rgba<u8> = Rgba([255, 255, 255, 41]);
/// Score bands, L* 84 / 74 / 58 (monotonic, so the order survives grey-scale
/// and every colour-vision deficiency).
const HIGH: Rgba<u8> = Rgba([0x5E, 0xE6, 0xC8, 255]); // #5EE6C8 teal, score >= 0.70
const MID: Rgba<u8> = Rgba([0xF5, 0xA5, 0x24, 255]); // #F5A524 amber, 0.50..0.70
const LOW: Rgba<u8> = Rgba([0xF2, 0x54, 0x5B, 255]); // #F2545B red, < 0.50
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
/// Mono label grey #AEB4C0.
const MONO_LABEL: Rgba<u8> = Rgba([0xAE, 0xB4, 0xC0, 255]);
/// Mono glyphs are drawn at 80 % so the number stays the brightest thing.
const MONO_GLYPH_OPACITY: f32 = 0.80;

// --- Geometry (mirrors image::badge so layouts line up) ---

const BASE_BADGE_HEIGHT: f32 = 58.0;
const BASE_BADGE_PADDING_H: f32 = 14.0;
const BASE_TEXT_LABEL_PADDING_H: f32 = 8.0;
const BASE_BADGE_VALUE_PADDING_H: f32 = 10.0;
const BASE_PILL_PADDING: f32 = 10.0;
const BASE_PILL_PADDING_V: f32 = 6.0;
const BASE_FONT_SIZE: f32 = 34.0;
const BASE_LABEL_FONT_SIZE: f32 = 26.0;
const BASE_ICON_HEIGHT: f32 = 48.0;
const BASE_VERT_BADGE_WIDTH: f32 = 88.0;
const BASE_VERT_BADGE_PADDING_V: f32 = 8.0;
const BASE_VERT_GAP: f32 = 4.0;
/// Corner radius of the rounded shape, as a share of the base badge height
/// (upstream uses 10/58 = 0.17; a little softer reads better at TV distance).
const RADIUS_SHARE: f32 = 0.22;
/// Meter thickness as a share of the base badge height (5 px at 58).
const METER_SHARE: f32 = 0.085;
/// The Average disc is 1.6 badge heights across (93 px at the 580 px
/// poster): it is alone, so it can be bigger than one badge of a row (and
/// its figures as tall as theirs) while taking less room than three.
const DISC_SHARE: f32 = 1.6;
/// Number size inside the disc, as a share of the disc (figures 0.6 of it).
const DISC_FONT_SHARE: f32 = 0.36;
/// Smallest badge height drawn (px); keeps tiny thumbnails from degenerating.
const MIN_BADGE_HEIGHT: u32 = 4;

/// Score thresholds on the 0..1 scale (SPEC: red < 5, amber 5–6.9, green >= 7).
const HIGH_FROM: f32 = 0.70;
const MID_FROM: f32 = 0.50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Score,
    Mono,
    Average,
}

impl Look {
    fn of(background: BadgeBackground) -> Option<Self> {
        match background {
            BadgeBackground::Score => Some(Self::Score),
            BadgeBackground::Mono => Some(Self::Mono),
            BadgeBackground::Average => Some(Self::Average),
            BadgeBackground::Default
            | BadgeBackground::Dark
            | BadgeBackground::Transparent
            | BadgeBackground::None => None,
        }
    }

    /// The label style this look draws. Mono swaps the coloured official
    /// logos for the white source glyphs; the other looks keep the setting.
    fn label_style(self, configured: LabelStyle) -> LabelStyle {
        match (self, configured) {
            (Self::Mono, LabelStyle::Official) => LabelStyle::Icon,
            (_, other) => other,
        }
    }

    fn plate_alpha(self) -> u8 {
        match self {
            Self::Score => SCORE_PLATE_ALPHA,
            Self::Mono => MONO_PLATE_ALPHA,
            Self::Average => AVERAGE_PLATE_ALPHA,
        }
    }

    fn label_color(self) -> Rgba<u8> {
        match self {
            Self::Mono => MONO_LABEL,
            _ => WHITE,
        }
    }
}

/// True for the fork's badge looks (`s`, `m`, `a`).
pub fn is_fork_look(background: BadgeBackground) -> bool {
    Look::of(background).is_some()
}

/// The fork's looks, in the order the admin form lists them.
pub const FORK_LOOKS: [BadgeBackground; 3] = [
    BadgeBackground::Score,
    BadgeBackground::Mono,
    BadgeBackground::Average,
];

// --- Scores ---

/// A rating normalised to 0..1 using each source's own scale: IMDb and MAL
/// out of 10, Letterboxd out of 5, Roger Ebert out of 4, Metacritic and the
/// MDBList score out of 100, and anything ending in `%` out of 100.
/// `None` for values that don't parse ("", "N/A").
pub fn normalized_score(badge: &RatingBadge) -> Option<f32> {
    let raw = badge.value.trim();
    let (number, percent) = match raw.strip_suffix('%') {
        Some(n) => (n.trim(), true),
        None => (raw, false),
    };
    let v: f32 = number.parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    let max = if percent {
        100.0
    } else {
        match badge.source {
            RatingSource::Imdb | RatingSource::Mal => 10.0,
            RatingSource::Letterboxd => 5.0,
            RatingSource::Ebert => 4.0,
            RatingSource::Metacritic | RatingSource::Mdblist => 100.0,
            // These always carry `%` upstream; a bare number is a percentage too.
            RatingSource::Tmdb
            | RatingSource::Rt
            | RatingSource::RtAudience
            | RatingSource::Trakt => 100.0,
        }
    };
    Some((v / max).clamp(0.0, 1.0))
}

/// Band colour for a 0..1 score.
pub fn band_color(score: f32) -> Rgba<u8> {
    if score >= HIGH_FROM {
        HIGH
    } else if score >= MID_FROM {
        MID
    } else {
        LOW
    }
}

/// The text the Average disc shows for a 0..1 score: one decimal on 0–10,
/// "10" for a perfect score (so it fits the ring like "7.8" does).
pub fn average_text(score: f32) -> String {
    let v = score * 10.0;
    if v >= 9.95 {
        "10".to_string()
    } else {
        format!("{v:.1}")
    }
}

/// For the Average look, replace the badges with one badge carrying the mean
/// of their normalised scores (on IMDb's 0–10 scale, which `normalized_score`
/// reads back). No parseable rating means no badge. Every other look borrows
/// the input unchanged.
pub fn collapse(badges: &[RatingBadge], appearance: BadgeAppearance) -> Cow<'_, [RatingBadge]> {
    if appearance.background != BadgeBackground::Average {
        return Cow::Borrowed(badges);
    }
    let scores: Vec<f32> = badges.iter().filter_map(normalized_score).collect();
    if scores.is_empty() {
        return Cow::Owned(Vec::new());
    }
    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    Cow::Owned(vec![RatingBadge {
        source: RatingSource::Imdb,
        value: format!("{:.1}", mean * 10.0),
    }])
}

/// Sample ratings for the admin preview of a fork look: every source, with
/// scores spread over all three bands (upstream's samples are all 100 %,
/// which would only ever show teal). In the default order the first three
/// (the poster preview) are teal, amber, red. `None` for upstream looks.
pub fn preview_samples(background: BadgeBackground) -> Option<Vec<RatingBadge>> {
    if !is_fork_look(background) {
        return None;
    }
    let b = |source, value: &str| RatingBadge {
        source,
        value: value.to_string(),
    };
    Some(vec![
        b(RatingSource::Imdb, "6.4"),
        b(RatingSource::Tmdb, "72%"),
        b(RatingSource::Rt, "91%"),
        b(RatingSource::RtAudience, "64%"),
        b(RatingSource::Metacritic, "47"),
        b(RatingSource::Trakt, "55%"),
        b(RatingSource::Letterboxd, "2.1"),
        b(RatingSource::Mal, "8.12"),
        b(RatingSource::Mdblist, "68"),
        b(RatingSource::Ebert, "3.5"),
    ])
}

// --- Entry points called from image::badge (hooks) ---

/// Horizontal badges for a fork look (uniform label/value widths like
/// upstream's `render_badges_uniform`), or `None` for upstream looks.
pub fn render_row(
    badges: &[RatingBadge],
    font: &FontArc,
    label_style: LabelStyle,
    appearance: BadgeAppearance,
    badge_scale: f32,
) -> Option<Vec<RgbaImage>> {
    let look = Look::of(appearance.background)?;
    if badges.is_empty() {
        return Some(Vec::new());
    }
    if look == Look::Average {
        // Normally one badge already (see `collapse`); average again so a
        // caller that skipped `collapse` still gets one disc.
        let collapsed = collapse(badges, appearance);
        return Some(
            collapsed
                .iter()
                .map(|b| render_disc(normalized_score(b), font, badge_scale))
                .collect(),
        );
    }
    let m = Metrics::new(badge_scale, appearance.shape);
    let label_style = look.label_style(label_style);
    let label_scaled = font.as_scaled(m.label_scale);
    let value_scaled = font.as_scaled(m.value_scale);
    let label_w = match label_style {
        LabelStyle::Official => m.icon_h,
        LabelStyle::Icon => badges
            .iter()
            .map(|b| scaled_width(icons::icon_for_source(&b.source), m.icon_h))
            .max()
            .unwrap_or(m.icon_h),
        LabelStyle::Text => badges
            .iter()
            .map(|b| text_width(b.source.label(), &label_scaled))
            .max()
            .unwrap_or(0),
    };
    let value_w = badges
        .iter()
        .map(|b| text_width(&b.value, &value_scaled))
        .max()
        .unwrap_or(0);
    Some(
        badges
            .iter()
            .map(|b| render_horizontal(b, font, &m, look, label_style, label_w, value_w))
            .collect(),
    )
}

/// One vertical badge for a fork look, or `None` for upstream looks.
pub fn render_vertical(
    badge: &RatingBadge,
    font: &FontArc,
    label_style: LabelStyle,
    appearance: BadgeAppearance,
    badge_scale: f32,
) -> Option<RgbaImage> {
    let look = Look::of(appearance.background)?;
    if look == Look::Average {
        return Some(render_disc(normalized_score(badge), font, badge_scale));
    }
    let m = Metrics::new(badge_scale, appearance.shape);
    Some(render_vertical_inner(
        badge,
        font,
        &m,
        look,
        look.label_style(label_style),
    ))
}

// --- Drawing ---

struct Metrics {
    badge_h: u32,
    pad_h: u32,
    text_label_pad_h: u32,
    value_pad_h: u32,
    pill_pad: u32,
    shape: BadgeShape,
    icon_h: u32,
    radius: f32,
    hairline: f32,
    meter_h: u32,
    value_scale: PxScale,
    label_scale: PxScale,
    scale: f32,
}

fn px(base: f32, scale: f32) -> u32 {
    (base * scale).round().max(0.0) as u32
}

impl Metrics {
    fn new(scale: f32, shape: BadgeShape) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let (pill_pad, pill_pad_v) = match shape {
            BadgeShape::Pill => (px(BASE_PILL_PADDING, scale), px(BASE_PILL_PADDING_V, scale)),
            BadgeShape::Rounded => (0, 0),
        };
        let badge_h = (px(BASE_BADGE_HEIGHT, scale) + pill_pad_v).max(MIN_BADGE_HEIGHT);
        Self {
            badge_h,
            pad_h: px(BASE_BADGE_PADDING_H, scale),
            text_label_pad_h: px(BASE_TEXT_LABEL_PADDING_H, scale),
            value_pad_h: px(BASE_BADGE_VALUE_PADDING_H, scale),
            pill_pad,
            shape,
            icon_h: px(BASE_ICON_HEIGHT, scale).max(1),
            radius: (BASE_BADGE_HEIGHT * scale * RADIUS_SHARE).max(1.0),
            hairline: scale.clamp(1.0, 4.0),
            meter_h: px(BASE_BADGE_HEIGHT * METER_SHARE, scale).max(2),
            value_scale: PxScale::from((BASE_FONT_SIZE * scale).max(1.0)),
            label_scale: PxScale::from((BASE_LABEL_FONT_SIZE * scale).max(1.0)),
            scale,
        }
    }
}

fn plate(look: Look, w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(
        w.max(1),
        h.max(1),
        Rgba([OFF_AIR[0], OFF_AIR[1], OFF_AIR[2], look.plate_alpha()]),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_horizontal(
    badge: &RatingBadge,
    font: &FontArc,
    m: &Metrics,
    look: Look,
    label_style: LabelStyle,
    label_w: u32,
    value_w: u32,
) -> RgbaImage {
    let uses_icon = label_style.uses_icon();
    let label_pad = if uses_icon {
        m.pad_h
    } else {
        m.text_label_pad_h
    };
    let value_x = m.pill_pad + label_w + label_pad * 2;
    let total_w = value_x + value_w + m.value_pad_h + m.value_pad_h / 2 + 2 + m.pill_pad;
    let h = m.badge_h;
    let meter_h = if look == Look::Score {
        m.meter_h.min(h / 3)
    } else {
        0
    };
    // Content is centred in the part above the meter.
    let content_h = h.saturating_sub(meter_h);
    let mut img = plate(look, total_w, h);

    if uses_icon {
        let icon = label_icon(badge, label_style, look, m.icon_h);
        let ix = m.pill_pad + label_pad + label_w.saturating_sub(icon.width()) / 2;
        let iy = content_h.saturating_sub(icon.height()) / 2;
        imageops::overlay(&mut img, &icon, ix as i64, iy as i64);
    } else {
        let label = badge.source.label();
        let lw = text_width(label, &font.as_scaled(m.label_scale));
        let lx = m.pill_pad + label_pad + label_w.saturating_sub(lw) / 2;
        let ly = (content_h as i32 - m.label_scale.y as i32) / 2;
        draw_text_mut(
            &mut img,
            look.label_color(),
            lx as i32,
            ly,
            m.label_scale,
            font,
            label,
        );
    }

    let vw = text_width(&badge.value, &font.as_scaled(m.value_scale));
    let vx = value_x + m.value_pad_h + value_w.saturating_sub(vw) / 2;
    let vy = (content_h as i32 - m.value_scale.y as i32) / 2;
    draw_text_mut(
        &mut img,
        WHITE,
        vx as i32,
        vy,
        m.value_scale,
        font,
        &badge.value,
    );

    if meter_h > 0 {
        draw_meter(&mut img, normalized_score(badge), meter_h);
    }
    let radius = match m.shape {
        BadgeShape::Pill => h as f32 / 2.0,
        BadgeShape::Rounded => m.radius,
    };
    finish_plate(&mut img, radius, m.hairline);
    img
}

fn render_vertical_inner(
    badge: &RatingBadge,
    font: &FontArc,
    m: &Metrics,
    look: Look,
    label_style: LabelStyle,
) -> RgbaImage {
    let uses_icon = label_style.uses_icon();
    let w = px(BASE_VERT_BADGE_WIDTH, m.scale).max(MIN_BADGE_HEIGHT);
    let pad_v = px(BASE_VERT_BADGE_PADDING_V, m.scale) + m.pill_pad;
    let label_area_h = if uses_icon {
        m.icon_h
    } else {
        m.label_scale.y as u32
    };
    let gap = px(BASE_VERT_GAP, m.scale);
    let meter_h = if look == Look::Score { m.meter_h } else { 0 };
    let h = (pad_v + label_area_h + gap + m.value_scale.y as u32 + pad_v + meter_h)
        .max(MIN_BADGE_HEIGHT);
    let value_area_y = pad_v + label_area_h + gap / 2;
    let mut img = plate(look, w, h);

    if uses_icon {
        let icon = label_icon(badge, label_style, look, m.icon_h);
        let ix = w.saturating_sub(icon.width()) / 2;
        let iy = value_area_y.saturating_sub(icon.height()) / 2;
        imageops::overlay(&mut img, &icon, ix as i64, iy as i64);
    } else {
        let label = badge.source.label();
        let lw = text_width(label, &font.as_scaled(m.label_scale));
        let lx = w.saturating_sub(lw) / 2;
        let ly = value_area_y.saturating_sub(m.label_scale.y as u32) / 2;
        draw_text_mut(
            &mut img,
            look.label_color(),
            lx as i32,
            ly as i32,
            m.label_scale,
            font,
            label,
        );
    }

    let vw = text_width(&badge.value, &font.as_scaled(m.value_scale));
    let vx = w.saturating_sub(vw) / 2;
    let vy = value_area_y + pad_v / 2;
    draw_text_mut(
        &mut img,
        WHITE,
        vx as i32,
        vy as i32,
        m.value_scale,
        font,
        &badge.value,
    );

    if meter_h > 0 {
        draw_meter(&mut img, normalized_score(badge), meter_h.min(h / 3));
    }
    let radius = match m.shape {
        BadgeShape::Pill => w as f32 / 2.0,
        BadgeShape::Rounded => m.radius,
    };
    finish_plate(&mut img, radius, m.hairline);
    img
}

/// The source label as an image: official logos (Score), or the white
/// source glyphs (Icon; Mono always), scaled to `icon_h` like upstream.
fn label_icon(badge: &RatingBadge, label_style: LabelStyle, look: Look, icon_h: u32) -> RgbaImage {
    let (icon, (w, h)) = match label_style {
        LabelStyle::Official => {
            let icon = icons::official_icon_for_badge(badge);
            (icon, fit_in_box(icon, icon_h))
        }
        _ => {
            let icon = icons::icon_for_source(&badge.source);
            (icon, (scaled_width(icon, icon_h), icon_h))
        }
    };
    let mut out = if icon.width() == w && icon.height() == h {
        icon.clone()
    } else {
        imageops::resize(icon, w.max(1), h.max(1), imageops::FilterType::Lanczos3)
    };
    if look == Look::Mono {
        for p in out.pixels_mut() {
            p.0[3] = (p.0[3] as f32 * MONO_GLYPH_OPACITY).round() as u8;
        }
    }
    out
}

/// The meter along the bottom edge: a white 16 % track, filled from the
/// left to the score in its band colour. Unparseable scores show the track.
fn draw_meter(img: &mut RgbaImage, score: Option<f32>, meter_h: u32) {
    let (w, h) = (img.width(), img.height());
    let fill_to = score.map(|s| s * w as f32).unwrap_or(0.0);
    let color = score.map(band_color).unwrap_or(TRACK);
    for y in h.saturating_sub(meter_h)..h {
        for x in 0..w {
            // Anti-aliased right end of the fill.
            let cov = (fill_to - x as f32).clamp(0.0, 1.0);
            let base = *img.get_pixel(x, y);
            let with_track = blend(base, TRACK, 1.0);
            img.put_pixel(x, y, blend(with_track, color, cov));
        }
    }
}

/// Anti-aliased rounded-rect mask over the whole image, plus an inner white
/// hairline of `hairline` px following the same outline.
fn finish_plate(img: &mut RgbaImage, radius: f32, hairline: f32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let inner_r = (r - hairline).max(0.0);
    for y in 0..img.height() {
        for x in 0..img.width() {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let outer = coverage(rounded_rect_sdf(cx, cy, 0.0, 0.0, w, h, r));
            if outer <= 0.0 {
                img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                continue;
            }
            let inner = coverage(rounded_rect_sdf(
                cx,
                cy,
                hairline,
                hairline,
                w - hairline,
                h - hairline,
                inner_r,
            ));
            let stroke = (outer - inner).max(0.0);
            let mut p = blend(*img.get_pixel(x, y), WHITE, stroke * HAIRLINE_ALPHA);
            p.0[3] = (p.0[3] as f32 * outer).round() as u8;
            img.put_pixel(x, y, p);
        }
    }
}

/// The Average look: a dark disc with a score ring (track + band-coloured
/// arc clockwise from 12 o'clock) and the 0–10 number in the middle.
fn render_disc(score: Option<f32>, font: &FontArc, badge_scale: f32) -> RgbaImage {
    let scale = if badge_scale.is_finite() && badge_scale > 0.0 {
        badge_scale
    } else {
        1.0
    };
    let d = ((BASE_BADGE_HEIGHT * scale * DISC_SHARE).round() as u32).max(MIN_BADGE_HEIGHT * 2);
    let df = d as f32;
    let c = df / 2.0;
    let radius = df / 2.0;
    let inset = (df * 0.07).round().max(1.0);
    let thickness = (df * 0.085).round().max(2.0);
    let r_out = radius - inset;
    let r_in = (r_out - thickness).max(1.0);
    let r_mid = (r_out + r_in) / 2.0;
    let sweep = score.unwrap_or(0.0) * std::f32::consts::TAU;
    let color = score.map(band_color).unwrap_or(TRACK);
    let plate_px = Rgba([OFF_AIR[0], OFF_AIR[1], OFF_AIR[2], AVERAGE_PLATE_ALPHA]);

    let mut img = RgbaImage::new(d, d);
    for y in 0..d {
        for x in 0..d {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let dist = (dx * dx + dy * dy).sqrt();
            let disc = (radius - dist + 0.5).clamp(0.0, 1.0);
            if disc <= 0.0 {
                continue;
            }
            let mut p = plate_px;
            p.0[3] = (p.0[3] as f32 * disc).round() as u8;
            let ring = ((r_out - dist).min(dist - r_in) + 0.5).clamp(0.0, 1.0);
            if ring > 0.0 {
                // Angle clockwise from 12 o'clock, 0..TAU.
                let mut theta = dx.atan2(-dy);
                if theta < 0.0 {
                    theta += std::f32::consts::TAU;
                }
                let arc = ((sweep - theta) * r_mid + 0.5).clamp(0.0, 1.0);
                p = blend(p, TRACK, ring * (1.0 - arc));
                p = blend(p, color, ring * arc);
            }
            img.put_pixel(x, y, p);
        }
    }

    let text = match score {
        Some(s) => average_text(s),
        None => "\u{2013}".to_string(),
    };
    let max_w = 2.0 * r_in * 0.78;
    let mut size = (df * DISC_FONT_SHARE).max(1.0);
    let w = text_width(&text, &font.as_scaled(PxScale::from(size))) as f32;
    if w > max_w && w > 0.0 {
        size = (size * max_w / w).max(1.0);
    }
    let font_scale = PxScale::from(size);
    let tw = text_width(&text, &font.as_scaled(font_scale)) as f32;
    // ab_glyph's px size is Inter's ascent-to-descent height: the baseline
    // sits 0.80 size below the line top and figures are 0.60 size tall, so
    // their middle is 0.50 size down.
    let tx = (c - tw / 2.0).round() as i32;
    let ty = (c - 0.5 * size).round() as i32;
    draw_text_mut(&mut img, WHITE, tx, ty, font_scale, font, &text);
    img
}

// --- Small helpers ---

/// Signed distance from (`px`, `py`) to the rounded rectangle spanning
/// (`x0`, `y0`)..(`x1`, `y1`) with corner radius `r` (negative inside).
fn rounded_rect_sdf(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = (((x1 - x0) / 2.0).max(0.0), ((y1 - y0) / 2.0).max(0.0));
    let r = r.min(hx).min(hy).max(0.0);
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

fn coverage(sdf: f32) -> f32 {
    (0.5 - sdf).clamp(0.0, 1.0)
}

/// Straight-alpha source-over of `src` (its alpha times `amount`) onto `dst`.
fn blend(dst: Rgba<u8>, src: Rgba<u8>, amount: f32) -> Rgba<u8> {
    let sa = src.0[3] as f32 / 255.0 * amount.clamp(0.0, 1.0);
    if sa <= 0.0 {
        return dst;
    }
    let da = dst.0[3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let ch = |i: usize| {
        ((src.0[i] as f32 * sa + dst.0[i] as f32 * da * (1.0 - sa)) / oa)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Rgba([ch(0), ch(1), ch(2), (oa * 255.0).round() as u8])
}

fn text_width(text: &str, font: &ab_glyph::PxScaleFont<&FontArc>) -> u32 {
    let width: f32 = text.chars().map(|c| font.h_advance(font.glyph_id(c))).sum();
    width.ceil().max(0.0) as u32
}

fn scaled_width(icon: &RgbaImage, target_h: u32) -> u32 {
    if icon.height() == 0 {
        target_h
    } else {
        (icon.width() as f32 * target_h as f32 / icon.height() as f32).ceil() as u32
    }
}

fn fit_in_box(icon: &RgbaImage, size: u32) -> (u32, u32) {
    let (w, h) = (icon.width(), icon.height());
    if w == 0 || h == 0 {
        return (size, size);
    }
    let s = (size as f32 / w as f32).min(size as f32 / h as f32);
    (
        ((w as f32 * s).ceil() as u32).max(1),
        ((h as f32 * s).ceil() as u32).max(1),
    )
}

#[cfg(test)]
mod tests;
