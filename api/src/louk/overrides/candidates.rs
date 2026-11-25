//! Every image a title could use for one kind: TMDB (all languages, with
//! size and votes) and fanart.tv, the automatic pick marked. TMDB and fanart
//! only: no rating API is ever called here.
//!
//! Ordering (fork Finding 13): TMDB first, then fanart; within each, the
//! requested language, then language-less art, then English, then the other
//! languages A-Z; within a language, votes (likes) descending, and equal
//! votes keep TMDB's/fanart's own order (a stable sort, first of equals
//! first; upstream's `max_by` picks the LAST of equals).

use serde::{Deserialize, Serialize};

use super::{OverrideSource, is_tmdb_file_path};
use crate::AppState;
use crate::cache::ImageType;
use crate::id::{self, IdType, MediaType, ResolvedId};
use crate::services::fanart::{FanartClient, FanartPoster};
use crate::services::lang::lang_base;
use crate::services::tmdb::{TmdbClient, TmdbImage};

pub const TMDB_THUMB: &str = "https://image.tmdb.org/t/p/w300";
pub const TMDB_FULL: &str = "https://image.tmdb.org/t/p/original";

/// One TMDB image with the fields upstream's `TmdbImage` does not carry.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct TmdbImageFull {
    pub file_path: String,
    #[serde(default)]
    pub iso_639_1: Option<String>,
    #[serde(default)]
    pub iso_3166_1: Option<String>,
    #[serde(default)]
    pub vote_average: f64,
    #[serde(default)]
    pub vote_count: i64,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub aspect_ratio: f64,
}

impl TmdbImageFull {
    fn as_upstream(&self) -> TmdbImage {
        TmdbImage {
            file_path: self.file_path.clone(),
            iso_639_1: self.iso_639_1.clone(),
            iso_3166_1: self.iso_3166_1.clone(),
            vote_average: self.vote_average,
            aspect_ratio: self.aspect_ratio,
        }
    }
}

#[derive(Deserialize, Default)]
struct TmdbImagesAll {
    #[serde(default)]
    posters: Vec<TmdbImageFull>,
    #[serde(default)]
    logos: Vec<TmdbImageFull>,
    #[serde(default)]
    backdrops: Vec<TmdbImageFull>,
}

#[derive(Deserialize, Default)]
struct TmdbStills {
    #[serde(default)]
    stills: Vec<TmdbImageFull>,
}

/// One candidate, as the picker shows it. `source` + `value` (+ `fanart_id`)
/// are exactly what `PUT /api/admin/louk/overrides/…` takes.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Candidate {
    pub source: &'static str,
    pub value: String,
    pub fanart_id: Option<String>,
    /// ISO 639-1 (`en`), or null for language-less art (TMDB null, fanart `00`/empty).
    pub lang: Option<String>,
    /// TMDB `vote_average`; null for fanart.
    pub votes: Option<f64>,
    /// TMDB `vote_count`; null for fanart.
    pub vote_count: Option<i64>,
    /// fanart likes; null for TMDB.
    pub likes: Option<i64>,
    /// Source size in px; null for fanart (its API does not say).
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumb_url: String,
    pub full_url: String,
    /// The image the automatic selection uses today.
    pub auto: bool,
    /// The image the current override points at.
    pub current: bool,
}

pub fn tmdb_candidates(images: &[TmdbImageFull]) -> Vec<Candidate> {
    images
        .iter()
        // .svg logos cannot be rendered (no SVG decoder); hide them.
        .filter(|i| is_tmdb_file_path(&i.file_path))
        .map(|i| Candidate {
            source: "tmdb",
            value: i.file_path.clone(),
            fanart_id: None,
            lang: i.iso_639_1.clone().filter(|l| !l.is_empty()),
            votes: Some(i.vote_average),
            vote_count: Some(i.vote_count),
            likes: None,
            width: (i.width > 0).then_some(i.width),
            height: (i.height > 0).then_some(i.height),
            thumb_url: format!("{TMDB_THUMB}{}", i.file_path),
            full_url: format!("{TMDB_FULL}{}", i.file_path),
            auto: false,
            current: false,
        })
        .collect()
}

/// fanart.tv's small preview of an image URL.
pub fn fanart_preview_url(url: &str) -> String {
    url.replacen("/fanart/", "/preview/", 1)
}

pub fn fanart_candidates(images: &[FanartPoster]) -> Vec<Candidate> {
    images
        .iter()
        .filter(|p| super::is_fanart_url(&p.url) && super::is_fanart_id(&p.id))
        .map(|p| Candidate {
            source: "fanart",
            value: p.url.clone(),
            fanart_id: Some(p.id.clone()),
            lang: Some(p.lang.clone()).filter(|l| !l.is_empty() && l != "00"),
            votes: None,
            vote_count: None,
            likes: Some(p.likes.parse::<i64>().unwrap_or(0)),
            width: None,
            height: None,
            thumb_url: fanart_preview_url(&p.url),
            full_url: p.url.clone(),
            auto: false,
            current: false,
        })
        .collect()
}

fn lang_rank(lang: Option<&str>, req: &str) -> (u8, String) {
    match lang {
        Some(l) if !req.is_empty() && (l == req || l == lang_base(req)) => (0, String::new()),
        None => (1, String::new()),
        Some("en") => (2, String::new()),
        Some(l) => (3, l.to_owned()),
    }
}

/// Stable sort (see the module docs); equal entries keep their order.
pub fn sort_candidates(c: &mut [Candidate], req_lang: &str) {
    c.sort_by(|a, b| {
        let block = |x: &Candidate| u8::from(x.source != "tmdb");
        let score = |x: &Candidate| x.votes.unwrap_or(0.0).max(x.likes.unwrap_or(0) as f64);
        block(a)
            .cmp(&block(b))
            .then_with(|| {
                lang_rank(a.lang.as_deref(), req_lang).cmp(&lang_rank(b.lang.as_deref(), req_lang))
            })
            .then_with(|| {
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
}

/// Upstream's automatic TMDB pick for a TMDB-preferred key (default `lang`
/// handling, not textless), as a `file_path`.
pub fn auto_tmdb(
    kind: ImageType,
    images: &[TmdbImageFull],
    lang: &str,
    default_path: Option<&str>,
) -> Option<String> {
    let up: Vec<TmdbImage> = images.iter().map(TmdbImageFull::as_upstream).collect();
    match kind {
        // `generate_poster_with_source`: lang=en → the details poster_path;
        // other languages → select_poster, falling back to poster_path.
        ImageType::Poster => {
            if lang == "en" {
                default_path.map(str::to_owned)
            } else {
                TmdbClient::select_poster(&up, lang, false)
                    .map(|i| i.file_path.clone())
                    .or_else(|| default_path.map(str::to_owned))
            }
        }
        // `try_tmdb_logo_backdrop` (+ the en fallback for logos).
        ImageType::Logo => TmdbClient::select_image(&up, lang, false)
            .or_else(|| {
                if lang != "en" {
                    TmdbClient::select_image(&up, "en", false)
                } else {
                    None
                }
            })
            .map(|i| i.file_path.clone()),
        ImageType::Backdrop => {
            TmdbClient::select_image(&up, "", false).map(|i| i.file_path.clone())
        }
        // Episodes use the episode's own still_path.
        ImageType::Episode => default_path.map(str::to_owned),
    }
}

/// Upstream's fanart pick (used when TMDB has nothing for logos/backdrops).
pub fn auto_fanart(
    kind: ImageType,
    images: &[FanartPoster],
    lang: &str,
) -> Option<(String, String)> {
    let lang = if kind == ImageType::Backdrop {
        ""
    } else {
        lang
    };
    FanartClient::select_image(images, lang, false).map(|(p, _)| (p.id.clone(), p.url.clone()))
}

/// Set the `auto` and `current` marks.
pub fn mark(c: &mut [Candidate], auto: Option<&OverrideSource>, current: Option<&OverrideSource>) {
    let same = |x: &Candidate, s: &OverrideSource| x.source == s.name() && x.value == s.value();
    for x in c.iter_mut() {
        x.auto = auto.is_some_and(|s| same(x, s));
        x.current = current.is_some_and(|s| same(x, s));
    }
}

/// The candidates endpoint's result.
#[derive(Serialize, Debug)]
pub struct CandidateList {
    /// The automatic pick (`{source, value, fanart_id}`), null when none is known.
    pub auto: Option<AutoPick>,
    pub candidates: Vec<Candidate>,
    /// Non-fatal failures (`"tmdb: …"`, `"fanart: …"`).
    pub errors: Vec<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct AutoPick {
    pub source: &'static str,
    pub value: String,
    pub fanart_id: Option<String>,
}

impl AutoPick {
    fn from_source(s: &OverrideSource) -> Option<Self> {
        match s {
            OverrideSource::Auto | OverrideSource::Upload { .. } => None,
            _ => Some(Self {
                source: s.name(),
                value: s.value().to_owned(),
                fanart_id: s.fanart_id().map(str::to_owned),
            }),
        }
    }
}

/// Gather the candidates of one title + kind. `lang` is the request language
/// the automatic pick is computed for (`en` by default); `current` is the
/// override in effect for it.
pub async fn list(
    state: &AppState,
    media_type: &'static str,
    tmdb_id: u64,
    kind: ImageType,
    season: i32,
    episode: i32,
    lang: &str,
    current: Option<&OverrideSource>,
) -> CandidateList {
    let mut errors = Vec::new();
    let mut tmdb: Vec<TmdbImageFull> = Vec::new();
    let mut default_path: Option<String> = None;
    let mut resolved: Option<ResolvedId> = None;

    if kind == ImageType::Episode {
        let path = format!("/tv/{tmdb_id}/season/{season}/episode/{episode}/images");
        match state.tmdb.get::<TmdbStills>(&path, &[]).await {
            Ok(r) => tmdb = r.stills,
            Err(e) => errors.push(format!("tmdb: {e}")),
        }
        let ep_id = format!("episode-{tmdb_id}-S{season}E{episode}");
        match id::resolve(IdType::Tmdb, &ep_id, &state.tmdb, &state.id_cache).await {
            Ok(r) => default_path = r.episode.as_ref().and_then(|e| e.still_path.clone()),
            Err(e) => errors.push(format!("tmdb: {e}")),
        }
    } else {
        let path = format!("/{media_type}/{tmdb_id}/images");
        match state.tmdb.get::<TmdbImagesAll>(&path, &[]).await {
            Ok(r) => {
                tmdb = match kind {
                    ImageType::Poster => r.posters,
                    ImageType::Logo => r.logos,
                    _ => r.backdrops,
                }
            }
            Err(e) => errors.push(format!("tmdb: {e}")),
        }
        let canonical = if media_type == "movie" {
            format!("movie-{tmdb_id}")
        } else {
            format!("series-{tmdb_id}")
        };
        match id::resolve(IdType::Tmdb, &canonical, &state.tmdb, &state.id_cache).await {
            Ok(r) => {
                default_path = r.poster_path.clone();
                resolved = Some(r);
            }
            Err(e) => errors.push(format!("tmdb: {e}")),
        }
    }

    let mut out = tmdb_candidates(&tmdb);
    let mut auto = auto_tmdb(kind, &tmdb, lang, default_path.as_deref())
        .filter(|p| is_tmdb_file_path(p))
        .map(|file_path| OverrideSource::Tmdb { file_path });

    // fanart.tv has posters, logos and backdrops (no stills).
    if kind != ImageType::Episode {
        if let Some(fc) = state.fanart.as_ref() {
            let r = resolved.unwrap_or_else(|| ResolvedId {
                imdb_id: None,
                tmdb_id,
                tvdb_id: None,
                media_type: if media_type == "movie" {
                    MediaType::Movie
                } else {
                    MediaType::Tv
                },
                poster_path: None,
                release_date: None,
                episode: None,
            });
            match crate::image::serve::fetch_fanart_images(fc, &state.tmdb, &state.fanart_cache, &r)
                .await
            {
                Some(imgs) => {
                    let list = match kind {
                        ImageType::Poster => &imgs.posters,
                        ImageType::Logo => &imgs.logos,
                        _ => &imgs.backdrops,
                    };
                    if auto.is_none() && kind != ImageType::Poster {
                        auto = auto_fanart(kind, list, lang)
                            .filter(|(id, url)| {
                                super::is_fanart_id(id) && super::is_fanart_url(url)
                            })
                            .map(|(id, url)| OverrideSource::Fanart { id, url });
                    }
                    out.extend(fanart_candidates(list));
                }
                None => errors.push("fanart: images unavailable".into()),
            }
        }
    }

    let req_lang = if matches!(kind, ImageType::Poster | ImageType::Logo) {
        lang
    } else {
        ""
    };
    sort_candidates(&mut out, req_lang);
    mark(&mut out, auto.as_ref(), current);
    CandidateList {
        auto: auto.as_ref().and_then(AutoPick::from_source),
        candidates: out,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(path: &str, lang: Option<&str>, votes: f64) -> TmdbImageFull {
        TmdbImageFull {
            file_path: path.into(),
            iso_639_1: lang.map(str::to_owned),
            iso_3166_1: None,
            vote_average: votes,
            vote_count: 0,
            width: 800,
            height: 310,
            aspect_ratio: 2.58,
        }
    }

    fn values(c: &[Candidate]) -> Vec<&str> {
        c.iter().map(|x| x.value.as_str()).collect()
    }

    #[test]
    fn candidates_sort_stable_first_of_equal() {
        // The Man in the High Castle case: three English logos, all 0 votes.
        let logos = vec![
            img("/first.png", Some("en"), 0.0),
            img("/second.png", Some("en"), 0.0),
            img("/third.png", Some("en"), 0.0),
        ];
        let mut c = tmdb_candidates(&logos);
        sort_candidates(&mut c, "en");
        assert_eq!(values(&c), ["/first.png", "/second.png", "/third.png"]);
        // Upstream's automatic pick is the LAST of equals (max_by); marked as such.
        let auto = auto_tmdb(ImageType::Logo, &logos, "en", None).unwrap();
        assert_eq!(auto, "/third.png");
        mark(
            &mut c,
            Some(&OverrideSource::Tmdb { file_path: auto }),
            None,
        );
        assert_eq!(
            c.iter().map(|x| x.auto).collect::<Vec<_>>(),
            [false, false, true]
        );
    }

    #[test]
    fn candidates_sort_by_lang_then_votes() {
        let imgs = vec![
            img("/de.png", Some("de"), 9.0),
            img("/en-low.png", Some("en"), 1.0),
            img("/null.png", None, 2.0),
            img("/fr.png", Some("fr"), 3.0),
            img("/en-high.png", Some("en"), 5.0),
            img("/ar.png", Some("ar"), 1.0),
            img("/logo.svg", Some("en"), 10.0),
        ];
        let mut c = tmdb_candidates(&imgs);
        let fan = vec![
            FanartPoster {
                id: "1".into(),
                url: "https://assets.fanart.tv/fanart/a.png".into(),
                lang: "en".into(),
                likes: "1".into(),
            },
            FanartPoster {
                id: "2".into(),
                url: "https://assets.fanart.tv/fanart/b.png".into(),
                lang: "en".into(),
                likes: "7".into(),
            },
            FanartPoster {
                id: "3".into(),
                url: "https://evil.example/fanart/c.png".into(),
                lang: "en".into(),
                likes: "9".into(),
            },
        ];
        c.extend(fanart_candidates(&fan));
        sort_candidates(&mut c, "fr");
        assert_eq!(
            values(&c),
            [
                "/fr.png",
                "/null.png",
                "/en-high.png",
                "/en-low.png",
                "/ar.png",
                "/de.png",
                "https://assets.fanart.tv/fanart/b.png",
                "https://assets.fanart.tv/fanart/a.png",
            ]
        );
        let fanart = c.iter().find(|x| x.source == "fanart").unwrap();
        assert_eq!(fanart.thumb_url, "https://assets.fanart.tv/preview/b.png");
        assert_eq!(fanart.fanart_id.as_deref(), Some("2"));
        let tm = &c[0];
        assert_eq!((tm.width, tm.height), (Some(800), Some(310)));
        assert_eq!(tm.thumb_url, "https://image.tmdb.org/t/p/w300/fr.png");
    }

    #[test]
    fn auto_pick_per_kind() {
        let posters = vec![
            img("/p-de.jpg", Some("de"), 5.0),
            img("/p-en.jpg", Some("en"), 5.0),
        ];
        assert_eq!(
            auto_tmdb(ImageType::Poster, &posters, "en", Some("/details.jpg")).as_deref(),
            Some("/details.jpg")
        );
        let backdrops = vec![
            img("/b1.jpg", None, 1.0),
            img("/b2.jpg", None, 4.0),
            img("/b-en.jpg", Some("en"), 9.0),
        ];
        assert_eq!(
            auto_tmdb(ImageType::Backdrop, &backdrops, "en", None).as_deref(),
            Some("/b2.jpg")
        );
        let logos = vec![img("/l-en.png", Some("en"), 1.0)];
        assert_eq!(
            auto_tmdb(ImageType::Logo, &logos, "de", None).as_deref(),
            Some("/l-en.png")
        );
        assert_eq!(
            auto_tmdb(ImageType::Episode, &[], "en", Some("/still.jpg")).as_deref(),
            Some("/still.jpg")
        );
    }
}
