# The loukdekker.nl fork of OpenPosterDB

This file is the owner's manual for the fork. It says what the fork is, what it adds, which settings it has,
where to switch things on, what is not done, and how to carry it (back up, roll back, release, sync with
upstream). It describes `main` at the release `v1.3.0-louk.2` (2026-11-25), the successor of `v1.3.0-louk.1`.

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
- New behaviour is opt-in. With no new setting chosen, the fork answers every image URL like upstream
  `v1.3.0` (same cache keys, same bytes; the `ETag` header is the one visible addition). The admin pages
  and the title index are new, but they only add things next to what upstream has.

Related files: [baseline.md](baseline.md) (test counts), [memory.md](memory.md) (memory: what was
measured and decided), [speed.md](speed.md) (latency and the HTTP caching work),
[badge-styles.md](badge-styles.md) (design of the badge looks), [safe-areas.md](safe-areas.md) (backdrop
safe areas), `scripts/louk/README.md` (load harness).

## What the fork adds

| Feature | What it does | Default |
| --- | --- | --- |
| Backdrop cap (`BACKDROP_MAX_WIDTH`) | Caps how wide `large` and `very-large` backdrops are drawn. A 3840 px backdrop uses about 4 times the memory of a 1920 px one, and many TMDB originals are only 1920 px wide and were stretched. Badges scale with the picture. Posters, logos, episode stills and the `small`/`medium` backdrops are not touched. | Unset = exactly upstream. |
| `RATINGS_DISABLED` | For dev and test only. No rating lookups at all, no badges, and the server starts without any rating provider. A local instance or a load test can then never call MDBList or OMDb. | Off. **Never set it in production.** |
| Memory settings | Two glibc variables (`MALLOC_ARENA_MAX`, `MALLOC_MMAP_THRESHOLD_`), no code. With the backdrop cap they cut the peak memory under a catalog burst to about a third. See "Memory". | Not set until production sets them. |
| Title dashboard | A title index in the database and an admin Library: every title with its cached images, searchable, with purge and re-fetch per title. | Always on; changes no image. |
| Per-title overrides | Swap the poster, logo, backdrop or one episode still of a title for another TMDB or fanart.tv image or an upload, and turn its badges off. | Only titles you override change. |
| Badge looks | Three extra values of "Badge background": Score colour, Mono, One score (average). | Off: every key keeps upstream's `Default`. |
| Real-art previews | `?art=movie-<id>` on the admin preview endpoints draws the sample badges on a real title's TMDB image. | API only, nothing changes without `?art=`. |
| Badge gallery | A static page that shows every look on six real titles. [badges.html](badges.html) | Documentation. |
| ETag / 304 (`LOUK_HTTP_ETAG`) | Image responses get an `ETag`; a client that already has the image gets a `304` with no body. | On (adds one header). |
| Stale-while-revalidate (`LOUK_RATINGS_SWR`) | A cached image goes out at once when only its ratings index is stale; the ratings are fetched and the image redrawn in the background. | Off. |
| Backdrop safe-area notes | Where the AIOStreams app crops and darkens backdrops, and which existing settings keep ratings visible. No code. | Documentation. |
| Load harness | Scripts to start a local instance, warm its id caches, fire a catalog-sized burst (150 titles x poster, backdrop, logo, plus 40 episode stills) while recording peak memory and latency, and probe latency one request at a time. Results: [memory.md](memory.md), [speed.md](speed.md). | Not part of the image. |

## Settings

All fork environment variables. Set them in the container's `environment` (`openposterdb/compose.yaml` in
the stacks repo). Full text per variable: [configuration.md](../configuration.md#fork-only-loukdekkernl).

| Variable | Default | Production | What it does |
| --- | --- | --- | --- |
| `BACKDROP_MAX_WIDTH` | unset (upstream widths) | `1920` | Integer 320 to 3840. Larger `imageSize` backdrops render at this width. A bad value logs a warning and is ignored. |
| `RATINGS_DISABLED` | `false` | never set | Dev and test only. `true` stops every rating lookup; a typo stops startup. |
| `LOUK_HTTP_ETAG` | on | leave on | `false`, `0`, `no` or `off` removes the `ETag`/304 layer and restores upstream's headers. |
| `LOUK_RATINGS_SWR` | `false` | off for now | `true` or `1` turns on stale-while-revalidate (see "ETag and stale-while-revalidate"). |
| `MALLOC_ARENA_MAX` | glibc default | `2` | glibc setting, not an app setting. Recommended production environment. |
| `MALLOC_MMAP_THRESHOLD_` | glibc default | `131072` | glibc setting, not an app setting. Recommended production environment. |

The three that production sets (`BACKDROP_MAX_WIDTH`, the two `MALLOC_*` lines) are explained under
"Production setting". The title dashboard, the overrides and the badge looks have no environment variable.

## Where to switch each feature on

| Feature | Where |
| --- | --- |
| Backdrop cap, memory settings, ETag, SWR, `RATINGS_DISABLED` | Container environment (see Settings). |
| Title dashboard | Nothing to switch. Admin sidebar, **Library**. It fills from traffic; right after an upgrade it is sparse (see below). |
| Per-title overrides | Admin, Library, open a title, **Override** next to Posters, Backdrops or Logos (or on an episode's row). "Using the picker" below. |
| Badge looks | Admin, Settings (every key without its own settings) or Admin, API keys, a key's settings, then **Badge background** in the Poster, Logo, Backdrop and Episode boxes. Or `?badge_background=s` (`m`, `a`) on one image URL. |
| Real-art previews | `?art=` on `/api/admin/preview/...` (not in the admin UI). |
| Gallery | Open `docs/louk/badges.html` in a browser. |
| Safe areas | The existing per-key settings `position`, `badge_direction`, `ratings_limit` (see "Backdrop safe areas"). |

## The features in detail

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

### ETag and stale-while-revalidate

Both are speed work (P6); the numbers and the options that were rejected are in [speed.md](speed.md).

- **ETag/304 (`LOUK_HTTP_ETAG`, on).** Poster, logo, backdrop, episode and the `/c/` CDN routes carry a
  strong `ETag` (the first 128 bits of the SHA-256 of the image bytes). A request whose `If-None-Match`
  matches gets `304 Not Modified` with no body. The 200 response is otherwise unchanged. The tag is
  computed once when an image enters the memory cache. AIOMetadata's art proxy already passes
  `If-None-Match` and 304s through, so hourly revalidations cost a few hundred bytes instead of 68 to
  475 KB. Code: `api/src/louk/http_cache.rs`.
- **Stale-while-revalidate (`LOUK_RATINGS_SWR`, off).** When only a title's ratings index is stale (the
  `available_ratings` row is older than its stale interval), upstream makes the request wait for MDBList
  and then usually sends the old picture anyway. With this on, the cached picture goes out at once and
  the same request is replayed in the background (one replay per image, two overall). It makes no extra
  rating call; the fetch only moves off the request path. That one response can carry the previous
  ratings. Ignored with `RATINGS_DISABLED`. Code: `api/src/louk/swr.rs`.
  **Why it is off:** the answer can be older than upstream's, and the background path's real rating fetch
  could not be exercised on the dev box (no rating keys there). Turn it on as a separate change once the
  ETag image has run in production.

### Badge looks, real-art previews and gallery

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

Hooks: `services/db.rs`, `image/badge.rs`, `image/generate.rs`, `handlers/preview.rs` and `RenderSettingsForm.vue` in "Divergence points in upstream files". Rollback: upstream reads a
stored `s`, `m` or `a` as its default (`d`) and logs a warning; images keyed `.bgs`/`.bgm`/`.bga` are
simply never requested again.

### Backdrop safe areas

Evidence and numbers: [safe-areas.md](safe-areas.md), measured from the AIOStreams web app's own CSS (the Android TV, desktop and web apps are one app). Both its Home banner and title page show OpenPosterDB's backdrop.
- Wide screens (TV, desktop, web) crop the bottom of the backdrop and darken its left, so ratings are safest top-right, which is the default (`position=tr`, insets 0). The default is a column of up to 5 badges: on the Home banner its bottom two dim, so use `ratings_limit=3` or `badge_direction=h` (admin UI: "Max ratings" / "Badge direction" in the key's "Backdrop (series/movie)" box).
- On a title page "Background from artwork" is on by default and leaves the top right clear; no change needed.
- A phone held upright shows only the middle third, so top-right badges vanish; only `position=tc` survives, at the cost of dimmer badges on the TV banner. No single setting serves both: use one key per device class.
- No code preset was built: the existing `position`, `edge_inset_x/y`, `badge_direction` and `ratings_limit` settings already express this.
- Open, needs a real-device check: TV overscan, trailers that replace the backdrop on TVs, the Android status bar over the phone banner, and the desktop app's window controls.

## Production setting

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

## Memory: what is measured and what production sets

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
source while they wait for a render permit is not bounded (that is the next lever, see "Not done / open items").
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

Every upstream file the fork edits. These are the places a sync can conflict. Each hook is one line (or a
few) tagged `louk-fork:`; do not keep line numbers here, find them with:

```sh
grep -rn "louk-fork:" api web .github
```

(129 hooks in `api/src` and `web/src` on the integrated branch, 6 more in `.github`; the markers inside
fork-only files under `api/src/louk/` and `web/src/louk/` are not divergence.)

Conflict risk on a sync: **low** = upstream's own line is untouched (the fork only added a line), so a
conflict needs upstream to edit the same neighbourhood. **medium** = a hook replaces or wraps upstream
lines, so a change by upstream to those lines conflicts. **high** = the whole file differs.

| File | Hooks | What they do | Risk |
| --- | --- | --- | --- |
| `api/src/lib.rs` | 2 | `pub mod louk;`; `AppState` gets one field `louk: Arc<LoukState>`. | low |
| `api/src/main.rs` | 4 | Runs `louk::schema::apply` after the `MIGRATIONS` loop; `louk` in the `AppState` literal; starts the title index workers; the `image_mem_cache` weigher calls `http_cache::note_insert`. | low |
| `api/src/config.rs` | 6 | `Config` field `louk: LoukConfig` and its `Debug` and `from_env` lines; test helper names; marker for the fork's unit tests; one early return for `RATINGS_DISABLED` just above upstream's "need one ratings source" check. | low, one medium (the early return) |
| `api/src/routes/admin.rs` | 1 | Merges `louk::routes::louk_admin_routes()` into the admin routes (admin auth applies). | low |
| `api/src/routes/app.rs` | 2 | `image_routes()` and `cdn_routes()` are wrapped in `louk::http_cache::layer`. | low |
| `api/src/handlers/admin.rs` | 1 | `purge_title` made `pub(crate)`. | low (same line as the signature) |
| `api/src/handlers/image.rs` | 2 | `try_cdn_redirect`: capped backdrops get their own `/c/<hash>/`; one doc-comment line on `badge_background` naming the fork values. | low |
| `api/src/handlers/preview.rs` | 30 | `preview_backdrop` width and badge scale from `render_dims` and the capped cache key; in all four preview handlers the `?art=` extractor, the `.art...` cache-key token, fork-look sample badges, the sample-image line replaced by `preview_art::source`, and the preview-cache inserts skipping art previews. | medium |
| `api/src/image/serve.rs` | 35 | `RATINGS_DISABLED` guards (4); title index notes in `resolve_with_ratings`, the three handlers (`note_resolved`, `note_cross_ids`, `touch`); backdrop cap (`render_dims`, `tmdb_size`, the `.mw<N>` token, 6 places); override hooks (`badge_limit_hint`, `badge_limit`, `strip_badges`, `render_poster`, `render_episode`, `source_bytes`, `fetch_fanart_images` made `pub(crate)`); ETag (`note_key` in `check_caches`); SWR (`fast_path_sources` and `revalidate` in the three fast paths). | low, several medium (hooks that replace upstream lines: the cap arms and the `if fanart_is_primary` lines) |
| `api/src/image/badge.rs` | 3 | `section_colors` arm for the new looks; first line of `render_badges_uniform` and `render_vertical_badge` hands fork looks to `louk::badge_looks`. | low |
| `api/src/image/generate.rs` | 4 | One line after `for_shape` in each `render_*_sync`: `badge_looks::collapse` (the average look). | low |
| `api/src/services/db.rs` | 11 | `BadgeBackground` gets `Score`, `Mono`, `Average` (tokens `s`, `m`, `a`), their `as_str`/`parse` arms and a longer error message. | medium (an upstream background value would touch the same lines) |
| `api/tests/common/mod.rs` | 9 | `TestAppOptions.louk`, the extra `louk` argument of `_setup_test_app`, `louk` in the `Config` and `AppState` literals, the fork schema, the weigher hook. | medium (long upstream lines edited in place) |
| `web/src/router/index.ts` | 2 | Three child routes of `/admin` between a `louk-fork:` and a `louk-fork: end` comment. | low |
| `web/src/components/AppSidebar.vue` | 2 | The Library item; `isActive` keeps it highlighted on title pages. | low, one medium |
| `web/src/components/ImageListView.vue` | 2 | `LibraryBig` import; a link icon per row to the title page. | low, one medium |
| `web/src/components/RenderSettingsForm.vue` | 9 | Imports `badgeLooks.ts`; the four "Badge background" selects list the fork looks. | low |
| `.github/workflows/test.yml` | 4 | Runs on pushes to every branch with no path filter; three jobs pinned to `ubuntu-24.04` (`ubuntu-latest` is 26.04 and Playwright 1.60 cannot install Chromium there). | medium. If upstream fixes the pin, take upstream's. |
| `.github/workflows/release.yml` | 2 | The whole file is rewritten: one amd64 job, ghcr only, no Docker Hub, no arm64, no manifest-merge job, runner pinned. | **high** (keep ours on every sync) |

Also changed, without a marker (documentation, easy to merge): `docs/configuration.md` (a fork section at
the end, with the recommended production environment and the badge looks), `docs/api.md` (a paragraph under
the backdrop sizes, a sub-bullet under `badge_background`, and the sections "Title dashboard", "Per-title
overrides" and "Admin previews on real artwork" at the end), `docs/architecture.md` (the badge looks in the
cache-token and parameter tables).

`Cargo.toml` and `Cargo.lock` are unchanged: the app still reports version `1.3.0`. The git tag carries
the fork version. That keeps the most conflict-prone files out of the diff.

### Files upstream does not have

| Path | What |
| --- | --- |
| `api/src/louk/mod.rs` | Fork module root. |
| `api/src/louk/config.rs` | `LoukConfig`: reads `RATINGS_DISABLED`, `BACKDROP_MAX_WIDTH`, `LOUK_HTTP_ETAG`, `LOUK_RATINGS_SWR`. |
| `api/src/louk/backdrop_cap.rs` | All the cap logic (size plan, cache-key token, CDN hash, TMDB size) and its unit tests. |
| `api/src/louk/http_cache.rs`, `swr.rs` | ETag/304 middleware, weigher and `check_caches` hooks, tag side map; stale-while-revalidate (stale-row lookup, deduplicated background replay, counters). |
| `api/src/louk/schema.rs`, `state.rs` | Fork tables (`louk_titles`, `louk_title_aliases`, `louk_episodes`, `louk_overrides`), additive only; `LoukState`, the one `AppState` field. |
| `api/src/louk/titles.rs`, `handlers.rs`, `routes.rs` | Title index (hooks, flush, details and backfill workers) and the title dashboard admin endpoints. |
| `api/src/louk/overrides/` | Per-title overrides: `store.rs`, `uploads.rs`, `candidates.rs`, `apply.rs` (render-path hooks), `purge.rs`, `handlers.rs`. |
| `api/src/louk/badge_looks.rs`, `badge_looks/tests.rs` | The three badge looks (drawing, score scale, average, preview samples) and unit tests, including `louk_defaults_do_not_change_cache_keys`. |
| `api/src/louk/preview_art.rs` | `?art=` for the admin previews: parsing, cache token, TMDB fetch, JSON errors, unit tests. |
| `api/tests/louk_*.rs` | Route tests: `louk_backdrop_cap`, `louk_titles` (including the rollback check `upstream_schema_opens_fork_db`), `louk_overrides_api`, `louk_overrides_render`, `louk_http_cache`, `louk_swr`, `louk_badge_looks`. |
| `web/src/louk/**` | Frontend: `api.ts`, `format.ts`, `overrides.ts`, `badgeLooks.ts`, views (`LibraryView`, `TitleView`, `AliasRedirect`), components (`TitleCard`, `ImageTile`, `SeasonList`, `SegmentedControl`, `OverridePicker`). |
| `web/src/__tests__/louk/**`, `web/src/louk/__tests__/` | Frontend unit tests of the above. |
| `docs/louk/` | This file, `baseline.md`, `memory.md`, `speed.md`, `badge-styles.md`, `safe-areas.md` with `safe-areas/`, `badges.html` with `badges/`. |
| `scripts/louk/` | Load harness (`burst.py` with its `--warmup` pass, `dev-instance.sh`, `run-variant.sh`, `results-table.py`, `fetch-titles.py`, `titles.json`, `speed-probe.py`, `selftest.sh`, README), the gallery tools (`gallery.py`, `gallery-shots.js`) and `sync-upstream.sh`. |

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

Production pins the full tag (for example `ghcr.io/l0unk/openposterdb:v1.3.0-louk.2`), never `latest`: the release workflow moves `latest` and `v1` on every build.
`Cargo.toml` stays at `1.3.0`, so the running app says `1.3.0`; the image tag tells which fork build it is.

## Backups

Everything stateful is in `./db`, which Backrest backs up: SQLite (API keys and settings), and the folder
`db/uploads/` (uploaded override images). The fork adds four tables there (`louk_titles`,
`louk_title_aliases`, `louk_episodes`, `louk_overrides`). The first three can be rebuilt from traffic and the
backfill; losing them only empties the library for a while. `louk_overrides` and `db/uploads/` hold your
choices and cannot be rebuilt, so restore `./db` as one piece. Badge looks are stored in the existing
settings tables and need nothing extra.

The `cache` volume holds the rendered images. It is not backed up and does not need to be: every file is
made again on request.

## Going back to upstream (rollback)

1. Set the image line in `openposterdb/compose.yaml` back to `ghcr.io/pnrxa/openposterdb:latest` (or
   the pinned `v1.3.0`).
2. Remove `BACKDROP_MAX_WIDTH`, `LOUK_HTTP_ETAG` and `LOUK_RATINGS_SWR` from the environment (upstream
   ignores them anyway). The two `MALLOC_*` lines can stay: they are glibc settings and help upstream's image too.
3. Deploy.

Why it is safe:

- The fork's only schema change is four new tables with a `louk_` prefix (title dashboard and
  `louk_overrides`) plus the `db/uploads/` folder. Upstream
  does not know them and leaves them alone; no upstream table or column changes. Nothing in `./db`
  needs restoring for this rollback.
- A stored badge look (`s`, `m`, `a`) is read by upstream as its default (`d`) with a warning in the log; overrides stop applying (the art goes back to upstream's automatic pick) and their table and uploads stay unused in `./db`.
- The `.mw1920` cache files are new names that upstream never builds. It ignores them. They only take
  disk space until you purge or clear the cache.
- After a rollback, large backdrops come from the old 3840 px files if they are still on disk. If they
  were purged, upstream draws them again at 3840 px, with the old memory cost.
- The test `upstream_schema_opens_fork_db` runs upstream's startup schema and migrations again on a
  fork-migrated database with rows in the fork tables, then upstream queries and writes; it passes.
  Not yet verified with a real upstream binary against a copy of the production database.

## How this release was tested

`v1.3.0-louk.2` was built from `main` (tree identical to `louk/next` @4015b4e, the commit of the last
release candidate `rc.4`). The compile and test loop ran on GitHub Actions (backend 558 lib tests, 993 cargo
tests in all suites, vitest 394+, e2e 141; see [baseline.md](baseline.md)); nothing was built locally. On top of
that, the release-candidate binaries (taken out of the ghcr images, no docker) ran on the dev box as real
instances with `RATINGS_DISABLED=true`, on real TMDB data. Two smoke tests, reports in the dev box's
`/home/dev/opdb-fork/reports/` (`smoke-rc3-api.md`, `smoke-rc3-ui.md`), not in this repo:

- **API / server (rc.3): passed.** Louk's four URL patterns (posters, `imageSize=large` backdrops at
  1920x1080, logos, episode stills), byte-identical repeat requests; strong `ETag` equal to the body hash,
  `If-None-Match` answers `304` without a body, `LOUK_HTTP_ETAG=false` removes the header and the bodies stay
  identical; the title index (search, filters, show detail with seasons, purge); per-title overrides on
  The Man in the High Castle (logo set and cleared on the tmdb, imdb and tvdb routes, the automatic pick
  marked correctly, uploads and all rejections, delayed re-purge after 60 s) and on a movie and an episode
  still; the `?art=` previews of the three looks.
- **Memory burst (rc.3, 490 requests at once, production settings): peak 705 MB** (VmHWM), idle 279 MB,
  p95 12.6 s. P1.4 had measured 744 to 785 MB with the same three settings; upstream with none of them
  peaked at about 2.4 GB. 8 of the 490 requests answer `500`: the known logos that cannot be decoded.
- **Rollback check:** the upstream `v1.3.0` binary (also taken from its ghcr image) opened a copy of the
  database the fork had migrated: login, keys, settings and the cache lists work, and an unknown badge look
  (`s`, `m`, `a`) reads as the default. This proves going back to the upstream image needs no restore.
- **Admin UI in a real headless browser (rc.3, then rc.4): the first run failed on UI details, the re-check
  passed.** Found on rc.3 and fixed in rc.4: the picker was unusable at 412 px (buttons off-screen), no
  lasting "override active" marker, an empty row after saving, no rating note in the purge dialog, a dark
  checkerboard that hid black logos. rc.4 re-checked on desktop and phone width: picking, clearing and
  uploading end to end, chips that survive a reload, merged alias tiles, keyboard use of the picker, no
  console errors.

**What was not tested:**

- **Live rating APIs.** Every run used `RATINGS_DISABLED=true`, no MDBList, OMDb or Trakt key. Badges on
  real ratings, the "no badges" override against real scores, the stale-while-revalidate background fetch
  and the ETag image under real traffic ran only in unit tests and with sample scores.
- **The docker image's entrypoint and healthcheck.** Only the binary was extracted and run; the image
  itself never ran (no docker on the dev box). Whether the entrypoint's `su` passes the two `MALLOC_*`
  variables to the process is checked by the deploy script, not before.
- **Real devices.** No TV, phone or desktop app looked at a badge look or at a backdrop with a badge;
  the badge gallery and the headless browser only simulate them.
- The title dashboard and overrides on a copy of the production database (the rollback check used a
  development database), and the upstream PR branch on a real instance.

## Not done / open items

True for the integrated branch `louk/next`. Nothing here is listed as "not started" if it is built.

| Item | Where it stands | Next step |
| --- | --- | --- |
| **Upstream PR (per-title overrides)** | Prepared on the branch `pr/per-title-overrides` in the dev-box clone (cut from upstream `v1.3.0`, CI checked on `louk/pr-ci`). **Not opened** on github.com/PNRxA. | Louk reads it and opens the PR himself. |
| **Frosted-glass badge look** | Not built. Upstream draws every badge as a separate sprite before it knows where it lands, so a blur behind it is a bigger change (see [badge-styles.md](badge-styles.md)). | Only if the three looks are not enough after real use. |
| **Safe-area preset in code** | **Not built, by decision.** The existing settings already express a safe area. | Louk sets one or two fields per key by hand (see "Backdrop safe areas"). |
| **Stale-while-revalidate** | Built, **off by default**: that one response can be older than upstream's, and its background rating fetch has never run against live rating APIs. | Run the ETag image in production first, then turn it on as its own stacks change. |
| **Failed renders are not cached** | A title whose TMDB data is missing or whose source image cannot be decoded answers `500` every time and calls TMDB again each time. Upstream does the same; production logged 31 such episode requests in 4 days. | Cache the failure briefly if it ever matters (see [speed.md](speed.md)). |
| **Backdrop cap range 1281 to 1919** | With `BACKDROP_MAX_WIDTH` in that range a `medium` backdrop would fetch TMDB's `original` instead of `w780`: more memory, the opposite of the aim. Caps up to 1280 fetch `w1280` instead of `w780`. Production uses 1920, where `small` and `medium` are not capped. | If another value is ever needed, never fetch a larger TMDB size than upstream does for that `imageSize` (one-line change in `reports/P1.2-review.md`, SHOULD-FIX 1). |
| **Real-device checks** | Not done: TV overscan, trailers that replace the backdrop on TVs, the Android status bar over the phone banner, the desktop app's window controls; and how the badge looks read on a real TV and phone (the gallery only simulates the app's shades). | Louk looks at a key set to a look, on his devices. |
| **Uploads are never cleaned up** | Files in `db/uploads/` stay after an override is changed or cleared. | Delete by hand, or build a sweep if it grows. |
| **Never run against live rating APIs** | Every dev and test run used `RATINGS_DISABLED=true`. Badges on real ratings, the "no badges" override, the SWR background fetch and the ETag image under real traffic have run only in tests and with sample scores. The title dashboard and overrides were not run on a copy of the production database either (the rollback check is a test). | First production hours: watch `openposterdb-mdblist-watch` and the container memory. |
| **Next memory lever** | Not built. If production still gets killed at 1536m after the three settings, bound how many requests hold a downloaded source while they wait for a render permit (about 2 x `RENDER_CONCURRENCY`). Measured at -80 to -140 MB (10 to 15%), but it touches the fetch path of the four image kinds in `generate.rs`. Details in [memory.md](memory.md). | Only if needed. |
| **Small UI nice-to-haves** (left from the rc.4 re-check) | The "Override active" chip says only "TMDB": language, size and "no badges" would help (`web/src/louk/views/TitleView.vue`, `overrideLabel`). A section header counts Logos 2 while the merged alias tile shows one. The "Served now" caption is cut off on a phone (`OverridePicker.vue`, allow two lines). The look descriptions only appear after selecting one, not inside the dropdown (`RenderSettingsForm.vue`). None blocks use. | Fix together in one small change if any of them annoys. |
| **Figures on a phone grid stay small** | In the gallery's 412 px phone grid the figures of `s` are about 6 to 7 px and of `a` about 6 px: a render cannot know the app's card size. Not fixable in the render; the advice is under "Phone-first keys" above. | For a key used mainly on phones: Badge background `a`, Badge size `xl`, Max ratings 1 or 2; keep the row looks (`s`) for the TV key. |
| MDBList quota view | Not built. | Only if the MDBList client already reads usage headers. |
| Badge looks in the free-key form | `web/src/components/FreeApiKeyCard.vue` does not list `s`/`m`/`a`; the server accepts them. Production runs `FREE_KEY_ENABLED=false`. | Add the same `v-for` as in `RenderSettingsForm.vue` if free keys are ever enabled. |

Spoiler blur for episode stills already exists upstream per key (`episode_blur`); the fork adds nothing there.

### Known limits

- **Rate limit.** Upstream limits image requests to a burst of 240, then 1 per 200 ms (5 a second), per API
  key and client address. In production AIOMetadata is the only caller, so one key behaves as one caller: a
  catalog that opens more than about 240 images at once can get some `429` answers even today. The fork does
  not change this.
- **Sharper than upstream medium.** A capped `large` backdrop is fetched from TMDB `original`, so in
  production it is sharper than upstream's `medium` (a `w780` source). Intended.
- **Route tests.** The cap's slow path, generation closure, background refresh and TMDB size hooks have no
  route-level test (the crate has no TMDB mock). They are covered by unit tests and by review. The same
  holds for the `RATINGS_DISABLED` hooks in `serve.rs`; the guarantee that no rating clients are built is
  tested at the config level.
- **Startup line hidden.** See "Production setting": `RUST_LOG=warn` hides the INFO line.
- **Old titles never redraw.** Titles older than a year are never re-rated (a production setting). Purging
  one title in the admin cache page redraws only that title.
- **Dev box.** Never run a cargo or frontend build next to another heavy job; the shared memory limit is
  6 GB soft, 8 GB hard. Use GitHub Actions.
