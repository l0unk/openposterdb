//! Render-path hooks (called from `image/serve.rs`, one line each).
//!
//! Every hook first asks the in-memory store; with no override row for the
//! title it returns its input unchanged (or `None`), so cache keys and bytes
//! are exactly upstream's. The store never queries the DB per request and
//! nothing here returns an error: when an override's source cannot be
//! fetched, read or decoded, the hook logs a warning and the automatic
//! selection runs as if there were no override.

use std::io::Cursor;
use std::sync::Arc;

use image::{ImageFormat, ImageReader, Limits};

use super::{OverrideKey, OverrideSource, TitleOverride, purge, store, uploads};
use crate::AppState;
use crate::cache::{self, ImageType};
use crate::id::{MediaType, ResolvedId};
use crate::image::generate;
use crate::image::serve::resolve_image_size;
use crate::services::db::{ImageSize, RenderSettings};
use crate::services::ratings::RatingBadge;

/// Largest source an override may point at (upstream's decode limit).
const MAX_SOURCE_PIXELS: u64 = 8192 * 8192;

/// `(media_type, tmdb_id, season, episode)` an override of `kind` is keyed
/// on for this resolution. Non-episode kinds of an episode id belong to the
/// show (upstream uplifts them); stills need an episode resolution.
pub fn title_of(resolved: &ResolvedId, kind: ImageType) -> Option<(&'static str, u64, i32, i32)> {
    let num = |n: u32| i32::try_from(n).ok();
    match (kind, resolved.media_type) {
        (ImageType::Episode, MediaType::Episode) => {
            let e = resolved.episode.as_ref()?;
            Some((
                "tv",
                e.show_tmdb_id,
                num(e.season_number)?,
                num(e.episode_number)?,
            ))
        }
        (ImageType::Episode, _) => None,
        (_, MediaType::Movie) => Some(("movie", resolved.tmdb_id, -1, -1)),
        (_, MediaType::Tv) => Some(("tv", resolved.tmdb_id, -1, -1)),
        (_, MediaType::Episode) => resolved
            .episode
            .as_ref()
            .map(|e| ("tv", e.show_tmdb_id, -1, -1)),
    }
}

/// The override in effect for this render, if any.
pub async fn find(
    state: &AppState,
    resolved: &ResolvedId,
    kind: ImageType,
    lang: &str,
) -> Option<TitleOverride> {
    find_keyed(state, resolved, kind, lang)
        .await
        .map(|(_, ov)| ov)
}

/// [`find`], plus the key of the row that matched.
pub async fn find_keyed(
    state: &AppState,
    resolved: &ResolvedId,
    kind: ImageType,
    lang: &str,
) -> Option<(OverrideKey, TitleOverride)> {
    let (media_type, tmdb_id, season, episode) = title_of(resolved, kind)?;
    store(state)
        .lookup_keyed(&state.db, media_type, tmdb_id, kind, season, episode, lang)
        .await
}

/// The override's image for this render. When an override with art cannot
/// deliver it, the automatic art is rendered and cached under the same key,
/// so a delayed re-purge is scheduled (once per override version).
async fn source_for(
    state: &AppState,
    key: &OverrideKey,
    ov: &TitleOverride,
    kind: ImageType,
    image_size: Option<ImageSize>,
) -> Option<Vec<u8>> {
    if ov.source == OverrideSource::Auto {
        return None;
    }
    let bytes = fetch_source(state, &ov.source, kind, image_size).await;
    if bytes.is_none() {
        purge::note_source_failure(state, key, ov.updated_at);
    }
    bytes
}

/// True when an override replaces the art (any source but `auto`).
pub async fn has_art(state: &AppState, resolved: &ResolvedId, kind: ImageType, lang: &str) -> bool {
    find(state, resolved, kind, lang)
        .await
        .is_some_and(|o| o.source != OverrideSource::Auto)
}

/// The ratings limit to render with: 0 when a "no badges" override applies,
/// else `limit` unchanged. Upstream's limit-0 handling then gives the
/// badge-free cache key on both the fast and the slow path.
pub async fn badge_limit(
    state: &AppState,
    resolved: &ResolvedId,
    kind: ImageType,
    lang: &str,
    limit: i32,
) -> i32 {
    if limit == 0 {
        return 0;
    }
    match find(state, resolved, kind, lang).await {
        Some(o) if o.no_badges => 0,
        _ => limit,
    }
}

/// [`badge_limit`] before the request is resolved (the fast path): only
/// when some override has "no badges", and only from `id_cache` (no
/// network). Unknown ids keep `limit`; the slow path re-checks.
pub async fn badge_limit_hint(
    state: &AppState,
    id_type: &str,
    id_value: &str,
    kind: ImageType,
    lang: &str,
    limit: i32,
) -> i32 {
    if limit == 0 || !store(state).any_no_badges(&state.db).await {
        return limit;
    }
    match state.id_cache.get(&format!("{id_type}/{id_value}")).await {
        Some(resolved) => badge_limit(state, &resolved, kind, lang, limit).await,
        None => limit,
    }
}

/// `badges`, or none when a "no badges" override applies (refresh paths
/// that compute badges from the unmodified settings).
pub async fn strip_badges(
    state: &AppState,
    resolved: &ResolvedId,
    kind: ImageType,
    lang: &str,
    badges: Vec<RatingBadge>,
) -> Vec<RatingBadge> {
    if badges.is_empty() {
        return badges;
    }
    match find(state, resolved, kind, lang).await {
        Some(o) if o.no_badges => Vec::new(),
        _ => badges,
    }
}

/// Source bytes of the override for this logo/backdrop render, or `None` for
/// the automatic selection.
pub async fn source_bytes(
    state: &AppState,
    resolved: &ResolvedId,
    kind: ImageType,
    lang: &str,
    image_size: Option<ImageSize>,
) -> Option<Vec<u8>> {
    let (key, ov) = find_keyed(state, resolved, kind, lang).await?;
    source_for(state, &key, &ov, kind, image_size).await
}

/// Render a poster from the override's source (called at the top of
/// `generate_poster_with_source`, before the TMDB-vs-fanart branches).
/// Mirrors upstream's `generate_poster` call there; `None` = no override or
/// it failed (logged), and the automatic selection runs.
pub async fn render_poster(
    state: &AppState,
    resolved: &ResolvedId,
    badges: &[RatingBadge],
    settings: &RenderSettings,
    image_size: Option<ImageSize>,
) -> Option<Vec<u8>> {
    let (key, ov) = find_keyed(state, resolved, ImageType::Poster, &settings.lang).await?;
    let bytes = source_for(state, &key, &ov, ImageType::Poster, image_size).await?;
    let badges: &[RatingBadge] = if ov.no_badges { &[] } else { badges };
    let resolved_size = resolve_image_size(image_size);
    let tmdb_size: Arc<str> = resolved_size.tmdb_size().into();
    let r = generate::generate_poster(generate::ImageParams {
        poster_path: ov.source.value(),
        badges,
        tmdb: &state.tmdb,
        font: &state.font,
        quality: state.config.image_quality,
        cache_dir: &state.config.cache_dir,
        image_stale_secs: state.config.image_stale_secs,
        poster_bytes_override: Some(bytes),
        poster_position: settings.poster_position,
        badge_style: settings.poster_badge_style,
        label_style: settings.poster_label_style,
        badge_appearance: settings.poster_appearance(),
        badge_direction: settings.poster_badge_direction,
        poster_badge_split: settings.poster_badge_split,
        poster_fit: settings.poster_fit,
        render_semaphore: state.render_semaphore.clone(),
        target_width: resolved_size.poster_target_width(),
        badge_scale: resolved_size.badge_scale(ImageType::Poster)
            * settings.poster_badge_size.scale_factor(),
        badge_size: settings.poster_badge_size,
        tmdb_size,
        external_cache_only: state.config.external_cache_only,
    })
    .await;
    match r {
        Ok(b) => Some(b),
        Err(e) => {
            purge::note_source_failure(state, &key, ov.updated_at);
            tracing::warn!(error = %e, tmdb_id = resolved.tmdb_id, source = ov.source.name(), "override: poster render failed, using the automatic art");
            None
        }
    }
}

/// Render an episode still from the override's source (called at the top of
/// `generate_episode`). Mirrors upstream's still render; `None` = no
/// override or it failed (logged).
pub async fn render_episode(
    state: &AppState,
    resolved: &ResolvedId,
    badges: &[RatingBadge],
    settings: &RenderSettings,
    image_size: Option<ImageSize>,
) -> Option<Vec<u8>> {
    let (key, ov) = find_keyed(state, resolved, ImageType::Episode, "").await?;
    let bytes = source_for(state, &key, &ov, ImageType::Episode, image_size).await?;
    let badges: Vec<RatingBadge> = if ov.no_badges {
        Vec::new()
    } else {
        badges.to_vec()
    };
    let resolved_size = resolve_image_size(image_size);
    let target_width = resolved_size.episode_target_width();
    let badge_scale =
        resolved_size.badge_scale(ImageType::Episode) * settings.episode_badge_size.scale_factor();
    let font = state.font.clone();
    let quality = state.config.image_quality;
    let position = settings.episode_position;
    let badge_style = settings.episode_badge_style;
    let label_style = settings.episode_label_style;
    let appearance = settings.episode_appearance();
    let direction = settings.episode_badge_direction;
    let badge_size = settings.episode_badge_size;
    let blur = settings.episode_blur;
    let _permit = state.render_semaphore.acquire().await.ok()?;
    let r = tokio::task::spawn_blocking(move || {
        generate::render_episode_sync(
            &bytes,
            &badges,
            &font,
            quality,
            position,
            badge_style,
            label_style,
            appearance,
            direction,
            target_width,
            badge_scale,
            badge_size,
            blur,
        )
    })
    .await;
    match r {
        Ok(Ok(b)) => Some(b),
        Ok(Err(e)) => {
            purge::note_source_failure(state, &key, ov.updated_at);
            tracing::warn!(error = %e, source = ov.source.name(), "override: still render failed, using the automatic art");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "override: still render task failed");
            None
        }
    }
}

/// The bytes an override points at, checked to be a decodable raster
/// image of sane size (header only), or `None` (logged).
pub async fn fetch_source(
    state: &AppState,
    source: &OverrideSource,
    kind: ImageType,
    image_size: Option<ImageSize>,
) -> Option<Vec<u8>> {
    let bytes = match source {
        OverrideSource::Auto => return None,
        OverrideSource::Tmdb { file_path } => {
            let size = resolve_image_size(image_size).tmdb_size();
            fetch_tmdb(state, file_path, size).await
        }
        OverrideSource::Fanart { id, url } => fetch_fanart(state, id, url, kind).await,
        OverrideSource::Upload { sha256 } => {
            uploads::read_upload(&state.config.db_dir, sha256).await
        }
    }?;
    if header_ok(&bytes) {
        Some(bytes)
    } else {
        tracing::warn!(
            source = source.name(),
            value = source.value(),
            "override: source is not a usable PNG/JPEG/WebP image, using the automatic art"
        );
        None
    }
}

/// Raster format we can decode and dimensions within upstream's limit.
pub fn header_ok(bytes: &[u8]) -> bool {
    let Ok(reader) = ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return false;
    };
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return false;
    }
    match reader.into_dimensions() {
        Ok((w, h)) => w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_SOURCE_PIXELS,
        Err(_) => false,
    }
}

/// Decoder limits of the render path (`generate.rs`: 8192² RGBA).
pub fn decode_limits() -> Limits {
    let mut l = Limits::default();
    l.max_image_width = Some(8192);
    l.max_image_height = Some(8192);
    l.max_alloc = Some(MAX_SOURCE_PIXELS * 4);
    l
}

/// Full decode under [`decode_limits`] (once per fresh download, under a
/// render permit to bound memory). `None` (logged) when it fails, so a
/// corrupt image is never cached and the automatic art is used.
async fn fully_decodes(state: &AppState, bytes: Vec<u8>) -> Option<Vec<u8>> {
    let _permit = state.render_semaphore.acquire().await.ok()?;
    let checked = tokio::task::spawn_blocking(move || {
        let ok = decodes(&bytes);
        (bytes, ok)
    })
    .await;
    match checked {
        Ok((bytes, true)) => Some(bytes),
        Ok((_, false)) => {
            tracing::warn!("override: downloaded source does not decode, using the automatic art");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "override: source decode task failed");
            None
        }
    }
}

/// True when `bytes` is a PNG/JPEG/WebP that fully decodes under the render
/// path's limits.
pub fn decodes(bytes: &[u8]) -> bool {
    if !header_ok(bytes) {
        return false;
    }
    let Ok(mut reader) = ImageReader::new(Cursor::new(bytes)).with_guessed_format() else {
        return false;
    };
    reader.limits(decode_limits());
    reader.decode().is_ok()
}

/// A cached source, or `None` (and the file removed) when its header is no
/// longer a usable image.
async fn read_cached_source(path: &std::path::Path) -> Option<Vec<u8>> {
    let entry = cache::read(path, 0).await?;
    if header_ok(&entry.bytes) {
        return Some(entry.bytes);
    }
    tracing::warn!(path = %path.display(), "override: removing an unusable cached source");
    let _ = tokio::fs::remove_file(path).await;
    None
}

/// TMDB image through the same base-image disk cache upstream uses for
/// posters (skipped with `EXTERNAL_CACHE_ONLY`).
async fn fetch_tmdb(state: &AppState, file_path: &str, size: &str) -> Option<Vec<u8>> {
    let disk = if state.config.external_cache_only {
        None
    } else {
        cache::base_poster_path(&state.config.cache_dir, file_path, size).ok()
    };
    if let Some(p) = &disk {
        if let Some(bytes) = read_cached_source(p).await {
            return Some(bytes);
        }
    }
    match state.tmdb.fetch_image_bytes(file_path, size).await {
        Ok(bytes) => {
            let bytes = fully_decodes(state, bytes).await?;
            if let Some(p) = &disk {
                if let Err(e) = cache::write(p, &bytes).await {
                    tracing::warn!(error = %e, "override: caching a TMDB source failed");
                }
            }
            Some(bytes)
        }
        Err(e) => {
            tracing::warn!(error = %e, file_path, "override: TMDB source unavailable, using the automatic art");
            None
        }
    }
}

/// fanart.tv image through upstream's base fanart disk cache.
async fn fetch_fanart(state: &AppState, id: &str, url: &str, kind: ImageType) -> Option<Vec<u8>> {
    let disk = if state.config.external_cache_only {
        None
    } else {
        cache::base_fanart_path(&state.config.cache_dir, id, kind.ext()).ok()
    };
    if let Some(p) = &disk {
        if let Some(bytes) = read_cached_source(p).await {
            return Some(bytes);
        }
    }
    let Some(fc) = state.fanart.as_ref() else {
        tracing::warn!(
            fanart_id = id,
            "override: fanart source but no FANART_API_KEY client, using the automatic art"
        );
        return None;
    };
    match fc.fetch_poster_bytes(url).await {
        Ok(bytes) => {
            let bytes = fully_decodes(state, bytes).await?;
            if let Some(p) = &disk {
                if let Err(e) = cache::write(p, &bytes).await {
                    tracing::warn!(error = %e, "override: caching a fanart source failed");
                }
            }
            Some(bytes)
        }
        Err(e) => {
            tracing::warn!(error = %e, fanart_id = id, "override: fanart source unavailable, using the automatic art");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::EpisodeInfo;

    fn resolved(media_type: MediaType, tmdb_id: u64, ep: Option<(u64, u32, u32)>) -> ResolvedId {
        ResolvedId {
            imdb_id: None,
            tmdb_id,
            tvdb_id: None,
            media_type,
            poster_path: None,
            release_date: None,
            episode: ep.map(|(show, s, e)| EpisodeInfo {
                show_tmdb_id: show,
                season_number: s,
                episode_number: e,
                still_path: None,
            }),
        }
    }

    #[test]
    fn keys_use_the_resolved_title() {
        let movie = resolved(MediaType::Movie, 603, None);
        assert_eq!(
            title_of(&movie, ImageType::Logo),
            Some(("movie", 603, -1, -1))
        );
        assert_eq!(title_of(&movie, ImageType::Episode), None);
        let show = resolved(MediaType::Tv, 1399, None);
        assert_eq!(
            title_of(&show, ImageType::Poster),
            Some(("tv", 1399, -1, -1))
        );
        // An episode id: stills are keyed on show + season/episode, the other
        // kinds on the show (upstream uplifts them).
        let ep = resolved(MediaType::Episode, 63056, Some((1399, 1, 2)));
        assert_eq!(title_of(&ep, ImageType::Episode), Some(("tv", 1399, 1, 2)));
        assert_eq!(
            title_of(&ep, ImageType::Backdrop),
            Some(("tv", 1399, -1, -1))
        );
    }

    #[test]
    fn corrupt_body_with_valid_header_does_not_decode() {
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            image::Rgba([(x * 7 + y * 13) as u8, (x * y) as u8, (x ^ y) as u8, 255])
        });
        let mut good = Vec::new();
        img.write_to(&mut Cursor::new(&mut good), ImageFormat::Png)
            .unwrap();
        assert!(decodes(&good));
        let mut broken = good.clone();
        broken.truncate(good.len() / 2);
        assert!(header_ok(&broken), "the header alone looks fine");
        assert!(!decodes(&broken));
    }

    #[tokio::test]
    async fn unusable_cached_source_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.png");
        std::fs::write(&bad, b"not an image").unwrap();
        assert!(read_cached_source(&bad).await.is_none());
        assert!(!bad.exists());
        let good = dir.path().join("good.png");
        std::fs::write(&good, uploads::tests::png(4, 4)).unwrap();
        assert!(read_cached_source(&good).await.is_some());
    }

    #[test]
    fn header_check() {
        assert!(header_ok(&uploads::tests::png(4, 2)));
        assert!(!header_ok(b"<svg/>"));
        assert!(!header_ok(b"GIF89a\x01\x00\x01\x00"));
        assert!(!header_ok(&uploads::tests::png(4, 2)[..12]));
    }
}
