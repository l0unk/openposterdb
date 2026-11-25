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
| Load harness | Scripts to start a local instance, warm its id caches, and fire a catalog-sized burst (150 titles x poster, backdrop, logo, plus 40 episode stills) while recording peak memory and latency. `run-variant.sh` does one variant in one command. Its results are in [memory.md](memory.md). | `scripts/louk/README.md`. | Not part of the image. |
| Backdrop safe-area notes | Where the AIOStreams app crops and darkens backdrops, and which existing settings keep ratings visible. No code. | See "Backdrop safe areas" below. | Nothing changes by itself. |

Both settings are documented in [configuration.md](../configuration.md#fork-only-loukdekkernl).

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
`grep -rn "louk-fork:" api web .github scripts docs --include='*' | grep -v '^docs/louk/'` (30 lines
today).

Conflict risk on a sync: **low** = upstream's own line is untouched (the fork only added a line), so a
conflict needs upstream to edit the same neighbourhood. **medium** = the hook replaces or wraps upstream
lines, so a change by upstream to those lines conflicts. **high** = the whole file differs.

| File | What the hook does | Conflict risk |
| --- | --- | --- |
| `api/src/lib.rs:8` | `pub mod louk;` declares the fork module root. | low |
| `api/src/config.rs:24` | `Config` gets one field `louk: LoukConfig`, which carries all fork settings. | low |
| `api/src/config.rs:49` | `Debug` impl prints that field. | low |
| `api/src/config.rs:114` | `from_env` fills the field (`LoukConfig::from_env()`). | low |
| `api/src/config.rs:116` | One early-return line: with `RATINGS_DISABLED` it drops the OMDb/MDBList/Trakt keys and returns before upstream's "need one ratings source" panic. | medium (sits just above upstream's check) |
| `api/src/config.rs:162` | Two names added to the test helper `clear_config_env`. | low |
| `api/src/config.rs:326` | Marker comment for the fork's `RATINGS_DISABLED` unit tests, appended at the end of `mod tests`. | low (end of file) |
| `api/src/image/serve.rs:397` | First line of `read_available_ratings_cached`: with `RATINGS_DISABLED`, never build fast-path keys from rating sources an enabled run recorded. | low |
| `api/src/image/serve.rs:420` | First line of `upsert_available_ratings_cached`: with `RATINGS_DISABLED`, do not overwrite the `available_ratings` index with an empty list. | low |
| `api/src/image/serve.rs:449` | First line of `resolve_with_ratings`: `RATINGS_DISABLED` forces `skip_ratings` on every path. | low |
| `api/src/image/serve.rs:1200` | `trigger_logo_backdrop_refresh`: the backdrop arm of the `(width, badge scale)` match calls `backdrop_cap::render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/image/serve.rs:1639` | `try_tmdb_logo_backdrop`: the TMDB size comes from `backdrop_cap::tmdb_size`. Replaces 1 upstream line. | medium |
| `api/src/image/serve.rs:1865` | Fast path of `handle_logo_backdrop_inner`: one added line splices the `.mw<N>` token into the cache suffix. | low |
| `api/src/image/serve.rs:1903` | Slow path of the same function: same added line. | low |
| `api/src/image/serve.rs:2041` | Generation closure: the backdrop arm calls `render_dims`. Replaces 4 upstream lines with 1. | medium |
| `api/src/handlers/image.rs:528` | `try_cdn_redirect`: one added line gives capped backdrops their own `/c/<hash>/` URL. | low |
| `api/src/handlers/preview.rs:306` | `preview_backdrop`: width and badge scale from `render_dims`. Replaces 2 upstream lines with 1. | medium |
| `api/src/handlers/preview.rs:327` | `preview_backdrop`: one added line for the capped preview cache key. | low |
| `api/tests/common/mod.rs:22,35` | `TestAppOptions` gets a `louk` field and its default. | low |
| `api/tests/common/mod.rs:48,52,55` | `_setup_test_app` takes one extra argument `louk`; two callers pass it. These are long upstream lines edited in place. | medium |
| `api/tests/common/mod.rs:135` | `louk` in the test `Config` literal. | low |
| `.github/workflows/test.yml:7` | Runs on pushes to every branch, no `paths:` filter (upstream: `main` only, and only when `api/`, `web/` or the workflow change). | medium |
| `.github/workflows/test.yml:20,37,58` | Three jobs pinned to `ubuntu-24.04`. `ubuntu-latest` is now 26.04 and Playwright 1.60 cannot install Chromium there. | medium. If upstream fixes this (new Playwright), take upstream's version and drop the pin. |
| `.github/workflows/release.yml:6,10` | The whole file is rewritten: one amd64 job, ghcr only, no Docker Hub, no arm64, no manifest-merge job, runner pinned. | **high** (keep ours on every sync) |

Also changed, without a marker (documentation, easy to merge):

| File | Change |
| --- | --- |
| `docs/configuration.md` | A "Fork-only (loukdekker.nl)" section appended at the end. |
| `docs/api.md` | One paragraph under the backdrop sizes about `BACKDROP_MAX_WIDTH`. |

`Cargo.toml` and `Cargo.lock` are unchanged: the app still reports version `1.3.0`. The git tag
carries the fork version. That keeps the most conflict-prone files out of the diff.

### Files upstream does not have

| Path | What |
| --- | --- |
| `api/src/louk/mod.rs` | Fork module root. |
| `api/src/louk/config.rs` | `LoukConfig`, reads `RATINGS_DISABLED` and `BACKDROP_MAX_WIDTH`. |
| `api/src/louk/backdrop_cap.rs` | All the cap logic (size plan, cache-key token, CDN hash, TMDB size) and its unit tests. |
| `api/tests/louk_backdrop_cap.rs` | Route-level tests for the cap. |
| `docs/louk/` | This file, `baseline.md`, `memory.md`, `safe-areas.md` and its `safe-areas/` evidence. |
| `scripts/louk/` | Load harness (`burst.py` with its `--warmup` pass, `dev-instance.sh`, `run-variant.sh`, `results-table.py`, `fetch-titles.py`, `titles.json`, `selftest.sh`, README) and `sync-upstream.sh`. |

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

Everything stateful is in `./db` (SQLite: API keys and settings), which Backrest backs up. Today the
fork writes nothing new there. When overrides and uploads arrive (not built yet), they will live in
`./db` too.

The `cache` volume holds the rendered images. It is not backed up and does not need to be: every
file is made again on request.

## Going back to upstream (rollback)

1. Set the image line in `openposterdb/compose.yaml` back to `ghcr.io/pnrxa/openposterdb:latest` (or
   the pinned `v1.3.0`).
2. Remove `BACKDROP_MAX_WIDTH` from the environment (upstream ignores it anyway). The two `MALLOC_*`
   lines can stay: they are glibc settings and help upstream's image too.
3. Deploy.

Why it is safe:

- The fork has **no schema changes** today. The database is the one upstream made. Nothing in `./db`
  needs restoring for this rollback.
- The `.mw1920` cache files are new names that upstream never builds. It ignores them. They only take
  disk space until you purge or clear the cache.
- After a rollback, large backdrops come from the old 3840 px files if they are still on disk. If they
  were purged, upstream draws them again at 3840 px, with the old memory cost.
- Not verified with a binary: no upstream binary was started against a database the fork touched.
  There is nothing to test yet because no schema changes exist. When the dashboard and overrides add
  tables, the rule is additive changes only, and a test (upstream opens a fork-migrated copy of the
  database) must be added before they ship.

## Not done yet

| Part | Where it stands | Next step |
| --- | --- | --- |
| **Title dashboard (P2)** | The frontend (library and title pages) is on branch `louk/p2-2`, CI green. The backend (title table, admin routes) is unfinished and uncommitted work in a local worktree (branch `louk/p2-1`); none of it is pushed. Not merged, not in any release. | Finish and commit the backend, then merge both, after a local smoke run against a real instance. Add the "upstream opens a fork-migrated database" test first. |
| **Per-title overrides and uploads, and the upstream PR (P3)** | Not started. The design is written down (key = resolved title + image kind, value = TMDB path, Fanart image or upload; admin only). The wrong-logo case (The Man in the High Castle: three logos with 0 votes, upstream picks the last of equal ones) is not fixed. | Build after the dashboard, then prepare the branch `pr/per-title-overrides` from upstream `main` for Louk to open on GitHub. |
| **New badge styles (P4)** | Not started. | Design 3 to 5 styles, each an option next to the existing `badge_style`, default unchanged. |
| **Speed and precache (P6)** | Not started. The burst harness gives cold latency per image kind ([memory.md](memory.md)); no warm-path or steady-traffic measurements yet. Ideas on the list: prerender siblings, stale-while-revalidate, keep a hot set warm, `Cache-Control` and `ETag`, cheap disk hits. | Measure cold and warm latency first, then pick. |
| **Safe-area preset in code (P5)** | **Documentation only, by decision.** The existing settings already express a safe area, so no preset and no crop overlay was built. | Louk sets one or two fields per key by hand (see "Backdrop safe areas"). |
| MDBList quota view | Not built. | Only if the MDBList client already reads usage headers. |

Spoiler blur for episode stills already exists upstream per key (`episode_blur`); the fork adds
nothing there.

Because the dashboard, overrides and badge styles are not built, there is nothing new in the
dashboard to turn on. The only per-key looks to set are the existing backdrop settings named under
"Backdrop safe areas". The deploy switches on no new looks.

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
