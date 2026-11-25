// louk-fork: typed fetchers for the title dashboard (REST contract: PLAN.md "Task P2.1").
// Everything the Library and Title views know about the backend lives in this file, so a
// contract correction is a one-file change.
import { adminApi, get, post, type PurgeScope } from '@/lib/api'

export type MediaType = 'movie' | 'tv'
export type ImageKind = 'poster' | 'logo' | 'backdrop' | 'episode'
export type TitleTypeFilter = '' | MediaType
export type TitleSort = 'last' | 'name'

export interface ImageCounts {
  poster: number
  logo: number
  backdrop: number
  episode: number
}

/** One row of the Library list. `name` is null until the title's details have been fetched. */
export interface TitleSummary {
  media_type: MediaType
  tmdb_id: number
  name: string | null
  year: number | null
  imdb_id: string | null
  poster_thumb_url: string | null
  /** Unix seconds. */
  last_requested: number
  image_counts: ImageCounts
}

export interface TitleList {
  items: TitleSummary[]
  total: number
}

/** A cached rendered image of a title (a row of `image_meta`). */
export interface TitleImage {
  kind: ImageKind
  id_type: string
  cache_value: string
  /** Unix seconds. */
  created_at: number
  /** Path of the existing admin image endpoint; needs the bearer token, so fetch it as a blob. */
  admin_image_url: string
}

/** An id this title has been requested under (the same title can be requested by imdb, tmdb and tvdb). */
export interface TitleAlias {
  id_type: string
  id_value: string
  season?: number | null
  episode?: number | null
}

export interface EpisodeEntry {
  episode: number
  name: string | null
  still_thumb_url: string | null
  cached: TitleImage[]
}

export interface SeasonEntry {
  season: number
  episodes: EpisodeEntry[]
}

export interface TitleDetail extends TitleSummary {
  tvdb_id?: number | null
  /** null: details not fetched yet; negative: the last TMDB details fetch failed (retried after a day). */
  resolved_at?: number | null
  aliases: TitleAlias[]
  images: TitleImage[]
  /** Only present for shows. */
  seasons?: SeasonEntry[]
}

export interface TitleListParams {
  q?: string
  type?: TitleTypeFilter
  sort?: TitleSort
  page?: number
  perPage?: number
}

export interface AliasTarget {
  media_type: MediaType
  tmdb_id: number
}

export const LIBRARY_PAGE_SIZE = 48

const BASE = '/api/admin/louk/titles'

/** Query string for the list endpoint; empty values are left out so URLs stay short. */
export function buildTitlesQuery(params: TitleListParams = {}): string {
  const qs = new URLSearchParams()
  const q = params.q?.trim()
  if (q) qs.set('q', q)
  if (params.type) qs.set('type', params.type)
  if (params.sort) qs.set('sort', params.sort)
  if (params.page && params.page > 1) qs.set('page', String(params.page))
  if (params.perPage) qs.set('per_page', String(params.perPage))
  const s = qs.toString()
  return s ? `?${s}` : ''
}

/** Reads the `error` field of a JSON error body, falling back to the raw text or the status. */
async function errorMessage(res: Response): Promise<string> {
  try {
    const text = await res.text()
    try {
      const body = JSON.parse(text)
      if (body && typeof body.error === 'string') return body.error
    } catch {
      // not JSON
    }
    if (text) return text
  } catch {
    // body unreadable
  }
  return `Request failed (${res.status})`
}

async function expectOk(res: Response): Promise<Response> {
  if (!res.ok) throw new Error(await errorMessage(res))
  return res
}

export async function listTitles(params: TitleListParams = {}): Promise<TitleList> {
  const res = await expectOk(await get(`${BASE}${buildTitlesQuery(params)}`))
  return res.json()
}

/** A show page can take a long time the first time it is opened (the server fetches episode names from TMDB). */
export const TITLE_TIMEOUT_MS = 90_000

/** Rejects when `promise` has not settled after `ms`; the request itself is left to finish on the server. */
function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), ms)
  })
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer))
}

const seg = (v: number | string) => encodeURIComponent(String(v))

export async function getTitle(
  mediaType: MediaType,
  tmdbId: number | string,
  timeoutMs: number = TITLE_TIMEOUT_MS,
): Promise<TitleDetail> {
  const res = await withTimeout(
    get(`${BASE}/${mediaType}/${seg(tmdbId)}`),
    timeoutMs,
    "The server is still busy with this title. A show's episode names are fetched from TMDB the first time it is opened; try again in a moment.",
  )
  await expectOk(res)
  return res.json()
}

/** Which title an id belongs to. Resolves to null when the id has not been indexed (HTTP 404). */
export async function resolveAlias(idType: string, idValue: string): Promise<AliasTarget | null> {
  const res = await get(`${BASE}/by-alias/${encodeURIComponent(idType)}/${encodeURIComponent(idValue)}`)
  if (res.status === 404) return null
  await expectOk(res)
  return res.json()
}

/** Body of a 200 response, or `{}` when it is not JSON. */
async function bodyOf(res: Response): Promise<Record<string, unknown>> {
  try {
    const body = await res.json()
    return body && typeof body === 'object' ? (body as Record<string, unknown>) : {}
  } catch {
    return {}
  }
}

/**
 * Purges every cached image of the title, under every id it was requested by.
 * The backend answers 200 with `ok: false` and an `errors` list when some ids failed.
 */
export async function purgeTitle(mediaType: MediaType, tmdbId: number | string): Promise<void> {
  const res = await expectOk(await post(`${BASE}/${mediaType}/${seg(tmdbId)}/purge`))
  const body = await bodyOf(res)
  if (body.ok === false) {
    const errors = Array.isArray(body.errors) ? body.errors.filter((e) => typeof e === 'string') : []
    throw new Error(errors.length ? errors.join('; ') : 'Some images could not be purged')
  }
}

/**
 * Renders the title's images that are not cached yet: one kind, or poster, logo and backdrop when `kind` is
 * left out. Episode stills are not part of this (the backend answers 400); they are re-fetched per episode.
 * The backend answers 200 with `ok: false` and per-kind results when some kinds failed.
 */
export async function refetchTitle(
  mediaType: MediaType,
  tmdbId: number | string,
  kind?: Exclude<ImageKind, 'episode'>,
): Promise<void> {
  const qs = kind ? `?kind=${kind}` : ''
  const res = await expectOk(await post(`${BASE}/${mediaType}/${seg(tmdbId)}/refetch${qs}`))
  const body = await bodyOf(res)
  if (body.ok === false) {
    const results = Array.isArray(body.results) ? (body.results as Record<string, unknown>[]) : []
    const failed = results
      .filter((r) => r && r.ok === false)
      .map((r) => `${String(r.kind)}: ${typeof r.error === 'string' && r.error ? r.error : 'failed'}`)
    throw new Error(failed.length ? failed.join('; ') : 'Some images could not be fetched')
  }
}

/** Loads one cached image (needs the bearer token, so it cannot be an <img src>). */
export async function fetchImageBlob(adminImageUrl: string): Promise<Blob> {
  const res = await expectOk(await get(adminImageUrl))
  return res.blob()
}

/** Purges exactly one rendered variant through the existing per-kind admin endpoint. */
export async function purgeImageVariant(image: Pick<TitleImage, 'kind' | 'id_type' | 'cache_value'>): Promise<void> {
  const scope: PurgeScope = 'variant'
  const fn = {
    poster: adminApi.purgePoster,
    logo: adminApi.purgeLogo,
    backdrop: adminApi.purgeBackdrop,
    episode: adminApi.purgeEpisode,
  }[image.kind]
  await expectOk(await fn(image.id_type, image.cache_value, scope))
}

// --- small display helpers (kept here so views stay free of contract knowledge) ---

/** Display name; an unresolved title falls back to its id. */
export function titleLabel(t: Pick<TitleSummary, 'name' | 'media_type' | 'tmdb_id'>): string {
  return t.name?.trim() || `tmdb ${t.tmdb_id}`
}

export function tmdbUrl(t: Pick<TitleSummary, 'media_type' | 'tmdb_id'>): string {
  return `https://www.themoviedb.org/${t.media_type}/${t.tmdb_id}`
}

export function imdbUrl(imdbId: string): string {
  return `https://www.imdb.com/title/${imdbId}/`
}

/** Route location of a title page. */
export function titleRoute(t: Pick<TitleSummary, 'media_type' | 'tmdb_id'>) {
  return { name: 'library-title', params: { mediaType: t.media_type, tmdbId: String(t.tmdb_id) } }
}
