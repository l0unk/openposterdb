# The loukdekker.nl fork of OpenPosterDB

This file is for Louk. It says what the fork is, what it does today, what it does not do yet, and
how to carry it. Written 2026-11-25, for release `v1.3.0-louk.1`.

## What the fork is, and why

The fork is [PNRxA/openposterdb](https://github.com/PNRxA/openposterdb) at `v1.3.0` plus a few small
changes. It lives on Forgejo (`louk/openposterdb`) and is mirrored to
[github.com/l0unk/openposterdb](https://github.com/l0unk/openposterdb), where the tests and the image
build run.

It is a last resort. Louk's problems with OpenPosterDB (it runs out of memory, the dashboard shows only
ids, no per-title control, few badge styles, backdrops with clashing badges, speed) could not be fixed
by settings alone. A fork has to be maintained, so the rule is: keep the difference to upstream small.

- New code goes in new files: `api/src/louk/`, `api/tests/louk_*.rs`, `scripts/louk/`, `docs/louk/`.
- Changes to upstream's own files are one-line hooks marked `louk-fork:` (list below).
- No clean-ups of upstream code.
- Every new feature is off by default. With no new setting, the fork answers every URL like upstream
  `v1.3.0`.

Related files: [baseline.md](baseline.md) (test counts), [memory.md](memory.md) (memory: what was
measured and decided), [safe-areas.md](safe-areas.md) (backdrop safe areas), `scripts/louk/README.md` (load
harness).

## What it adds today

Two settings, a load harness (with the memory measurements it produced) and some documentation.

| Feature | What it does | How to turn it on | Default |
| --- | --- | --- | --- |
| `BACKDROP_MAX_WIDTH` | Caps how wide `large` and `very-large` backdrops are drawn. A 3840 px backdrop uses about 4 times the memory of a 1920 px one, and many TMDB originals are only 1920 px wide and were being stretched. Badges scale with the picture. Posters, logos, episode stills and the `small`/`medium` backdrops are not touched. | Set the env var to a number from 320 to 3840, e.g. `BACKDROP_MAX_WIDTH=1920`. | Unset = exactly upstream. A bad value (e.g. `1920px`) is ignored with a warning, so you get upstream's 3840 again. |
| `RATINGS_DISABLED` | For dev and test only. No rating lookups at all, no badges, and the server starts without any rating provider. It guarantees a local instance or a load test can never call MDBList or OMDb. | `RATINGS_DISABLED=true`. A typo stops the server at startup instead of silently spending quota. | Off. **Never set it in production.** |
| `LOUK_HTTP_ETAG` (speed) | Image responses get an `ETag`, and a client that already has the image gets a `304` with no body instead of the whole picture again. AIOMetadata's art proxy already passes `If-None-Match` and 304s through, so clients revalidate their hourly copies for a few hundred bytes instead of 68-475 KB. See `docs/louk/speed.md`. | On by default (it only adds a header; the image and every other header stay the same). `LOUK_HTTP_ETAG=false` turns it off. | On. |
| `LOUK_RATINGS_SWR` (speed) | When a title's list of rating sources is older than its refresh interval (daily for new releases), upstream makes the request wait for MDBList (up to a few seconds) and then usually sends the old picture anyway. With this on, the cached picture goes out at once and the same request runs again in the background to fetch the ratings and redraw. Same number of rating calls. | `LOUK_RATINGS_SWR=true`. | Off (that one answer may show the previous ratings). |
| Load harness | Scripts to start a local instance, warm its id caches, and fire a catalog-sized burst (150 titles x poster, backdrop, logo, plus 40 episode stills) while recording peak memory and latency. `run-variant.sh` does one variant in one command. Its results are in [memory.md](memory.md). | `scripts/louk/README.md`. | Not part of the image. |
| Backdrop safe-area notes | Where the AIOStreams app crops and darkens backdrops, and which existing settings keep ratings visible. No code. | See "Backdrop safe areas" below. | Nothing changes by itself. |
| Badge looks | Three extra rating-badge looks made for the AIOStreams app: Score colour, Mono, One score (average). | Per key or globally, "Badge background" in each image box; see "Badge styles" below. | Off: every key keeps upstream's `Default`. |

Both settings are documented in [configuration.md](../configuration.md#fork-only-loukdekkernl).

### Title dashboard (backend)

The admin dashboard only knew cache keys. The fork now keeps a title index in the SQLite database:
every id an image was requested by (imdb, tmdb, tvdb, episode ids) and the title it belongs to, with
name, year and TMDB poster. Admin endpoints under `/api/admin/louk/titles` list titles (search, filter,
sort), show one title with all its cached images (shows: per season and episode, with TMDB episode
names), find the title of an id, purge a title under all its ids, and re-fetch it. The contract is in
[api.md](../api.md#title-dashboard-fork-only-admin). The Library and title pages in the admin UI use it
(see "Title dashboard (frontend)" below).

- Code: `api/src/louk/{titles,handlers,routes,schema,state}.rs`; tests `api/tests/louk_titles.rs`.
- On the image path the fork only notes the id in memory (no database, no network, no waiting). A
  background task writes the notes every 30 s; up to 20 000 pending ids, beyond that new ones are
  dropped until the next write (the backfill picks them up later).
- Background TMDB calls: names/years for new titles, the backfill of old cache entries (50 every 10
  minutes, first run 5 minutes after start) and episode names when a show's page is opened (kept 7
  days). One call at a time, at least 250 ms apart. Never OMDb, MDBList or Trakt.
- Always on; it changes no image, cache key or response. Tables: `louk_titles`,
  `louk_title_aliases`, `louk_episodes`.
- The flush writes in transactions of 100 items, write-first (no read before the first write, so
  a busy WAL cannot fail it with `SQLITE_BUSY_SNAPSHOT`), retries a failed chunk twice (100 ms,
  400 ms) and puts anything it could not write back into the pending maps. A show page spends at
  most 3 s (10 seasons) on TMDB episode names; the rest load in the background
  (`episodes_pending: true`).
- **First run after an upgrade:** the Library starts empty or sparse. Only titles requested after the
  upgrade are indexed straight away (within about a minute); the backfill indexes about 7,200 older
  cached ids per day (50 every 10 minutes, starting 5 minutes after boot), so older titles appear over
  the first day. The Library says so in its empty state and under a short list.

### Title dashboard (frontend)

New admin pages in `web/src/louk/`, built with the existing shadcn-vue components and Tailwind:

- **Library** (`/admin/library`, a sidebar item): a poster wall of every indexed title with search
  (name, imdb id or tmdb id), a movie/show filter, sort (last requested or name) and paging, all kept
  in the URL. Each card shows the TMDB poster, name, year, last request and how many posters, logos,
  backdrops and episode stills are cached.
- **Title page** (`/admin/library/{movie|tv}/{tmdb id}`): the cached posters, backdrops and logos with
  view and purge per render, "Re-fetch" per kind and for all, "Purge all" (every id the title was
  requested by), the list of ids it was requested as, and for shows seasons with their episodes. A
  show page fetches episode names from TMDB the first time it is opened (at most about 3 s; further
  seasons load in the background and the response says `episodes_pending`); the page shows its
  header and a waiting message meanwhile.
- **Alias redirect** (`/admin/library/alias/{id_type}/{id_value}`): the new link icon in the rows of the
  poster, logo, backdrop and episode lists lands here and is sent on to the title page.
- Re-fetch draws what is not cached yet with the global settings (it does not replace cached images;
  purge first for that). Purge and re-fetch can cost a few MDBList/OMDb lookups, like the existing
  buttons; the confirmation dialogs say so. Episode stills are not re-fetched per title.
- "Override" buttons (per kind, and per episode in the season list) open the art picker, see
  "Using the picker" under Per-title overrides.

### Per-title overrides

Swap the art of one title (poster, logo, backdrop, or one episode still) for any other TMDB image
(all languages), a fanart.tv image or an uploaded file, and optionally turn its rating badges off.
This fixes cases like *The Man in the High Castle*, where upstream picks the last of three equal
English logos. The API is in [api.md](../api.md#per-title-overrides-fork-only-admin); the picker is on
the title page (below).

- **Key:** the resolved title (`movie`/`tv` + TMDB id), the kind, season/episode for stills and an
  optional request language (posters and logos). imdb, tmdb and tvdb requests of a title all hit the
  same override. **Value:** a TMDB `file_path`, a fanart image (id + URL), an upload (sha256), or
  `auto` (keep the automatic art), plus a "no badges" flag.
- **Render:** applied before upstream's TMDB-vs-fanart choice on every path (normal,
  `ENABLE_CDN_REDIRECTS`, `EXTERNAL_CACHE_ONLY`, background refresh, posters without a TMDB poster).
  Badges are drawn on top unless "no badges" is set; that flag works by rendering with ratings limit 0,
  so the image gets upstream's badge-free cache key and no rating lookup is made for it. If the
  override's image cannot be fetched or decoded, the automatic art is served and a warning logged.
- **Without an override nothing changes:** the table is loaded into memory once; a lookup is a hash
  probe (no database query, no network, no error path), and with no row the hooks return their
  input unchanged, so cache keys and bytes stay upstream's. Titles with an override keep upstream's
  cache keys too; the purge below is what makes the new art appear. With `image_source=fanart`, a
  poster with an override skips the fanart branch, so it is cached under the TMDB-variant key and
  such requests always take the slow path (resolution and ratings are still cached in memory).
- **Purge on change:** setting, changing or clearing an override runs upstream's per-title purge
  (`purge_title`: disk files of every settings suffix, `image_meta`, `available_ratings`,
  `image_mem_cache`, `image_inflight`, `id_cache`) for every id the title is known under: the
  canonical `tmdb/movie-N`/`tmdb/series-N`, its imdb and tvdb ids, every alias in the title index
  (requested ids and the cross-id copies upstream writes) and every alias with cached renders of that
  kind; for stills only the ids of that episode. Pending index entries are flushed first, so an id
  requested seconds ago is included. Because a render already in flight during the change (or its
  cross-id copies) can still land after that purge, the same purge runs again 60 s later (one
  pending job per title + kind, at most 256 pending; more are dropped with a warning). When an
  override's image cannot be fetched or decoded, the automatic art is served and cached under the
  same key, so a re-purge runs 10 minutes later, once per override version. The title page's
  "Draw it now"/re-fetch purges a kind that has an override before drawing it. (A per-title
  generation check before the cache write would need new hooks in upstream's write path
  (`post_render_cache`, `spawn_cross_id_cache`); the fork does not add them; the P3.4 upstream PR
  should.) `preview_cache` is not touched: it only holds the settings
  previews drawn from a bundled sample image, never title art. CDN copies cannot be purged from here
  (production has the CDN off).
- **Uploads:** `POST /api/admin/louk/uploads` with the file as the raw body. PNG, JPEG or WebP by
  magic bytes (content type and name are ignored), at most 15 MiB (enforced while reading), at most
  8000 px per side (checked from the header before decoding), fully decoded under decoder limits.
  Stored as `db/uploads/<sha256>.<png|jpg|webp>`, written to a temp file and renamed; only 64-char hex
  names are ever used as paths. Served back only through the admin route
  `/api/admin/louk/uploads/<sha256>`. Uploads are never deleted automatically. Validation decodes
  under a render permit with the poster path's limits (8192² RGBA); uploads with a side over 3840 px
  also get a downscaled render copy `<sha256>.r.<png|jpg>`, so an 8000 px upload costs one full
  decode at upload time instead of ~256 MB per render.
- **Sources from TMDB or fanart.tv** are fully decoded once when downloaded and only then kept in the
  base cache; a cached file whose header is no longer an image is deleted. An unusable source falls
  back to the automatic art instead of failing the request.
- **Sync cost:** `apply::render_poster`/`render_episode` mirror upstream's `generate_poster` call and
  still render. After an upstream sync, compare them with `generate_poster_with_source` and
  `generate_episode`: a new `ImageParams` field breaks the build (good), but new post-processing
  upstream adds there would not reach override posters/stills.
- **Where it lives:** table `louk_overrides` and `db/uploads/`, both inside `./db`, so Backrest backs
  them up. Code `api/src/louk/overrides/`; tests `api/tests/louk_overrides_{api,render}.rs`.
- **Upstream PR seam:** only `overrides::purge::alias_targets` reads the title index
  (`louk::titles`); a port without the index replaces that one function.

#### Using the picker

1. Open the title (Library, click the poster). Next to Posters, Backdrops or Logos click **Override**; for
   an episode still click **Override** on the episode's row in the season list.
2. The dialog asks TMDB and fanart.tv for every image of that kind (a few seconds) and shows them as
   thumbnails with the source, the language, the votes (TMDB) or likes (fanart), and the pixel size.
   The badge **Automatic pick** marks what the server chooses on its own; **Current override** marks
   what is set now. Use the Language and Source filters to narrow the list. A logo can be white on
   transparent: switch the backing (Checks, Dark, Light) to see it.
3. Click a thumbnail. The top of the dialog shows **Served now** (the image as it is drawn today) next
   to **After** (the chosen art). Press **Use this logo** (or poster, backdrop, episode still) to save.
4. **Upload your own** (or drop a file on the dialog): PNG, JPEG or WebP, at most 15 MB. The file is
   checked in the browser first; if the server refuses it (for example more than 8000 px per side), its
   message is shown. A good upload is selected for you; press the save button to use it.
5. **No rating badges** turns the badges off for this one image. With nothing else picked it keeps the
   automatic art and only removes the badges. **Applies to** (posters and logos) limits the override to
   one request language; the default is every language.
6. **Clear override** goes back to the automatic pick, badges included.

After a change the cached renders of that image are removed and the title page says so; the image is
drawn again on its next request (the banner has **Draw it now** for posters, backdrops and logos; it
can cost a few rating lookups, like Re-fetch). Apps show the new art once their own caches expire.
Unused uploads stay on disk (see the limits above).

### Production setting

Production sets three environment variables (decided 2026-11-25; the numbers are in [memory.md](memory.md)):

```
BACKDROP_MAX_WIDTH=1920
MALLOC_ARENA_MAX=2
MALLOC_MMAP_THRESHOLD_=131072
```

`BACKDROP_MAX_WIDTH` is the fork's setting (Louk approved the 1920 px cap on 2026-11-25; his AIOMetadata
URLs, `?imageSize=large`, do not change). The two `MALLOC_*` lines are glibc settings, not app settings,
and need no code. Reasons and the stacks-PR text are in
[configuration.md](../configuration.md#recommended-production-environment).

What happens at deploy:

- Every `?imageSize=large` backdrop gets a **new cache key** with the token `.mw1920`. The first
  request for each one renders it again (smaller, so cheaper than before).
- The old 3840 px files (`.zl`) stay on disk and are not used. They sit on the `cache` volume, which is
  not backed up. They can be purged later in the admin UI, or left alone.
- `small` and `medium` backdrops, posters, logos and stills keep their keys and keep hitting the
  disk cache.
- **Possible MDBList / OMDb cost.** The in-memory ratings cache is empty after the container restart.
  The first large-backdrop request for each title therefore looks up its ratings once (unless the
  key's backdrop ratings limit is 0). Upstream would have served those from disk. It is one lookup per
  title, shared with that title's poster and logo. Deploy at a quiet hour and watch
  `openposterdb-mdblist-watch` (alert at 80%) on the first day.
- Startup log line to look for: `BACKDROP_MAX_WIDTH set: large and very-large backdrops render at most
  1920 px wide`. **It is an INFO line, and `openposterdb/compose.yaml` sets `RUST_LOG=warn`, so it does
  not show.** To see it, set `RUST_LOG=warn,openposterdb_api=info` for the first start. Without that,
  the only log sign is a warning when the value is invalid (`BACKDROP_MAX_WIDTH ignored`), so no
  warning means the value was accepted. You can also open a `large` backdrop and check it is 1920 px
  wide, or look for `.mw1920` files in the cache.

### Memory: what is measured and what production sets

Measured on the dev box with the released `v1.3.0-louk.1` binary, under a worst-case catalog burst (490
requests at once, every render cold, ratings off). Peak memory (VmHWM):

| Setting | Peak | Share of upstream |
| --- | --- | --- |
| Upstream behaviour (none of the three) | 2299–2449 MB | 100% |
| `BACKDROP_MAX_WIDTH=1920` only | 1617 MB | 68% |
| The three above | **744–785 MB** | **31–33%** |
| `MALLOC_ARENA_MAX=2` and `MALLOC_MMAP_THRESHOLD_=131072` on the unmodified upstream behaviour (no cap) | 917 MB | 39% |

With the three, the memory after the burst falls back to about 290 MB, and p95 latency is about 11 s
against about 19 s for upstream. The container limit stays at `1536m`. Nothing in the code changed for
this: no allocator is built in (jemalloc and mimalloc did worse than tuned glibc), the in-flight render
cache is not weighted by bytes (about 0 gain at 1920 px), and the number of requests holding a downloaded
source while they wait for a render permit is not bounded (that is the next lever, see "Known limits").
Not measured: the CPU cost of `MALLOC_MMAP_THRESHOLD_`, and a production-like single caller (that gets
about 247 x 429 on a 490-request cold burst, with or without these settings).

**Check that the variables reached the process** (after the deploy; the image's entrypoint starts the
server through `su`, so this has to be looked at once). Run on the server, as Louk; it prints names and
values of these three tuning variables only, never the rest of the environment:

```sh
docker exec openposterdb sh -c 'tr "\0" "\n" < /proc/$(pidof -s openposterdb)/environ | grep -E "^(MALLOC_ARENA_MAX|MALLOC_MMAP_THRESHOLD_|BACKDROP_MAX_WIDTH)="'
```

Expected: exactly three lines, `BACKDROP_MAX_WIDTH=1920`, `MALLOC_ARENA_MAX=2`,
`MALLOC_MMAP_THRESHOLD_=131072`. Fewer lines means a variable did not reach the server. After a catalog
burst, `dstats openposterdb` from the dev box should show well under `1536m`, and idle memory should settle
around 300 MB (a fresh process starts near 40 MB).

## Divergence points in upstream files

Every place where the fork edits a file upstream also has. These are the places a sync can conflict.
Line numbers are at `v1.3.0-louk.1` and drift; search for `louk-fork:`.

To regenerate the list:
`grep -rn "louk-fork:" api web .github scripts docs --include='*' | grep -v '^docs/louk/'` (about 58
lines with the title dashboard; the web ones inside `web/src/louk/` and its tests are fork-only files
and not listed in the table).
`grep -rn "louk-fork:" api web .github scripts docs --include='*' | grep -v '^docs/louk/'` (87 lines
after the badge looks and preview art).

Conflict risk on a sync: **low** = upstream's own line is untouched (the fork only added a line), so a
conflict needs upstream to edit the same neighbourhood. **medium** = the hook replaces or wraps upstream
lines, so a change by upstream to those lines conflicts. **high** = the whole file differs.

| File | What the hook does | Conflict risk |
| --- | --- | --- |
| `api/src/lib.rs:8` | `pub mod louk;` declares the fork module root. | low |
| `api/src/lib.rs:69` | `AppState` gets one field `louk: Arc<LoukState>` (title index pending maps, TMDB pacing). | low |
| `api/src/main.rs:161` | Runs the fork schema (`louk::schema::apply`) right after upstream's `MIGRATIONS` loop. | low |
| `api/src/main.rs:342` | `louk` in the `AppState` literal. | low |
| `api/src/main.rs:366` | Starts the title index workers (`louk::titles::spawn_workers`) after the `last_used_at` flush task. | low |
| `api/src/routes/admin.rs:35` | Merges `louk::routes::louk_admin_routes()` into `admin_routes()`, so the fork routes get admin auth. | low |
| `api/src/handlers/admin.rs:600` | `purge_title` made `pub(crate)` so the title purge reuses it. | low (same line as upstream's signature) |
| `api/src/config.rs:24` | `Config` gets one field `louk: LoukConfig`, which carries all fork settings. | low |
| `api/src/config.rs:49` | `Debug` impl prints that field. | low |
| `api/src/config.rs:114` | `from_env` fills the field (`LoukConfig::from_env()`). | low |
| `api/src/config.rs:116` | One early-return line: with `RATINGS_DISABLED` it drops the OMDb/MDBList/Trakt keys and returns before upstream's "need one ratings source" panic. | medium (sits just above upstream's check) |
| `api/src/config.rs:162` | Four names added to the test helper `clear_config_env`. | low |
| `api/src/config.rs:326` | Marker comment for the fork's `RATINGS_DISABLED` unit tests, appended at the end of `mod tests`. | low (end of file) |
| `api/src/image/serve.rs:397` | First line of `read_available_ratings_cached`: with `RATINGS_DISABLED`, never build fast-path keys from rating sources an enabled run recorded. | low |
| `api/src/image/serve.rs:420` | First line of `upsert_available_ratings_cached`: with `RATINGS_DISABLED`, do not overwrite the `available_ratings` index with an empty list. | low |
| `api/src/image/serve.rs:449` | First line of `resolve_with_ratings`: `RATINGS_DISABLED` forces `skip_ratings` on every path. | low |
| `api/src/image/serve.rs:452` | `resolve_with_ratings`, after `id::resolve`: `louk::titles::note_resolved` notes the id and what it resolved to (before the episode uplift, so episode ids keep season/episode). In-memory only. | low |
| `api/src/image/serve.rs:460` | Inside the episode uplift: `note_cross_ids` notes the show ids upstream also caches the render under. | low |
| `api/src/image/serve.rs:714` | `handle_inner` (posters), after `validate_id_value`: `louk::titles::touch` notes the request time. In-memory only. | low |
| `api/src/image/serve.rs:1709` | `handle_episode_inner`: same `touch`. | low |
| `api/src/image/serve.rs:1829` | `handle_logo_backdrop_inner`: same `touch`. | low |
| `api/src/image/serve.rs:1211` | `trigger_logo_backdrop_refresh`: the backdrop arm of the `(width, badge scale)` match calls `backdrop_cap::render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:1652` | `try_tmdb_logo_backdrop`: the TMDB size comes from `backdrop_cap::tmdb_size`. Replaces 1 upstream line. | medium |
| `api/src/image/serve.rs:1886` | Fast path of `handle_logo_backdrop_inner`: one added line splices the `.mw<N>` token into the cache suffix. | low |
| `api/src/image/serve.rs:1926` | Slow path of the same function: same added line. | low |
| `api/src/image/serve.rs:2065` | Generation closure: the backdrop arm calls `render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:719` | `handle_inner`, before the fast path: `overrides::apply::badge_limit_hint` sets the poster ratings limit to 0 for a "no badges" override (only when such a row exists, and only from `id_cache`). | low |
| `api/src/image/serve.rs:794,795` | `handle_inner`, after `resolve_with_ratings`: the same with the resolved title (`badge_limit`), and `use_fanart` is shadowed to false when an override replaces the art, so the fanart branch is skipped. | low |
| `api/src/image/serve.rs:1162` | `trigger_logo_backdrop_refresh`: `strip_badges` after the badges are computed (the refresh gets the unmodified settings). | low |
| `api/src/image/serve.rs:1169,1170` | `trigger_logo_backdrop_refresh`: `source_bytes` of the override; the `if fanart_is_primary` line becomes `if let Some(b) = louk_bytes { b } else if fanart_is_primary`. | medium (edits an upstream line) |
| `api/src/image/serve.rs:1234` | First line of `generate_episode`: `render_episode` draws the still from the override and returns early. | low |
| `api/src/image/serve.rs:1330` | First line of `generate_poster_with_source`: `render_poster` draws the poster from the override and returns early (before the TMDB-vs-fanart chain and the "no poster_path" error). | low |
| `api/src/image/serve.rs:1458` | `fetch_fanart_images` made `pub(crate)` for the override candidates. | low (same line as upstream's signature) |
| `api/src/image/serve.rs:1714,1766` | `handle_episode_inner`: `badge_limit_hint` before the fast path, `badge_limit` after the episode check. | low |
| `api/src/image/serve.rs:1838,1911` | `handle_logo_backdrop_inner`: the same two lines, shadowing `type_ratings_limit`. | low |
| `api/src/image/serve.rs:2007,2008` | Logo/backdrop generation closure: `source_bytes`; the `let (primary, fallback) = if fanart_is_primary` line becomes `if let Some(b) = louk_bytes { (Some(b), None) } else if fanart_is_primary`. | medium (edits an upstream line) |
| `api/src/routes/app.rs:140,146` | `image_routes()` (and `cdn_routes()`) are wrapped in `louk::http_cache::layer`, which adds the `ETag`/304 middleware unless `LOUK_HTTP_ETAG=false`. | low |
| `api/src/main.rs:228` | The `image_mem_cache` weigher calls `louk::http_cache::note_insert`, which hashes a newly inserted image once and remembers its tag. | low |
| `api/src/image/serve.rs:664` | `check_caches`, memory-hit branch: `louk::http_cache::note_key` tells the ETag layer which entry the request serves. | low |
| `api/tests/common/mod.rs:105` | The test `image_mem_cache` gets the same weigher hook (weight 1, so capacity is unchanged). | low |
| `api/src/image/serve.rs:736,1726,1882` | Fast paths of `handle_inner`, `handle_episode_inner`, `handle_logo_backdrop_inner`: one line after the `available_ratings` lookup, `louk::swr::fast_path_sources` falls back to a stale row when `LOUK_RATINGS_SWR` is on (else returns upstream's value unchanged). | low |
| `api/src/image/serve.rs:769,1739,1900` | The same three fast-path hits: one line before `return`, `louk::swr::revalidate` replays the request in the background when the key came from a stale row. | low |
| `api/src/handlers/image.rs:528` | `try_cdn_redirect`: one added line gives capped backdrops their own `/c/<hash>/` URL. | low |
| `api/src/handlers/preview.rs:306` | `preview_backdrop`: width and badge scale from `render_dims`. Replaces 2 upstream lines with 1. | medium |
| `api/src/handlers/preview.rs:327` | `preview_backdrop`: one added line for the capped preview cache key. | low |
| `api/tests/common/mod.rs:22,35` | `TestAppOptions` gets a `louk` field and its default. | low |
| `api/tests/common/mod.rs:48,52,55` | `_setup_test_app` takes one extra argument `louk`; two callers pass it. These are long upstream lines edited in place. | medium |
| `api/tests/common/mod.rs:137` | `louk` in the test `Config` literal. | low |
| `api/tests/common/mod.rs:79` | Runs the fork schema after upstream's `MIGRATIONS` loop. | low |
| `api/tests/common/mod.rs:194` | `louk` in the test `AppState` literal. | low |
| `web/src/router/index.ts:112-131` | Three child routes of `/admin` (`library`, `library/alias/:idType/:idValue`, `library/:mediaType/:tmdbId`) between a `louk-fork:` and a `louk-fork: end` comment. Pure insertion. | low |
| `web/src/components/AppSidebar.vue:27` | The "Library" item of the sidebar list; the `lucide-vue-next` import on line 5 also gets `Library` (no marker). | low |
| `web/src/components/AppSidebar.vue:37` | `isActive` keeps "Library" highlighted on title pages. Rewrites upstream's `return route.path === path`. | medium |
| `web/src/components/ImageListView.vue:5` | The `lucide-vue-next` import gets `LibraryBig`. Rewrites upstream's line. | medium |
| `web/src/components/ImageListView.vue:319` | A link icon in each row to `/admin/library/alias/...` (opens the title page). Pure insertion in the actions cell. | low |
| `api/src/image/serve.rs:1200` | `trigger_logo_backdrop_refresh`: the backdrop arm of the `(width, badge scale)` match calls `backdrop_cap::render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:1639` | `try_tmdb_logo_backdrop`: the TMDB size comes from `backdrop_cap::tmdb_size`. Replaces 1 upstream line. | medium |
| `api/src/image/serve.rs:1865` | Fast path of `handle_logo_backdrop_inner`: one added line splices the `.mw<N>` token into the cache suffix. | low |
| `api/src/image/serve.rs:1903` | Slow path of the same function: same added line. | low |
| `api/src/image/serve.rs:2041` | Generation closure: the backdrop arm calls `render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/handlers/image.rs:529` | `try_cdn_redirect`: one added line gives capped backdrops their own `/c/<hash>/` URL. | low |
| `api/src/handlers/preview.rs:311` | `preview_backdrop`: width and badge scale from `render_dims`. Replaces 2 upstream lines with 1. | medium |
| `api/src/handlers/preview.rs:332` | `preview_backdrop`: one added line for the capped preview cache key. | low |
| `api/tests/common/mod.rs:22,35` | `TestAppOptions` gets a `louk` field and its default. | low |
| `api/tests/common/mod.rs:48,52,55` | `_setup_test_app` takes one extra argument `louk`; two callers pass it. These are long upstream lines edited in place. | medium |
| `api/tests/common/mod.rs:135` | `louk` in the test `Config` literal. | low |
| `api/src/louk/mod.rs:2` | `pub mod badge_looks;` (fork file, listed for completeness). | none |
| `api/src/services/db.rs:247-295` | `BadgeBackground` gets three variants (`Score`, `Mono`, `Average`), their tokens `s`/`m`/`a`, the `as_str`/`parse` arms and a longer error message. Everything else (query param, settings JSON, DB, cache token `.bg<x>`, CDN hash) follows from `parse`/`as_str`. | medium (upstream adding its own background value would touch the same lines; pick tokens that do not clash) |
| `api/src/handlers/image.rs:94` | One doc-comment line on `badge_background` naming the fork values (shows in the OpenAPI schema). | low |
| `api/src/image/badge.rs:38` | `section_colors` match arm for the three variants (the match is exhaustive; unreachable in practice). | low |
| `api/src/image/badge.rs:218,376` | First line of `render_badges_uniform` and `render_vertical_badge`: fork looks are drawn by `louk::badge_looks`, upstream looks fall through unchanged. | low |
| `api/src/image/generate.rs:359,443,632,732` | One line after `for_shape` in each `render_*_sync`: `badge_looks::collapse` turns the badge list into one average badge for the `a` look (borrows the list for every other look). | low |
| `api/src/handlers/preview.rs` poster 182-241, logo 249-298, backdrop 306-362 (besides the cap lines), episode 391-445 | Per handler: an extra extractor argument `PreviewArt` (`?art=`); one line splicing the `.art…` token into the preview cache key (`preview_art::keyed`); the badges line takes the spread samples for fork looks and for any real-art preview (`badge_looks::preview_samples`, else upstream's `sample_badges()`); the sample-image line replaced by `preview_art::source` (the sample without `art`; with it, the title's TMDB image plus a render-semaphore permit held through the render); `&` on the render call's first argument; and both `preview_cache.insert` lines skip art previews (the memory cache counts entries, not bytes). Every touched line carries `louk-fork:`. | medium (upstream edits to the preview handlers will touch them) |
| `web/src/components/RenderSettingsForm.vue:18` and 8 lines at 688-1211 | Imports `web/src/louk/badgeLooks.ts`; each of the four "Badge background" selects lists the fork looks (`v-for`) and shows a one-line description under the select when one is chosen. | low |
| `.github/workflows/test.yml:7` | Runs on pushes to every branch, no `paths:` filter (upstream: `main` only, and only when `api/`, `web/` or the workflow change). | medium |
| `.github/workflows/test.yml:20,37,58` | Three jobs pinned to `ubuntu-24.04`. `ubuntu-latest` is now 26.04 and Playwright 1.60 cannot install Chromium there. | medium. If upstream fixes this (new Playwright), take upstream's version and drop the pin. |
| `.github/workflows/release.yml:6,10` | The whole file is rewritten: one amd64 job, ghcr only, no Docker Hub, no arm64, no manifest-merge job, runner pinned. | **high** (keep ours on every sync) |

Also changed, without a marker (documentation, easy to merge):

| File | Change |
| --- | --- |
| `docs/configuration.md` | A "Fork-only (loukdekker.nl)" section appended at the end. |
| `docs/api.md` | One paragraph under the backdrop sizes about `BACKDROP_MAX_WIDTH`; "Title dashboard (fork-only, admin)" and "Per-title overrides (fork-only, admin)" sections appended at the end. |
| `docs/api.md` | One paragraph under the backdrop sizes about `BACKDROP_MAX_WIDTH`; one sub-bullet under `badge_background` for the fork looks. |
| `docs/configuration.md` | A "Badge looks" subsection at the end of the fork section. |
| `docs/architecture.md` | The fork's `badge_background` values added to the cache-token and parameter tables. |

`Cargo.toml` and `Cargo.lock` are unchanged: the app still reports version `1.3.0`. The git tag
carries the fork version. That keeps the most conflict-prone files out of the diff.

### Files upstream does not have

| Path | What |
| --- | --- |
| `api/src/louk/mod.rs` | Fork module root. |
| `api/src/louk/config.rs` | `LoukConfig`, reads `RATINGS_DISABLED`, `BACKDROP_MAX_WIDTH`, `LOUK_HTTP_ETAG` and `LOUK_RATINGS_SWR`. |
| `api/src/louk/http_cache.rs` | `ETag`/304 middleware for image routes, the weigher and `check_caches` hooks, tag side map (tags only, no bytes). |
| `api/src/louk/swr.rs` | Stale-while-revalidate for a stale ratings index: stale-row lookup, background replay (deduplicated, at most 2), counters. |
| `api/src/louk/backdrop_cap.rs` | All the cap logic (size plan, cache-key token, CDN hash, TMDB size) and its unit tests. |
| `api/tests/louk_backdrop_cap.rs` | Route-level tests for the cap. |
| `api/src/louk/schema.rs` | Fork tables (`louk_titles`, `louk_title_aliases`, `louk_episodes`), additive only. |
| `api/src/louk/state.rs` | `LoukState`, the one `AppState` field: pending maps of the image-path hooks, TMDB pacing. |
| `api/src/louk/titles.rs` | Title index: hooks, flush, details/backfill workers, queries shared with P3 (`aliases_for_title`). |
| `api/src/louk/handlers.rs`, `routes.rs` | Title dashboard admin endpoints. |
| `api/tests/louk_titles.rs` | Tests for the index, the endpoints and the rollback check (`upstream_schema_opens_fork_db`). |
| `api/src/louk/overrides/` | Per-title overrides: `store.rs` (table + in-memory map), `uploads.rs`, `candidates.rs`, `apply.rs` (render-path hooks), `purge.rs`, `handlers.rs`. The table SQL is listed in `louk/schema.rs`; the store hangs off `LoukState`. |
| `api/tests/louk_overrides_api.rs`, `louk_overrides_render.rs` | Override API, upload and store tests; render-path and purge tests. |
| `web/src/louk/**` | Title dashboard frontend: `api.ts` (typed fetchers for the endpoints), `format.ts`, views (`LibraryView`, `TitleView`, `AliasRedirect`) and components (`TitleCard`, `ImageTile`, `SeasonList`, `SegmentedControl`). |
| `web/src/__tests__/louk/**` | Unit tests of the above (api, Library, title page, the ImageListView link). |
| `docs/louk/` | This file, `baseline.md`, `memory.md`, `safe-areas.md` and its `safe-areas/` evidence. |
| `api/tests/louk_http_cache.rs` | Route tests for the ETag layer. |
| `api/tests/louk_swr.rs` | Route tests for `LOUK_RATINGS_SWR`. |
| `scripts/louk/` | Load harness (`burst.py` with its `--warmup` pass, `dev-instance.sh`, `run-variant.sh`, `results-table.py`, `fetch-titles.py`, `titles.json`, `selftest.sh`, README) and `sync-upstream.sh`. |
| `api/src/louk/badge_looks.rs`, `api/src/louk/badge_looks/tests.rs` | The three badge looks (drawing, score scale, average, preview samples) and their unit tests, including `louk_defaults_do_not_change_cache_keys`. |
| `api/src/louk/preview_art.rs` | `?art=` for the admin previews: parsing, cache token, TMDB fetch, JSON errors, unit tests. |
| `api/tests/louk_badge_looks.rs` | Route-level tests: preview endpoints (including `?art=`), image query param, global and per-key settings. |
| `web/src/louk/badgeLooks.ts`, `web/src/louk/__tests__/badgeLooks.spec.ts` | Names and one-line descriptions of the looks for the settings form, and their test. |
| `docs/louk/` | This file, `baseline.md`, `memory.md`, `safe-areas.md` and its `safe-areas/` evidence, `badge-styles.md` (design notes for the badge looks). |
| `scripts/louk/` | Load harness (`burst.py`, `dev-instance.sh`, `results-table.py`, `fetch-titles.py`, `titles.json`, `selftest.sh`, README) and `sync-upstream.sh`. |

### Workflow differences

| Workflow | Upstream | Fork |
| --- | --- | --- |
| `Tests` (`test.yml`) | Pushes to `main` only, and only for `api/`, `web/` and the workflow; pull requests to `main` | Every push to every branch, no path filter, pull requests to `main`; jobs on `ubuntu-24.04` |
| `Release` (`release.yml`) | On release published: amd64 and arm64, Docker Hub and ghcr, manifest merge | On release published: amd64 only, `ghcr.io/l0unk/openposterdb`, tags `<tag>`, `<tag without v>`, `v1` and `latest`; build arg `CARGO_FEATURES` from a repo variable (empty) |

## Syncing upstream

One command from a clean clone of the fork on the dev box:

```sh
scripts/louk/sync-upstream.sh              # merges upstream/main
scripts/louk/sync-upstream.sh v1.4.0       # or an upstream tag
```

What it does: fetches `upstream` and `origin`; says "already up to date" if there is nothing new
(today `upstream/main` is still `v1.3.0`, so it does say that); otherwise creates `louk/sync-<date>`
from `origin/main`, merges upstream into it and, if the merge is clean, pushes the branch to Forgejo.
Forgejo mirrors it to GitHub, where Actions runs the tests. It prints the URL to watch and the next
steps. It never touches `main`.

If there are conflicts it stops with the merge open and lists the files. Look the file up in the
divergence table above: most conflicts are one of the one-line hooks. Keep upstream's new code and put
the hook line back. `release.yml` always keeps the fork's version.

Tests run on GitHub, not on the dev box (cargo builds can stall the box that friends stream from).
`--local-tests` exists for a person who accepts that: it runs `cargo test -j1` and `vitest` with `nice`
and a time limit. `--no-push` stops after the merge.

After a clean sync and green CI: review the diff, merge the branch into `main`, then tag a release
(`v<upstream version>-louk.1`).

## Releases

A release is a tag. Always in this order:

1. Tag the commit and push the tag to **Forgejo**: `git tag -a v1.3.0-louk.2 -m "..."` then
   `git push origin v1.3.0-louk.2`.
2. Wait until the tag shows up on GitHub (the Forgejo mirror carries it, within about a minute).
3. Create the GitHub release **on that existing tag** (tag name only, no target commit). Publishing
   it starts the `Release` workflow, which builds the amd64 image and pushes it to
   `ghcr.io/l0unk/openposterdb:<tag>`.
4. Check the package can be pulled without logging in (it must be public).

**Never create tags or branches directly on GitHub.** The Forgejo mirror is a `git push --mirror`:
it makes GitHub look exactly like Forgejo and deletes every ref Forgejo does not have. A tag made on
GitHub would be deleted at the next push.

Production pins the full tag (`ghcr.io/l0unk/openposterdb:v1.3.0-louk.1`), never `latest`.
`Cargo.toml` stays at `1.3.0`, so the running app says `1.3.0`; the image tag tells which fork build it is.

## Backups

Everything stateful is in `./db` (SQLite: API keys and settings), which Backrest backs up. The title
dashboard adds three tables there (`louk_titles`, `louk_title_aliases`, `louk_episodes`); they can be
rebuilt from traffic and the backfill, so losing them only empties the library for a while. Per-title
overrides are the table `louk_overrides` and the files in `db/uploads/`; both are in `./db`, so
Backrest backs them up, and restoring `./db` restores them together.

The `cache` volume holds the rendered images. It is not backed up and does not need to be: every
file is made again on request.

## Going back to upstream (rollback)

1. Set the image line in `openposterdb/compose.yaml` back to `ghcr.io/pnrxa/openposterdb:latest` (or
   the pinned `v1.3.0`).
2. Remove `BACKDROP_MAX_WIDTH` from the environment (upstream ignores it anyway). The two `MALLOC_*`
   lines can stay: they are glibc settings and help upstream's image too.
3. Deploy.

Why it is safe:

- The fork's only schema change is four new tables with a `louk_` prefix (title dashboard and
  `louk_overrides`) plus the `db/uploads/` folder. Upstream
  does not know them and leaves them alone; no upstream table or column changes. Nothing in `./db`
  needs restoring for this rollback.
- The `.mw1920` cache files are new names that upstream never builds. It ignores them. They only take
  disk space until you purge or clear the cache.
- After a rollback, large backdrops come from the old 3840 px files if they are still on disk. If they
  were purged, upstream draws them again at 3840 px, with the old memory cost.
- The test `upstream_schema_opens_fork_db` runs upstream's startup schema and migrations again on a
  fork-migrated database with rows in the fork tables, then upstream queries and writes; it passes.
  Not yet verified with a real upstream binary against a copy of the production database.

## Not done yet

| Part | Where it stands | Next step |
| --- | --- | --- |
| **Memory measurements and allocator (P1.4 to P1.7)** | **No numbers exist.** The release build that was meant to measure them was interrupted on 2026-11-25 by a memory stall on the dev box (a cargo release build and a frontend build ran together under a small memory limit; the box thrashed for 5 hours). The load harness is ready in `scripts/louk/` (its self-test passes against a stub; it has never run against a real binary). The `BACKDROP_MAX_WIDTH` change is in, but its effect on peak memory is **not measured**. No allocator (mimalloc/jemalloc) is built in, `MALLOC_ARENA_MAX` is untested, and the in-flight render cache is not bounded. | One release build with `-j1`, then the runs A to D. Exact procedure in [memory.md](memory.md). A local branch `louk/p1-4` in the dev-box clone has the source at that point; it has the same content as `main`, so nothing is lost. |
| **Title dashboard (P2)** | Backend and frontend are merged on the branch `louk/p2` (CI green). Not on `main`, not in any release. The Library is sparse right after an upgrade (about 7,200 old ids are indexed per day). A show page needed 10 to 40 s the first time while the server fetched episode names; on `louk/p3` it answers within about 3 s and loads the rest in the background. | Smoke run against a real instance (the library fills, names arrive, a show page lists seasons), then merge. |
| **Per-title overrides and uploads, and the upstream PR (P3)** | Backend done on branch `louk/p3` (store, uploads, candidates, render hooks, purge; see "Per-title overrides"). Not merged, not in any release, not yet run against a real instance. No picker UI yet, and no upstream PR branch. | Picker on the title page (P3.3); a smoke run (set the High Castle logo, check all three ids); then `pr/per-title-overrides` from upstream `main` (P3.4). |
| **New badge styles (P4)** | Not started. | Design 3 to 5 styles, each an option next to the existing `badge_style`, default unchanged. |
| **Speed and precache (P6)** | (d) ETag/304 built, on by default; (b) SWR for a stale ratings index built, off by default; both on branch `louk/p6-impl`, CI green; not in a release yet. Measurements in `docs/louk/speed.md`. | Real-instance smoke test (see `reports/P6-review.md`), then release; turn SWR on later as a separate stacks change. |
| **Title dashboard (P2)** | The frontend (library and title pages) is on branch `louk/p2-2`, CI green. The backend (title table, admin routes) is unfinished and uncommitted work in a local worktree (branch `louk/p2-1`); none of it is pushed. Not merged, not in any release. | Finish and commit the backend, then merge both, after a local smoke run against a real instance. Add the "upstream opens a fork-migrated database" test first. |
| **Per-title overrides and uploads, and the upstream PR (P3)** | Not started. The design is written down (key = resolved title + image kind, value = TMDB path, Fanart image or upload; admin only). The wrong-logo case (The Man in the High Castle: three logos with 0 votes, upstream picks the last of equal ones) is not fixed. | Build after the dashboard, then prepare the branch `pr/per-title-overrides` from upstream `main` for Louk to open on GitHub. |
| **New badge styles (P4)** | Three looks built (P4.1, branch `louk/p4-1`), default unchanged; see "Badge styles". Seen on real art in the [gallery](badges.html) (P4.2). | One tuning round from the gallery notes, then re-render the gallery. |
| **Speed and precache (P6)** | Not started; no latency measurements yet. Ideas on the list: prerender siblings, stale-while-revalidate, keep a hot set warm, `Cache-Control` and `ETag`, cheap disk hits. | Measure cold and warm latency first, then pick. Needs the memory work done first. |
| **Safe-area preset in code (P5)** | **Documentation only, by decision.** The existing settings already express a safe area, so no preset and no crop overlay was built. | Louk sets one or two fields per key by hand (see "Backdrop safe areas"). |
| MDBList quota view | Not built. | Only if the MDBList client already reads usage headers. |
| Badge looks in the free-key form | `web/src/components/FreeApiKeyCard.vue` (self-service free keys) does not list `s`/`m`/`a`; the server accepts them. Production runs `FREE_KEY_ENABLED=false`. | Add the same `v-for` as in `RenderSettingsForm.vue` if free keys are ever enabled. |

Spoiler blur for episode stills already exists upstream per key (`episode_blur`); the fork adds
nothing there.

Because the dashboard and overrides are not built, the only new per-key choices are the badge looks
(see "Badge styles", all off) and the existing backdrop settings named under "Backdrop safe areas".
The deploy switches on no new looks.

## Known limits and open items

- **Next memory lever, not built.** If production still gets killed at 1536m after the three settings, bound how
  many requests hold a downloaded source while they wait for a render permit (a separate semaphore of about
  2 x `RENDER_CONCURRENCY`, not a move of the fetch under the render permit). Measured at -80 to -140 MB
  (10-15%). It touches the fetch path of the four image kinds in upstream's `generate.rs`, so it adds
  divergence and its latency must be re-measured. Details in [memory.md](memory.md).
- **Rate limit.** Upstream limits image requests to a burst of 240, then 1 per 200 ms (5 a second),
  per API key and client address. In production AIOMetadata is the only caller, so one key behaves as
  one caller: a catalog that opens more than about 240 images at once can get some `429` answers even
  today. The fork does not change this. It matters for any speed work.
- **Caps between 1281 and 1919.** With `BACKDROP_MAX_WIDTH` in that range, a `medium` backdrop would
  fetch TMDB's `original` instead of `w780`: more memory, the opposite of the aim. Caps up to 1280
  fetch `w1280` instead of `w780`. Production uses 1920, where `small` and `medium` are not capped, so
  this does not apply. The fix, when needed, is to never fetch a larger TMDB size than upstream does
  for that `imageSize` (the reviewer's one-line change is in `P1.2-review.md`, SHOULD-FIX 1).
- **Sharper than upstream medium.** A capped `large` backdrop is fetched from TMDB `original`, so in
  production it is sharper than upstream's `medium` (a `w780` source), not identical to it. Intended.
- **Route tests.** The cap's slow path, generation closure, background refresh and TMDB size hooks have
  no route-level test (that needs a TMDB mock the crate does not have). They are covered by unit tests
  and by review. The same holds for the `RATINGS_DISABLED` hooks in `serve.rs`; the hard guarantee (no
  rating clients are built) is tested at the config level.
- **Safe areas need a real device.** TV overscan, trailers that replace the backdrop on TVs, the Android
  status bar over the phone banner, and the desktop app's window controls are unchecked. The TV
  viewport (1280 px wide) is "very likely", not certain.
- **Startup line hidden.** See "Production setting": `RUST_LOG=warn` hides the INFO line.
- **Old titles never redraw.** Titles older than a year are never re-rated (a production setting).
  Purging one title in the admin cache page redraws only that title.
- **Dev box.** Never run a cargo or frontend build next to another heavy job; the shared memory limit
  is 6 GB soft, 8 GB hard. Use GitHub Actions.

## Backdrop safe areas

Evidence and numbers: [safe-areas.md](safe-areas.md), measured from the AIOStreams web app's own CSS (the Android TV, desktop and web apps are one app). Both its Home banner and title page show OpenPosterDB's backdrop.
- Wide screens (TV, desktop, web) crop the bottom of the backdrop and darken its left, so ratings are safest top-right, which is the default (`position=tr`, insets 0). The default is a column of up to 5 badges: on the Home banner its bottom two dim, so use `ratings_limit=3` or `badge_direction=h` (admin UI: "Max ratings" / "Badge direction" in the key's "Backdrop (series/movie)" box).
- On a title page "Background from artwork" is on by default and leaves the top right clear; no change needed.
- A phone held upright shows only the middle third, so top-right badges vanish; only `position=tc` survives, at the cost of dimmer badges on the TV banner. No single setting serves both: use one key per device class.
- No code preset was built: the existing `position`, `edge_inset_x/y`, `badge_direction` and `ratings_limit` settings already express this.
- Open, needs a real-device check: TV overscan, trailers that replace the backdrop on TVs, the Android status bar over the phone banner, and the desktop app's window controls.

## Badge styles

Three extra looks for rating badges, made for the AIOStreams app (a dark UI, posters about 240 px wide
on a TV across the room, small phone grids). They are extra choices in the existing **Badge background**
setting, so nothing changes until you pick one. Design notes, colours and contrast:
[badge-styles.md](badge-styles.md).

| Name in the dashboard | Value | What you see |
| --- | --- | --- |
| Score colour | `s` | A dark plate with the source logo and a white number, and a bar along the bottom that fills with the score: teal for good (7 and up), amber for middling (5 to 6.9), red for weak. |
| Mono | `m` | The same plate, lighter, with white source symbols instead of the coloured logos. No colour at all. |
| One score (average) | `a` | One figure: the average of the shown ratings on a 0 to 10 scale, next to a ring that fills with the score in the same colours. In a row (posters) it is a pill as tall as a Score badge; in the backdrop's right-hand column a round disc. |

**Where to turn them on.** Admin → API keys → the key's settings (or Admin → Settings for every key
that has no settings of its own) → in the **Poster**, **Logo**, **Backdrop (series/movie)** and
**Episode** boxes → **Badge background**. The preview next to each box shows the look with sample
scores in all three colours. To judge a look on a real title, call the preview with
`?art=movie-<tmdb id>` or `?art=series-<tmdb id>` (see `docs/api.md`, "Admin previews on real artwork"). A single image can also ask for one with `?badge_background=s` (or `m`,
`a`). The deploy turns none of them on.

**Recommendation for the AIOStreams app: Score colour (`s`) on posters and episode stills, One score
(`a`) on backdrops.** Score colour keeps the number white on a near-opaque plate, so it reads on bright
and busy posters, and the coloured bar says "good or not" before the number is read, even at TV
distance; its colours were chosen to stay apart for colour-blind viewers, and the bar's length carries
the same information. On backdrops the default is a column of five badges at the top right, whose bottom
two sit in the Home banner's dark fade (see "Backdrop safe areas"); one disc in that corner stays in
the clean area. In phone grids no row of small badges reads, so a phone-only key would use `a` on
posters too; see "Phone-first keys" below. These are recommendations from the design and the measurements; the gallery
(below) shows them on real titles and its first look did not contradict them (open tuning items are
listed in the P4.2 report).

**Phone-first keys.** A render cannot know how small the app will show it: a 580 px poster becomes a
~130 px card in a 3-column phone grid (4.5x smaller), where a 20 px figure is 4-5 px. So for a key used
mainly on phones: set **Badge background** to **One score (average)** (`a`), **Badge size** to
**Extra large** (`badge_size=xl`, 1.5x: a 102 px pill with 48 px figures, about 10 px on the card), and,
if you keep a row look, **Max ratings** to 1 or 2 (`ratings_limit=1|2`): fewer badges leave room for
Large or Extra large badges in one row (upstream puts at most 2 large horizontal badges per row). All in
the key's **Poster** box. Score colour stays the choice for the TV poster wall.

**Gallery.** [badges.html](badges.html) shows every look (`d` for comparison, `s`, `m`, `a`) on six real
titles chosen to stress them (very dark, very bright, busy, animated, white top-right on the backdrop):
a TV row of 240 x 360 cards on a 1920 px layout, a 412 px phone grid, backdrops as the Home hero
under the app's shades at the TV size (1280 x 576; default column and the horizontal top-right row),
episode stills and logos.
Open the file in a browser; the images are in `badges/`. Re-render with `scripts/louk/gallery.py`
against a local instance (see the header of that script); `scripts/louk/gallery-shots.js` takes
screenshots of the page. The previews use the same sample ratings for every look, so the four are
comparable. The app dims badges together with the picture under its hero shades, so a grey figure low in
the hero column is the shade, not the render.

Hooks: see the badge-look rows in "Divergence points in upstream files". Rollback: upstream reads a
stored `s`, `m` or `a` as its default (`d`) and logs a warning; images keyed `.bgs`/`.bgm`/`.bga` are
simply never requested again.

