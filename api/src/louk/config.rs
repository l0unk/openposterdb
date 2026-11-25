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
    /// `BACKDROP_MAX_WIDTH`: wired up by the backdrop-cap task; always `None`
    /// until then.
    pub backdrop_max_width: Option<u32>,
}

impl LoukConfig {
    pub fn from_env() -> Self {
        Self {
            ratings_disabled: parse_bool_env("RATINGS_DISABLED"),
            backdrop_max_width: None,
        }
    }
}

/// `true` when the variable is exactly "true" or "1" (upstream's convention
/// for boolean env vars); anything else, or unset, is `false`.
pub fn parse_bool_env(key: &str) -> bool {
    env::var(key).map(|v| v == "true" || v == "1").unwrap_or(false)
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
            LoukConfig { ratings_disabled: false, backdrop_max_width: None }
        );
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
    fn louk_config_from_env_reads_ratings_disabled() {
        unsafe { env::remove_var("RATINGS_DISABLED") };
        assert_eq!(LoukConfig::from_env(), LoukConfig::default());
        unsafe { env::set_var("RATINGS_DISABLED", "true") };
        assert!(LoukConfig::from_env().ratings_disabled);
        unsafe { env::remove_var("RATINGS_DISABLED") };
    }
}
