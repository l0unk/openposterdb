# scripts/louk: local instance and catalog-burst harness (fork tooling)

Measures the server's memory under the burst a client causes when it opens a catalog:
150 titles x (poster + backdrop `?imageSize=large` + logo) + 40 episode stills = 490 requests,
all fired at once, with Louk's exact URL patterns. Ratings are disabled, so no OMDb/MDBList
quota is ever spent; the harness refuses to run otherwise.

```sh
S=scripts/louk
BIN=~/.cache/opdb-target/release/openposterdb-api
PROD='IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 RATINGS_STALE_SECS=86400 RATINGS_MAX_AGE_SECS=31536000 IMAGE_STALE_SECS=0 DISABLE_PUBLIC_PAGES=true FREE_KEY_ENABLED=false ENABLE_CDN_REDIRECTS=false IMAGE_QUALITY=85'
$S/dev-instance.sh wipe-cache; $S/dev-instance.sh wipe-db
$S/dev-instance.sh start $BIN $PROD BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2   # extras per variant
$S/dev-instance.sh bootstrap                 # admin louk-dev + API key in ~/opdb-dev/api-key (600)
python3 -I $S/burst.py --label C-cap1920 --pid $(cat ~/opdb-dev/pid) --idle 90   # cold burst
python3 -I $S/burst.py --label C-cap1920-warm --pid $(cat ~/opdb-dev/pid) --idle 30  # warm: same instance, no wipe
$S/dev-instance.sh stop                      # always
python3 -I $S/results-table.py               # compare all runs in ~/opdb-dev/results
```

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
  --titles-limit --episodes-limit --seed --timeout --nice --bucket-size --single-bucket --rss-abort-mb`.
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
- `selftest.sh`: offline check of all of the above against a throwaway stub server.
