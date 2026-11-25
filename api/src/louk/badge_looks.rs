//! Fork badge looks: three extra `BadgeBackground` variants drawn here instead
//! of in `image::badge`.
//!
//! - `s` Score: one dark "off-air" plate, the source mark and a white number,
//!   and under them a rounded score bar, inset from the plate's edges, that
//!   fills in a colour-blind-checked teal / amber / red scale.
//! - `m` Mono: upstream's badge size on a lighter plate, no colour at all:
//!   white source glyphs (official logos are swapped for them; glyphs that
//!   are filled tiles, IMDb and MDBList, become text) or grey text labels.
//! - `a` Average: one score. In a row (posters, logos, horizontal backdrops)
//!   a pill as tall as a Score badge, with a small score ring and a large
//!   figure; in a column (the backdrop and episode default) a ring disc.
//!
//! Geometry follows upstream's badge metrics (height 58, value font 34,
//! label font 26, icon 48, vertical width 88, all times `badge_scale`) so the
//! layout code in `image::generate` places these badges like upstream ones.
//! Upstream looks never reach this module: every entry point returns `None`
//! (or borrows the input) for them, so their bytes are unchanged.
//! Design rationale and the tuning round: `docs/louk/badge-styles.md`.

use std::borrow::Cow;
use std::sync::LazyLock;

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
/// Mono plate opacity 80 %: white text >= 10.3:1 and the grey label >= 4.9:1
/// over pure white (72 % let busy art show through in the gallery).
const MONO_PLATE_ALPHA: u8 = 204;
/// Average pill / disc opacity 88 %.
const AVERAGE_PLATE_ALPHA: u8 = 224;
/// Inner hairline: white at 14 %, so a dark plate still has an edge on dark art.
const HAIRLINE_ALPHA: f32 = 0.14;
/// Unfilled bar / ring track: white at 16 %.
const TRACK: Rgba<u8> = Rgba([255, 255, 255, 41]);
/// Score bands, L* 84 / 74 / 58 (monotonic, so the order survives grey-scale
/// and every colour-vision deficiency).
const HIGH: Rgba<u8> = Rgba([0x5E, 0xE6, 0xC8, 255]); // #5EE6C8 teal, score >= 0.70
const MID: Rgba<u8> = Rgba([0xF5, 0xA5, 0x24, 255]); // #F5A524 amber, 0.50..0.70
const LOW: Rgba<u8> = Rgba([0xF2, 0x54, 0x5B, 255]); // #F2545B red, < 0.50
const WHITE: Rgba<u8> = Rgba([255, 255, 255, 255]);
/// Mono text labels (label style "text") #AEB4C0.
const MONO_LABEL: Rgba<u8> = Rgba([0xAE, 0xB4, 0xC0, 255]);
/// Mono marks: glyphs at 82 % white, and the text that replaces tile glyphs
/// in the same tone #D6DAE0, so every source mark is one system and the
/// figure stays the brightest thing.
const MONO_GLYPH_OPACITY: f32 = 0.82;
const MONO_MARK_TEXT: Rgba<u8> = Rgba([0xD6, 0xDA, 0xE0, 255]);

// --- Geometry (base pixels at badge_scale 1, i.e. a 580 px poster) ---

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
/// Corner radius of the rounded shape (12.8 px; upstream uses 10).
const RADIUS_SHARE: f32 = 0.22;

/// Score row badge: content zone 52, then the bar (6.4) and its bottom
/// inset (5.6): 64 px in all.
const BASE_SCORE_CONTENT: f32 = 52.0;
/// In rows (posters, logos, horizontal backdrops) the figure is the
/// dominant element: 38 px type (upstream 34, digits 23 px) next to a source
/// mark held to a 36 px box (upstream 48). Posters are shown 4-5x smaller on
/// phones, so every pixel of figure counts there. Columns keep upstream's
/// 34 / 48 (their width is fixed at 88).
const BASE_ROW_FONT_SIZE: f32 = 38.0;
const BASE_ROW_ICON_BOX: f32 = 36.0;
/// Content zone height per pixel of figure type (38 px type in a 52 px zone).
const CONTENT_PER_FONT_PX: f32 = 1.3;
const BASE_BAR_THICKNESS: f32 = 6.4;
const BASE_BAR_BOTTOM: f32 = 5.6;
/// Smallest distance between the bar's ends and the plate's sides.
const BASE_BAR_SIDE: f32 = 10.0;

/// Average pill: 68 px, about a Score badge's height, so it sits in the
/// row's band; the figure is the pill's main element at 0.78 of its height
/// (digits 32 px, 1.4x a row badge's), the ring dial 0.56.
const BASE_AVERAGE_PILL: f32 = 68.0;
const PILL_FONT_SHARE: f32 = 0.78;
const PILL_DIAL_SHARE: f32 = 0.56;
/// Ring thickness as a share of the dial / disc diameter.
const DIAL_RING_SHARE: f32 = 0.16;
/// Average disc (columns): 1.6 badge heights across (93 px at 580),
/// figure 0.40 of it, ring 0.085, inset 0.055.
const DISC_SHARE: f32 = 1.6;
const DISC_FONT_SHARE: f32 = 0.40;
const DISC_RING_SHARE: f32 = 0.085;
const DISC_INSET_SHARE: f32 = 0.055;

/// Smallest drawn sizes in output pixels, so tiny renders stay a legible
/// figure instead of a smudge: figures never below 9 px (row badges) or
/// 12 px (Average), the Average pill never below 22 px, the disc 32 px.
const MIN_FIGURE_PX: f32 = 9.0;
const MIN_AVERAGE_FIGURE_PX: f32 = 12.0;
const MIN_AVERAGE_PILL_PX: f32 = 22.0;
const MIN_DISC_PX: f32 = 32.0;
/// Below this badge scale (badge height under ~26 px) Score and Mono drop
/// the source mark and keep the figure (and the bar).
const COMPACT_BELOW: f32 = 0.45;
/// Smallest badge height drawn (px).
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

    fn plate_alpha(self) -> u8 {
        match self {
            Self::Score => SCORE_PLATE_ALPHA,
            Self::Mono => MONO_PLATE_ALPHA,
            Self::Average => AVERAGE_PLATE_ALPHA,
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

/// The text the Average look shows for a 0..1 score: one decimal on 0–10,
/// "10" for a perfect score.
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
///
/// MDBList's own score is already an aggregate of the other sources, so it
/// is left out of the mean when any other source is shown (otherwise it
/// would count those sources twice); alone, it is used as is.
pub fn collapse(badges: &[RatingBadge], appearance: BadgeAppearance) -> Cow<'_, [RatingBadge]> {
    if appearance.background != BadgeBackground::Average {
        return Cow::Borrowed(badges);
    }
    let others = badges
        .iter()
        .any(|b| b.source != RatingSource::Mdblist && normalized_score(b).is_some());
    let scores: Vec<f32> = badges
        .iter()
        .filter(|b| !(others && b.source == RatingSource::Mdblist))
        .filter_map(normalized_score)
        .collect();
    if scores.is_empty() {
        return Cow::Owned(Vec::new());
    }
    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    Cow::Owned(vec![RatingBadge {
        source: RatingSource::Imdb,
        value: format!("{:.1}", mean * 10.0),
    }])
}

/// Sample ratings for the admin preview: every source, with scores spread
/// over all three bands (upstream's samples are all 100 %, which would only
/// ever show teal). In the default order the first three (the poster
/// preview) are teal, amber, red. Used for the fork looks, and for every
/// look when the preview draws on real art (`with_art`), so `d` and the fork
/// looks can be compared side by side. `None` otherwise (upstream samples).
pub fn preview_samples(background: BadgeBackground, with_art: bool) -> Option<Vec<RatingBadge>> {
    if !is_fork_look(background) && !with_art {
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
    let scale = sane_scale(badge_scale);
    if look == Look::Average {
        // Normally one badge already (see `collapse`); average again so a
        // caller that skipped `collapse` still gets one pill.
        let collapsed = collapse(badges, appearance);
        return Some(
            collapsed
                .iter()
                .map(|b| render_average_pill(normalized_score(b), font, scale))
                .collect(),
        );
    }
    let m = Metrics::new(look, scale, appearance.shape, false);
    let value_scaled = font.as_scaled(m.value_scale);
    let marks: Vec<Mark> = badges
        .iter()
        .map(|b| mark_for(b, look, label_style, &m))
        .collect();
    // One label area for the whole row (the widest mark), like upstream.
    let label_area = marks.iter().map(|k| k.area(&m, font)).max().unwrap_or(0);
    let value_w = badges
        .iter()
        .map(|b| text_width(&b.value, &value_scaled))
        .max()
        .unwrap_or(0);
    Some(
        badges
            .iter()
            .zip(&marks)
            .map(|(b, mark)| render_horizontal(b, mark, font, &m, look, label_area, value_w))
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
    let scale = sane_scale(badge_scale);
    if look == Look::Average {
        return Some(render_disc(normalized_score(badge), font, scale));
    }
    let m = Metrics::new(look, scale, appearance.shape, true);
    let mark = mark_for(badge, look, label_style, &m);
    Some(render_vertical_inner(badge, &mark, font, &m, look))
}

// --- Metrics ---

fn sane_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn px(base: f32, scale: f32) -> u32 {
    (base * scale).round().max(0.0) as u32
}

struct Metrics {
    /// Height of the zone the mark and figure are centred in.
    content_h: u32,
    /// Bar thickness and bottom inset (0 for Mono).
    bar_t: u32,
    bar_bottom: u32,
    bar_side: f32,
    pad_h: u32,
    text_label_pad_h: u32,
    value_pad_h: u32,
    pill_pad: u32,
    shape: BadgeShape,
    icon_box: u32,
    radius: f32,
    hairline: f32,
    value_scale: PxScale,
    label_scale: PxScale,
    /// No source mark: the badge is too small for one to read.
    compact: bool,
    scale: f32,
}

impl Metrics {
    fn new(look: Look, scale: f32, shape: BadgeShape, vertical: bool) -> Self {
        let (pill_pad, pill_pad_v) = match shape {
            BadgeShape::Pill => (px(BASE_PILL_PADDING, scale), px(BASE_PILL_PADDING_V, scale)),
            BadgeShape::Rounded => (0, 0),
        };
        let compact = !vertical && scale < COMPACT_BELOW;
        let font_base = if vertical {
            BASE_FONT_SIZE
        } else {
            BASE_ROW_FONT_SIZE
        };
        let font_px = (font_base * scale).max(if compact { MIN_FIGURE_PX } else { 1.0 });
        let score = look == Look::Score;
        let content_base = if score && !vertical {
            BASE_SCORE_CONTENT
        } else {
            BASE_BADGE_HEIGHT
        };
        let content_h = (px(content_base, scale) + pill_pad_v)
            .max((font_px * CONTENT_PER_FONT_PX).ceil() as u32)
            .max(MIN_BADGE_HEIGHT);
        let (bar_t, bar_bottom) = if score {
            (
                px(BASE_BAR_THICKNESS, scale).max(2),
                px(BASE_BAR_BOTTOM, scale).max(1),
            )
        } else {
            (0, 0)
        };
        let icon_base = if vertical {
            BASE_ICON_HEIGHT
        } else {
            BASE_ROW_ICON_BOX
        };
        Self {
            content_h,
            bar_t,
            bar_bottom,
            bar_side: (BASE_BAR_SIDE * scale).max(2.0),
            pad_h: px(BASE_BADGE_PADDING_H, scale),
            text_label_pad_h: px(BASE_TEXT_LABEL_PADDING_H, scale),
            value_pad_h: px(BASE_BADGE_VALUE_PADDING_H, scale),
            pill_pad,
            shape,
            icon_box: px(icon_base, scale).max(1),
            radius: (BASE_BADGE_HEIGHT * scale * RADIUS_SHARE).max(1.0),
            hairline: scale.clamp(1.0, 4.0),
            value_scale: PxScale::from(font_px),
            label_scale: PxScale::from((BASE_LABEL_FONT_SIZE * scale).max(1.0)),
            compact,
            scale,
        }
    }

    /// Total height of a horizontal badge (content zone plus the bar zone).
    fn row_height(&self) -> u32 {
        self.content_h + self.bar_t + self.bar_bottom
    }
}

// --- Source marks ---

/// What identifies the source on a badge.
enum Mark {
    None,
    Image(RgbaImage),
    Text(&'static str, Rgba<u8>),
}

impl Mark {
    fn width(&self, m: &Metrics, font: &FontArc) -> u32 {
        match self {
            Mark::None => 0,
            Mark::Image(img) => img.width(),
            Mark::Text(t, _) => text_width(t, &font.as_scaled(m.label_scale)),
        }
    }

    /// Width of the label area this mark needs (mark plus padding both
    /// sides). Without a mark, a half value padding keeps the figure centred.
    fn area(&self, m: &Metrics, font: &FontArc) -> u32 {
        match self {
            Mark::None => m.value_pad_h / 2 + 1,
            Mark::Image(img) => img.width() + 2 * m.pad_h,
            Mark::Text(..) => self.width(m, font) + 2 * m.text_label_pad_h,
        }
    }
}

/// Glyphs that are filled tiles with knocked-out letters (IMDb, MDBList):
/// at badge size they read as a bright blob, so Mono writes the name instead.
fn glyph_is_tile(source: RatingSource) -> bool {
    matches!(source, RatingSource::Imdb | RatingSource::Mdblist)
}

fn mark_for(badge: &RatingBadge, look: Look, label_style: LabelStyle, m: &Metrics) -> Mark {
    if m.compact {
        return Mark::None;
    }
    match (look, label_style) {
        (Look::Mono, LabelStyle::Text) => Mark::Text(badge.source.label(), MONO_LABEL),
        (Look::Mono, _) if glyph_is_tile(badge.source) => {
            Mark::Text(badge.source.label(), MONO_MARK_TEXT)
        }
        (Look::Mono, _) => Mark::Image(glyph(badge.source, m.icon_box, MONO_GLYPH_OPACITY)),
        (_, LabelStyle::Text) => Mark::Text(badge.source.label(), WHITE),
        (_, LabelStyle::Icon) => Mark::Image(glyph(badge.source, m.icon_box, 1.0)),
        (_, LabelStyle::Official) => Mark::Image(official(badge, m.icon_box)),
    }
}

/// White source glyph scaled to `box_h`, alpha times `opacity`.
fn glyph(source: RatingSource, box_h: u32, opacity: f32) -> RgbaImage {
    let icon = icons::icon_for_source(&source);
    let mut out = scale_to(icon, (scaled_width(icon, box_h), box_h));
    if opacity < 1.0 {
        for p in out.pixels_mut() {
            p.0[3] = (p.0[3] as f32 * opacity).round() as u8;
        }
    }
    out
}

/// Official logo fit into a `box_h` square. Letterboxd's logo sits on its own
/// dark tile, which showed as a box on the plate; it is keyed out so the
/// three coloured dots sit on the plate directly.
fn official(badge: &RatingBadge, box_h: u32) -> RgbaImage {
    let icon: &RgbaImage = match badge.source {
        RatingSource::Letterboxd => &*LETTERBOXD_DOTS,
        _ => icons::official_icon_for_badge(badge),
    };
    scale_to(icon, fit_in_box(icon, box_h))
}

/// The official Letterboxd logo without its dark tile, cropped to the dots.
static LETTERBOXD_DOTS: LazyLock<RgbaImage> = LazyLock::new(|| {
    let badge = RatingBadge {
        source: RatingSource::Letterboxd,
        value: String::new(),
    };
    key_out_dark_tile(icons::official_icon_for_badge(&badge))
});

/// Make dark, unsaturated pixels transparent (soft edge) and crop to what
/// is left. Saturated or light pixels (the logo itself) are kept.
fn key_out_dark_tile(icon: &RgbaImage) -> RgbaImage {
    let mut out = icon.clone();
    for p in out.pixels_mut() {
        let [r, g, b, a] = p.0;
        let mx = r.max(g).max(b) as f32;
        let mn = r.min(g).min(b) as f32;
        let keep = ((mx - mn - 25.0) / 35.0)
            .max((mx - 110.0) / 50.0)
            .clamp(0.0, 1.0);
        p.0[3] = (a as f32 * keep).round() as u8;
    }
    crop_to_alpha(&out).unwrap_or(out)
}

fn crop_to_alpha(img: &RgbaImage) -> Option<RgbaImage> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 8 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 <= x1 && y0 <= y1)
        .then(|| imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image())
}

fn scale_to(icon: &RgbaImage, (w, h): (u32, u32)) -> RgbaImage {
    if icon.width() == w && icon.height() == h {
        icon.clone()
    } else {
        imageops::resize(icon, w.max(1), h.max(1), imageops::FilterType::Lanczos3)
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_mark(
    img: &mut RgbaImage,
    mark: &Mark,
    font: &FontArc,
    m: &Metrics,
    cx: u32,
    area_w: u32,
    area_y: u32,
    area_h: u32,
) {
    let w = mark.width(m, font);
    let x = cx + area_w.saturating_sub(w) / 2;
    match mark {
        Mark::None => {}
        Mark::Image(icon) => {
            let y = area_y + area_h.saturating_sub(icon.height()) / 2;
            imageops::overlay(img, icon, x as i64, y as i64);
        }
        Mark::Text(t, color) => {
            let y = area_y as i32 + (area_h as i32 - m.label_scale.y as i32) / 2;
            draw_text_mut(img, *color, x as i32, y, m.label_scale, font, t);
        }
    }
}

// --- Drawing ---

fn plate(look: Look, w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_pixel(
        w.max(1),
        h.max(1),
        Rgba([OFF_AIR[0], OFF_AIR[1], OFF_AIR[2], look.plate_alpha()]),
    )
}

fn render_horizontal(
    badge: &RatingBadge,
    mark: &Mark,
    font: &FontArc,
    m: &Metrics,
    look: Look,
    area: u32,
    value_w: u32,
) -> RgbaImage {
    let value_x = m.pill_pad + area;
    let total_w = value_x + value_w + m.value_pad_h + m.value_pad_h / 2 + 2 + m.pill_pad;
    let h = m.row_height();
    let mut img = plate(look, total_w, h);

    draw_mark(&mut img, mark, font, m, m.pill_pad, area, 0, m.content_h);
    let vw = text_width(&badge.value, &font.as_scaled(m.value_scale));
    let vx = value_x + m.value_pad_h + value_w.saturating_sub(vw) / 2;
    let vy = (m.content_h as i32 - m.value_scale.y as i32) / 2;
    draw_text_mut(
        &mut img,
        WHITE,
        vx as i32,
        vy,
        m.value_scale,
        font,
        &badge.value,
    );

    let radius = match m.shape {
        BadgeShape::Pill => h as f32 / 2.0,
        BadgeShape::Rounded => m.radius,
    };
    if m.bar_t > 0 {
        draw_bar(&mut img, normalized_score(badge), m, radius);
    }
    finish_plate(&mut img, radius, m.hairline);
    img
}

fn render_vertical_inner(
    badge: &RatingBadge,
    mark: &Mark,
    font: &FontArc,
    m: &Metrics,
    look: Look,
) -> RgbaImage {
    let w = px(BASE_VERT_BADGE_WIDTH, m.scale).max(MIN_BADGE_HEIGHT);
    let pad_v = px(BASE_VERT_BADGE_PADDING_V, m.scale) + m.pill_pad;
    // Every mark gets the same area, text included, so all plates in a
    // column have one height (text labels are centred in it).
    let label_area_h = match mark {
        Mark::Image(_) | Mark::Text(..) => m.icon_box,
        Mark::None => 0,
    };
    let gap = px(BASE_VERT_GAP, m.scale);
    let content_h = pad_v + label_area_h + gap + m.value_scale.y as u32 + pad_v;
    let h = (content_h + m.bar_t + m.bar_bottom).max(MIN_BADGE_HEIGHT);
    let value_area_y = pad_v + label_area_h + gap / 2;
    let mut img = plate(look, w, h);

    draw_mark(&mut img, mark, font, m, 0, w, 0, value_area_y);
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

    let radius = match m.shape {
        BadgeShape::Pill => w as f32 / 2.0,
        BadgeShape::Rounded => m.radius,
    };
    if m.bar_t > 0 {
        draw_bar(&mut img, normalized_score(badge), m, radius);
    }
    finish_plate(&mut img, radius, m.hairline);
    img
}

/// The Score bar: a rounded track (white 16 %) inset from the plate's sides
/// and bottom, filled from the left in the band colour. The side inset is at
/// least what keeps the bar clear of the corner arcs, so it is never clipped.
fn draw_bar(img: &mut RgbaImage, score: Option<f32>, m: &Metrics, corner_r: f32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let t = m.bar_t as f32;
    let y1 = h - m.bar_bottom as f32;
    let y0 = y1 - t;
    // Distance from the corner circle's centre row to the bar's bottom edge.
    let r = corner_r.min(w / 2.0).min(h / 2.0);
    let dy = (r - m.bar_bottom as f32).max(0.0);
    let corner_clear = r - (r * r - dy * dy).max(0.0).sqrt();
    let side = m.bar_side.max(corner_clear + 1.5 * m.hairline).min(w / 3.0);
    let (x0, x1) = (side, w - side);
    if x1 <= x0 + t {
        return;
    }
    let fill_to = score.map(|s| x0 + s * (x1 - x0));
    let color = score.map(band_color).unwrap_or(TRACK);
    let (ys, ye) = (
        y0.floor().max(0.0) as u32,
        (y1.ceil() as u32).min(img.height()),
    );
    for y in ys..ye {
        for x in (x0.floor() as u32)..(x1.ceil() as u32).min(img.width()) {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cov = coverage(rounded_rect_sdf(cx, cy, x0, y0, x1, y1, t / 2.0));
            if cov <= 0.0 {
                continue;
            }
            let base = *img.get_pixel(x, y);
            let mut p = blend(base, TRACK, cov);
            if let Some(f) = fill_to {
                // The filled part is a capsule of its own, so its right end is round too.
                let fill_cov =
                    coverage(rounded_rect_sdf(cx, cy, x0, y0, f.max(x0 + t), y1, t / 2.0));
                let fill_cov = if f <= x0 { 0.0 } else { fill_cov };
                p = blend(p, color, fill_cov);
            }
            img.put_pixel(x, y, p);
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

/// A score ring (track + band-coloured arc clockwise from 12 o'clock) drawn
/// onto `img` around (`cx`, `cy`) between radii `r_in` and `r_out`.
fn draw_ring(img: &mut RgbaImage, cx: f32, cy: f32, r_in: f32, r_out: f32, score: Option<f32>) {
    let sweep = score.unwrap_or(0.0) * std::f32::consts::TAU;
    let color = score.map(band_color).unwrap_or(TRACK);
    let r_mid = (r_in + r_out) / 2.0;
    let (x0, x1) = (
        (cx - r_out - 1.0).max(0.0) as u32,
        ((cx + r_out + 1.0) as u32).min(img.width()),
    );
    let (y0, y1) = (
        (cy - r_out - 1.0).max(0.0) as u32,
        ((cy + r_out + 1.0) as u32).min(img.height()),
    );
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let dist = (dx * dx + dy * dy).sqrt();
            let ring = ((r_out - dist).min(dist - r_in) + 0.5).clamp(0.0, 1.0);
            if ring <= 0.0 {
                continue;
            }
            let mut theta = dx.atan2(-dy);
            if theta < 0.0 {
                theta += std::f32::consts::TAU;
            }
            let arc = ((sweep - theta) * r_mid + 0.5).clamp(0.0, 1.0);
            let mut p = blend(*img.get_pixel(x, y), TRACK, ring * (1.0 - arc));
            p = blend(p, color, ring * arc);
            img.put_pixel(x, y, p);
        }
    }
}

/// Size a figure so it is at most `max_w` wide; returns the scale and width.
fn fit_text(text: &str, font: &FontArc, size: f32, max_w: f32) -> (PxScale, f32) {
    let w = text_width(text, &font.as_scaled(PxScale::from(size))) as f32;
    let size = if w > max_w && w > 0.0 {
        (size * max_w / w).max(1.0)
    } else {
        size
    };
    let s = PxScale::from(size);
    (s, text_width(text, &font.as_scaled(s)) as f32)
}

/// ab_glyph's px size is Inter's ascent-to-descent height: the baseline sits
/// 0.80 size below the line top and figures are 0.60 size tall, so their
/// middle is 0.50 size down.
fn figure_top(centre_y: f32, size: f32) -> i32 {
    (centre_y - 0.5 * size).round() as i32
}

fn average_figure(score: Option<f32>) -> String {
    match score {
        Some(s) => average_text(s),
        None => "\u{2013}".to_string(),
    }
}

/// The Average look in a row: a pill as tall as a Score badge, so it sits
/// exactly where a badge row sits, with a ring dial at the left and the
/// figure, larger than a row badge's, to its right.
fn render_average_pill(score: Option<f32>, font: &FontArc, scale: f32) -> RgbaImage {
    let h = (BASE_AVERAGE_PILL * scale).round().max(MIN_AVERAGE_PILL_PX);
    let size = (h * PILL_FONT_SHARE).max(MIN_AVERAGE_FIGURE_PX);
    let dial = (h * PILL_DIAL_SHARE).round();
    let side = ((h - dial) / 2.0).round();
    let gap = (h * 0.20).round();
    let text = average_figure(score);
    let (font_scale, tw) = fit_text(&text, font, size, f32::MAX);
    let right = side + (h * 0.12).round();
    let w = (side + dial + gap + tw + right).ceil() as u32;
    let mut img = plate(Look::Average, w, h as u32);
    let ring_t = (dial * DIAL_RING_SHARE).round().max(2.0);
    let (cx, cy) = (side + dial / 2.0, h / 2.0);
    draw_ring(&mut img, cx, cy, dial / 2.0 - ring_t, dial / 2.0, score);
    let tx = (side + dial + gap).round() as i32;
    draw_text_mut(
        &mut img,
        WHITE,
        tx,
        figure_top(cy, font_scale.y),
        font_scale,
        font,
        &text,
    );
    finish_plate(&mut img, h / 2.0, scale.clamp(1.0, 4.0));
    img
}

/// The Average look in a column (backdrop and episode defaults): a dark disc
/// with a score ring and the 0–10 figure in the middle.
fn render_disc(score: Option<f32>, font: &FontArc, scale: f32) -> RgbaImage {
    let d = (BASE_BADGE_HEIGHT * scale * DISC_SHARE)
        .round()
        .max(MIN_DISC_PX) as u32;
    let df = d as f32;
    let c = df / 2.0;
    let radius = df / 2.0;
    let inset = (df * DISC_INSET_SHARE).round().max(1.0);
    let thickness = (df * DISC_RING_SHARE).round().max(2.0);
    let r_out = radius - inset;
    let r_in = (r_out - thickness).max(1.0);
    let plate_px = Rgba([OFF_AIR[0], OFF_AIR[1], OFF_AIR[2], AVERAGE_PLATE_ALPHA]);

    let mut img = RgbaImage::new(d, d);
    for y in 0..d {
        for x in 0..d {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let dist = (dx * dx + dy * dy).sqrt();
            let disc = (radius - dist + 0.5).clamp(0.0, 1.0);
            if disc > 0.0 {
                let mut p = plate_px;
                p.0[3] = (p.0[3] as f32 * disc).round() as u8;
                img.put_pixel(x, y, p);
            }
        }
    }
    draw_ring(&mut img, c, c, r_in, r_out, score);

    let text = average_figure(score);
    let size = (df * DISC_FONT_SHARE).max(MIN_AVERAGE_FIGURE_PX);
    let (font_scale, tw) = fit_text(&text, font, size, 2.0 * r_in * 0.80);
    let tx = (c - tw / 2.0).round() as i32;
    draw_text_mut(
        &mut img,
        WHITE,
        tx,
        figure_top(c, font_scale.y),
        font_scale,
        font,
        &text,
    );
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
