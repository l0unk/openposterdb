//! Fork configuration, read from the environment next to upstream's `Config`.
//!
//! Every field defaults to upstream behaviour: `LoukConfig::default()` changes
//! nothing.

use std::env;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoukConfig {
    /// `RATINGS_DISABLED` ("true" | "1"): never build the OMDb, MDBList or
    /// Trakt clients and skip every rating lookup (including TMDB votes), so
    /// images render without badges. For dev and test instances only.
    pub ratings_disabled: bool,
    /// `BACKDROP_MAX_WIDTH` (320..=3840): `large` / `very-large` backdrops
    /// render at most this wide (see `louk::backdrop_cap`). `None` (unset or
    /// invalid) keeps upstream's widths.
    pub backdrop_max_width: Option<u32>,
    /// `LOUK_HTTP_ETAG=false` (or 0/no/off): do not add `ETag`/304 to image
    /// responses (`louk::http_cache`). Unset = ETags on: the 200 bytes and
    /// headers are unchanged, only validators are added.
    pub http_etag_disabled: bool,
    /// `LOUK_RATINGS_SWR` ("true" | "1"): serve the cached image at once when
    /// only the ratings index is stale and revalidate in the background
    /// (`louk::swr`). Off = upstream.
    pub ratings_swr: bool,
}

impl LoukConfig {
    pub fn from_env() -> Self {
        Self {
            ratings_disabled: parse_safety_bool_env("RATINGS_DISABLED"),
            backdrop_max_width: read_backdrop_max_width(),
            http_etag_disabled: is_off(env::var("LOUK_HTTP_ETAG").ok().as_deref()),
            ratings_swr: parse_bool_env("LOUK_RATINGS_SWR"),
        }
    }
}

/// `true` for an explicit "off" value (false/0/no/off, any case); unset or
/// anything else is not off.
pub fn is_off(raw: Option<&str>) -> bool {
    matches!(raw.map(|v| v.trim().to_ascii_lowercase()).as_deref(), Some("false" | "0" | "no" | "off"))
}

/// `BACKDROP_MAX_WIDTH` via `backdrop_cap::parse_cap`; logs the effective
/// value, and warns (and ignores it) when set but invalid.
fn read_backdrop_max_width() -> Option<u32> {
    let raw = env::var("BACKDROP_MAX_WIDTH").ok()?;
    let cap = crate::louk::backdrop_cap::parse_cap(Some(&raw));
    match cap {
        Some(cap) => tracing::info!(backdrop_max_width = cap, "BACKDROP_MAX_WIDTH set: large and very-large backdrops render at most {cap} px wide"),
        None => tracing::warn!(value = %raw, "BACKDROP_MAX_WIDTH ignored: expected an integer from {} to {}", crate::louk::backdrop_cap::MIN_CAP, crate::louk::backdrop_cap::MAX_CAP),
    }
    cap
}

/// `true` when the variable is exactly "true" or "1" (upstream's convention
/// for boolean env vars); anything else, or unset, is `false`.
pub fn parse_bool_env(key: &str) -> bool {
    env::var(key).map(|v| v == "true" || v == "1").unwrap_or(false)
}

/// Strict boolean for safety flags (`RATINGS_DISABLED`): a typo must not
/// silently mean "false" and re-enable rating spend. Trimmed, any case:
/// `true`/`1`/`yes`/`on` → true; unset, empty, `false`/`0`/`no`/`off` → false;
/// anything else panics at startup.
pub fn parse_safety_bool_env(key: &str) -> bool {
    let Ok(raw) = env::var(key) else { return false };
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        "" | "false" | "0" | "no" | "off" => false,
        _ => panic!("{key} must be true/false/1/0/yes/no/on/off, got {raw:?}"),
    }
}

/// `Config` with the OMDb, MDBList and Trakt keys removed, so `main.rs` never
/// builds those clients (`RATINGS_DISABLED`). Logs the startup warning.
pub fn without_rating_keys(mut c: crate::config::Config) -> crate::config::Config {
    c.omdb_api_key = None;
    c.mdblist_api_key = None;
    c.trakt_client_id = None;
    tracing::warn!("RATINGS_DISABLED=true: no rating lookups; images render without badges");
    c
}

/// The `skip_ratings` a render should use: the caller's own choice, forced to
/// `true` when ratings are disabled fork-wide.
pub fn effective_skip_ratings(cfg: &LoukConfig, skip: bool) -> bool {
    skip || cfg.ratings_disabled
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn louk_config_default_is_upstream() {
        assert_eq!(
            LoukConfig::default(),
            LoukConfig { ratings_disabled: false, backdrop_max_width: None, http_etag_disabled: false, ratings_swr: false }
        );
    }

    #[test]
    fn is_off_only_for_explicit_off_values() {
        for v in ["false", "0", "no", "off", " OFF ", "False"] {
            assert!(is_off(Some(v)), "{v:?}");
        }
        for v in ["true", "1", "", "yes", "nope"] {
            assert!(!is_off(Some(v)), "{v:?}");
        }
        assert!(!is_off(None));
    }

    #[test]
    fn effective_skip_ratings_respects_flag() {
        let off = LoukConfig::default();
        let on = LoukConfig { ratings_disabled: true, ..LoukConfig::default() };
        assert!(!effective_skip_ratings(&off, false));
        assert!(effective_skip_ratings(&off, true));
        assert!(effective_skip_ratings(&on, false));
        assert!(effective_skip_ratings(&on, true));
    }

    #[test]
    #[serial]
    fn parse_bool_env_accepts_true_and_1_only() {
        let key = "LOUK_TEST_PARSE_BOOL_ENV";
        for (val, want) in [("true", true), ("1", true), ("false", false), ("0", false), ("TRUE", false), ("yes", false), ("", false)] {
            unsafe { env::set_var(key, val) };
            assert_eq!(parse_bool_env(key), want, "value {val:?}");
        }
        unsafe { env::remove_var(key) };
        assert!(!parse_bool_env(key));
    }

    #[test]
    #[serial]
    fn parse_safety_bool_env_is_strict() {
        let key = "LOUK_TEST_PARSE_SAFETY_BOOL_ENV";
        for (val, want) in [
            ("true", true), ("1", true), ("yes", true), ("on", true), ("TRUE", true), (" True ", true), ("ON", true),
            ("false", false), ("0", false), ("no", false), ("off", false), ("", false), ("  ", false), ("FALSE", false), ("Off", false),
        ] {
            unsafe { env::set_var(key, val) };
            assert_eq!(parse_safety_bool_env(key), want, "value {val:?}");
        }
        for bad in ["ture", "2", "y", "enabled", "true1"] {
            unsafe { env::set_var(key, bad) };
            let r = std::panic::catch_unwind(|| parse_safety_bool_env(key));
            assert!(r.is_err(), "value {bad:?} must panic");
        }
        unsafe { env::remove_var(key) };
        assert!(!parse_safety_bool_env(key));
    }

    #[test]
    #[serial]
    fn louk_config_from_env_reads_ratings_disabled() {
        unsafe { env::remove_var("RATINGS_DISABLED") };
        assert_eq!(LoukConfig::from_env(), LoukConfig::default());
        unsafe { env::set_var("RATINGS_DISABLED", "true") };
        assert!(LoukConfig::from_env().ratings_disabled);
        unsafe { env::remove_var("RATINGS_DISABLED") };
    }

    #[test]
    #[serial]
    fn louk_config_from_env_reads_backdrop_max_width() {
        unsafe { env::remove_var("BACKDROP_MAX_WIDTH") };
        assert_eq!(LoukConfig::from_env().backdrop_max_width, None);
        unsafe { env::set_var("BACKDROP_MAX_WIDTH", "1920") };
        assert_eq!(LoukConfig::from_env().backdrop_max_width, Some(1920));
        for bad in ["", "abc", "0", "5000", "-1"] {
            unsafe { env::set_var("BACKDROP_MAX_WIDTH", bad) };
            assert_eq!(LoukConfig::from_env().backdrop_max_width, None, "{bad:?}");
        }
        unsafe { env::remove_var("BACKDROP_MAX_WIDTH") };
    }
}
