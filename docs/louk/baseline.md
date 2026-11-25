# Fork baseline

## Base

- Base tag: `v1.3.0` (2026-09-08), commit `61803c13da463472e2cac6989def6fe88e3fbe14`.
- `upstream/main` at fork time (2026-11-25): `61803c13da463472e2cac6989def6fe88e3fbe14` (identical to `v1.3.0`).
- Fork `main` started at exactly that commit; `Cargo.toml` `version` stays `1.3.0` (the git tag carries the fork version).

## Upstream test baseline

Measured 2026-11-25 with the Zig toolchain on the dev box. Later phases must not leave either suite worse.

| Suite | Result |
| --- | --- |
| `cargo test` (all suites) | 474 + 49 + 42 + 7 + 7 + 4 tests pass |
| `npx vitest run` (web) | 328 tests pass |

## Fork tests today

On the fork branch that became `v1.3.0-louk.1` (debug `cargo test`, CI green on the branch), the suites
are: lib 499 (upstream 474, plus 25 fork tests), `louk_backdrop_cap` 8 (new), and the upstream suites
unchanged (preview 49, security 42, is_valid 7, middleware 7, external_cache_only 4, and the rest as
upstream). Frontend: vitest 328, unchanged. No fork change may leave these worse.

## Release-build memory baseline

**Not measured yet. There are no numbers.** The overnight release build of 2026-11-25 was interrupted
by a memory stall on the dev box, so no burst was ever run against a real binary. Nothing here is a
guess to be quoted: when the runs exist, the A-baseline row goes here and in [memory.md](memory.md).
The exact procedure is in [memory.md](memory.md).

## CI

Fork CI runs on ubuntu-24.04 because Playwright 1.60 cannot install Chromium on ubuntu-latest (26.04), as of 2026-11-25.
