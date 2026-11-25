# What this fork adds

Fork of [PNRxA/openposterdb](https://github.com/PNRxA/openposterdb) at `v1.3.0`, built for loukdekker.nl.
Fork code lives in new modules (`api/src/louk/`, `web/src/louk/`, `scripts/louk/`, `docs/louk/`) and
hooks into upstream files are marked `// louk-fork:`. Baseline numbers: [baseline.md](baseline.md).

## Divergence points in upstream files

| File | Hook | Why |
| --- | --- | --- |
| `.github/workflows/test.yml` | `on:` runs on pushes to every branch, no `paths:` filter | Every commit on every branch gets a CI run |
| `.github/workflows/release.yml` | single amd64 job, ghcr only, no Docker Hub | The fork ships `ghcr.io/l0unk/openposterdb` only |
| `api/src/lib.rs` | `pub mod louk;` | Declares the fork module root `api/src/louk/` |
| `api/src/config.rs` | `Config.louk` field, its `Debug` line, `louk:` initializer plus one early-return line in `from_env` (`louk::config::without_rating_keys`), two env names in the test `clear_config_env` list, fork tests at the end of `mod tests` | One `LoukConfig` field carries all fork config; `RATINGS_DISABLED` drops the OMDb/MDBList/Trakt keys, logs a warning and returns before upstream's "one ratings source" panic |
| `api/src/image/serve.rs` | first line of `resolve_with_ratings`; first line of `upsert_available_ratings_cached`; first line of `read_available_ratings_cached` | `RATINGS_DISABLED` forces `skip_ratings` on every render path, stops callers (whose own `skip_ratings` is `ratings_limit == 0`) from overwriting the `available_ratings` index with an empty source list, and never builds a fast-path key from sources an enabled run recorded (so a disabled run cannot serve old badged files or write badge-less renders under badged keys). These hooks have no route test: that needs a TMDB mock the crate does not have; the hard guarantee (no rating clients) is pinned at the config layer |
| `api/src/image/serve.rs` | `BACKDROP_MAX_WIDTH`: backdrop arm of the `(target_width, badge_scale)` match in `trigger_logo_backdrop_refresh` and in the `handle_logo_backdrop_inner` generation closure (`backdrop_cap::render_dims`); `let size` in `try_tmdb_logo_backdrop` (`backdrop_cap::tmdb_size`); one added `let suffix = backdrop_cap::backdrop_suffix(..)` line after each `settings_cache_suffix_with_ratings` call in `handle_logo_backdrop_inner` (fast and slow path) | Capped canvas, badge scale and TMDB size; the `.mw<N>` token reaches the cache key, the background-refresh key and the cross-id copies (all built from that suffix) |
| `api/src/handlers/image.rs` | one added `let hash = backdrop_cap::cdn_hash(..)` line in `try_cdn_redirect` | Capped backdrops get their own `/c/<hash>/` URL |
| `api/src/handlers/preview.rs` | `preview_backdrop`: `(target_width, badge_scale)` from `backdrop_cap::render_dims`, one added `backdrop_suffix` line for the preview cache key | Admin previews show the capped render and never share a preview cache entry with uncapped ones |
| `api/tests/common/mod.rs` | `louk` in the `Config` literal; `TestAppOptions.louk` (default `LoukConfig::default()`) threaded through `_setup_test_app` as one extra argument | Test app state with upstream defaults, or a fork config per test |

## Settings added

| Setting | Where | What |
| --- | --- | --- |
| `RATINGS_DISABLED` | env (`api/src/louk/config.rs`) | Dev/test only, never production and never behind a CDN; strict parsing (a typo stops startup instead of re-enabling rating spend): no rating clients, no rating lookups, no badges, no startup panic without a rating provider. Guarantees local instances and load tests never call MDBList or OMDb. See [configuration.md](../configuration.md#fork-only-loukdekkernl) |
| `BACKDROP_MAX_WIDTH` | env (`api/src/louk/config.rs`, logic in `api/src/louk/backdrop_cap.rs`) | Caps the render width of `large`/`very-large` backdrops (production: `1920`); unset = upstream. Capped keys get `.mw<N>` right before the image-size token, so upstream's cache upgrades still parse them. Fanart backdrops are capped at render; their download is unchanged |

## Syncing upstream

Placeholder: `scripts/louk/sync-upstream.sh` (added in P7b).

## Backups

Overrides and uploads live in `./db`, which Backrest backs up.

## Going back to upstream

Placeholder: filled in as fork features land (all schema changes are additive, so the upstream
binary can open a fork-migrated database).

## Backdrop safe areas

Evidence and numbers: [safe-areas.md](safe-areas.md), measured from the AIOStreams web app's own CSS (the Android TV, desktop and web apps are one app). Both its Home banner and title page show OpenPosterDB's backdrop.
- Wide screens (TV, desktop, web) crop the bottom of the backdrop and darken its left, so ratings are safest top-right, which is the default (`position=tr`, insets 0). The default is a column of up to 5 badges: on the Home banner its bottom two dim, so use `ratings_limit=3` or `badge_direction=h` (admin UI: "Max ratings" / "Badge direction" in the key's "Backdrop (series/movie)" box).
- On a title page "Background from artwork" is on by default and leaves the top right clear; no change needed.
- A phone held upright shows only the middle third, so top-right badges vanish; only `position=tc` survives, at the cost of dimmer badges on the TV banner. No single setting serves both: use one key per device class.
- No code preset was built: the existing `position`, `edge_inset_x/y`, `badge_direction` and `ratings_limit` settings already express this.
- Open, needs a real-device check: TV overscan, trailers that replace the backdrop on TVs, the Android status bar over the phone banner, and the desktop app's window controls.
