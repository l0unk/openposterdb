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

On `main` as released in `v1.3.0-louk.2` (its tree is identical to `louk/next` @4015b4e, which passed CI and both smoke tests as rc.4): backend lib 558
(upstream 474, plus 84 fork tests), 993 cargo tests across all suites, 0 failed. Frontend: vitest 394 tests in
35 files at the integration commit (@f1b4853); the later picker fixes (@4015b4e) added more specs, so
read it as "394+" (the exact number is in the latest CI run). Playwright e2e 141. For comparison, upstream
`v1.3.0` is cargo 474 + 49 + 42 + 7 + 7 + 4 and vitest 328 (table above). No fork change may leave these worse.

## Release-build memory baseline

Measured 2026-11-25 (P1.4) with the released `v1.3.0-louk.1` binary, `BACKDROP_MAX_WIDTH` unset
(= upstream v1.3.0 rendering), production settings from SPEC §2, ratings disabled (no badges drawn).
The burst is 490 requests at once (150 titles × poster, large backdrop, logo + 40 episode stills),
every render cache key cold, the id caches warmed first. Method, all variants and caveats are in
[memory.md](memory.md).

| run | VmHWM | RSS end of burst | RSS +40 s | RSS idle (+90 s) | p50 / p95 / max | statuses |
| --- | --- | --- | --- | --- | --- | --- |
| A | 2449 MB | 2211 MB | 2148 MB | 2148 MB | 8.7 / 18.5 / 19.7 s | 200 × 474, 404 × 8, 500 × 8 |
| A (repeat) | 2299 MB | 2026 MB | 1971 MB | 1971 MB | 7.5 / 19.0 / 20.1 s | 200 × 474, 404 × 8, 500 × 8 |

Baseline peak: about 2.4 GB (mean 2374 MB), well above production's 1536 MiB limit, and glibc keeps
about 2 GB after the burst. The 50% goal is therefore ≤ about 1187 MB. With
`BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=131072` the same burst peaks at
744–785 MB (31–33%), without a p95 regression.

## CI

Fork CI runs on ubuntu-24.04 because Playwright 1.60 cannot install Chromium on ubuntu-latest (26.04), as of 2026-11-25.
