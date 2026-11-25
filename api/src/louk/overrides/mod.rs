//! Per-title art overrides and admin uploads (fork-only; the core of the
//! planned upstream PR, see docs/louk/FORK.md "Per-title overrides").
//!
//! An override is keyed on the RESOLVED title (`movie`/`tv` + TMDB id), the
//! image kind, season/episode for stills and an optional language, never on
//! the requested id, so imdb/tmdb/tvdb requests of one title all hit it.
//!
//! - [`store`]: the `louk_overrides` table, mirrored by an in-memory map that
//!   is loaded once; the render path never queries the DB per request.
//! - [`uploads`]: admin uploads (PNG/JPEG/WebP, sniffed, decoded with limits,
//!   stored by sha256 under `DB_DIR/uploads/`).
//! - [`candidates`]: every TMDB (all languages) and fanart image of a title.
//! - [`handlers`]: the admin REST endpoints under `/api/admin/louk/`.
//!
//! Nothing here depends on the title dashboard (`louk::titles`) except
//! `purge::alias_targets`, the one seam the upstream port replaces.

pub mod apply;
pub mod candidates;
pub mod handlers;
pub mod purge;
pub mod store;
pub mod uploads;

use crate::AppState;
use crate::cache::ImageType;
use crate::error::AppError;

/// Where the art of an override comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverrideSource {
    /// Keep the automatic pick; only the flags (`no_badges`) apply.
    Auto,
    /// A TMDB image `file_path` (e.g. `/abc.png`).
    Tmdb { file_path: String },
    /// A fanart.tv image: its id (base-cache file name) and URL.
    Fanart { id: String, url: String },
    /// An admin upload, by the sha256 of its bytes.
    Upload { sha256: String },
}

impl OverrideSource {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Tmdb { .. } => "tmdb",
            Self::Fanart { .. } => "fanart",
            Self::Upload { .. } => "upload",
        }
    }

    /// The stored `value` column: file path, URL, sha256, or "" for `auto`.
    pub fn value(&self) -> &str {
        match self {
            Self::Auto => "",
            Self::Tmdb { file_path } => file_path,
            Self::Fanart { url, .. } => url,
            Self::Upload { sha256 } => sha256,
        }
    }

    pub fn fanart_id(&self) -> Option<&str> {
        match self {
            Self::Fanart { id, .. } => Some(id),
            _ => None,
        }
    }

    /// Build and validate a source from its stored/requested parts. Values
    /// are checked strictly: they end up in URLs and file names.
    pub fn from_parts(source: &str, value: &str, fanart_id: Option<&str>) -> Result<Self, String> {
        let value = value.trim();
        match source {
            "auto" => {
                if value.is_empty() {
                    Ok(Self::Auto)
                } else {
                    Err("source auto takes no value".into())
                }
            }
            "tmdb" => {
                if is_tmdb_file_path(value) {
                    Ok(Self::Tmdb {
                        file_path: value.to_owned(),
                    })
                } else {
                    Err(
                        "value must be a TMDB file_path like /abc123.png (jpg, jpeg, png or webp)"
                            .into(),
                    )
                }
            }
            "fanart" => {
                if !is_fanart_url(value) {
                    return Err("value must be an assets.fanart.tv image URL".into());
                }
                let id = fanart_id.map(str::trim).unwrap_or("");
                if !is_fanart_id(id) {
                    return Err("fanart_id is required (letters, digits, - and _)".into());
                }
                Ok(Self::Fanart {
                    id: id.to_owned(),
                    url: value.to_owned(),
                })
            }
            "upload" => {
                if uploads::is_sha256_hex(value) {
                    Ok(Self::Upload {
                        sha256: value.to_owned(),
                    })
                } else {
                    Err(
                        "value must be the sha256 (64 lowercase hex characters) of an upload"
                            .into(),
                    )
                }
            }
            _ => Err("source must be auto, tmdb, fanart or upload".into()),
        }
    }
}

/// One override: the art source plus its flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleOverride {
    pub source: OverrideSource,
    /// Draw no rating badges on this title + kind.
    pub no_badges: bool,
    /// Unix seconds of the last change.
    pub updated_at: i64,
}

/// The resolved title an override belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideKey {
    /// `"movie"` or `"tv"` (episodes belong to their show).
    pub media_type: &'static str,
    pub tmdb_id: u64,
    pub kind: ImageType,
    /// Season/episode for `episode` stills; `-1` otherwise.
    pub season: i32,
    pub episode: i32,
    /// Request language the override applies to; `""` = any.
    pub lang: String,
}

/// `-1` = not an episode (the DB default).
pub const NO_EPISODE: i32 = -1;

impl OverrideKey {
    /// Parse and validate the REST path/query parts of a key.
    pub fn parse(
        media_type: &str,
        tmdb_id: &str,
        kind: &str,
        season: Option<&str>,
        episode: Option<&str>,
        lang: Option<&str>,
    ) -> Result<Self, AppError> {
        let media_type = parse_media_type(media_type)?;
        let tmdb_id = parse_tmdb_id(tmdb_id)?;
        let kind = parse_kind(kind)?;
        let season = opt(season);
        let episode = opt(episode);
        let lang = opt(lang).unwrap_or("");
        let (season, episode) = if kind == ImageType::Episode {
            if media_type != "tv" {
                return Err(AppError::BadRequest(
                    "episode overrides belong to a tv title".into(),
                ));
            }
            let s = parse_number("season", season)?;
            let e = parse_number("episode", episode)?;
            (s, e)
        } else {
            if season.is_some() || episode.is_some() {
                return Err(AppError::BadRequest(
                    "season and episode are only for kind episode".into(),
                ));
            }
            (NO_EPISODE, NO_EPISODE)
        };
        if !lang.is_empty() {
            if !matches!(kind, ImageType::Poster | ImageType::Logo) {
                return Err(AppError::BadRequest(
                    "lang is only for poster and logo overrides (backdrops and stills are language-agnostic)".into(),
                ));
            }
            if !is_lang(lang) {
                return Err(AppError::BadRequest(
                    "lang must look like en or pt-BR".into(),
                ));
            }
        }
        Ok(Self {
            media_type,
            tmdb_id,
            kind,
            season,
            episode,
            lang: lang.to_owned(),
        })
    }
}

fn opt(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

pub fn parse_media_type(s: &str) -> Result<&'static str, AppError> {
    match s {
        "movie" => Ok("movie"),
        "tv" => Ok("tv"),
        _ => Err(AppError::BadRequest(
            "media_type must be movie or tv".into(),
        )),
    }
}

pub fn parse_tmdb_id(s: &str) -> Result<u64, AppError> {
    s.parse::<u64>()
        .ok()
        .filter(|v| *v > 0 && *v <= i64::MAX as u64)
        .ok_or_else(|| AppError::BadRequest("tmdb_id must be a positive integer".into()))
}

pub fn parse_kind(s: &str) -> Result<ImageType, AppError> {
    kind_from_name(s).ok_or_else(|| {
        AppError::BadRequest("kind must be poster, logo, backdrop or episode".into())
    })
}

pub fn kind_from_name(s: &str) -> Option<ImageType> {
    match s {
        "poster" => Some(ImageType::Poster),
        "logo" => Some(ImageType::Logo),
        "backdrop" => Some(ImageType::Backdrop),
        "episode" => Some(ImageType::Episode),
        _ => None,
    }
}

fn parse_number(name: &str, v: Option<&str>) -> Result<i32, AppError> {
    v.and_then(|s| s.parse::<i32>().ok())
        .filter(|n| (0..=100_000).contains(n))
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "{name} is required for kind episode (a number >= 0)"
            ))
        })
}

/// `en`, `pt-BR`.
pub fn is_lang(s: &str) -> bool {
    let b = s.as_bytes();
    match b.len() {
        2 => b.iter().all(u8::is_ascii_lowercase),
        5 => {
            b[..2].iter().all(u8::is_ascii_lowercase)
                && b[2] == b'-'
                && b[3..].iter().all(u8::is_ascii_uppercase)
        }
        _ => false,
    }
}

/// `/` + one path segment of `[A-Za-z0-9_-]` + a raster extension.
pub fn is_tmdb_file_path(s: &str) -> bool {
    let Some(name) = s.strip_prefix('/') else {
        return false;
    };
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    s.len() <= 128
        && !stem.is_empty()
        && stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && matches!(
            ext.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp"
        )
}

/// An image URL on fanart.tv's asset host (no other host is ever fetched).
pub fn is_fanart_url(s: &str) -> bool {
    let rest = s
        .strip_prefix("https://assets.fanart.tv/")
        .or_else(|| s.strip_prefix("http://assets.fanart.tv/"));
    match rest {
        Some(path) => {
            s.len() <= 400
                && !path.is_empty()
                && !path.contains("..")
                && path
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        }
        None => false,
    }
}

pub fn is_fanart_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The store every override function reads (one accessor so a port can move
/// it out of `LoukState`).
pub fn store(state: &AppState) -> &store::OverrideStore {
    &state.louk.overrides
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_validation() {
        assert_eq!(
            OverrideSource::from_parts("auto", "", None),
            Ok(OverrideSource::Auto)
        );
        assert!(OverrideSource::from_parts("auto", "/x.png", None).is_err());
        assert_eq!(
            OverrideSource::from_parts("tmdb", "/abc_D-1.png", None),
            Ok(OverrideSource::Tmdb {
                file_path: "/abc_D-1.png".into()
            })
        );
        for bad in [
            "abc.png",
            "/a/b.png",
            "/../x.png",
            "/x.svg",
            "/x",
            "/.png",
            "/x.png?y",
            "",
        ] {
            assert!(
                OverrideSource::from_parts("tmdb", bad, None).is_err(),
                "{bad}"
            );
        }
        assert!(
            OverrideSource::from_parts(
                "fanart",
                "https://assets.fanart.tv/fanart/movies/1/hdmovielogo/x-1.png",
                Some("123")
            )
            .is_ok()
        );
        assert!(
            OverrideSource::from_parts("fanart", "https://assets.fanart.tv/fanart/x.png", None)
                .is_err()
        );
        assert!(
            OverrideSource::from_parts("fanart", "https://evil.example/fanart/x.png", Some("1"))
                .is_err()
        );
        assert!(
            OverrideSource::from_parts("fanart", "https://assets.fanart.tv/../x.png", Some("1"))
                .is_err()
        );
        assert!(
            OverrideSource::from_parts("fanart", "https://assets.fanart.tv/x.png", Some("../1"))
                .is_err()
        );
        let sha = "a".repeat(64);
        assert_eq!(
            OverrideSource::from_parts("upload", &sha, None),
            Ok(OverrideSource::Upload {
                sha256: sha.clone()
            })
        );
        assert!(OverrideSource::from_parts("upload", &"A".repeat(64), None).is_err());
        assert!(OverrideSource::from_parts("upload", "../etc/passwd", None).is_err());
        assert!(OverrideSource::from_parts("other", "", None).is_err());
    }

    #[test]
    fn key_validation() {
        let k = OverrideKey::parse("movie", "603", "logo", None, None, Some("en")).unwrap();
        assert_eq!(
            (
                k.media_type,
                k.tmdb_id,
                k.kind,
                k.season,
                k.episode,
                k.lang.as_str()
            ),
            ("movie", 603, ImageType::Logo, -1, -1, "en")
        );
        let e = OverrideKey::parse("tv", "1399", "episode", Some("1"), Some("2"), None).unwrap();
        assert_eq!((e.season, e.episode), (1, 2));
        assert!(OverrideKey::parse("movie", "1", "episode", Some("1"), Some("2"), None).is_err());
        assert!(OverrideKey::parse("tv", "1", "episode", Some("1"), None, None).is_err());
        assert!(OverrideKey::parse("tv", "1", "poster", Some("1"), None, None).is_err());
        assert!(OverrideKey::parse("movie", "1", "backdrop", None, None, Some("en")).is_err());
        assert!(OverrideKey::parse("movie", "1", "logo", None, None, Some("english")).is_err());
        assert!(OverrideKey::parse("movie", "0", "logo", None, None, None).is_err());
        assert!(OverrideKey::parse("show", "1", "logo", None, None, None).is_err());
        assert!(OverrideKey::parse("movie", "1", "thumb", None, None, None).is_err());
        assert_eq!(
            OverrideKey::parse("movie", "1", "poster", Some(""), Some(" "), Some(""))
                .unwrap()
                .lang,
            ""
        );
    }

    #[test]
    fn lang_format() {
        for ok in ["en", "pt-BR"] {
            assert!(is_lang(ok));
        }
        for bad in ["", "e", "EN", "pt-br", "pt_BR", "eng"] {
            assert!(!is_lang(bad), "{bad}");
        }
    }
}
