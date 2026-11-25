//! `BACKDROP_MAX_WIDTH`: cap the render width of `large` / `very-large`
//! backdrops.
//!
//! Upstream renders those sizes on a 3840 px canvas from the TMDB `original`
//! (often a 1920 px source, so the render is an upscale), which is the
//! biggest single allocation per request. With a cap set, a backdrop whose
//! native width is above the cap renders at the cap instead, fetches the
//! smallest TMDB size that still covers it, scales its badges with the canvas
//! exactly as upstream does for its own smaller sizes, and carries a
//! `.mw<N>` cache-key token so capped and uncapped renders never share a key.
//!
//! Unset (the default), or a cap that does not reduce the native width,
//! changes nothing: same width, same TMDB size, same badge scale, same cache
//! key and CDN hash as upstream. Logos, posters and episode stills are never
//! affected.

use sha2::{Digest, Sha256};

use crate::cache::ImageType;
use crate::image::serve::resolve_image_size;
use crate::louk::config::LoukConfig;
use crate::services::db::ImageSize;

/// Smallest accepted cap (below this a backdrop is not useful).
pub const MIN_CAP: u32 = 320;
/// Largest accepted cap (upstream's widest backdrop render).
pub const MAX_CAP: u32 = 3840;

/// Badge base width for backdrops: upstream's `ImageSize::badge_scale` divides
/// the backdrop canvas width by this (badges are designed at 1920 px).
const BACKDROP_BADGE_BASE_WIDTH: f32 = 1920.0;

/// The image-size tokens upstream always puts LAST in a cache-key suffix.
const SIZE_TOKENS: [&str; 4] = [".zvl", ".zs", ".zm", ".zl"];

#[derive(Debug, PartialEq, Eq)]
pub struct BackdropPlan {
    /// Render canvas width in px.
    pub target_width: u32,
    /// TMDB CDN size to download the source at.
    pub tmdb_size: &'static str,
    /// Cache-key token (`.mw<N>`), present only when the cap reduces the width.
    pub token: Option<String>,
}

/// How a backdrop of `size` renders under `cap`.
pub fn plan(size: ImageSize, cap: Option<u32>) -> BackdropPlan {
    let native = size.backdrop_target_width();
    match cap {
        Some(cap) if cap < native => BackdropPlan {
            target_width: cap,
            // TMDB backdrop sizes are w300, w780, w1280 and original. SPEC
            // P1: the smaller of w1280 / original that covers the cap.
            tmdb_size: if cap <= 1280 { "w1280" } else { "original" },
            token: Some(format!(".mw{cap}")),
        },
        _ => BackdropPlan { target_width: native, tmdb_size: size.tmdb_size(), token: None },
    }
}

/// Parse `BACKDROP_MAX_WIDTH`. Valid: an integer in `MIN_CAP..=MAX_CAP`;
/// anything else (empty, non-numeric, out of range) is `None`.
pub fn parse_cap(raw: Option<&str>) -> Option<u32> {
    let n: u32 = raw?.trim().parse().ok()?;
    (MIN_CAP..=MAX_CAP).contains(&n).then_some(n)
}

/// Insert `token` immediately before the trailing image-size token
/// (`.zs/.zm/.zl/.zvl`), which upstream's cache upgrades expect to stay last.
pub fn splice_token(suffix: String, token: Option<&str>) -> String {
    let Some(token) = token else { return suffix };
    for size in SIZE_TOKENS {
        if let Some(head) = suffix.strip_suffix(size) {
            return format!("{head}{token}{size}");
        }
    }
    // Unreachable with upstream's suffix builder (the size token is always
    // appended). Still keep the token so capped and uncapped never collide.
    tracing::warn!(suffix = %suffix, "backdrop cap: no image-size token in cache suffix; appending");
    format!("{suffix}{token}")
}

/// The cap token for this request, if any (backdrops only).
fn token_for(cfg: &LoukConfig, kind: ImageType, image_size: Option<ImageSize>) -> Option<String> {
    if kind != ImageType::Backdrop {
        return None;
    }
    plan(resolve_image_size(image_size), cfg.backdrop_max_width).token
}

/// Cache-key suffix for a render: upstream's suffix, plus the cap token for a
/// capped backdrop. A no-op for every other kind and for uncapped backdrops.
pub fn backdrop_suffix(cfg: &LoukConfig, kind: ImageType, image_size: Option<ImageSize>, suffix: String) -> String {
    let token = token_for(cfg, kind, image_size);
    splice_token(suffix, token.as_deref())
}

/// CDN settings hash: upstream's hash unchanged unless the cap token applies,
/// else a hash of the upstream hash plus the token, with the same length as
/// upstream's (so `/c/<hash>/` URLs look alike).
pub fn cdn_hash(cfg: &LoukConfig, kind: ImageType, image_size: Option<ImageSize>, hash: String) -> String {
    let Some(token) = token_for(cfg, kind, image_size) else { return hash };
    let mut hasher = Sha256::new();
    hasher.update(hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    let n = hash.len().div_ceil(2).clamp(1, digest.len());
    let mut out: String = digest[..n].iter().map(|b| format!("{b:02x}")).collect();
    out.truncate(hash.len().clamp(1, digest.len() * 2));
    out
}

/// `(target_width, badge_scale)` for a backdrop render, with
/// `badge_size_factor` (the per-key badge size) applied like upstream does.
/// Uncapped: exactly upstream's `(backdrop_target_width(), badge_scale(Backdrop) * f)`.
/// Capped: the canvas is `cap` wide and badges scale with it by upstream's
/// own rule (`width / 1920`), so they keep the same size relative to the image.
pub fn render_dims(cfg: &LoukConfig, size: ImageSize, badge_size_factor: f32) -> (u32, f32) {
    let p = plan(size, cfg.backdrop_max_width);
    let scale = if p.token.is_none() {
        size.badge_scale(ImageType::Backdrop)
    } else {
        p.target_width as f32 / BACKDROP_BADGE_BASE_WIDTH
    };
    (p.target_width, scale * badge_size_factor)
}

/// TMDB CDN size to fetch for a logo/backdrop source. Backdrops follow the
/// cap's plan; every other kind keeps upstream's `ImageSize::tmdb_size`.
pub fn tmdb_size(cfg: &LoukConfig, kind: ImageType, image_size: Option<ImageSize>) -> &'static str {
    let size = resolve_image_size(image_size);
    match kind {
        ImageType::Backdrop => plan(size, cfg.backdrop_max_width).tmdb_size,
        _ => size.tmdb_size(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ImageSize::{Large, Medium, Small, VeryLarge};

    const ALL: [ImageSize; 4] = [Small, Medium, Large, VeryLarge];

    fn capped(cap: u32) -> LoukConfig {
        LoukConfig { backdrop_max_width: Some(cap), ..LoukConfig::default() }
    }

    #[test]
    fn plan_unset_is_upstream() {
        for s in ALL {
            assert_eq!(plan(s, None), BackdropPlan { target_width: s.backdrop_target_width(), tmdb_size: s.tmdb_size(), token: None });
        }
    }

    #[test]
    fn plan_large_capped_1920() {
        assert_eq!(plan(Large, Some(1920)), BackdropPlan { target_width: 1920, tmdb_size: "original", token: Some(".mw1920".into()) });
    }

    #[test]
    fn plan_very_large_capped_1920() {
        assert_eq!(plan(VeryLarge, Some(1920)), BackdropPlan { target_width: 1920, tmdb_size: "original", token: Some(".mw1920".into()) });
    }

    #[test]
    fn plan_cap_not_reducing_has_no_token() {
        assert_eq!(plan(Medium, Some(1920)).token, None);
        assert_eq!(plan(Small, Some(1920)).token, None);
        assert_eq!(plan(Medium, Some(1920)).tmdb_size, "w780");
        assert_eq!(plan(Small, Some(1920)).tmdb_size, "w780");
    }

    #[test]
    fn plan_cap_at_or_above_native_is_upstream() {
        for s in ALL {
            for cap in [s.backdrop_target_width(), MAX_CAP] {
                assert_eq!(plan(s, Some(cap)), plan(s, None), "{s:?} cap {cap}");
            }
        }
    }

    #[test]
    fn plan_cap_1280_uses_w1280() {
        assert_eq!(plan(Large, Some(1280)), BackdropPlan { target_width: 1280, tmdb_size: "w1280", token: Some(".mw1280".into()) });
        // Small is natively 1280 wide: a 1280 cap does not reduce it.
        assert_eq!(plan(Small, Some(1280)), plan(Small, None));
        assert_eq!(plan(Medium, Some(1280)).token.as_deref(), Some(".mw1280"));
    }

    #[test]
    fn parse_cap_validates() {
        assert_eq!(parse_cap(Some("1920")), Some(1920));
        assert_eq!(parse_cap(Some(" 1920 ")), Some(1920));
        assert_eq!(parse_cap(Some("320")), Some(320));
        assert_eq!(parse_cap(Some("3840")), Some(3840));
        for bad in ["", "abc", "0", "100", "319", "3841", "5000", "-1", "1920px", "19.5"] {
            assert_eq!(parse_cap(Some(bad)), None, "{bad:?}");
        }
        assert_eq!(parse_cap(None), None);
    }

    #[test]
    fn splice_before_size_token() {
        assert_eq!(splice_token("@ir.ptr.sh.li.dv.bl.shr.bgd.zl".into(), Some(".mw1920")), "@ir.ptr.sh.li.dv.bl.shr.bgd.mw1920.zl");
        assert_eq!(splice_token("@.ptr.zvl".into(), Some(".mw1920")), "@.ptr.mw1920.zvl");
        assert_eq!(splice_token("@.ptr.eh5.zm".into(), Some(".mw1280")), "@.ptr.eh5.mw1280.zm");
        assert_eq!(splice_token("@x.zl".into(), None), "@x.zl");
    }

    #[test]
    fn backdrop_suffix_only_touches_capped_backdrops() {
        let s = "@.ptr.sh.li.dv.bm.shr.bgd.zl".to_string();
        let cfg = capped(1920);
        assert_eq!(backdrop_suffix(&cfg, ImageType::Backdrop, Some(Large), s.clone()), "@.ptr.sh.li.dv.bm.shr.bgd.mw1920.zl");
        for kind in [ImageType::Poster, ImageType::Logo, ImageType::Episode] {
            assert_eq!(backdrop_suffix(&cfg, kind, Some(Large), s.clone()), s, "{kind:?}");
        }
        assert_eq!(backdrop_suffix(&LoukConfig::default(), ImageType::Backdrop, Some(Large), s.clone()), s);
        // No imageSize = Medium = 1920 native: no token.
        let m = "@.ptr.sh.li.dv.bm.shr.bgd.zm".to_string();
        assert_eq!(backdrop_suffix(&cfg, ImageType::Backdrop, None, m.clone()), m);
    }

    #[test]
    fn cdn_hash_unchanged_without_token() {
        let h = "0123456789abcdef0123456789abcdef".to_string();
        assert_eq!(cdn_hash(&LoukConfig::default(), ImageType::Backdrop, Some(Large), h.clone()), h);
        let cfg = capped(1920);
        assert_eq!(cdn_hash(&cfg, ImageType::Backdrop, Some(Medium), h.clone()), h);
        assert_eq!(cdn_hash(&cfg, ImageType::Backdrop, None, h.clone()), h);
        assert_eq!(cdn_hash(&cfg, ImageType::Logo, Some(Large), h.clone()), h);
        assert_eq!(cdn_hash(&cfg, ImageType::Episode, Some(VeryLarge), h.clone()), h);
    }

    #[test]
    fn cdn_hash_differs_with_token_and_keeps_length() {
        let h = "0123456789abcdef0123456789abcdef".to_string();
        let a = cdn_hash(&capped(1920), ImageType::Backdrop, Some(Large), h.clone());
        let b = cdn_hash(&capped(1280), ImageType::Backdrop, Some(Large), h.clone());
        assert_ne!(a, h);
        assert_ne!(a, b);
        assert_eq!(a.len(), h.len());
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        // Deterministic, and Large/VeryLarge share it only via different
        // upstream hashes (their size token differs upstream already).
        assert_eq!(a, cdn_hash(&capped(1920), ImageType::Backdrop, Some(VeryLarge), h));
    }

    #[test]
    fn render_dims_unset_is_upstream_bit_for_bit() {
        for s in ALL {
            for f in [0.5_f32, 1.0, 1.5, 2.0] {
                let (w, scale) = render_dims(&LoukConfig::default(), s, f);
                assert_eq!(w, s.backdrop_target_width());
                assert_eq!(scale.to_bits(), (s.badge_scale(ImageType::Backdrop) * f).to_bits(), "{s:?} {f}");
            }
        }
    }

    #[test]
    fn render_dims_capped_matches_upstream_medium() {
        // A 1920-capped large backdrop renders exactly like upstream's medium:
        // same canvas, same badge scale (badges keep their size relative to
        // the image, as upstream scales them between its own sizes).
        for s in [Large, VeryLarge] {
            for f in [0.5_f32, 1.0, 1.5, 2.0] {
                let (w, scale) = render_dims(&capped(1920), s, f);
                assert_eq!(w, Medium.backdrop_target_width());
                assert_eq!(scale.to_bits(), (Medium.badge_scale(ImageType::Backdrop) * f).to_bits());
            }
        }
        let (w, scale) = render_dims(&capped(1280), Large, 1.0);
        assert_eq!((w, scale), (1280, 1280.0 / 1920.0));
    }

    #[test]
    fn tmdb_size_backdrop_only() {
        let cfg = capped(1280);
        assert_eq!(tmdb_size(&cfg, ImageType::Backdrop, Some(Large)), "w1280");
        assert_eq!(tmdb_size(&cfg, ImageType::Logo, Some(Large)), "original");
        assert_eq!(tmdb_size(&cfg, ImageType::Backdrop, Some(Medium)), "w1280");
        assert_eq!(tmdb_size(&cfg, ImageType::Backdrop, Some(Small)), "w780");
        for s in ALL {
            for kind in [ImageType::Backdrop, ImageType::Logo] {
                assert_eq!(tmdb_size(&LoukConfig::default(), kind, Some(s)), s.tmdb_size());
            }
        }
        assert_eq!(tmdb_size(&LoukConfig::default(), ImageType::Backdrop, None), Medium.tmdb_size());
    }
}
