# Memory: baseline, variants and what they mean

Measured 2026-11-25 (P1.4) on the dev box against the released `v1.3.0-louk.1` binary
(`ghcr.io/l0unk/openposterdb:v1.3.0-louk.1`, run from the extracted image, no rebuild).

## Summary

- **Upstream behaviour peaks at about 2.4 GB under a catalog burst** (A: VmHWM 2449 and 2299 MB) and
  keeps about 2 GB after the burst: glibc never gives it back. Production kills the container at 1536 MiB,
  so one cold catalog burst is enough to kill it. One A run where the poster lookups still went to the
  TMDB API during the burst (so requests queued longer) peaked at 2988 MB.
- **The memory is malloc heap**, not file mappings: file-backed RSS is a constant 16–17 MB (the binary)
  in every run, so everything else is anonymous memory. The 1 s side sampler's anonymous peak is 98–99%
  of VmHWM with default glibc (A, C), but only 79–91% in the variants that free memory quickly (C2, C3,
  A3, Cm). Under tuned glibc the peak is a short spike above a sustained level about 20% lower (C3:
  sustained about 620 MB, VmHWM 744–785 MB). That matters for `mem_limit`: the kill happens on the spike.
- **`BACKDROP_MAX_WIDTH=1920` alone is not enough:** 1617 MB (68% of A), still above 1536. It makes
  p95 latency 38% better (11.7 s vs 18.5–19.0 s).
- **glibc tuning through the environment is the big lever.** With the cap, `MALLOC_ARENA_MAX=2` gives
  846–910 MB (36–38% of A). Adding `MALLOC_MMAP_THRESHOLD_=131072` gives **744–785 MB (31–33% of A)**,
  with idle RSS 286–290 MB and no p95 regression (10.8–11.1 s). **Goal (≤ 50% of A, no p95 regression):
  met with env settings only.**
- The same two glibc settings on upstream behaviour (no cap) give 917 MB (39% of A).
- Allocators through `LD_PRELOAD` (a stand-in for a built-in global allocator): jemalloc 1016 MB peak,
  437 MB idle; mimalloc 2.0.9 1331 MB peak, 427 MB idle. **Both lose to tuned glibc on peak and on idle.**

Recommended production env (stacks PR):

```
BACKDROP_MAX_WIDTH=1920
MALLOC_ARENA_MAX=2
MALLOC_MMAP_THRESHOLD_=131072
```

Keep `mem_limit: 1536m`: the measured peak (about 790 MB) is above the 600 MB line, and the
burst below is not the worst case production can see (see Caveats).

## Method

- **Binary:** `v1.3.0-louk.1`, extracted from the image, run directly on the dev box host (Debian 12,
  glibc 2.36, the same glibc as the image's `debian:bookworm-slim`), as user `dev`, so the image's
  `entrypoint.sh` (chown + `su opdb`) is bypassed. The host has 6 vCPUs, like production, so tokio runs
  6 workers. Started with `scripts/louk/dev-instance.sh` from the tag (copied out with `git archive`).
- **Settings for every run** (SPEC §2, production): `IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4
  RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000 IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true
  FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85`, plus `RATINGS_DISABLED=true` and
  no OMDb/MDBList/Trakt key (the scripts enforce both). TMDB and Fanart keys came from the dev env file.
- **Burst:** `scripts/louk/burst.py` defaults: 150 titles × (poster, `?imageSize=large` backdrop, logo) +
  40 episode stills = 490 requests, all at once (512 client threads), spread over 3 `X-Forwarded-For`
  groups so the upstream limiter (240 burst + 5/s per key and address) lets all of them through. VmHWM is
  reset (`clear_refs`) just before the burst; then the harness waits 90 s idle. RSS abort at 3000 MB.
- **State per run:** server stopped, render cache AND database wiped, new process, then a **prime pass**
  (see below), then the burst. Every burst's render cache keys were cold.
- **Side sampler** (1 s): VmRSS, `/proc/<pid>/smaps_rollup` (Anonymous, Pss_File), thread count, and the
  dev slice's PSI and `memory.current`. It would have stopped the server if PSI full avg60 > 20 or the
  slice > 6000 MB. Neither happened: PSI stayed at 0.00, and the slice peaked at 4.8 GB in total.
- Scripts and every raw result (JSON, md, run logs, side samples) are in the fork job's
  `reports/P1.4-results/` on the dev box (`scripts/` there has the runner, the prime pass, the sampler
  and the analysis script).

### Harness problem found on first contact, and the workaround

The first A run (`A0`, kept as INVALID) had 135 × 500: **TMDB's own API returned 429** (247 requests
"failed after 2 retries"). The database only stores render metadata (`image_meta`). Id resolution
(`id_cache`, 1 h) and the TMDB image lists (`tmdb_images_cache`, 30 min) live in in-memory moka caches.
So a fresh process sends every one of the 490 lookups to the TMDB API at once.

Workaround (`prime.py`): after start, the same titles are requested once at client concurrency 4 with
**different render cache keys**: backdrops and episodes `?imageSize=small` (w780 source, not the
burst's `original`), posters `?badge_style=v` (default is `h`) and logos `?badge_style=h` (default is `v`).
With ratings disabled no badge is drawn, so this is the same work under another key. It warms the id and
TMDB-list caches, plus the on-disk w780 poster/still sources (`cache/base/`), but none of the burst's
render keys. The prime uses its own `X-Forwarded-For` groups, so the burst's limiter buckets stay full.
Two prime mistakes were caught and rerun: posters/logos reject `imageSize=small` (400; run `A1`), and
`badge_style=v` is the logo default, which warmed the burst's logo keys (logo p50 4 ms; run `A2-logowarm`).
Neither is in the main table. The harness labels every primed burst "warm", because it counts cache
files, but the burst's own keys were cold (check: render latency, and the `.sh`/`.sv` key pairs in
`cache/`). After the prime, the process sits at 100–280 MB RSS with a partly filled `image_mem_cache`,
which is closer to a production process that has been up for a while than a fresh one.

## Results

VmHWM, RSS and anonymous memory in MB; latency over status-200 responses in seconds. "+40 s" is
40 s after the last response (the `image_inflight` TTL is 30 s); "idle" is +90 s. Statuses for every
valid run: 200 × 474, 404 × 8, 500 × 8. The 8 × 500 are logos that fail to decode ("The image format
could not be determined"): the same 8 titles in every run and in the prime, so not a load effect.
The 404s are titles without that art.

| run | env on top of production | VmHWM | % of A | RSS end of burst | RSS +40 s | RSS idle | anon peak | threads max/idle | p50 / p95 / max s |
|---|---|---|---|---|---|---|---|---|---|
| A | (none: upstream 3840 px large backdrops) | **2449** | 103% | 2211 | 2148 | 2148 | 2426 | 42/21 | 8.7 / 18.5 / 19.7 |
| A r2 | same | **2299** | 97% | 2026 | 1971 | 1971 | 2219 | 42/22 | 7.5 / 19.0 / 20.1 |
| C | `BACKDROP_MAX_WIDTH=1920` | **1617** | 68% | 1425 | 1422 | 1422 | 1585 | 41/22 | 5.5 / 11.7 / 12.3 |
| C2 | C + `MALLOC_ARENA_MAX=2` | **910** | 38% | 658 | 657 | 657 | 800 | 35/18 | 6.1 / 11.6 / 12.1 |
| C2 r2 | same | **846** | 36% | 593 | 593 | 593 | 668 | 36/20 | 6.0 / 11.2 / 11.9 |
| C3 | C2 + `MALLOC_MMAP_THRESHOLD_=131072` | **785** | 33% | 288 | 286 | 286 | 622 | 85/39 | 3.6 / 10.8 / 12.8 |
| C3 r2 | same | **744** | 31% | 334 | 290 | 290 | 623 | 70/39 | 3.3 / 11.1 / 13.4 |
| Cj | C + jemalloc 5.3.0 (`LD_PRELOAD`), `MALLOC_CONF=background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:1000` | **1016** | 43% | 538 | 437 | 437 | 956 | 40/23 | 6.9 / 12.5 / 13.2 |
| Cm | C + mimalloc 2.0.9 (`LD_PRELOAD`), defaults (`decommit_delay` 25 ms) | **1331** | 56% | 973 | 427 | 427 | 1208 | 74/39 | 6.2 / 10.8 / 13.0 |
| A2 | `MALLOC_ARENA_MAX=2` (no cap) | **1339** | 56% | 896 | 896 | 896 | 1324 | 33/19 | 9.8 / 16.8 / 19.6 |
| A3 | `MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=131072` (no cap) | **917** | 39% | 408 | 407 | 407 | 804 | 64/39 | 3.6 / 18.2 / 20.3 |

"% of A" is against the mean of the two A runs (2374 MB). File-backed RSS was 16–17 MB in every run.

Per image kind (p50 / p95 / max, seconds):

| run | poster | backdrop | logo | episode | response bytes |
|---|---|---|---|---|---|
| A | 7.2 / 18.4 / 18.9 | 11.2 / 18.6 / 19.7 | 7.3 / 18.5 / 18.9 | 8.8 / 17.7 / 18.8 | 253 MB |
| A r2 | 4.0 / 18.8 / 19.3 | 11.7 / 19.0 / 20.1 | 5.9 / 18.9 / 19.3 | 6.5 / 18.6 / 19.0 | 253 MB |
| C | 5.1 / 11.7 / 11.9 | 6.0 / 11.6 / 12.3 | 5.0 / 11.7 / 12.0 | 6.7 / 11.4 / 11.8 | 102 MB |
| C2 | 6.3 / 11.6 / 11.8 | 6.1 / 11.5 / 12.1 | 5.7 / 11.6 / 11.8 | 7.4 / 11.1 / 11.6 | 102 MB |
| C2 r2 | 6.1 / 11.1 / 11.6 | 6.0 / 11.2 / 11.9 | 5.7 / 11.2 / 11.6 | 7.1 / 10.8 / 11.3 | 102 MB |
| C3 | 1.9 / 3.5 / 3.7 | 7.4 / 12.0 / 12.8 | 4.4 / 7.7 / 9.3 | 2.2 / 3.4 / 3.6 | 102 MB |
| C3 r2 | 2.0 / 3.4 / 3.6 | 7.2 / 12.7 / 13.4 | 3.8 / 5.5 / 6.7 | 2.1 / 3.3 / 3.5 | 102 MB |
| Cj | 7.0 / 12.5 / 12.8 | 7.1 / 12.5 / 13.2 | 6.5 / 12.5 / 12.8 | 8.1 / 12.2 / 12.5 | 102 MB |
| Cm | 6.3 / 8.5 / 9.0 | 6.2 / 12.2 / 13.0 | 5.9 / 9.3 / 10.0 | 6.9 / 8.2 / 8.8 | 102 MB |
| A2 | 9.9 / 15.4 / 15.4 | 10.1 / 18.7 / 19.6 | 9.5 / 15.6 / 16.8 | 11.4 / 15.3 / 15.4 | 253 MB |
| A3 | 1.6 / 3.6 / 3.8 | 13.3 / 19.5 / 20.3 | 4.7 / 9.2 / 10.1 | 1.8 / 3.5 / 3.5 | 253 MB |

Mean response size: poster 127 KB, logo 138 KB, episode 65 KB, backdrop 1445 KB at 3840 px (A) vs
413 KB at 1920 px (C). The cap's `.mw1920` token was on all 348 large backdrop files of every C run, and
a sanity fetch after each run returned 3840×2160 for A runs and 1920×1080 for C runs.

### Diagnostic runs (to test hypotheses; not candidates)

| run | what changed | VmHWM | RSS idle | p95 s | notes |
|---|---|---|---|---|---|
| C2 conc16 | C2, client concurrency 16 (at most 16 requests at the server) | 768 | 481 | 0.8 (per request; wall time 13.5 s, same as C2) | H3 |
| C conc16 | C, client concurrency 16 | 1087 | 916 | 0.7 (wall 13.1 s) | H3 with default glibc |
| C2 rc2 | C2 + `RENDER_CONCURRENCY=2` | 759 | 506 | 20.4 (wall 24.7 s) | H3; p95 +80% vs C2 |
| C2 single bucket | C2, `--single-bucket` (one caller address, like production) | 751 | 484 | 5.5 | INVALID by design: **246 × 429**, 236 × 200 |
| C3 single bucket | C3, `--single-bucket` | 577 | 197 | 5.6 | INVALID by design: **248 × 429**, 234 × 200 |
| A0 | A without the prime pass | 1839 | 1627 | 14.3 | INVALID: 135 × 500 from TMDB API 429s |
| A1 | A, prime without posters/logos (they got 400) | 2988 | 2680 | 22.8 | valid; poster lookups hit the TMDB API during the burst |

## What the numbers say per hypothesis (SPEC §5 P1)

**1. glibc malloc arenas fragment under parallel renders: confirmed; this is the biggest factor.**
Without a cap, `MALLOC_ARENA_MAX=2` alone takes the peak from about 2374 to 1339 MB (−44%), and with
the cap from 1617 to 846–910 MB (−45%). RSS after the burst also falls from 1422 to 593–657 MB. glibc
gives each thread that allocates in parallel its own arena (up to 8 × 6 cores = 48). Up to 42 threads
were alive: 6 tokio workers plus blocking threads for renders and for `tokio::fs`. Each arena keeps its
freed multi-MB image buffers. The second glibc setting matters as much: glibc's mmap threshold starts at
128 KiB but **rises up to 32 MiB** each time a large mmapped block is freed. After the first renders, the
3–33 MB decode buffers and canvases are therefore served from the arenas and never returned to the OS.
`MALLOC_MMAP_THRESHOLD_=131072` fixes the threshold (and disables the adjustment), so every buffer
≥ 128 KiB is its own mapping and is unmapped on free. Result: 744–785 MB peak, and RSS falls back to
about 290 MB right after the burst. No other variant releases memory that well. Wall time and p95 did
not get worse (13.3–14.0 s wall vs 13.1–13.4 s for C2). Poster and episode latency got much better
(p95 3.4 s vs 11 s), probably because page-faulting fresh mappings is cheaper than splitting fragmented
arenas under contention; that is an observation, not a measured cause. CPU time was not measured.

The global allocators, tested by `LD_PRELOAD`, return memory after the burst. jemalloc with 1 s decay
drops to 437 MB within 3 s. mimalloc drops to 427 MB 9–12 s after the burst, exactly when tokio's
idle blocking threads exit (10 s keep-alive; threads 74 → 39), so its memory comes back when threads
die, not on a timer. But both **peak higher** than glibc with 2 arenas (1016 and 1331 MB vs 846–910).
jemalloc's p95 was 7% worse than C.

**2. Oversized sources / 3840 px canvases: confirmed, second-biggest factor.** The cap alone takes
2374 → 1617 MB (−32%) with default glibc, 1339 → 846–910 (−34%) with 2 arenas, and 917 → 744–785
(−17%) with both glibc settings. The tuned-glibc difference (~150 MB) is close to the working set the
code implies: `render_backdrop_sync` (`generate.rs` ~611) decodes the source, `resize_exact` makes a new
image (3840×2160 RGB ≈ 25 MB), and `to_rgba8()` copies it into a 33 MB RGBA canvas. That is roughly
60–90 MB per 4K render × 4 permits, against roughly 15–25 MB per render at 1920. With default glibc the
saving is 5× bigger than that working set, because fragmentation multiplies every large buffer.
Backdrop responses shrink from 1445 KB to 413 KB, and p95 latency improves 38%.

**3. Source images fetched and held before the render permit: confirmed, moderate.** In
`generate_poster`/`generate_logo`/`generate_backdrop` and `generate_episode` (`serve.rs` ~1262), the
source `Vec<u8>` is downloaded (or read from `cache/base/`) before `render_semaphore.acquire()`. With 512
requests at once, about 486 wait for 4 permits, each holding its source. Limiting how many requests
reach the server (client concurrency 16; wall time unchanged at 13.1–13.5 s, so throughput is the same)
lowers the peak from 846–910 to 768 MB with 2 arenas (−80 to −140 MB, −10 to −15%), and from 1617 to
1087 MB with default glibc (−33%: more live buffers on more threads mean more arenas grow).
`RENDER_CONCURRENCY=2` gives 759 MB but almost doubles p95 (20.4 s vs 11.2–11.6 s), so it is rejected.
The single-bucket runs agree: half the renders (236) still peak at 751 MB with C2, because the queue of
held sources and the 4 canvases are what set the peak, not the number of renders.

**4. `image_inflight` holds completed renders for 30 s, unweighted: real but small at 1920 px.** The
cache holds `Bytes` that `image_mem_cache` (byte-weighted, 128 MB) shares, since `bytes.clone()` is a
refcount, not a copy (`post_render_cache` and the insert in `serve.rs` ~855–885). So it only adds the
outputs that the 128 MB cache has already evicted. The burst's responses total 253 MB at 3840 px and
102 MB at 1920 px, which gives an upper bound of about 125 MB extra at 4K and close to 0 at 1920 (plus
whatever the prime left). The RSS curves show no step at +30 s in any run. glibc does not return the
memory anyway. Under jemalloc and mimalloc, every drop happened at +0–3 s (decay) or at +9–12 s (thread
exit), and nothing happened at +30–40 s. moka 0.12.15 has no background housekeeper, so expired entries
are only dropped by later cache activity, which an idle server does not have. Under steady traffic the
risk is the 1000-entry bound: 1000 × 1.4 MB 4K backdrops would be 1.4 GB. At 1920 px it is about 0.4 GB
in the worst case, and that needs 1000 distinct renders within 30 s.

**5. Whole bodies buffered twice: present, transient, not separately measurable.**
`tmdb.rs` (`fetch_poster_bytes` ~74, `fetch_image_bytes` ~184) does `resp.bytes().await?.to_vec()`,
which copies every downloaded source once. `cache::write` uses `tokio::fs::write`, which copies its slice
into an owned buffer before the blocking write. That happens for every output, for every cross-id copy
(`spawn_cross_id_cache`), and for every `cache/base/` source. All of these copies are freed at once. With
default glibc they add to arena fragmentation. With `MALLOC_MMAP_THRESHOLD_=131072` the large ones are
separate mappings and are unmapped immediately. The response body itself is a `Bytes` (no copy).

**Other caches** (id, ratings, api_key, fanart, tmdb_images, settings): entry-bounded metadata, small for
this burst. RSS after start is 37 MB, and after the prime (490 renders, every cache filled) 100 MB with
C3 settings, of which up to about 65 MB is prime renders in `image_mem_cache`.

## Caveats

- **No rating badges** (`RATINGS_DISABLED=true`): production draws badges, so each render costs a
  little more and the absolute numbers here are a little below production.
- **The burst is a worst case for one moment, not for a day:** three limiter buckets (as if three
  configs opened a catalog at the same time), every render key cold, and 512 requests at once. Production
  has one caller address (AIOMetadata). One key then gets 240 at once plus 5/s, and the single-bucket
  runs show that **a 490-request cold catalog burst gets ~247 × 429 in production**.
- **After a restart, production is colder than this method:** without the prime pass, the TMDB API
  rate-limits the id lookups (A0: 135 × 500). After an out-of-memory restart, clients therefore get errors
  until the id cache is warm. A1 (posters still resolving during the burst) peaked at 2988 MB, so
  slower upstream lookups mean a longer queue and a higher peak.
- **Repeats:** A, C2 and C3 ran twice (spread 6%, 7%, 5%). The other rows ran once.
- **`LD_PRELOAD` is a stand-in for a compiled-in global allocator.** It replaces malloc for everything,
  including the C code in the process (SQLite). A `#[global_allocator]` replaces it only for Rust
  allocations. Debian's jemalloc is 5.3.0, the version `tikv-jemallocator` bundles. Debian's mimalloc
  is 2.0.9: the Rust `mimalloc` crate bundles a newer 2.x with other purge defaults, so a built-in
  mimalloc could behave differently. burst.py's JSON `env` field does not record `LD_PRELOAD`; the run
  logs (`mapped allocator libs:` from `/proc/<pid>/maps`) are the record that the library was loaded.
- **Env through the image's entrypoint:** these runs bypass `entrypoint.sh`. `su` without `-l` keeps
  `MALLOC_*` variables, and glibc only strips them for privilege-gaining (setuid) execs; root running
  `su` gains nothing. Production should still verify once after the deploy:
  `docker exec openposterdb sh -c 'tr "\0" "\n" < /proc/$(pidof openposterdb)/environ | grep MALLOC'`.
- `MALLOC_MMAP_THRESHOLD_` costs one `mmap`/`munmap` and fresh zeroed pages per buffer ≥ 128 KiB. Wall
  time and latency did not get worse here. CPU time was not measured, and the thread peak rose from
  35 to 70–85, all short-lived blocking threads.

## What is still worth doing (after the env settings)

The goal is met with env settings, so the code changes below are optional; they are ranked by expected
gain against risk.

1. **Bound queued source fetches** (H3): a separate semaphore so that at most about
   2 × `RENDER_CONCURRENCY` requests hold a downloaded source while they wait for a render permit. Do not
   simply move the fetch under the render permit: then permits sit idle during downloads. Expected gain
   from the measurements: −80 to −140 MB of peak with 2 arenas (10–15%). Risk: medium (touches the fetch
   path of four image kinds in upstream files, and latency must be re-measured).
2. **Weigh `image_inflight` by bytes** (H4, the P1.6 plan: weigher plus `LOUK_INFLIGHT_MAX_MB`, default
   64). Expected gain at 1920 px for this burst: about 0. It protects against the steady-traffic
   worst case (up to 1000 retained renders). Risk: low (a one-line hook; coalescing is unaffected).
3. **Built-in allocator** (P1.5): not worth a build now. Tuned glibc beats both allocators on peak and on
   idle memory. If it is ever done, take jemalloc (`tikv-jemallocator`, `background_threads`,
   `dirty_decay_ms`/`muzzy_decay_ms` 1000) and measure it with a limited arena count; only the
   unlimited version was measured here.
4. **Copies in the fetch/write path** (H5): pass `Bytes` instead of `to_vec()`, and write the owned `Vec`
   in `spawn_blocking` instead of `tokio::fs::write`. Small, transient gain. Not worth a divergence
   from upstream on its own.

## How to rerun

The P1.4 runner (in the fork job's `reports/P1.4-results/scripts/`) does, per variant: stop, wipe
the cache and the database, start with the production env plus the variant's extras, bootstrap, prime
(`prime.py`, concurrency 4), the side sampler, `burst.py --idle 90 --rss-abort-mb 3000`, a sanity
fetch, and stop. On the dev box: one heavy job at a time; check
`grep full /sys/fs/cgroup/user.slice/user-1100.slice/memory.pressure` first (avg60 < 20), and never leave
the instance running (`pgrep -u dev -x openposterdb` must be empty afterwards).
