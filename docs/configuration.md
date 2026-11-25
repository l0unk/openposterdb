# Configuration

OpenPosterDB is configured entirely through environment variables. When using Docker Compose, set them in a `.env` file at the project root (copy `api/.env.example` to get started); when using `docker run`, pass them with `-e`.

## API keys

At minimum you need a **TMDB key** (for artwork), a **JWT secret** (for auth), and **one ratings source**. MDBList is recommended because a single key covers all nine rating sources.

| Variable | Default | Description |
|---|---|---|
| `TMDB_API_KEY` | *required* | [TMDB](https://www.themoviedb.org/settings/api) API v3 key |
| `JWT_SECRET` | *required* | 32-byte hex string (`openssl rand -hex 32`) |
| `MDBLIST_API_KEY` | — | [MDBList](https://mdblist.com/preferences/) key — preferred, covers all 9 rating sources (IMDb, RT Critics, RT Audience, Metacritic, Trakt, Letterboxd, MAL, MDBList score, Roger Ebert) |
| `OMDB_API_KEY` | — | [OMDb](https://www.omdbapi.com/apikey.aspx) key (IMDb, RT Critics, Metacritic only). Also required for IMDB episode ratings |
| `TRAKT_CLIENT_ID` | — | [Trakt](https://trakt.tv/oauth/applications) Client ID — Trakt community ratings for movies, shows, and episodes |
| `FANART_API_KEY` | — | [Fanart.tv](https://fanart.tv/get-an-api-key/) key — enables Fanart.tv as an alternative or preferred image source for posters, logos, and backdrops |

## Server & storage

| Variable | Default | Description |
|---|---|---|
| `LISTEN_ADDR` | `0.0.0.0:3000` | Server bind address |
| `CACHE_DIR` | `./cache` | Poster and metadata cache directory |
| `DB_DIR` | `./db` | SQLite database directory |
| `IMAGE_QUALITY` | `85` | JPEG output quality (1-100) |
| `IMAGE_MEM_CACHE_MB` | `512` | In-memory cache size in MB |
| `RENDER_CONCURRENCY` | `CPUs × 2` | Max concurrent image render tasks |
| `CROSS_ID_CONCURRENCY` | `CPUs` | Max concurrent cross-ID cache write tasks |

## Auth & access

| Variable | Default | Description |
|---|---|---|
| `COOKIE_SECURE` | `true` | HTTPS-only cookies. Set to `false` if you access the admin UI over plain HTTP (no TLS) — otherwise the browser drops auth cookies and login appears broken |
| `CORS_ORIGIN` | — | Allowed origin for admin requests |
| `ADMIN_USERNAME` | — | Seed admin username on first run |
| `ADMIN_PASSWORD` | — | Seed admin password on first run |
| `FREE_KEY_ENABLED` | — | Force-enable (`true`) or force-disable (`false`) the free API key, overriding the admin UI toggle. When set, the UI toggle is locked. Omit to let admins control it from the settings page |
| `DISABLE_PUBLIC_PAGES` | `false` | Hide the landing page, docs, legal, and all unauthenticated routes, redirecting visitors to the login page. All pages remain accessible to authenticated users. Useful for private instances |

## Caching & refresh

| Variable | Default | Description |
|---|---|---|
| `RATINGS_STALE_SECS` | `86400` | Min ratings cache lifetime |
| `RATINGS_MAX_AGE_SECS` | `31536000` | Film age after which ratings stop refreshing |
| `IMAGE_STALE_SECS` | `0` | Base image cache lifetime (0 = never re-fetch) |
| `ENABLE_CDN_REDIRECTS` | `false` | Enable content-addressed CDN redirects (see [Deployment](deployment.md#cloudflare)) |
| `EXTERNAL_CACHE_ONLY` | `false` | Skip image file writes to disk; rely on a CDN for caching. SQLite metadata is still written (see [Architecture](architecture.md#external-cache-only)) |

## Logging

| Variable | Default | Description |
|---|---|---|
| `RUST_LOG` | `warn` | Log level filter — levels: `error`, `warn`, `info`, `debug`, `trace`. Supports comma-separated per-module overrides. Relevant modules: `openposterdb_api` (app), `tower_http` (HTTP tracing), `sea_orm` / `sqlx` (database), `reqwest` / `hyper` (HTTP client/server). Example: `warn,openposterdb_api=info,tower_http=debug` |

## Fork-only (loukdekker.nl)

These variables exist only in the loukdekker.nl fork (see [docs/louk/FORK.md](louk/FORK.md)). Unset, the server behaves exactly like upstream.

| Variable | Default | Description |
|---|---|---|
| `RATINGS_DISABLED` | `false` | **For dev and test instances only; never in production.** `true`, `1`, `yes` or `on` (any case, surrounding spaces ignored; `false`/`0`/`no`/`off`/empty mean off, any other value stops startup with an error) ignores `OMDB_API_KEY`, `MDBLIST_API_KEY` and `TRAKT_CLIENT_ID` (those clients are never built, so no rating quota is spent), skips the "one ratings source" startup check, and skips every rating lookup including TMDB votes. Images render without badges, and the `available_ratings` index is neither read nor written. Never run it behind a CDN or with `ENABLE_CDN_REDIRECTS=true`: CDN responses carry long cache lifetimes and would keep the badge-less art |
| `BACKDROP_MAX_WIDTH` | unset | Integer from 320 to 3840. Unset (or invalid, which logs a warning and is ignored) keeps upstream widths. When set, backdrops whose `imageSize` is wider than this (`large`/`very-large` are 3840 px; `medium` 1920, `small` 1280) render at this width instead, keeping the aspect ratio and scaling badges with the canvas; the TMDB source is fetched at the smallest of `w1280`/`original` that covers it. Capped renders carry a `.mw<N>` cache-key token (and their own CDN-redirect hash), so they never mix with uncapped ones; sizes the cap does not reduce keep their upstream keys. Logos, posters and episode stills are unaffected. Production: `BACKDROP_MAX_WIDTH=1920` |
| `LOUK_HTTP_ETAG` | on | Image responses (poster, logo, backdrop, episode, and the `/c/` CDN routes) carry a strong `ETag` (first 128 bits of the SHA-256 of the image bytes), and a request whose `If-None-Match` matches it (a tag, a list of tags, `W/` variants or `*`) gets `304 Not Modified` with no body. The 200 response is otherwise unchanged (same body, same headers). The tag is computed once when an image enters the memory cache, not per request. Error responses never carry it. `false`, `0`, `no` or `off` turns it off and restores upstream's headers exactly. `If-Modified-Since` is not supported (no `Last-Modified`) |
| `LOUK_RATINGS_SWR` | `false` | `true` or `1`: when only the ratings index of a title is stale (the `available_ratings` row is older than the title's stale interval, see `RATINGS_STALE_SECS`), a cached image is served at once instead of after a ratings fetch, and the same request is replayed in the background, which fetches the ratings once, refreshes the index and re-renders exactly as upstream would have done in the foreground. At most one replay per image at a time and two overall; when both are busy the request is still served from cache and a later one replays. No extra rating calls: the fetch only moves off the request path. That one response can carry the previous ratings (upstream already does this when the image file itself is stale). Ignored with `RATINGS_DISABLED`. Logs `louk-swr: served stale index, revalidating` and `louk-swr: revalidated … elapsed_ms` |
