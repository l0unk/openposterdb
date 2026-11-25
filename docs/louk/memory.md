# Memory work: status and how to measure it

## Status

**Not measured yet. There are no numbers in this file, and none may be quoted from it.**

What exists:

- The cap `BACKDROP_MAX_WIDTH` is built, tested and in `v1.3.0-louk.1`. It should cut the memory a
  large backdrop needs (a 1920 px canvas has a quarter of the pixels of a 3840 px one), but how much
  that changes the peak is **not measured**.
- The load harness is ready in `scripts/louk/` (`dev-instance.sh`, `burst.py`, `results-table.py`; usage
  in `scripts/louk/README.md`). Its self-test (`scripts/louk/selftest.sh`, 58 checks) passes against a
  stub server. It has **never run against a real binary**, so URL routing and the real rate limiter are
  verified only by the first real run.

What does not exist:

- Any release-build baseline (peak memory, idle memory, latency under the burst).
- Any allocator work (mimalloc or jemalloc behind a cargo feature), any test of `MALLOC_ARENA_MAX`,
  any bound on the in-flight render cache.

Why: the overnight release build (2026-11-25, about 03:10 UTC) ran at the same time as a frontend
build. The `dev` user slice had a 3 GB soft / 4 GB hard memory limit then. It thrashed for about 5
hours without an out-of-memory kill, every tool call stalled, and the release build was killed at about
08:00 UTC without finishing. The slice is now 6 GB soft (`memory.high`) / 8 GB hard (`memory.max`),
shared with every Claude session and every build on the box, and friends stream from it. Read the real values from
`/sys/fs/cgroup/user.slice/user-1100.slice/` before you start.

## The settings every run uses

The production settings (they match `openposterdb/compose.yaml` on stacks `main`, 2026-11-25), passed
to every variant so numbers compare:

```
IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000
IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false
IMAGE_QUALITY=85
```

`dev-instance.sh` always adds `RATINGS_DISABLED=true`, so no badge is drawn: the render cost is a little
lower than in production, and no MDBList or OMDb call is possible. Production runs with
`mem_limit: 1536m` (tier 3).

## Rules for the dev box

- **One heavy job at a time.** Heavy means any cargo build or test, `npm ci`, `vite`/`npm run build`,
  a full vitest run, Playwright or Chromium, ffmpeg, and the load runs. Nothing else heavy runs while
  one of them does, in any session.
- Cargo with `-j1`, `nice -n 19`, wrapped in `timeout`, in the foreground. Never detach a build
  (`nohup`, `setsid`, `&`).
- Before each heavy step and every few minutes while it runs:
  `grep full /sys/fs/cgroup/user.slice/user-1100.slice/memory.pressure`. If `avg60` is above 20, or
  `memory.current` is above 5 GB, **stop the job** (kill it) and hand back. Do not wait it out.
- Never call MDBList or OMDb. Never leave the instance running (`dev-instance.sh stop`).

## Procedure

1. **Build the release binary** (once; about 30 minutes cold at `-j2`, longer at `-j1`):

   ```sh
   . ~/.local/opt/rust-env.sh; export CARGO_TARGET_DIR=~/.cache/opdb-target
   cd api && timeout 90m nice -n 19 cargo build --release -j1
   ```

   Then, after cargo has finished (not at the same time), the frontend the server serves:
   `cd web && timeout 30m nice -n 19 npm ci && timeout 30m nice -n 19 npm run build-only`.
   Run everything below from the repository root, because `STATIC_DIR=web/dist` is relative.

2. **Run the variants.** All four use the same binary `~/.cache/opdb-target/release/openposterdb-api`.
   Start each from a fresh cache and database.

   | Label | Extra setting |
   | --- | --- |
   | `A-baseline` | none (upstream behaviour: 3840 px large backdrops) |
   | `B-arena2` | `MALLOC_ARENA_MAX=2` |
   | `C-cap1920` | `BACKDROP_MAX_WIDTH=1920` |
   | `D-cap1920-arena2` | both |

   For each variant `V` with extra settings `EXTRA` (from the harness's own usage notes):

   ```sh
   S=scripts/louk
   BIN=~/.cache/opdb-target/release/openposterdb-api
   PROD='IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000 IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85'

   $S/dev-instance.sh stop; $S/dev-instance.sh wipe-cache; $S/dev-instance.sh wipe-db
   $S/dev-instance.sh start $BIN $PROD $EXTRA        # e.g. EXTRA="MALLOC_ARENA_MAX=2 BACKDROP_MAX_WIDTH=1920"
   $S/dev-instance.sh bootstrap                      # admin + API key in ~/opdb-dev/api-key (mode 600, never printed)
   python3 -I $S/burst.py --label <V> --pid $(cat ~/opdb-dev/pid) --idle 90
   echo "exit code: $?"                              # 0 = valid, 2 = server died or RSS abort, 3 = INVALID
   $S/dev-instance.sh stop
   ```

   Pass `$PROD` to every variant: the harness warns and records it when a production variable is
   missing. The burst is 150 titles x (poster, `?imageSize=large` backdrop, logo) plus 40 episode stills
   = 490 requests, really rendered. It aborts the server if its memory passes 3000 MB (`--rss-abort-mb`).

3. **Noise check.** Repeat `A-baseline` and the best variant once more under new labels.
4. **Warm pass** of the best variant: same instance, no wipe, a second burst with a different label
   (`--idle 30`). It gives warm latency, and VmHWM is reset before it so it does not inherit the cold peak.
5. **Table:** `python3 -I $S/results-table.py` (cold/warm, buckets and INVALID are shown).

## Reading the results

- **Never put an INVALID run in a table** (exit code 3). It means the server did less work than asked
  (errors, 429s, too few 200s), so its memory looks better than it is.
- The number to quote is `vmhwm_mb` (peak resident memory, reset before the burst). `peak_rss_mb` is the
  highest sample taken during the burst. `idle_rss_mb` is memory after the idle wait.
- **Rate limit.** Upstream allows a burst of 240, then 1 request per 200 ms, per API key and client
  address. Production has one caller address (AIOMetadata), so one key is capped at 240 in flight and
  5 a second, and a real catalog burst through one key can get some 429s. The harness's default sends
  `X-Forwarded-For` in three groups of 200, which is a worst case for three configs at once (Louk, Zara,
  friends). `--single-bucket` is the prod-like case; with 490 requests it is INVALID by design, so use
  it only with `--titles-limit 70 --episodes-limit 20` or similar.
- Record the settings used and which variants were noise-checked next to every table.

## What to decide afterwards

Goal: peak memory under the burst at most 50% of `A-baseline`, with no p95 latency regression. The
pre-decided rule: take the variant with the lowest `vmhwm_mb` whose p95 is at most 1.10 x A's and whose
errors are not above A's. `BACKDROP_MAX_WIDTH=1920` stays in regardless (Louk approved it).

- If the winner uses `MALLOC_ARENA_MAX=2`: add it to the stacks `openposterdb/compose.yaml` environment.
  It also helps the upstream image, so it is a pure env change.
- If a global allocator is worth trying: build it behind a cargo feature (`louk-mimalloc`) so the
  default build stays upstream-identical, measure `E-mimalloc-cap1920` and `F-mimalloc`, and hard-code
  the feature in `release.yml` only if it wins.
- If peak stays above 50% after the best variant: bound the in-flight render cache (`image_inflight`,
  1,000 entries, not limited by bytes) and re-measure.
- Keep `mem_limit: 1536m` unless the peak is clearly under 600 MB.

## Hypotheses (all untested)

| # | Hypothesis | How it is tested | Status |
| --- | --- | --- | --- |
| 1 | glibc malloc arenas fragment under parallel renders | variants B and D (`MALLOC_ARENA_MAX=2`), then a global allocator | not tested |
| 2 | 3840 px backdrops (a 33 MB canvas each, often an upscale) are the biggest cost | variants A vs C | cap built; effect not measured |
| 3 | source images are fetched before the render permit, so queued requests each hold a full-size source | read in the code; a fix would need its own measurement | not tested |
| 4 | the in-flight result cache (`image_inflight`, 1,000 entries, 30 s, no byte limit) holds the whole burst | bound it by bytes and compare | not tested |
| 5 | response bodies are buffered twice | read the render and response path | not tested |

## Results

None yet. When runs exist, add one table per variant here (peak, VmHWM, idle, p50/p95/max, errors) and
copy the `A-baseline` numbers into [baseline.md](baseline.md).
