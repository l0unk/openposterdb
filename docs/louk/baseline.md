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

## Release-build memory baseline

Filled in by P1.4 (see docs/louk/memory.md).

<!-- TODO(P1.4): replace this placeholder with the release-build RSS/peak numbers (settings: SPEC section 2). -->

## CI

Fork CI runs on ubuntu-24.04 because Playwright 1.60 cannot install Chromium on ubuntu-latest (26.04), as of 2026-11-25.
