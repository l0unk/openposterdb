//! Fork: admin previews on real artwork.
//!
//! `?art=movie-603` (or `series-1399` / `tv-1399`; episodes add
//! `&season=1&episode=1`) makes the admin preview endpoints draw their sample
//! badges on the title's real TMDB image instead of the built-in gradient.
//! No rating lookup happens: the badges are the preview samples as always.
//! Without `art` nothing changes (same bytes, same preview cache keys); with
//! it, the preview cache key carries an `.art…` token before the size token.
//! Admin only: the self-service `/api/key/me/preview/*` routes refuse `art`.

use std::borrow::Cow;

use axum::extract::{FromRequestParts, Query};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio::sync::OwnedSemaphorePermit;

use crate::AppState;
use crate::cache::ImageType;
use crate::error::AppError;
use crate::image::serve::resolve_image_size;
use crate::services::db::ImageSize;
use crate::services::tmdb::TmdbClient;

/// Only previews under this prefix may fetch real art.
const ADMIN_PREFIX: &str = "/api/admin/";
/// TMDB ids are positive integers; 10 digits is far beyond any real id.
const MAX_ID_DIGITS: usize = 10;

#[derive(Debug, Default, Deserialize)]
pub struct ArtQuery {
    pub art: Option<String>,
    // Strings, parsed only when `art` is given: without `art`, a malformed
    // season/episode is ignored, as upstream ignores unknown parameters.
    pub season: Option<String>,
    pub episode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtMedia {
    Movie,
    Series,
}

/// A parsed `art` request (season/episode only matter for episode previews).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Art {
    pub media: ArtMedia,
    pub tmdb_id: u64,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

/// Extractor for the preview handlers: the parsed `art` request, if any.
#[derive(Debug, Default)]
pub struct PreviewArt(pub Option<Art>);

impl<S> FromRequestParts<S> for PreviewArt
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(q) = Query::<ArtQuery>::try_from_uri(&parts.uri)
            .map_err(|e| AppError::BadRequest(format!("invalid art/season/episode: {e}")))?;
        let art = match q.art.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(raw) => parse(
                Some(raw),
                number(q.season.as_deref(), "season")?,
                number(q.episode.as_deref(), "episode")?,
            )?,
        };
        if art.is_some() && !parts.uri.path().starts_with(ADMIN_PREFIX) {
            return Err(AppError::Forbidden(
                "art= is only available on the admin previews".into(),
            ));
        }
        Ok(Self(art))
    }
}

/// Parse `art` (`movie-<id>`, `series-<id>` or `tv-<id>`). `None`/empty
/// means "no real art".
/// A season or episode number (only read when `art` is given).
fn number(raw: Option<&str>, name: &str) -> Result<Option<u32>, AppError> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(v) => v
            .parse()
            .map(Some)
            .map_err(|_| AppError::BadRequest(format!("{name} must be a whole number, got {v:?}"))),
    }
}

pub fn parse(
    art: Option<&str>,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Option<Art>, AppError> {
    let Some(raw) = art.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let bad = || {
        AppError::BadRequest(format!(
            "art must be movie-<tmdb id> or series-<tmdb id>, got {raw:?}"
        ))
    };
    let (media, id) = raw.split_once('-').ok_or_else(bad)?;
    let media = match media {
        "movie" => ArtMedia::Movie,
        "series" | "tv" => ArtMedia::Series,
        _ => return Err(bad()),
    };
    if id.is_empty() || id.len() > MAX_ID_DIGITS || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let tmdb_id: u64 = id.parse().map_err(|_| bad())?;
    if tmdb_id == 0 {
        return Err(bad());
    }
    Ok(Some(Art {
        media,
        tmdb_id,
        season,
        episode,
    }))
}

impl Art {
    fn media_path(&self) -> &'static str {
        match self.media {
            ArtMedia::Movie => "movie",
            ArtMedia::Series => "tv",
        }
    }

    /// Cache-key token for this art and preview kind (path-safe).
    pub fn token(&self, kind: ImageType) -> String {
        let m = match self.media {
            ArtMedia::Movie => 'm',
            ArtMedia::Series => 's',
        };
        match (kind, self.season, self.episode) {
            (ImageType::Episode, Some(s), Some(e)) => format!(".art{m}{}s{s}e{e}", self.tmdb_id),
            _ => format!(".art{m}{}", self.tmdb_id),
        }
    }
}

/// Validate `art` for this preview kind and splice its token into the
/// preview cache suffix (before the size token). No art: suffix unchanged.
pub fn keyed(
    art: PreviewArt,
    kind: ImageType,
    suffix: String,
) -> Result<(Option<Art>, String), AppError> {
    let Some(art) = art.0 else {
        return Ok((None, suffix));
    };
    if kind == ImageType::Episode {
        if art.media != ArtMedia::Series {
            return Err(AppError::BadRequest(
                "episode previews need art=series-<tmdb id>".into(),
            ));
        }
        match art.episode {
            Some(e) if e >= 1 && art.season.is_some() => {}
            _ => {
                return Err(AppError::BadRequest(
                    "episode previews with art need season and episode (episode >= 1)".into(),
                ));
            }
        }
    }
    let token = art.token(kind);
    Ok((
        Some(art),
        crate::louk::backdrop_cap::splice_token(suffix, Some(&token)),
    ))
}

/// The source image for a preview: the built-in sample without art, else
/// the title's real TMDB image. Errors are ready-made JSON responses
/// (404 when TMDB has no such title or image, 502 when TMDB fails).
pub async fn source(
    state: &AppState,
    art: Option<&Art>,
    kind: ImageType,
    image_size: Option<ImageSize>,
    sample: &'static [u8],
) -> Result<(Cow<'static, [u8]>, Option<OwnedSemaphorePermit>), Response> {
    let Some(art) = art else {
        return Ok((Cow::Borrowed(sample), None));
    };
    let bytes = fetch(&state.tmdb, &state.config.louk, art, kind, image_size)
        .await
        .map_err(|e| e.into_response())?;
    // A real TMDB original (up to 3840 px) is decoded next to production
    // renders: take a render permit, like the image routes do, and let the
    // caller hold it until its render is done.
    let permit = state
        .render_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ArtError::Upstream("render queue closed".into()).into_response())?;
    Ok((Cow::Owned(bytes), Some(permit)))
}

/// Why fetching real art failed.
#[derive(Debug)]
pub enum ArtError {
    NotFound(String),
    Upstream(String),
}

impl IntoResponse for ArtError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ArtError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ArtError::Upstream(m) => (StatusCode::BAD_GATEWAY, m),
        };
        tracing::info!(%status, error = %msg, "preview art");
        (status, axum::Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

fn tmdb_error(what: &str, e: AppError) -> ArtError {
    match &e {
        AppError::Api(re) if re.status() == Some(reqwest::StatusCode::NOT_FOUND) => {
            ArtError::NotFound(format!("TMDB has no {what}"))
        }
        AppError::Api(re) => ArtError::Upstream(format!(
            "TMDB request for {what} failed{}",
            re.status()
                .map(|s| format!(" (HTTP {})", s.as_u16()))
                .unwrap_or_default()
        )),
        _ => ArtError::Upstream(format!("TMDB request for {what} failed")),
    }
}

#[derive(Deserialize)]
struct EpisodeDetails {
    still_path: Option<String>,
}

async fn fetch(
    tmdb: &TmdbClient,
    cfg: &crate::louk::config::LoukConfig,
    art: &Art,
    kind: ImageType,
    image_size: Option<ImageSize>,
) -> Result<Vec<u8>, ArtError> {
    let label = format!("{}-{}", art.media_path(), art.tmdb_id);
    let (path, size) = match kind {
        ImageType::Episode => {
            let (s, e) = (art.season.unwrap_or(0), art.episode.unwrap_or(0));
            let what = format!("episode {label} S{s}E{e}");
            let details: EpisodeDetails = tmdb
                .get(&format!("/tv/{}/season/{s}/episode/{e}", art.tmdb_id), &[])
                .await
                .map_err(|err| tmdb_error(&what, err))?;
            let still = details
                .still_path
                .ok_or_else(|| ArtError::NotFound(format!("TMDB has no still for {what}")))?;
            (still, resolve_image_size(image_size).tmdb_size())
        }
        _ => {
            let images = tmdb
                .get_images(art.media_path(), art.tmdb_id, "en")
                .await
                .map_err(|err| tmdb_error(&format!("images for {label}"), err))?;
            // The same selection the real routes use (TMDB side; English,
            // not textless; backdrops are language-agnostic), falling back to
            // the first candidate.
            // SVG files (some TMDB logos) cannot be decoded: skip them.
            let raster = |list: &[crate::services::tmdb::TmdbImage]| -> Vec<crate::services::tmdb::TmdbImage> {
                list.iter().filter(|i| !i.file_path.to_ascii_lowercase().ends_with(".svg")).cloned().collect()
            };
            let candidates = match kind {
                ImageType::Poster => raster(images.posters.as_slice()),
                ImageType::Logo => raster(images.logos.as_slice()),
                _ => raster(images.backdrops.as_slice()),
            };
            let selected = match kind {
                ImageType::Poster => TmdbClient::select_poster(&candidates, "en", false),
                ImageType::Logo => TmdbClient::select_image(&candidates, "en", false),
                _ => TmdbClient::select_image(&candidates, "", false),
            };
            let img = selected.or_else(|| candidates.first()).ok_or_else(|| {
                ArtError::NotFound(format!("TMDB has no {} for {label}", kind_name(kind)))
            })?;
            (
                img.file_path.clone(),
                crate::louk::backdrop_cap::tmdb_size(cfg, kind, image_size),
            )
        }
    };
    tmdb.fetch_image_bytes(&path, size)
        .await
        .map_err(|err| tmdb_error(&format!("image {path}"), err))
}

fn kind_name(kind: ImageType) -> &'static str {
    match kind {
        ImageType::Poster => "poster",
        ImageType::Logo => "logo",
        ImageType::Backdrop => "backdrop",
        ImageType::Episode => "episode still",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_movie_series_and_tv() {
        assert_eq!(
            parse(Some("movie-603"), None, None).unwrap(),
            Some(Art {
                media: ArtMedia::Movie,
                tmdb_id: 603,
                season: None,
                episode: None
            })
        );
        assert_eq!(
            parse(Some("series-1399"), Some(1), Some(2))
                .unwrap()
                .unwrap()
                .media,
            ArtMedia::Series
        );
        assert_eq!(
            parse(Some("tv-1399"), None, None).unwrap().unwrap().tmdb_id,
            1399
        );
        assert_eq!(parse(None, Some(1), Some(1)).unwrap(), None);
        assert_eq!(parse(Some("  "), None, None).unwrap(), None);
    }

    #[test]
    fn season_and_episode_numbers() {
        assert_eq!(number(None, "season").unwrap(), None);
        assert_eq!(number(Some(" "), "season").unwrap(), None);
        assert_eq!(number(Some("3"), "season").unwrap(), Some(3));
        assert!(number(Some("x"), "season").is_err());
        assert!(number(Some("-1"), "episode").is_err());
    }

    #[test]
    fn rejects_bad_art() {
        for bad in [
            "603",
            "movie-",
            "movie-0",
            "movie-abc",
            "film-603",
            "movie-603x",
            "movie-12345678901",
            "movie--1",
            "movie-../x",
        ] {
            assert!(parse(Some(bad), None, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn no_art_keeps_the_preview_key() {
        for kind in [
            ImageType::Poster,
            ImageType::Logo,
            ImageType::Backdrop,
            ImageType::Episode,
        ] {
            let (art, s) = keyed(
                PreviewArt(None),
                kind,
                "@mil.pbc.sd.lo.dd.bm.shr.bgs.zm".into(),
            )
            .unwrap();
            assert!(art.is_none());
            assert_eq!(s, "@mil.pbc.sd.lo.dd.bm.shr.bgs.zm");
        }
    }

    #[test]
    fn art_gets_its_own_key_before_the_size_token() {
        let movie = parse(Some("movie-603"), None, None).unwrap();
        let (_, s) = keyed(
            PreviewArt(movie.clone()),
            ImageType::Poster,
            "@mil.pbc.sd.lo.dd.bm.shr.bgs.zm".into(),
        )
        .unwrap();
        assert_eq!(s, "@mil.pbc.sd.lo.dd.bm.shr.bgs.artm603.zm");
        let (_, b) = keyed(
            PreviewArt(movie),
            ImageType::Backdrop,
            "@milrc.ptr.sv.lo.dd.bm.shr.bgs.mw1920.zl".into(),
        )
        .unwrap();
        assert_eq!(b, "@milrc.ptr.sv.lo.dd.bm.shr.bgs.mw1920.artm603.zl");
        let ep = parse(Some("series-1399"), Some(1), Some(9)).unwrap();
        let (_, e) = keyed(
            PreviewArt(ep),
            ImageType::Episode,
            "@m.ptr.sv.lo.dv.bl.shr.bgs.zm".into(),
        )
        .unwrap();
        assert_eq!(e, "@m.ptr.sv.lo.dv.bl.shr.bgs.arts1399s1e9.zm");
        // Different titles, different keys.
        let other = parse(Some("movie-604"), None, None).unwrap();
        let (_, s2) = keyed(
            PreviewArt(other),
            ImageType::Poster,
            "@mil.pbc.sd.lo.dd.bm.shr.bgs.zm".into(),
        )
        .unwrap();
        assert_ne!(s, s2);
    }

    #[test]
    fn episode_art_needs_a_series_season_and_episode() {
        let suffix = || "@m.ptr.sv.lo.dv.bl.shr.bgd.zm".to_string();
        assert!(
            keyed(
                PreviewArt(parse(Some("series-1399"), None, None).unwrap()),
                ImageType::Episode,
                suffix()
            )
            .is_err()
        );
        assert!(
            keyed(
                PreviewArt(parse(Some("series-1399"), Some(1), Some(0)).unwrap()),
                ImageType::Episode,
                suffix()
            )
            .is_err()
        );
        assert!(
            keyed(
                PreviewArt(parse(Some("movie-603"), Some(1), Some(1)).unwrap()),
                ImageType::Episode,
                suffix()
            )
            .is_err()
        );
        assert!(
            keyed(
                PreviewArt(parse(Some("series-1399"), Some(0), Some(1)).unwrap()),
                ImageType::Episode,
                suffix()
            )
            .is_ok()
        );
        // Season/episode are ignored (and not keyed) for the other kinds.
        let (_, p) = keyed(
            PreviewArt(parse(Some("series-1399"), Some(1), Some(1)).unwrap()),
            ImageType::Poster,
            "@mil.zm".into(),
        )
        .unwrap();
        assert_eq!(p, "@mil.arts1399.zm");
    }
}
