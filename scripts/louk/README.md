# scripts/louk: local instance and catalog-burst harness (fork tooling)

Measures the server's memory under the burst a client causes when it opens a catalog:
150 titles x (poster + backdrop `?imageSize=large` + logo) + 40 episode stills = 490 requests,
all fired at once, with Louk's exact URL patterns. Ratings are disabled, so no OMDb/MDBList
quota is ever spent; the harness refuses to run otherwise.

```sh
S=scripts/louk
BIN=~/.cache/opdb-target/release/openposterdb-api
$S/dev-instance.sh wipe-cache; $S/dev-instance.sh wipe-db
$S/dev-instance.sh start $BIN IMAGE_MEM_CACHE_MB=128 RENDER_CONCURRENCY=4 BACKDROP_MAX_WIDTH=1920
$S/dev-instance.sh bootstrap                 # admin louk-dev + API key in ~/opdb-dev/api-key (600)
python3 -I $S/burst.py --label C-cap1920 --pid $(cat ~/opdb-dev/pid) --idle 90
$S/dev-instance.sh stop                      # always
python3 -I $S/results-table.py               # compare all runs in ~/opdb-dev/results
```

- `dev-instance.sh start <binary> [VAR=VALUE ...]`: clean environment (`env -i`), `nice -n 19`,
  `127.0.0.1:3900`, state in `~/opdb-dev` (`OPDB_DEV_HOME`, `OPDB_DEV_PORT` override). Always sets
  `RATINGS_DISABLED=true`; OMDb/MDBList/Trakt keys are never passed. Extra `VAR=VALUE` win over the
  defaults (e.g. `MALLOC_ARENA_MAX=2`). Also `bootstrap`, `stop`, `status`, `wipe-cache`, `wipe-db`.
- `burst.py`: options `--base --key-file --pid --label --idle --concurrency --out --titles
  --titles-limit --episodes-limit --seed --timeout --nice`. Writes `<out>/<label>.json` (+ `.md`):
  `requests, errors, status_counts, p50_ms/p95_ms/max_ms` (overall and per kind in `kinds`),
  `wall_s, rss_start_mb, peak_rss_mb` (max VmRSS sampled every 200 ms during the burst and idle),
  `vmhwm_mb` (VmHWM right after the burst), `idle_rss_mb` (VmRSS after `--idle` s), `server_died`,
  `env` (allow-listed, non-secret server variables), `rss_series_mb`. Errors are transport
  failures, 5xx and 4xx other than 404 (a title without a logo is a status, not an error).
  Exit code 2 if the server died. Refuses a non-local `--base`, a server with `OMDB_API_KEY`,
  `MDBLIST_API_KEY` or `TRAKT_CLIENT_ID` set, or without `RATINGS_DISABLED=true`; waits while the
  user slice memory pressure (full avg10) is above 20.
- `titles.json`: committed list (100 movies + 50 shows with tmdb id, imdb id, type; 40 episodes of
  10 of those shows, S1E1-E4, by series imdb id). Regenerate with `fetch-titles.py` (TMDB only):
  `( set -a; . ~/.config/opdb-dev.env; set +a; unset OMDB_API_KEY MDBLIST_API_KEY; python3 -I scripts/louk/fetch-titles.py )`.
- `selftest.sh`: offline check of all of the above against a throwaway stub server.
