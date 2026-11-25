# Speed: before numbers and what the fork builds (P6)

Measured 2026-11-25 on the dev box against the released `v1.3.0-louk.1` binary (no build), with the
production settings from SPEC §2 plus `BACKDROP_MAX_WIDTH=1920 MALLOC_ARENA_MAX=2
MALLOC_MMAP_THRESHOLD_=131072`, `RATINGS_DISABLED=true` and no OMDb/MDBList/Trakt key. One local
server, sequential requests (concurrency 1), Louk's exact URL patterns, 20 titles per kind (12 movies
+ 8 shows from `scripts/louk/titles.json`; 20 episodes = 2 each of 10 shows). Probe:
`scripts/louk/speed-probe.py`. Raw data: `reports/P6.1-results/` on the dev box.

## Before: latency per kind (ms, client-side, loopback)

TTFB equals total within 0.4 ms in every row: responses are fully buffered `Bytes`, so there is no
streaming to speak of.

| pass | kind | n (200) | p50 | p95 | max | mean size |
|---|---|---|---|---|---|---|
| **cold** (empty cache + DB) | poster | 20 | 79 | 184 | 185 | 134 KB |
| | backdrop `?imageSize=large` (1920) | 20 | 338 | 459 | 462 | 475 KB |
| | logo | 19 (+1 × 500) | 25 | 33 | 37 | 128 KB |
| | episode still | 20 | 34 | 157 | 163 | 68 KB |
| **memory hit** | poster | 20 | 0.6 | 0.9 | 2.2 | |
| | backdrop | 20 | 0.9 | 1.4 | 1.9 | |
| | logo | 19 | 0.6 | 1.1 | 1.1 | |
| | episode | 20 | 0.5 | 0.5 | 0.6 | |
| **disk hit** after restart (production fast path) | poster | 20 | 1.4 | 3.0 | 4.4 | |
| | backdrop | 20 | 1.8 | 2.7 | 3.1 | |
| | logo | 19 | 1.4 | 2.2 | 2.3 | |
| | episode | 20 | 0.9 | 1.1 | 1.2 | |
| **memory hit** after that | all kinds | 79 | 0.6–1.0 | ≤ 1.5 | 2.4 | |
| **disk hit, index-miss path** after restart | poster | 20 | 3.1 | 52 | 97 | |
| | backdrop | 20 | 3.9 | 6.8 | 8.2 | |
| | logo | 19 | 1.1 | 1.6 | 1.7 | |
| | episode | 20 | 4.3 | 105 | 192 | |

- "Disk hit (production fast path)": the same URLs plus `ratings_limit=0`. With ratings disabled that
  is the same cache key, and it takes the path production takes on a hit: the cache key is rebuilt
  from the `available_ratings` row in SQLite, no TMDB or ratings call.
- "Index-miss path": Louk's exact URLs after a restart. With `RATINGS_DISABLED=true` the fork hook makes
  the `available_ratings` lookup return nothing, so every request takes the slow path (id resolve,
  TMDB on first contact, no ratings). It stands in for production's *stale index* case below, minus the
  ratings wait.
- The logo 500 is `series-46923` ("image format could not be determined"), the same upstream failure
  seen in P1.4. Failures are never cached, so every request re-downloads and fails again (14 ms here).

## Where cold time goes

From the server's own debug log (`RUST_LOG=openposterdb_api=debug,tower_http=debug`): `latency` per
request and `… render complete elapsed_ms` (time under the render permit: decode source, resize,
draw badges, encode). Medians over the 20 cold requests:

| kind | request latency | render (decode + resize + compose + encode) | rest (id resolve, TMDB image list, source download, disk + SQLite write) |
|---|---|---|---|
| poster | 78 | 60 (77%) | ~18 |
| backdrop 1920 | 336 | 306 (91%) | ~30 |
| logo | 23 | 6 | ~17 |
| episode | 33 | not logged (no render line for stills) | ~33 |

- The network share is small because the binary keeps warm HTTP/2 connections to TMDB. Timed from a
  fresh Python client (new TLS connection per call, `reports/P6.1-results/tmdb_parts.jsonl`): TMDB
  `find` ~150 ms, `images` 150–250 ms, original backdrop 1.0–2.3 MB in 110–280 ms, w780 poster ~115 ms,
  logo ~90 ms. Those are cold-connection upper bounds, not what the server pays.
- Backdrops are the expensive kind: the TMDB original (often 3840 px) is decoded and resized to 1920.
- Requests in this run arrive in poster → backdrop → logo order per title, as in a catalog, so the
  backdrop and logo can reuse the id resolution and TMDB image lists cached by the earlier requests;
  a backdrop opened on its own pays one or two more TMDB API round trips.
- **Ratings** cannot be timed with ratings disabled. Production's own log (only lines over 2 s are
  logged; 2026-11-21..24): 7 cold poster renders with `resolve_ms` 2.0–4.1 s, of which MDBList took
  2.0–4.3 s, OMDb ~130 ms, TMDB ~105 ms, and the render 70–90 ms. So in production a cold render of a
  title whose ratings are not in memory is dominated by MDBList, not by drawing.

### How ratings are cached and staled (from the code)

- Ratings values live only in memory: `ratings_cache` (moka, 30 min TTL, `try_get_with`, so concurrent
  callers share one fetch per title; episodes per episode). Nothing persists the values except the
  rendered image itself.
- `available_ratings` (SQLite + a 1 h moka in front) persists which sources a title has, so a request
  can rebuild the cache key without fetching ratings. It goes stale after `compute_stale_secs(release
  date)`: 1 day (`RATINGS_STALE_SECS`) for new or unknown titles, rising linearly to 1 year; titles older
  than `RATINGS_MAX_AGE_SECS` (1 year) never go stale.
- Rendered files go stale by the same formula, measured on the file mtime.
- **Stale file, fresh index:** the caller gets the old image at once; `trigger_*_refresh` re-renders in the
  background (deduplicated by `refresh_locks`). Upstream already does stale-while-revalidate here.
- **Stale index (a title nobody requested for longer than its stale interval):** the fast path returns
  nothing, the request takes the slow path and **waits for `resolve_with_ratings`** (MDBList/OMDb/TMDB),
  then finds the (also stale) file under the same key, returns the **old** image and starts the
  background re-render (which hits `ratings_cache`, so no second ratings call). The caller waits for the
  ratings and still gets the old picture. If the set of sources changed, the key changes and the caller
  also waits for a cold render.
- **Disk hit cost:** `cache::read` reads the whole file plus its metadata, wraps it in `Bytes`, and
  inserts it into the memory cache, plus one or two `image_meta` SQLite reads. No decode, no re-encode.
  Measured 0.9–1.8 ms p50 (memory hit 0.5–1.0 ms).

## HTTP caching as a client sees it

Same on all four kinds (and on 304-able inputs):

| check | result |
|---|---|
| headers | `Content-Type`, `Content-Length`, `Cache-Control: public, max-age=3600, stale-while-revalidate=86400`, `Vary: origin, access-control-request-method, access-control-request-headers` (CORS layer), `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy` |
| `ETag`, `Last-Modified` | none |
| `If-None-Match: *` or any tag | 200 with the full body |
| `If-Modified-Since` (future date) | 200 with the full body |
| `HEAD` | 200, empty body, correct `Content-Length` (axum) |
| `Range: bytes=0-99` | 200 with the full body, no `Accept-Ranges` |
| `Accept-Encoding: gzip, br` | not compressed (correct for JPEG/PNG) |
| errors (500) | `application/json`, no `Cache-Control` |

The fixed `max-age=3600` ignores the title's age; `compute_cdn_max_age` exists but only feeds the
`/c/` CDN routes, which production disables (`ENABLE_CDN_REDIRECTS=false`).

## AIOMetadata's side

Source: `cedya77/aiometadata` at `b3150f4` (2026-11-25; production logs say `AIOMetadata 4.1.0`, the
latest release, so the code read matches).

- **Built-in poster cache is off in production.** The stacks compose sets no `ENABLE_BUILTIN_POSTER_CACHE`
  or `POSTER_CACHE_*`, and `store.init()` always logs `Built-in poster cache active …` at start: 0 such
  lines since the 2026-11-19 start (startup lines are present). Caveat: these settings can also come from
  `.env` (unreadable) or the dashboard registry; either would still produce that log line.
- So every client fetch of `/poster-cache/proxy/{poster|background|logo}/…` (episode stills go through the
  `background` class) is a pass-through: `servePassThrough` → `openImageStream` to
  `http://openposterdb:3000/…`, streamed back.
- **TTL the client gets today:** the host `openposterdb` has no built-in preset and no rule, and
  `POSTER_PROXY_FOLLOW_UPSTREAM` is unset, so `passThroughCacheControl` applies:
  `min(POSTER_PROXY_MAX_AGE_DAYS = 1 day, upstream max-age 3600)` →
  **`public, max-age=3600, stale-while-revalidate=3600`**, with no validator, because OPDB sends none.
  A client re-downloads every image in full once its hour is up.
- **Revalidation:** pass-through already forwards the client's `If-None-Match`/`If-Modified-Since`
  upstream (`clientValidators` → `openImageStream` sets the headers), relays an upstream 304 as a 304,
  and copies the upstream `ETag`/`Last-Modified` to the client (`upstreamValidators`). The only missing
  piece is an ETag from OPDB. With the built-in cache on, the store would also revalidate expired
  entries with the upstream ETag (`revalidationHint`) and serve stale while it refreshes.
- If the built-in cache is ever turned on, OPDB art would be kept for the flat
  `POSTER_CACHE_TTL_DAYS=30` (unknown host, no preset), so ratings would sit 30 days. It then needs
  `POSTER_CACHE_PROVIDER_POLICIES=[{"domain":"openposterdb","policy":"infer"}]` to follow OPDB's
  `max-age`; posters also pass through `shapePoster` (2:3 reshape) in that mode.

## Decisions

All candidates were checked against: same bytes by default, memory during a burst (killed at 1536 MiB),
TMDB load and the 240-burst / 1 per 200 ms rate limit, and **no ratings lookup that would not happen
anyway**.

| candidate | gain for this setup | cost / risk | decision |
|---|---|---|---|
| (d) ETag + 304 | Hourly client revalidation becomes a 304 with no body instead of the full image: 68–475 KB per image per client per hour saved on the OPDB → AIOMetadata → Traefik → client path; a TV on 20 Mbit/s saves ~190 ms per backdrop. Works end-to-end with **no stacks change** (AIOMetadata pass-through forwards and relays validators). | sha256 per 200 response: 0.05–0.3 ms (SHA-NI; `sha2` is already a dependency). Same bytes, one added header. No ratings, no TMDB. | **BUILD** (default on) |
| (b) stale-while-revalidate for a stale ratings index | Removes the ratings wait (MDBList 0.1–4.3 s) from the first request for a title not requested within its stale interval: daily for new releases, i.e. the "new" catalogs. Same number of ratings calls (the fetch moves to the background). | Serves the old image for that request, which upstream already does for a stale file. Small: 3 hook lines + one module. Evidence in production is thin: only waits over 2 s are logged and none were stale-index hits in 4 days. | **BUILD** (opt-in, second priority) |
| (a) prerender siblings | ≤ ~340 ms on the first backdrop/logo view of a title (the poster's ratings are in `ratings_cache`, so no extra call for those). | Most catalog posters are never opened: a page of 100 posters → 100 × (1–2 MB TMDB download + ~300 ms CPU) at tier 3, extra render memory during exactly the bursts that killed the container, extra TMDB traffic under the same rate limits. Episode stills need per-episode OMDb ratings → not allowed. | DON'T BUILD |
| (c) keep the hot set warm 03:00–07:00 | Re-rendering before a render goes stale needs fresh ratings: that **is** a ratings lookup, for titles that may never be requested again → not allowed. Re-rendering with cached ratings returns the same bytes. Warming the memory cache from disk saves ~1 ms per request. | Ratings quota, TMDB load. | DON'T BUILD |
| (e) cheap disk hits | Already cheap: file read → `Bytes`, no decode/re-encode, 0.9–1.8 ms p50. | Divergence for < 1 ms. | DON'T BUILD |

Not built either (not in a–e, recorded for later): failed renders are never cached, so a broken TMDB
logo or a missing episode (`404` at TMDB → OPDB `500`; 31 in production's 4-day log) is re-fetched on
every request; the per-key+IP limiter (240 burst, then 5/s) sees AIOMetadata as one client.

### BUILD 1: ETag and 304 for image responses (`louk::http_cache`)

- **Files:** new `api/src/louk/http_cache.rs` (`pub mod http_cache;` in `louk/mod.rs`); one hook in
  `api/src/routes/app.rs`: `.merge(crate::louk::http_cache::layer(super::image::image_routes(), &state)) // louk-fork:`
  and the same around `cdn_routes()`.
- **Setting:** `LOUK_HTTP_ETAG` (bool, default **true**: pure efficiency gain, same bytes on every 200),
  `false` restores upstream headers exactly. Field `LoukConfig.http_etag`. `Cache-Control` stays
  `public, max-age=3600, stale-while-revalidate=86400`: with validators, revalidating hourly costs a 304,
  and a longer lifetime would delay new badges and (P3) override changes on clients.
- **Behaviour:** an `axum::middleware::from_fn_with_state` layer. For a `200` whose `Content-Type` starts
  with `image/`: take the body (`axum::body::to_bytes`, O(1) for the already-buffered body), compute
  `ETag: "<first 32 hex of sha256(body)>"` (strong), add the header. If the request's `If-None-Match`
  matches (RFC 9110 weak comparison: list parsing, `W/` stripped, `*` matches), answer `304` with the
  200's headers minus `Content-Length`/`Content-Type` and an empty body (keeps `Cache-Control`, `ETag`,
  `Vary`). GET and HEAD. Everything else (errors, JSON, redirects) passes untouched. `If-Modified-Since`
  stays unsupported (no `Last-Modified`: memory entries carry no mtime; RFC 9110 lets `If-None-Match`
  win anyway).
- **Cache key / ETag:** derived only from the bytes, so it is stable across restarts, memory/disk hits,
  cross-id copies and identical re-renders, and changes whenever the image does (new ratings, purge,
  override).
- **Tests:** unit (`http_cache.rs`): `if_none_match_parsing` (list, weak, `*`, garbage),
  `etag_is_sha256_prefix_quoted`. Route tests in `api/tests/louk_http_cache.rs`, seeding a cache file +
  `image_meta` row under `cache_dir_override` (no network): `etag_roundtrip_304` (GET → ETag; GET with it
  → 304, empty body, same ETag and Cache-Control), `head_with_matching_etag_is_304`,
  `non_matching_etag_is_200_full_body`, `error_responses_have_no_etag`,
  `no_headers_change_when_disabled` (`LOUK_HTTP_ETAG=false` → header set identical to today).
- **Measure after:** rerun `speed-probe.py` (memory-hit p50 must stay within +0.4 ms); add a conditional
  pass (send the ETag back) and record 304 rate, bytes and latency; in production, `dlogs aiometadata`
  has no per-request lines, so check one proxied URL twice with `curl -H 'If-None-Match: …'` through
  AIOMetadata from the Mac and expect a 304.

### BUILD 2: stale-while-revalidate for a stale ratings index (`louk::swr`)

- **Files:** new `api/src/louk/swr.rs`; three hooks in `api/src/image/serve.rs`, one per fast path
  (`handle_inner`, `handle_episode_inner`, `handle_logo_backdrop_inner`), each right after
  `fast_path_available` is computed and right before returning a fast-path hit.
- **Setting:** `LOUK_RATINGS_SWR` (bool, default **false**: the bytes served change, old ratings for one
  more request). Proposed on in the stacks PR. Ignored when `RATINGS_DISABLED=true`.
- **Behaviour:**
  1. `swr::fast_path_sources(state, id_key, available) -> (Option<String>, bool)`: when the setting is on,
     `available` is `None`, and the current task is not a revalidation, read the `available_ratings`
     row **ignoring its age** (one SQLite read) and return `(Some(sources), true)`; otherwise pass
     `available` through with `false`.
  2. On a cache hit with the stale flag set: `swr::revalidate(state, lock_key, fut)` spawns the
     *unchanged* slow path for the same request (`Box::pin(handle_inner(...))` etc.) inside a
     `tokio::task_local!` scope that makes step 1 return the plain value. The slow path does exactly what
     today's request would do (resolve + one `fetch_ratings`, upsert the index, render under a new key
     if the source set changed, or trigger the existing background refresh for the stale file), just
     detached from the client. Deduplicated by `refresh_locks` under `louk-swr:<cache key>` (5 min TTL).
  3. On a miss the request falls through to the slow path as today.
- **Ratings calls:** exactly one per stale window per title, as today: the fetch moves from the request
  to the spawned task, `ratings_cache` coalesces concurrent callers, and the background re-render after
  it hits `ratings_cache`. No warm-up, no prerender, no new call sites.
- **Memory:** at most what the client request would have run inline; renders share `render_semaphore`.
- **Tests:** unit: `fast_path_sources` decision table (off / fresh / stale / missing row / in
  revalidation scope / ratings disabled). Route tests (`api/tests/louk_swr.rs`) seed a stale
  `available_ratings` row (old `updated_at`) and the matching cache file; no network is needed for the
  hit: `swr_serves_cached_file_for_stale_index` (200, seeded bytes), `swr_spawns_one_revalidation_for_n_concurrent_requests`
  (counter in `louk::swr`), `swr_off_takes_slow_path` (today's behaviour), `swr_ignored_when_ratings_disabled`.
- **Measure after:** an `info` line `louk-swr: served stale index, revalidating` with id and the
  revalidation's `resolve_ms` (in production that is the wait each client no longer has); count per day
  from `dlogs openposterdb --grep louk-swr`. Locally the ratings wait cannot be reproduced (no ratings
  keys), so the local check is the route tests plus the probe's disk-hit numbers for a stale row.

### Stacks-side settings

- With BUILD 1: **nothing needed** in AIOMetadata. Its pass-through already forwards `If-None-Match`
  and relays OPDB's `ETag` and 304s.
- Not proposed now: `ENABLE_BUILTIN_POSTER_CACHE`. OPDB hits cost 1–2 ms on the same host, so it adds no
  speed. It would add resilience (stale-on-error during an OPDB restart, fewer 429s on big catalog
  bursts) at the cost of disk, up to 128 MB RAM in a 1024 MiB container (`POSTER_CACHE_MEMORY_SIZE=0`
  avoids that), a 2:3 reshape pass on posters, and the 30-day TTL unless
  `POSTER_CACHE_PROVIDER_POLICIES=[{"domain":"openposterdb","policy":"infer"}]` is set too. Revisit if
  429s or restarts show up in AIOMetadata's fallbacks.
- With BUILD 2: `LOUK_RATINGS_SWR=true` in `openposterdb/compose.yaml`.

## Built

### ETag and 304 (`LOUK_HTTP_ETAG`, on by default)

What changed from the design above: the tag is **not** hashed per response. It is computed once when an
image enters the memory cache (the cache's weigher calls `louk::http_cache::note_insert`; re-inserting
the same buffer every 60 s reuses the tag) and kept in a side map of tags only (no image bytes, 50 000
entries, 2 h). Every served image passes through the memory cache (a hit notes its key in
`check_caches`, a disk load or a render inserts it), and the layer adds the tag only when the response
body, the current memory-cache entry and the remembered tag are the same buffer. Anything else (a race,
an evicted tag) gets no `ETag`, never a wrong one. Cost: one SHA-256 per insert (≈ 0.05–0.3 ms with
SHA-NI) and one map lookup per response.

- **Strong** tag (`"<32 hex>"`): it names the exact bytes; images are never compressed or transformed
  on the way out, so byte-identical is the right promise. `If-None-Match` uses the weak comparison the
  RFC prescribes, so `W/"…"` from a client still matches.
- 304: no body, same `ETag`, `Cache-Control` and `Vary` as the 200, no `Content-Type`,
  `Content-Length` = the 200's length. Errors, redirects and rate-limit answers are untouched.
- `Cache-Control` is unchanged (`public, max-age=3600, stale-while-revalidate=86400`).

How to see it (any image URL; through AIOMetadata the same works with its `/poster-cache/proxy/…` URL,
because its pass-through forwards `If-None-Match` and relays the 304):

```sh
U='http://openposterdb:3000/<key>/tmdb/backdrop-default/movie-1368337.jpg?imageSize=large'
E=$(curl -sI "$U" | tr -d '\r' | awk -F': ' 'tolower($1)=="etag"{print $2}')
curl -s -o /dev/null -w '%{http_code} %{size_download}\n' -H "If-None-Match: $E" "$U"   # 304 0
```
