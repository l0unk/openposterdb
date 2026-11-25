# scripts/louk: local instance and catalog-burst harness (fork tooling)

Measures the server's memory under the burst a client causes when it opens a catalog:
150 titles x (poster + backdrop `?imageSize=large` + logo) + 40 episode stills = 490 requests,
all fired at once, with Louk's exact URL patterns. Ratings are disabled, so no OMDb/MDBList
quota is ever spent; the harness refuses to run otherwise.

One variant, from a fresh instance, in one command (stop, wipe cache and db, start with the extra env,
bootstrap, warm-up, burst, stop; needs the heavy slot). The binary and web files come from an extracted
release image (`docs/louk/memory.md`, "How to repeat this"):

```sh
scripts/louk/run-variant.sh --binary "$BINARY" --static-dir "$STATIC_DIR" C3 -- \
    BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2 MALLOC_MMAP_THRESHOLD_=131072 2>&1 | tee ~/opdb-dev/results/C3.run.log
```

The steps by hand (what the script does):

```sh
S=scripts/louk
BIN=~/.cache/opdb-target/release/openposterdb-api
PROD='IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000 IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85'
$S/dev-instance.sh wipe-cache; $S/dev-instance.sh wipe-db
$S/dev-instance.sh start $BIN $PROD BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2   # extras per variant
$S/dev-instance.sh bootstrap                 # admin louk-dev + API key in ~/opdb-dev/api-key (600)
python3 -I $S/burst.py --warmup --label C-cap1920 --pid $(cat ~/opdb-dev/pid)   # REQUIRED on a fresh process, see below
python3 -I $S/burst.py --label C-cap1920 --pid $(cat ~/opdb-dev/pid) --idle 90   # cold burst
python3 -I $S/burst.py --label C-cap1920-warm --pid $(cat ~/opdb-dev/pid) --idle 30  # warm: same instance, no wipe
$S/dev-instance.sh stop                      # always
python3 -I $S/results-table.py               # compare all runs in ~/opdb-dev/results
```

- `run-variant.sh [--binary BIN] [--static-dir DIR] [--no-warmup] <label> [burst.py flags] -- [VAR=VALUE ...]`:
  the sequence above. Burst flags go to the warm-up and the burst (`--idle 90` is the default, `--idle 30`
  overrides it). `BIN`/`DIR` default to `$OPDB_BINARY`/`$OPDB_STATIC_DIR`; results and the state dir follow
  `OPDB_RESULTS` (default `<OPDB_DEV_HOME>/results`), `OPDB_DEV_HOME` and `OPDB_DEV_PORT`. The nine SPEC section 2
  production settings are always set; the `VAR=VALUE` extras win over them. Prints which tuning variables
  reached the process (names only) and the allocator libraries mapped (for `LD_PRELOAD` runs). Stops the
  instance on every exit path. Exit codes: the burst's (0, 2, 3), 4 warm-up failed (no burst), 9 not started
  (slice memory pressure avg60 >= 20), 1 other. `--no-warmup` exists only to reproduce the failure below.
- `burst.py --warmup`: the warm-up pass (no burst, no result file). **Why:** a fresh process has empty
  in-memory id and TMDB-image-list caches (the database only stores render metadata), so a cold burst sends
  all 490 lookups to the TMDB API at once and TMDB answers 429 (the server returns 500; P1.4 run A0 had 135).
  The warm-up asks for the same titles once at `--warmup-concurrency` (default 4) with **other render cache
  keys** than the burst (posters `?badge_style=v`, logos `?badge_style=h`, backdrops and episodes
  `?imageSize=small`; the burst uses the defaults and `?imageSize=large`), so the lookups are cached while every
  render of the burst is still cold. It uses its own `X-Forwarded-For` groups (10.253.x.1), so the burst's rate-limit
  buckets stay full. Same safety checks and `--rss-abort-mb` as the burst. Prints one line; exit 3 (`WARMUP
  FAILED`) on any 429, transport error, or more than 5% errors (about 8 logos always fail to decode upstream:
  500, well under that). The burst afterwards is labelled "warm" (it counts cache files, and the warm-up created
  the small sources), but its own keys are cold: check `cache/` for the `.sh`/`.sv` pairs, or the render latency.
- `dev-instance.sh start <binary> [VAR=VALUE ...]`: clean environment (everything the caller exported
  is unset in the launching subshell, then the defaults are exported and the binary is exec'd, so no
  key or secret is ever on a command line), `nice -n 19`, `127.0.0.1:3900`, state in `~/opdb-dev`
  (`OPDB_DEV_HOME`, `OPDB_DEV_PORT` override). Always sets `RATINGS_DISABLED=true`; OMDb/MDBList/Trakt
  keys are never passed. Extra `VAR=VALUE` win over the defaults (e.g. `MALLOC_ARENA_MAX=2`). The server
  carries `OPDB_DEV_INSTANCE=<home>` in its environment: `stop` kills the pidfile pid and any orphan with
  that marker (lost pidfile), and a pidfile pid without the marker (a reused pid) counts as not running.
  Also `bootstrap`, `stop`, `status`, `wipe-cache`, `wipe-db`. Pass the nine SPEC section 2 prod
  variables (`$PROD` above) for a baseline; `burst.py` warns and records it when any is missing.
- Rate limiter: upstream allows one burst of 240 plus 1 request per 200 ms per (API key prefix, client
  IP), the IP being `CF-Connecting-IP`, else the rightmost `X-Forwarded-For`, else `X-Real-IP`
  (honoured from any peer, no setting needed). Prod has one caller IP per key, so one key is capped at
  240 in flight + 5/s; the default 3-bucket burst (X-Forwarded-For groups of 200 requests) is a worst
  case for three configs at once, and a real catalog burst through one key would partly get 429s. Say so
  in memory.md. `--single-bucket` sends no header (one bucket, the prod-like case): expect ~250 429s and
  an INVALID run, or use `--titles-limit/--episodes-limit` to stay under 240 requests.
- `burst.py`: options `--base --key-file --pid --label --idle --concurrency --out --titles
  --titles-limit --episodes-limit --seed --timeout --nice --bucket-size --single-bucket --rss-abort-mb
  --warmup --warmup-concurrency`.
  `--timeout` defaults to 600 s (per socket read; queued renders must not time out). Writes
  `<out>/<label>.json` (+ `.md`): `requests, errors, status_counts, p50_ms/p95_ms/max_ms` (over the 200
  responses only, overall and per kind in `kinds`, with `n` = requests and `n_ok` = 200s),
  `wall_s, rss_start_mb, peak_rss_mb` (max VmRSS sampled every 200 ms during the burst, taken before the
  idle wait), `vmhwm_mb` (VmHWM right after the burst; VmHWM is reset through `/proc/<pid>/clear_refs`
  before the burst, recorded as `vmhwm_reset`/`vmhwm_start_mb`, so a warm pass does not inherit the cold
  peak), `idle_rss_mb` (VmRSS after `--idle` s), `server_died`, `aborted`, `env` (allow-listed,
  non-secret server variables), `rss_series_mb`, `cache_files_before`/`db_bytes_before` (0 files =
  cold), `rate_limit_buckets`/`single_bucket`/`bucket_size`, `prod_env_complete`/`prod_env_missing`,
  `valid` and `invalid_reasons`. Errors are transport failures, 5xx and 4xx other than 404 (a title
  without a logo is a status, not an error).
- Valid or INVALID: a run is INVALID (`"valid": false`, `INVALID (...)` in front of the summary line and
  the `.md` title, `(INVALID)` in results-table.py) when the server died or was aborted, errors exceed
  max(2, 2%), any 429 or transport error/timeout happened, or poster/backdrop/episode got fewer than
  90% 200s: the server did less work, so its memory looks better than it is. Exit codes: 0 valid,
  2 server died (including the `--rss-abort-mb` abort), 3 invalid. Never use an INVALID run as a result.
- Safety: refuses a non-local `--base`, a server with `OMDB_API_KEY`, `MDBLIST_API_KEY` or
  `TRAKT_CLIENT_ID` set, one without `RATINGS_DISABLED=true`, or a `--pid` that does not own the
  listening socket of `--base` (pass the server pid, not a wrapper). Waits while the user slice memory
  pressure (full avg10) is above 20. Sends SIGTERM to the server and marks the run INVALID if its RSS
  exceeds `--rss-abort-mb` (default 3000; prod's cap is 1536m, but the dev slice is shared, 0 disables).
- URLs: poster `/{key}/imdb/poster-default/{imdb}.jpg`, backdrop
  `/{key}/tmdb/backdrop-default/{movie|series}-{tmdb}.jpg?imageSize=large`, logo
  `/{key}/tmdb/logo-default/{movie|series}-{tmdb}.png`, episode
  `/{key}/imdb/episode-default/episode-{imdb}-S{n}E{n}.jpg`. Upstream rejects any other tmdb prefix
  (`tv-`) with a 400; `titles.json` keeps `"type": "tv"` and the harness maps it to `series-`.
- `titles.json`: committed list (100 movies + 50 shows with tmdb id, imdb id, type; 40 episodes of
  10 of those shows, S1E1-E4, by series imdb id). Regenerate with `fetch-titles.py` (TMDB only):
  `( set -a; . ~/.config/opdb-dev.env; set +a; unset OMDB_API_KEY MDBLIST_API_KEY; python3 -I scripts/louk/fetch-titles.py )`.
- `speed-probe.py <label> <titles.json> <out.jsonl> [--fast]`: sequential latency probe (P6), 20 titles ×
  poster/backdrop large/logo + 20 episodes, one JSON line per request (status, TTFB, total, size, cache
  headers). `--fast` adds `ratings_limit=0` (same key with ratings disabled) to take production's
  fast path on a hit. Reads `$OPDB_DEV_HOME/api-key`; results in `docs/louk/speed.md`.
- `selftest.sh`: offline check of all of the above (including the warm-up's cache keys and `run-variant.sh`
  end to end) against a throwaway stub server. The stub rate-limits id lookups like TMDB does, so a burst
  without the warm-up fails there too.
