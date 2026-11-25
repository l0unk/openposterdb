# Upstream PR: per-title art overrides

Branch `pr/per-title-overrides` (Forgejo `louk/openposterdb`, mirrored to `l0unk/openposterdb`), cut from
upstream `main` at `61803c1` (v1.3.0). Patches of the same commits: `docs/louk/upstream-pr/`
(`git format-patch upstream/main..pr/per-title-overrides`). Built in task P3.4; the full report is
`reports/P3.4.md` on the dev box.

## Note to Louk: opening the PR

1. Check first:
   - the branch still applies to upstream `main`: `git fetch upstream && git log --oneline upstream/main -1`.
     If upstream moved past `61803c1`, rebase `pr/per-title-overrides` onto it (expect conflicts only in
     `api/src/image/serve.rs`, `api/src/lib.rs` `AppState`/`SCHEMA_SQL`, `api/src/routes/admin.rs` and
     `web/src/components/ImageListView.vue`), push it under a new name, and let the fork CI run on a copy
     (as `louk/pr-ci` was built: the branch plus the fork's `test.yml`);
   - `git diff upstream/main..pr/per-title-overrides -- .github` is empty (upstream's workflow untouched);
   - the screenshots below (the PR text asks for them; they are not in the branch);
   - whether to keep the last commit (`fix(tmdb): pick the first of equally voted images`). It is
     independent: drop it if the maintainer prefers a separate PR.
   - the commit author: the four commits are authored by `Claude (dev box) <claude@loukdekker.nl>` and
     carry a `Co-Authored-By: Claude` trailer. To put your name on them:
     `git rebase upstream/main --exec 'git commit --amend --no-edit --reset-author'` (as yourself), then
     push under a new branch name.
2. Open the PR from the fork: <https://github.com/PNRxA/openposterdb/compare/main...l0unk:openposterdb:pr/per-title-overrides>
   (base `PNRxA/openposterdb:main`, head `l0unk/openposterdb:pr/per-title-overrides`). Title and body below;
   paste the body from "## Summary" on, and attach the screenshots.
3. Upstream's `Tests` workflow runs on the PR itself (`pull_request` to `main`); the same tests already ran
   green on the fork (see `reports/P3.4.md`).

---

**Title:** `feat(admin): per-title art overrides and uploads`

## Summary

Some titles get the wrong automatic art, and today an admin can only purge and hope. Example: **The Man
in the High Castle** (`tv/62017`) has three English logos on TMDB, all with 0 votes. The vote-based pick
lands on one of them more or less at random (the last in TMDB's list), and there is no way to choose
another one short of voting on TMDB and waiting.

This PR adds **per-title overrides**: an admin pins the poster, logo, backdrop or an episode still of one
title to a different TMDB image, a fanart.tv image, or an uploaded file, and can switch the rating badges
off for that title + kind. It is reachable from the existing cache browser: every row of the Posters,
Logos, Backdrops and Episodes lists gets an "Override" button.

Defaults change nothing: without an override every request renders exactly as before (same cache keys,
same bytes, no extra DB query per request).

## How it works

- **Key:** the *resolved* title (`movie`/`tv` + TMDB id), the image kind, `season`/`episode` for stills,
  and optionally a request language (posters and logos only; a render uses its exact language, then the
  base language, then the any-language row). Never the requested id, so imdb, tmdb and tvdb requests of
  a title all get the override, and a show image requested with an episode id gets the show's override.
- **Value:** a TMDB `file_path`, a fanart.tv image (id + URL), an upload (sha256), or `auto` (keep the
  automatic art, only the badge flag applies), plus `no_badges`.
- **Render paths:** the override is applied before the TMDB-vs-fanart preference on every path: posters
  (including titles with no TMDB poster), logos, backdrops, episode stills, the logo/backdrop background
  refresh, and therefore also with `ENABLE_CDN_REDIRECTS` and `EXTERNAL_CACHE_ONLY`. Badges are still
  drawn on top unless `no_badges` is set; `no_badges` renders with ratings limit 0, i.e. the existing
  badge-free cache key and no rating lookups. If the override's image cannot be fetched or decoded, the
  automatic art is served and a warning is logged (and the title is purged again 10 minutes later, once
  per override version, so the fallback does not stay cached).
- **Storage:** one new table `image_overrides`, mirrored in memory and loaded once (the render path does a
  hash lookup, never a query). Uploads are stored by content under `DB_DIR/uploads/<sha256>.<ext>`, so
  they are backed up with the database directory.
- **Code:** `api/src/services/overrides/` (store, uploads, candidates, render hooks, purge),
  `api/src/handlers/overrides.rs`, routes in `routes/admin.rs`. The render paths in `image/serve.rs` get
  one call per hook (about 15 lines). Two existing functions become `pub(crate)`: the admin
  `purge_title` and `fetch_fanart_images`. `AppState` gets one field (`overrides`).

## API (admin auth; full reference in `docs/api.md` "Per-title art overrides")

| method + path | what |
| --- | --- |
| `GET /api/admin/overrides/{media_type}/{tmdb_id}` | every override of a title |
| `GET`, `PUT`, `DELETE /api/admin/overrides/{media_type}/{tmdb_id}/{kind}?season=&episode=&lang=` | one override; `PUT` body `{"source", "value", "fanart_id"?, "no_badges"?}` |
| `GET /api/admin/overrides/{media_type}/{tmdb_id}/{kind}/candidates?lang=` | every TMDB image (all languages) and fanart.tv image, with votes/likes, size, thumbnail URL, the automatic pick and the current override marked |
| `POST /api/admin/uploads` | raw-body upload (PNG/JPEG/WebP sniffed from the bytes, max 15 MiB, max 8000 px per side, fully decoded before it is stored) |
| `GET /api/admin/uploads/{sha256}` | an upload |
| `GET /api/admin/resolve/{id_type}/{id_value}?kind=` | the override key of a cached image id (used by the cache browser) |

Uploads are a raw request body rather than multipart so the PR needs no new crate feature
(`axum`'s `multipart` is off); the browser sends the `File` as the body.

## Schema

One additive table, created with `CREATE TABLE IF NOT EXISTS` in `SCHEMA_SQL`:
`image_overrides (media_type, tmdb_id, kind, season, episode, lang, source, value, fanart_id, no_badges, updated_at)`,
primary key `(media_type, tmdb_id, kind, season, episode, lang)`, every optional column with a default. No
existing table changes; an older build simply ignores the table.

## What a save or clear purges

There is no title/alias index upstream, so the purge works from the ids a title can be cached under and
runs the existing per-title purge (`purge_title`: every settings variant on disk, `image_meta`,
`available_ratings`, `image_mem_cache`, `image_inflight`, `id_cache`) for each of them:

- the canonical `tmdb/movie-N` / `tmdb/series-N` (stills: `tmdb/episode-N-S{s}E{e}`);
- the title's imdb and tvdb ids, from `id::resolve` of the canonical id (through `id_cache`; skipped with
  a warning if TMDB cannot be reached), in the `episode-…-S{s}E{e}` form for stills;
- every id in `id_cache` that resolves to the title (stills: to that episode), i.e. ids requested in the
  last hour, such as an episode's own imdb id or a lowercase `s1e2` id;
- for show posters, logos and backdrops, every `episode-…` id of the show that has a cached render of
  that kind in `image_meta` (a show image requested with an episode id is cached under that id).

It runs once on save/clear and once more 60 s later, so a render that was in flight during the change
cannot leave the old art cached. **May linger until stale:** renders under an id none of the above
produce (e.g. a still requested by the episode's own imdb id more than an hour ago, or an id that TMDB
does not cross-reference), and CDN copies (`ENABLE_CDN_REDIRECTS` / `EXTERNAL_CACHE_ONLY`), which cannot
be purged from the server. The per-title purge buttons still work for those.

A per-title generation counter checked in `post_render_cache` would close the in-flight race without the
delayed re-purge; I left it out to keep the change to the write path at zero. Happy to add it if you
prefer that.

## Separate commit: ties in `select_image_ranked`

`select_image_ranked` uses `Iterator::max_by`, which returns the **last** of equal elements, so a tie
(e.g. High Castle's three 0-vote logos) picks the last image in TMDB's list. The last commit makes ties
pick the first one (`.rev().max_by(…)`), with a test. It is independent of the overrides (their tests do
not depend on the tie rule) and can be dropped or split out. Note that it changes which image is served
for tied titles once their cached renders go stale or are purged.

## How to test by hand

1. Run with a TMDB key (and optionally `FANART_API_KEY`), log in to the admin UI.
2. Request a logo: `/{key}/imdb/logo-default/tt1740299.png` (The Man in the High Castle).
3. Admin → Logos: the row `imdb/tt1740299…` → "Override". The picker lists the TMDB logos in all
   languages and the fanart.tv logos; one is marked "Automatic pick".
4. Pick another English logo → Save. Request the logo again by imdb (`tt1740299`), tmdb (`series-62017`)
   and tvdb (`295829`): all three serve the new logo.
5. Re-open the picker, tick "No rating badges" → Save: the logo is drawn without badges (cache key
   `…@.…`).
6. Upload a PNG in the picker → Save: the upload is served. A GIF, an SVG or a 9000 px image is refused.
7. "Clear override": the automatic pick is served again.
8. Episodes: `/{key}/imdb/episode-default/episode-tt1740299-S1E2.jpg`, then Admin → Episodes → Override,
   upload a still → only S1E2 changes.
9. Repeat 4 with `EXTERNAL_CACHE_ONLY=true` and with `ENABLE_CDN_REDIRECTS=true`.

Automated: `cargo test` (new: `tests/overrides_api.rs` 14, `tests/overrides_render.rs` 14, 18 unit
tests in `services/overrides/` and `services/tmdb.rs`), `npx vitest run` (new:
`__tests__/lib/overrides.spec.ts` 11, `__tests__/components/OverridePicker.spec.ts` 9, one more in
`ImageListView.spec.ts`); e2e unchanged and passing.

## Screenshots

To be added by the submitter. Worth taking:

1. Admin → Logos with the new "Override" button in a row.
2. The picker for High Castle's logo (`tv/62017`): the candidates grid with "Automatic pick" marked,
   the "Applies to" language selector, "No rating badges", "Upload image".
3. The served logo before and after choosing another English logo (the same URL).
4. The picker after an upload (the upload tile selected), and the served result.
5. A logo or poster with "No rating badges" next to the same one with badges.
6. Admin → Episodes → the picker for one still.

## Limitations

- Uploads are never garbage-collected: replacing or clearing an override leaves the file in
  `DB_DIR/uploads/` (they are content-addressed, so re-uploading the same file reuses it).
- An override applies to textless poster requests too (the override image is used as is).
- Fanart.tv candidates have no size information (the fanart API does not give one).
- The candidates list calls TMDB `/images` without a language filter and fanart.tv once per picker open
  (cached like the render path's calls); no rating API is called.
- The picker is deliberately compact (no before/after preview of the rendered image); the list's own
  preview shows the result after a save.
