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

export async function getTitle(mediaType: MediaType, tmdbId: number | string): Promise<TitleDetail> {
  const res = await expectOk(await get(`${BASE}/${mediaType}/${tmdbId}`))
  return res.json()
}

/** Which title an id belongs to. Resolves to null when the id has not been indexed (HTTP 404). */
export async function resolveAlias(idType: string, idValue: string): Promise<AliasTarget | null> {
  const res = await get(`${BASE}/by-alias/${encodeURIComponent(idType)}/${encodeURIComponent(idValue)}`)
  if (res.status === 404) return null
  await expectOk(res)
  return res.json()
}

/** Purges every cached image of the title, under every id it was requested by. */
export async function purgeTitle(mediaType: MediaType, tmdbId: number | string): Promise<void> {
  await expectOk(await post(`${BASE}/${mediaType}/${tmdbId}/purge`))
}

/** Re-renders cached images of the title: one kind, or every kind when `kind` is left out. */
export async function refetchTitle(
  mediaType: MediaType,
  tmdbId: number | string,
  kind?: ImageKind,
): Promise<void> {
  const qs = kind ? `?kind=${kind}` : ''
  await expectOk(await post(`${BASE}/${mediaType}/${tmdbId}/refetch${qs}`))
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
