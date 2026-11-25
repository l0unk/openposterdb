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
| `api/src/config.rs` | `Config.louk` field, its `Debug` line, `louk:` initializer plus an early-return block in `from_env`, two env names in the test `clear_config_env` list, fork tests at the end of `mod tests` | One `LoukConfig` field carries all fork config; `RATINGS_DISABLED` drops the OMDb/MDBList/Trakt keys, logs a warning and returns before upstream's "one ratings source" panic |
| `api/src/image/serve.rs` | first line of `resolve_with_ratings`; first line of `upsert_available_ratings_cached` | `RATINGS_DISABLED` forces `skip_ratings` on every render path, and stops callers (whose own `skip_ratings` is `ratings_limit == 0`) from overwriting the `available_ratings` index with an empty source list |
| `api/tests/common/mod.rs` | `louk: Default::default()` in the `Config` literal | Test app state with upstream defaults |

## Settings added

| Setting | Where | What |
| --- | --- | --- |
| `RATINGS_DISABLED` | env (`api/src/louk/config.rs`) | Dev/test only, never production: no rating clients, no rating lookups, no badges, no startup panic without a rating provider. Guarantees local instances and load tests never call MDBList or OMDb. See [configuration.md](../configuration.md#fork-only-loukdekkernl) |

## Syncing upstream

Placeholder: `scripts/louk/sync-upstream.sh` (added in P7b).

## Backups

Overrides and uploads live in `./db`, which Backrest backs up.

## Going back to upstream

Placeholder: filled in as fork features land (all schema changes are additive, so the upstream
binary can open a fork-migrated database).
