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

Related files: [baseline.md](baseline.md) (test counts), [memory.md](memory.md) (memory work, not
measured yet), [safe-areas.md](safe-areas.md) (backdrop safe areas), `scripts/louk/README.md` (load
harness).

## What it adds today

Two settings, the title dashboard backend, a load harness and some documentation.

| Feature | What it does | How to turn it on | Default |
| --- | --- | --- | --- |
| `BACKDROP_MAX_WIDTH` | Caps how wide `large` and `very-large` backdrops are drawn. A 3840 px backdrop uses about 4 times the memory of a 1920 px one, and many TMDB originals are only 1920 px wide and were being stretched. Badges scale with the picture. Posters, logos, episode stills and the `small`/`medium` backdrops are not touched. | Set the env var to a number from 320 to 3840, e.g. `BACKDROP_MAX_WIDTH=1920`. | Unset = exactly upstream. A bad value (e.g. `1920px`) is ignored with a warning, so you get upstream's 3840 again. |
| `RATINGS_DISABLED` | For dev and test only. No rating lookups at all, no badges, and the server starts without any rating provider. It guarantees a local instance or a load test can never call MDBList or OMDb. | `RATINGS_DISABLED=true`. A typo stops the server at startup instead of silently spending quota. | Off. **Never set it in production.** |
| Load harness | Scripts to start a local instance and fire a catalog-sized burst (150 titles x poster, backdrop, logo, plus 40 episode stills) while recording peak memory and latency. Meant to measure memory. | `scripts/louk/README.md`. | Not part of the image. |
| Backdrop safe-area notes | Where the AIOStreams app crops and darkens backdrops, and which existing settings keep ratings visible. No code. | See "Backdrop safe areas" below. | Nothing changes by itself. |

Both settings are documented in [configuration.md](../configuration.md#fork-only-loukdekkernl).

### Title dashboard (backend)

The admin dashboard only knew cache keys. The fork now keeps a title index in the SQLite database:
every id an image was requested by (imdb, tmdb, tvdb, episode ids) and the title it belongs to, with
name, year and TMDB poster. Admin endpoints under `/api/admin/louk/titles` list titles (search, filter,
sort), show one title with all its cached images (shows: per season and episode, with TMDB episode
names), find the title of an id, purge a title under all its ids, and re-fetch it. The contract is in
[api.md](../api.md#title-dashboard-fork-only-admin). The Library and title pages (branch `louk/p2-2`)
use it.

- Code: `api/src/louk/{titles,handlers,routes,schema,state}.rs`; tests `api/tests/louk_titles.rs`.
- On the image path the fork only notes the id in memory (no database, no network, no waiting). A
  background task writes the notes every 30 s; up to 20 000 pending ids, beyond that new ones are
  dropped until the next write (the backfill picks them up later).
- Background TMDB calls: names/years for new titles, the backfill of old cache entries (50 every 10
  minutes, first run 5 minutes after start) and episode names when a show's page is opened (kept 7
  days). One call at a time, at least 250 ms apart. Never OMDb, MDBList or Trakt.
- Always on; it changes no image, cache key or response. Tables: `louk_titles`,
  `louk_title_aliases`, `louk_episodes`.

### Per-title overrides

Swap the art of one title (poster, logo, backdrop, or one episode still) for any other TMDB image
(all languages), a fanart.tv image or an uploaded file, and optionally turn its rating badges off.
This fixes cases like *The Man in the High Castle*, where upstream picks the last of three equal
English logos. The API is in [api.md](../api.md#per-title-overrides-fork-only-admin); the picker UI
on the title page is a separate task (P3.3).

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
  requested seconds ago is included. `preview_cache` is not touched: it only holds the settings
  previews drawn from a bundled sample image, never title art. CDN copies cannot be purged from here
  (production has the CDN off).
- **Uploads:** `POST /api/admin/louk/uploads` with the file as the raw body. PNG, JPEG or WebP by
  magic bytes (content type and name are ignored), at most 15 MiB (enforced while reading), at most
  8000 px per side (checked from the header before decoding), fully decoded under decoder limits.
  Stored as `db/uploads/<sha256>.<png|jpg|webp>`, written to a temp file and renamed; only 64-char hex
  names are ever used as paths. Served back only through the admin route
  `/api/admin/louk/uploads/<sha256>`. Uploads are never deleted automatically.
- **Where it lives:** table `louk_overrides` and `db/uploads/`, both inside `./db`, so Backrest backs
  them up. Code `api/src/louk/overrides/`; tests `api/tests/louk_overrides_{api,render}.rs`.
- **Upstream PR seam:** only `overrides::purge::alias_targets` reads the title index
  (`louk::titles`); a port without the index replaces that one function.

### Production setting

Production sets one thing: `BACKDROP_MAX_WIDTH=1920`. Louk approved the 1920 px cap on 2026-11-25.
Louk's AIOMetadata URLs (`?imageSize=large`) do not change.

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

## Divergence points in upstream files

Every place where the fork edits a file upstream also has. These are the places a sync can conflict.
Line numbers are at `v1.3.0-louk.1` and drift; search for `louk-fork:`.

To regenerate the list:
`grep -rn "louk-fork:" api web .github scripts docs --include='*' | grep -v '^docs/louk/'` (43 lines
with the title dashboard backend).

Conflict risk on a sync: **low** = upstream's own line is untouched (the fork only added a line), so a
conflict needs upstream to edit the same neighbourhood. **medium** = the hook replaces or wraps upstream
lines, so a change by upstream to those lines conflicts. **high** = the whole file differs.

| File | What the hook does | Conflict risk |
| --- | --- | --- |
| `api/src/lib.rs:8` | `pub mod louk;` declares the fork module root. | low |
| `api/src/lib.rs:69` | `AppState` gets one field `louk: Arc<LoukState>` (title index pending maps, TMDB pacing). | low |
| `api/src/main.rs:161` | Runs the fork schema (`louk::schema::apply`) right after upstream's `MIGRATIONS` loop. | low |
| `api/src/main.rs:341` | `louk` in the `AppState` literal. | low |
| `api/src/main.rs:365` | Starts the title index workers (`louk::titles::spawn_workers`) after the `last_used_at` flush task. | low |
| `api/src/routes/admin.rs:35` | Merges `louk::routes::louk_admin_routes()` into `admin_routes()`, so the fork routes get admin auth. | low |
| `api/src/handlers/admin.rs:600` | `purge_title` made `pub(crate)` so the title purge reuses it. | low (same line as upstream's signature) |
| `api/src/config.rs:24` | `Config` gets one field `louk: LoukConfig`, which carries all fork settings. | low |
| `api/src/config.rs:49` | `Debug` impl prints that field. | low |
| `api/src/config.rs:114` | `from_env` fills the field (`LoukConfig::from_env()`). | low |
| `api/src/config.rs:116` | One early-return line: with `RATINGS_DISABLED` it drops the OMDb/MDBList/Trakt keys and returns before upstream's "need one ratings source" panic. | medium (sits just above upstream's check) |
| `api/src/config.rs:162` | Two names added to the test helper `clear_config_env`. | low |
| `api/src/config.rs:326` | Marker comment for the fork's `RATINGS_DISABLED` unit tests, appended at the end of `mod tests`. | low (end of file) |
| `api/src/image/serve.rs:397` | First line of `read_available_ratings_cached`: with `RATINGS_DISABLED`, never build fast-path keys from rating sources an enabled run recorded. | low |
| `api/src/image/serve.rs:420` | First line of `upsert_available_ratings_cached`: with `RATINGS_DISABLED`, do not overwrite the `available_ratings` index with an empty list. | low |
| `api/src/image/serve.rs:449` | First line of `resolve_with_ratings`: `RATINGS_DISABLED` forces `skip_ratings` on every path. | low |
| `api/src/image/serve.rs:452` | `resolve_with_ratings`, after `id::resolve`: `louk::titles::note_resolved` notes the id and what it resolved to (before the episode uplift, so episode ids keep season/episode). In-memory only. | low |
| `api/src/image/serve.rs:460` | Inside the episode uplift: `note_cross_ids` notes the show ids upstream also caches the render under. | low |
| `api/src/image/serve.rs:713` | `handle_inner` (posters), after `validate_id_value`: `louk::titles::touch` notes the request time. In-memory only. | low |
| `api/src/image/serve.rs:1706` | `handle_episode_inner`: same `touch`. | low |
| `api/src/image/serve.rs:1824` | `handle_logo_backdrop_inner`: same `touch`. | low |
| `api/src/image/serve.rs:1208` | `trigger_logo_backdrop_refresh`: the backdrop arm of the `(width, badge scale)` match calls `backdrop_cap::render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:1649` | `try_tmdb_logo_backdrop`: the TMDB size comes from `backdrop_cap::tmdb_size`. Replaces 1 upstream line. | medium |
| `api/src/image/serve.rs:1880` | Fast path of `handle_logo_backdrop_inner`: one added line splices the `.mw<N>` token into the cache suffix. | low |
| `api/src/image/serve.rs:1919` | Slow path of the same function: same added line. | low |
| `api/src/image/serve.rs:2058` | Generation closure: the backdrop arm calls `render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:718` | `handle_inner`, before the fast path: `overrides::apply::badge_limit_hint` sets the poster ratings limit to 0 for a "no badges" override (only when such a row exists, and only from `id_cache`). | low |
| `api/src/image/serve.rs:791,792` | `handle_inner`, after `resolve_with_ratings`: the same with the resolved title (`badge_limit`), and `use_fanart` is shadowed to false when an override replaces the art, so the fanart branch is skipped. | low |
| `api/src/image/serve.rs:1159` | `trigger_logo_backdrop_refresh`: `strip_badges` after the badges are computed (the refresh gets the unmodified settings). | low |
| `api/src/image/serve.rs:1166,1167` | `trigger_logo_backdrop_refresh`: `source_bytes` of the override; the `if fanart_is_primary` line becomes `if let Some(b) = louk_bytes { b } else if fanart_is_primary`. | medium (edits an upstream line) |
| `api/src/image/serve.rs:1231` | First line of `generate_episode`: `render_episode` draws the still from the override and returns early. | low |
| `api/src/image/serve.rs:1327` | First line of `generate_poster_with_source`: `render_poster` draws the poster from the override and returns early (before the TMDB-vs-fanart chain and the "no poster_path" error). | low |
| `api/src/image/serve.rs:1455` | `fetch_fanart_images` made `pub(crate)` for the override candidates. | low (same line as upstream's signature) |
| `api/src/image/serve.rs:1711,1761` | `handle_episode_inner`: `badge_limit_hint` before the fast path, `badge_limit` after the episode check. | low |
| `api/src/image/serve.rs:1833,1904` | `handle_logo_backdrop_inner`: the same two lines, shadowing `type_ratings_limit`. | low |
| `api/src/image/serve.rs:2000,2001` | Logo/backdrop generation closure: `source_bytes`; the `let (primary, fallback) = if fanart_is_primary` line becomes `if let Some(b) = louk_bytes { (Some(b), None) } else if fanart_is_primary`. | medium (edits an upstream line) |
| `api/src/handlers/image.rs:528` | `try_cdn_redirect`: one added line gives capped backdrops their own `/c/<hash>/` URL. | low |
| `api/src/handlers/preview.rs:306` | `preview_backdrop`: width and badge scale from `render_dims`. Replaces 2 upstream lines with 1. | medium |
| `api/src/handlers/preview.rs:327` | `preview_backdrop`: one added line for the capped preview cache key. | low |
| `api/tests/common/mod.rs:22,35` | `TestAppOptions` gets a `louk` field and its default. | low |
| `api/tests/common/mod.rs:48,52,55` | `_setup_test_app` takes one extra argument `louk`; two callers pass it. These are long upstream lines edited in place. | medium |
| `api/tests/common/mod.rs:136` | `louk` in the test `Config` literal. | low |
| `api/tests/common/mod.rs:79` | Runs the fork schema after upstream's `MIGRATIONS` loop. | low |
| `api/tests/common/mod.rs:193` | `louk` in the test `AppState` literal. | low |
| `.github/workflows/test.yml:7` | Runs on pushes to every branch, no `paths:` filter (upstream: `main` only, and only when `api/`, `web/` or the workflow change). | medium |
| `.github/workflows/test.yml:20,37,58` | Three jobs pinned to `ubuntu-24.04`. `ubuntu-latest` is now 26.04 and Playwright 1.60 cannot install Chromium there. | medium. If upstream fixes this (new Playwright), take upstream's version and drop the pin. |
| `.github/workflows/release.yml:6,10` | The whole file is rewritten: one amd64 job, ghcr only, no Docker Hub, no arm64, no manifest-merge job, runner pinned. | **high** (keep ours on every sync) |

Also changed, without a marker (documentation, easy to merge):

| File | Change |
| --- | --- |
| `docs/configuration.md` | A "Fork-only (loukdekker.nl)" section appended at the end. |
| `docs/api.md` | One paragraph under the backdrop sizes about `BACKDROP_MAX_WIDTH`; "Title dashboard (fork-only, admin)" and "Per-title overrides (fork-only, admin)" sections appended at the end. |

`Cargo.toml` and `Cargo.lock` are unchanged: the app still reports version `1.3.0`. The git tag
carries the fork version. That keeps the most conflict-prone files out of the diff.

### Files upstream does not have

| Path | What |
| --- | --- |
| `api/src/louk/mod.rs` | Fork module root. |
| `api/src/louk/config.rs` | `LoukConfig`, reads `RATINGS_DISABLED` and `BACKDROP_MAX_WIDTH`. |
| `api/src/louk/backdrop_cap.rs` | All the cap logic (size plan, cache-key token, CDN hash, TMDB size) and its unit tests. |
| `api/tests/louk_backdrop_cap.rs` | Route-level tests for the cap. |
| `api/src/louk/schema.rs` | Fork tables (`louk_titles`, `louk_title_aliases`, `louk_episodes`), additive only. |
| `api/src/louk/state.rs` | `LoukState`, the one `AppState` field: pending maps of the image-path hooks, TMDB pacing. |
| `api/src/louk/titles.rs` | Title index: hooks, flush, details/backfill workers, queries shared with P3 (`aliases_for_title`). |
| `api/src/louk/handlers.rs`, `routes.rs` | Title dashboard admin endpoints. |
| `api/tests/louk_titles.rs` | Tests for the index, the endpoints and the rollback check (`upstream_schema_opens_fork_db`). |
| `api/src/louk/overrides/` | Per-title overrides: `store.rs` (table + in-memory map), `uploads.rs`, `candidates.rs`, `apply.rs` (render-path hooks), `purge.rs`, `handlers.rs`. The table SQL is listed in `louk/schema.rs`; the store hangs off `LoukState`. |
| `api/tests/louk_overrides_api.rs`, `louk_overrides_render.rs` | Override API, upload and store tests; render-path and purge tests. |
| `docs/louk/` | This file, `baseline.md`, `memory.md`, `safe-areas.md` and its `safe-areas/` evidence. |
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
2. Remove `BACKDROP_MAX_WIDTH` from the environment (upstream ignores it anyway).
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
| **Title dashboard (P2)** | The frontend (library and title pages) is on branch `louk/p2-2`. The backend (title index, admin endpoints, rollback test) is on branch `louk/p2-1`. Not merged, not in any release. | Merge both after a local smoke run against a real instance (check the library fills, names arrive, a show page lists seasons). |
| **Per-title overrides and uploads, and the upstream PR (P3)** | Backend done on branch `louk/p3` (store, uploads, candidates, render hooks, purge; see "Per-title overrides"). Not merged, not in any release, not yet run against a real instance. No picker UI yet, and no upstream PR branch. | Picker on the title page (P3.3); a smoke run (set the High Castle logo, check all three ids); then `pr/per-title-overrides` from upstream `main` (P3.4). |
| **New badge styles (P4)** | Not started. | Design 3 to 5 styles, each an option next to the existing `badge_style`, default unchanged. |
| **Speed and precache (P6)** | Not started; no latency measurements yet. Ideas on the list: prerender siblings, stale-while-revalidate, keep a hot set warm, `Cache-Control` and `ETag`, cheap disk hits. | Measure cold and warm latency first, then pick. Needs the memory work done first. |
| **Safe-area preset in code (P5)** | **Documentation only, by decision.** The existing settings already express a safe area, so no preset and no crop overlay was built. | Louk sets one or two fields per key by hand (see "Backdrop safe areas"). |
| MDBList quota view | Not built. | Only if the MDBList client already reads usage headers. |

Spoiler blur for episode stills already exists upstream per key (`episode_blur`); the fork adds
nothing there.

Because the dashboard, overrides and badge styles are not built, there is nothing new in the
dashboard to turn on. The only per-key looks to set are the existing backdrop settings named under
"Backdrop safe areas". The deploy switches on no new looks.

## Known limits and open items

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
