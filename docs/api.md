# API Reference

OpenPosterDB is a drop-in replacement for [RPDB](https://ratingposterdb.com) — use `http://localhost:3000` (or your instance URL) as the base URL in place of `https://api.ratingposterdb.com`.

- [Endpoints](#endpoints)
- [Common parameters](#common-parameters)
- [Image sizes](#image-sizes)

## Endpoints

### Poster

```
GET /{api_key}/{id_type}/poster-default/{id_value}.jpg
```

- Returns JPEG with rating badges overlaid on the poster
- Uses TMDB (default) or Fanart.tv as the poster source

### Logo

```
GET /{api_key}/{id_type}/logo-default/{id_value}.png
```

- Returns transparent PNG with rating badges stacked below the logo
- Uses TMDB (default) or Fanart.tv as the image source

### Backdrop

```
GET /{api_key}/{id_type}/backdrop-default/{id_value}.jpg
```

- Returns JPEG with rating badges overlaid on the backdrop (configurable position and direction)
- Uses TMDB (default) or Fanart.tv as the image source

### Episode

```
GET /{api_key}/{id_type}/episode-default/{id_value}.jpg
```

- Returns JPEG with per-episode ratings on an episode still image (landscape)
- Falls back to the series poster when no episode still is available
- Supports episode IMDb IDs (e.g. `tt0959621`), TMDB episode format (`episode-1396-S1E1`), TVDB episode IDs, and series-level ID + season/episode for all ID types (e.g. `episode-tt14786934-S1E1` for IMDb, `episode-81189-S3E5` for TVDB)
- Dedicated episode settings: position, direction, badge style/size, label style, ratings limit
- `?blur=true` applies Gaussian blur for spoiler protection (badges remain sharp)
- IMDB episode ratings require an OMDb API key (MDBList does not support episode-level ratings)

### Key validation

```
GET /{api_key}/isValid
```

- Returns `200 OK` if the API key is valid, `401 Unauthorized` otherwise
- Compatible with RPDB integrations that validate keys before use

Management endpoints (auth, keys, settings) are under `/api/` and return JSON.

## Common parameters

- `id_type`: `imdb`, `tmdb`, `tvdb`
- `id_value`: e.g. `tt1234567`, `movie-123`, `series-456`, `episode-1396-S1E1`. Episode IMDb IDs (e.g. `tt0959621`), TVDB episode IDs, and series-level IDs with season/episode (e.g. `episode-tt14786934-S1E1`, `episode-81189-S3E5`) are also supported
- `?fallback=true`: accepted for RPDB plugin compatibility but ignored as OPDB falls back to TMDB by default
- `?lang={code}`: override the image language for this request (e.g. `?lang=de` for German, `?lang=pt-BR` for Brazilian Portuguese). Supports regional variants — when a region-specific image exists (e.g. `pt-BR`), it is preferred; otherwise falls back to the base language (`pt`), then English. Applies to posters and logos. Backdrops are language-agnostic and ignore this parameter
- `?imageSize={size}`: control output image dimensions. Available sizes vary by image type (see [Image sizes](#image-sizes))
- `?ratings_limit={0-10}`: maximum number of rating badges to display (0 = no ratings)
- `?ratings_order={keys}`: comma-separated rating source keys controlling display order. Valid keys: `imdb`, `tmdb`, `rt` (RT Critics), `rta` (RT Audience), `mc` (Metacritic), `trakt`, `lb` (Letterboxd), `mal` (MyAnimeList), `mdblist` (MDBList score), `ebert` (Roger Ebert). Example: `?ratings_order=imdb,tmdb,rt`
- `?ratings_exclude={keys}`: comma-separated rating source keys to hide entirely (same valid keys as `ratings_order`). Excluded sources are dropped *before* ordering and limiting, so an excluded source frees its badge slot for the next preferred source rather than leaving a gap. Example: `?ratings_exclude=rt` shows your ratings but never RT Critics
- `?badge_style={h|v|d}`: badge layout — `h` (horizontal), `v` (vertical), `d` (default)
- `?label_style={t|i|o}`: label rendering — `t` (text), `i` (icon), `o` (official provider logos)
- `?badge_size={xs|s|m|l|xl}`: badge scale — extra-small, small, medium, large, extra-large
- `?badge_shape={r|p}`: badge corner shape — `r` (rounded, default), `p` (pill, fully rounded ends). Pills always render as a horizontal icon/label-left, value-right lozenge, even on image types whose default style is vertical (logos, backdrops, episodes)
- `?badge_background={d|k|t|n}`: badge background — `d` (default: source-coloured label + dark value), `k` (dark: uniformly dark), `t` (transparent: semi-transparent so the artwork shows through), `n` (none: no background, label/value drawn directly on the image with a drop shadow)
- `?image_source={t|f}`: image source — `t` (TMDB, default), `f` (Fanart.tv). Applies to all image types. The non-selected source is used as fallback
- `?badge_direction={d|h|v}`: badge stacking direction — `d` (default), `h` (horizontal), `v` (vertical). Applies to poster, backdrop, and episode endpoints
- `?position={bc|tc|l|r|tl|tr|bl|br}`: badge anchor position. Applies to poster, backdrop, and episode endpoints
- `?textless={true|false}`: prefer textless images when available (poster only). Works with both TMDB and Fanart.tv sources
- `?blur={true|false}`: apply Gaussian blur for spoiler protection (episode only). Badges remain sharp over the blurred still image
- `?split={true|false}`: split the badges evenly across two opposite sides of the poster (poster only). The axis follows the badge layout — a vertical layout splits left/right, horizontal rows split top/bottom. With an odd number of badges the configured side gets the extra one (e.g. 4 badges → 2 + 2, 3 badges → 2 + 1)
- `?fit={native|cover|pad|blur}`: how a poster is fit to the standard 2:3 output frame (poster only). `native` (default) keeps the source aspect ratio; `cover` scales to fill 2:3 and center-crops the overflow; `pad` fits the whole poster inside 2:3 with solid black bars; `blur` fits the whole poster inside 2:3 and fills the bars with a blurred, zoomed copy of the poster. The non-native modes guarantee a uniform 2:3 image so downstream apps that place posters in fixed 2:3 containers don't crop the art
- `?edge_inset_x={0-50}` / `?edge_inset_y={0-50}`: inset the backdrop ratings further from the anchored edge, as a percentage of the backdrop's width/height (backdrop only). Useful when a media player crops the backdrop and clips the ratings. `edge_inset_x` only applies to left/right positions and `edge_inset_y` only to top/bottom positions; the inset for a centered axis is ignored. Example: `?position=tr&edge_inset_y=10` nudges top-right ratings down by 10% of the height

Old RPDB parameter names `?poster_source=` and `?fanart_textless=` are accepted as aliases.

**Scope notes:** `textless`, `split`, and `fit` are poster-only. `blur` is episode-only. `edge_inset_x`/`edge_inset_y` are backdrop-only. `badge_direction` and `position` are silently ignored on logo endpoints. For shared parameters (`ratings_limit`, `badge_style`, `label_style`, `badge_size`, `badge_shape`, `badge_background`, `image_source`), the override is applied to the correct image-type-specific setting (e.g. `?badge_style=h` on the poster endpoint sets `poster_badge_style`, on the logo endpoint sets `logo_badge_style`).

## Image sizes

The `?imageSize=` parameter controls the output dimensions. When omitted, `medium` is used as the default. All badge elements scale proportionally with the image.

**Poster sizes:**

| Size | Dimensions |
|---|---|
| `medium` *(default)* | 580 × 859 |
| `large` | 1280 × 1896 |
| `very-large` / `verylarge` | 2000 × 2962 |

Heights are representative for a standard ~2:3 source under the default `native` fit, which preserves the source aspect ratio (so the exact height varies per poster). The `cover`, `pad`, and `blur` fits instead produce an exact 2:3 frame (580 × 870, 1280 × 1920, 2000 × 3000).

**Logo sizes:**

| Size | Dimensions |
|---|---|
| `medium` *(default)* | 780 × 244 |
| `large` | 1722 × 539 |
| `very-large` / `verylarge` | 2689 × 841 |

**Backdrop sizes:**

| Size | Dimensions |
|---|---|
| `small` | 1280 × 720 |
| `medium` *(default)* | 1920 × 1080 |
| `large` | 3840 × 2160 |

Fork (loukdekker.nl): when the server sets `BACKDROP_MAX_WIDTH`, backdrop sizes wider than it (`large`, `very-large`) render at that width instead, keeping the source aspect ratio, with badges scaled to the canvas as for the other sizes (e.g. `BACKDROP_MAX_WIDTH=1920` makes `large` 1920 × 1080). Sizes already at or below the cap are unchanged. See [configuration.md](configuration.md#fork-only-loukdekkernl).

**Episode sizes:**

| Size | Dimensions |
|---|---|
| `small` | 480 × 270 |
| `medium` *(default)* | 780 × 439 |
| `large` | 1280 × 720 |
| `very-large` / `verylarge` | 1920 × 1080 |

`small` is only valid for backdrops and episodes — requesting it for posters or logos returns `400 Bad Request`. `verylarge` is accepted as an alias for `very-large` for RPDB compatibility.

## Title dashboard (fork-only, admin)

Fork (loukdekker.nl) only. These endpoints sit under the admin login like every other `/api/admin/` route (`Authorization: Bearer <admin token>`; without it `401`). Errors are JSON `{"error": "<message>"}`: `400` for a bad parameter, `404` for an unknown title or id.

The fork records every id an image is requested by (`imdb/tt0133093`, `tmdb/movie-603`, `tvdb/…`, episode ids such as `imdb/episode-tt0944947-S1E2`) and the title it belongs to. Names, years and poster paths are fetched from TMDB in the background (TMDB only, one call at a time); cache entries from before the fork are indexed by a slow background backfill. Times are Unix seconds.

| Method | Path | What |
|---|---|---|
| `GET` | `/api/admin/louk/titles?q=&type=&sort=&page=&per_page=` | Title list. `q`: part of the name (case-insensitive), or an exact imdb id or tmdb id. `type`: `movie`, `tv` (empty or `all` = both). `sort`: `last` (default, last requested first) or `name`. `page` from 1, `per_page` default 48, max 200. Returns `{items: [title summary], total, page, per_page}`. |
| `GET` | `/api/admin/louk/titles/{media_type}/{tmdb_id}` | One title (`media_type` = `movie` or `tv`): the summary fields plus `tvdb_id`, `first_seen`, `resolved_at`, `aliases`, `images` and, for shows only, `seasons`. |
| `GET` | `/api/admin/louk/titles/by-alias/{id_type}/{id_value}` | Which title an id belongs to: `{media_type, tmdb_id}`, or `404`. A full cache value (`tt0133093@ir.zm`) is cut to its id. |
| `POST` | `/api/admin/louk/titles/{media_type}/{tmdb_id}/purge` | Purges every cached image of the title under every id it is known by (the per-title purge of the admin cache pages, once per id and kind). Returns `{ok, external_cache_only, ids, files_deleted, meta_deleted, ratings_deleted, errors: [string]}`. The index itself is kept. Upstream's purge also drops the stored ratings of each id, so the next request can cost a few MDBList/OMDb lookups. |
| `POST` | `/api/admin/louk/titles/{media_type}/{tmdb_id}/refetch?kind=` | Same as the admin cache pages' "fetch" (renders with the global settings; served from cache if already there) for the title's most recently requested id. `kind`: `poster`, `logo` or `backdrop`; omitted = all three. `episode` is `400` (fetch stills per episode with `POST /api/admin/episodes/{id_type}/{id_value}/fetch`). Like the existing fetch buttons, it can resolve ratings for a render (a few MDBList/OMDb lookups). Returns `{ok, results: [{kind, id_type, id_value, ok, error}]}`. |

Title summary: `{media_type, tmdb_id, name, year, imdb_id, poster_thumb_url, last_requested, image_counts: {poster, logo, backdrop, episode}}`. `name`, `year`, `imdb_id` and `poster_thumb_url` are `null` until known; `poster_thumb_url` is a TMDB `w185` URL the browser loads directly.

In the detail: `aliases` = `[{id_type, id_value, season, episode, last_requested}]` (`season`/`episode` set for episode ids). `images` = every cached render of the title under any of its ids, newest first: `[{kind, id_type, cache_value, created_at, updated_at, admin_image_url}]`, where `admin_image_url` is the existing admin image endpoint for that row (needs the admin token). `seasons` (shows only) = `[{season, episodes: [{episode, name, air_date, still_thumb_url, cached: [image]}]}]` for the seasons that have a requested episode or whose episode names are already stored; episode names come from TMDB `/tv/{id}/season/{n}` on first view and are kept 7 days. `still_thumb_url` is a TMDB `w300` URL.

`episodes_pending` (shows only, added later; `true`/`false`): a show page fetches stale season episode names from TMDB for at most 3 s and at most 10 seasons, seasons with cached stills first; the rest are fetched in the background and the response says `episodes_pending: true` (those seasons show what is stored, possibly no names yet). Open the page again a few seconds later to get them. The key is absent for movies.

## Per-title overrides (fork-only, admin)

Swap the art of one title for another TMDB or fanart.tv image or an uploaded file, and optionally
turn its rating badges off. All routes need the admin bearer token (`401` with an empty body without
it); other errors are `{"error": string}` with `400` (bad parameter), `404`, `413` (upload too
large) or `500`.

An override is keyed on the **resolved title** (`movie`/`tv` + TMDB id), the image kind (`poster`,
`logo`, `backdrop`, `episode`), `season`/`episode` for episode stills, and a request language
(`lang`, posters and logos only; empty = any). Requests by imdb, tmdb or tvdb id all hit the same
override. A render uses the row for its exact language, then the base language (`pt-BR` → `pt`),
then the language-less row. The override is applied before the TMDB-vs-fanart preference on every
path (normal, `ENABLE_CDN_REDIRECTS`, `EXTERNAL_CACHE_ONLY`, posters without a TMDB poster); badges
are still drawn on top unless `no_badges` is set. If the override's image cannot be fetched or
decoded, the automatic art is served and a warning is logged. Titles without an override render
exactly as before (same cache keys and bytes).

Setting, changing or clearing an override purges every cached render of that title and kind: on
disk, in `image_meta`, and in the in-memory caches, under every id the title is known by (the
canonical `tmdb/movie-N`/`tmdb/series-N`, the imdb/tvdb ids, every requested id and cross-id copy in
the title index). CDN copies (`ENABLE_CDN_REDIRECTS`) cannot be purged from here.

### `Override` object

| field | type | notes |
| --- | --- | --- |
| `media_type` | `"movie" \| "tv"` | |
| `tmdb_id` | int | |
| `kind` | `"poster" \| "logo" \| "backdrop" \| "episode"` | |
| `season`, `episode` | int \| null | episode stills only |
| `lang` | string \| null | null = any language |
| `source` | `"auto" \| "tmdb" \| "fanart" \| "upload"` | `auto` keeps the automatic art (only `no_badges` applies) |
| `value` | string | TMDB `file_path`, fanart URL, upload sha256, `""` for `auto` |
| `fanart_id` | string \| null | fanart only |
| `no_badges` | bool | |
| `updated_at` | int | Unix seconds |
| `thumb_url` | string \| null | TMDB `w300` URL, fanart preview URL, or `/api/admin/louk/uploads/{sha256}` (needs the bearer token: fetch as blob); null for `auto` |

`PurgeResult` = `{"ids": string[], "files_deleted": int, "meta_deleted": int, "errors": string[], "external_cache_only": bool}`
(`ids` are `{id_type}/{id_value}`; `errors` lists ids whose purge failed, the call still succeeds).

### Key parameters

Path `{media_type}/{tmdb_id}/{kind}`; query `season` and `episode` (both required for `episode`, a
number ≥ 0, `tv` only; forbidden for the other kinds) and `lang` (`en` or `pt-BR` form; posters and
logos only). Bad values → `400`.

### `GET /api/admin/louk/overrides/{media_type}/{tmdb_id}`

All overrides of a title (kinds in the order poster, logo, backdrop, episode).
`200 {"overrides": Override[]}`.

### `GET /api/admin/louk/overrides/{media_type}/{tmdb_id}/{kind}?season=&episode=&lang=`

The row stored under exactly this key (no language fallback). `200 {"override": Override | null}`.

### `PUT /api/admin/louk/overrides/{media_type}/{tmdb_id}/{kind}?season=&episode=&lang=`

JSON body `{"source": "tmdb"|"fanart"|"upload"|"auto", "value": string, "fanart_id"?: string, "no_badges"?: bool}`
(`value` may be omitted for `auto`; `no_badges` defaults to false). Validation: `tmdb` = `/name.ext`
with `jpg`, `jpeg`, `png` or `webp` (no `.svg`); `fanart` = an `https://assets.fanart.tv/…` URL plus
`fanart_id`; `upload` = the sha256 of an existing upload. Creates or replaces the row, then purges.

```json
{"override": {"media_type": "tv", "tmdb_id": 60622, "kind": "logo", "season": null, "episode": null,
              "lang": null, "source": "tmdb", "value": "/gdFFKKCDfubJVbj88JnaCHRJDGZ.png", "fanart_id": null,
              "no_badges": false, "updated_at": 1764060000,
              "thumb_url": "https://image.tmdb.org/t/p/w300/gdFFKKCDfubJVbj88JnaCHRJDGZ.png"},
 "purge": {"ids": ["tmdb/series-60622", "imdb/tt1740299", "tvdb/295829"], "files_deleted": 3,
           "meta_deleted": 3, "errors": [], "external_cache_only": false}}
```

### `DELETE /api/admin/louk/overrides/{media_type}/{tmdb_id}/{kind}?season=&episode=&lang=`

Removes the row and purges. `200 {"deleted": bool, "purge": PurgeResult | null}` (`purge` is null when
there was no row).

### `POST /api/admin/louk/uploads`

The request body **is the image file** (raw bytes, not multipart; any `Content-Type`, the format is
read from the bytes). PNG, JPEG or WebP only, at most 15 MiB, at most 8000 px per side, and it must
decode. Stored once per content under `DB_DIR/uploads/<sha256>.<png|jpg|webp>` (backed up with
`./db`).

- `200 {"sha256": string, "format": "png"|"jpeg"|"webp", "width": int, "height": int, "bytes": int, "created": bool, "url": "/api/admin/louk/uploads/<sha256>"}`
  (`created` false = the same bytes were already stored).
- `413` over 15 MiB (a larger `Content-Length` is refused before reading; a streamed body is cut at
  the limit). `400` not PNG/JPEG/WebP (a multipart body gets a hint to send the raw file), a side
  over 8000 px, or undecodable.

```sh
curl -H "Authorization: Bearer $TOKEN" --data-binary @logo.png https://posters.example/api/admin/louk/uploads
```

### `GET /api/admin/louk/uploads/{sha256}`

The stored file with its content type (`Cache-Control: private, max-age=31536000, immutable`).
`400` unless 64 lowercase hex characters, `404` unknown.

### `GET /api/admin/louk/candidates/{media_type}/{tmdb_id}/{kind}?season=&episode=&lang=`

Every image the title could use: TMDB (all languages; for `episode` the episode's stills) and
fanart.tv (posters, logos, backdrops). Calls TMDB and fanart.tv only. `lang` (default `en`) is the
request language the automatic pick and `current` are computed for.

Order: TMDB first, then fanart; within each, the `lang` language, then language-less art, then
English, then other languages A–Z; within a language by votes (fanart: likes) descending, and equal
votes keep the source's own order (first of equals first; upstream's automatic pick takes the last
of equals). `.svg` logos are left out (they cannot be drawn).

`200`:

| field | type |
| --- | --- |
| `media_type`, `tmdb_id`, `kind`, `season`, `episode` | as in `Override` |
| `lang` | string, the language used |
| `current` | `Override` \| null, the override a request in `lang` uses now |
| `auto` | `{"source": "tmdb"\|"fanart", "value": string, "fanart_id": string\|null}` \| null, the automatic pick for a TMDB-preferred key in `lang` |
| `candidates` | `Candidate[]` |
| `errors` | string[], non-fatal (`"tmdb: …"`, `"fanart: images unavailable"`) |

`Candidate` = `{"source": "tmdb"|"fanart", "value": string, "fanart_id": string|null, "lang": string|null,
"votes": number|null, "vote_count": int|null, "likes": int|null, "width": int|null, "height": int|null,
"thumb_url": string, "full_url": string, "auto": bool, "current": bool}`. `lang` null = no language
(textless). `votes`/`vote_count` are TMDB's, `likes` fanart's; fanart gives no size. `thumb_url` and
`full_url` are public image URLs (TMDB `w300`/`original`, fanart preview/full). To apply a
candidate, `PUT` its `source`, `value` and `fanart_id`.

```json
{"media_type": "tv", "tmdb_id": 60622, "kind": "logo", "season": null, "episode": null, "lang": "en",
 "current": null,
 "auto": {"source": "tmdb", "value": "/huev0mCSEtziNHhP32vcy7N2H2w.png", "fanart_id": null},
 "candidates": [
   {"source": "tmdb", "value": "/gdFFKKCDfubJVbj88JnaCHRJDGZ.png", "fanart_id": null, "lang": "en",
    "votes": 3.334, "vote_count": 3, "likes": null, "width": 2505, "height": 1299,
    "thumb_url": "https://image.tmdb.org/t/p/w300/gdFFKKCDfubJVbj88JnaCHRJDGZ.png",
    "full_url": "https://image.tmdb.org/t/p/original/gdFFKKCDfubJVbj88JnaCHRJDGZ.png",
    "auto": false, "current": false},
   {"source": "fanart", "value": "https://assets.fanart.tv/fanart/tv/295829/hdtvlogo/x.png", "fanart_id": "12345",
    "lang": "en", "votes": null, "vote_count": null, "likes": 4, "width": null, "height": null,
    "thumb_url": "https://assets.fanart.tv/preview/tv/295829/hdtvlogo/x.png",
    "full_url": "https://assets.fanart.tv/fanart/tv/295829/hdtvlogo/x.png", "auto": false, "current": false}
 ],
 "errors": []}
```
