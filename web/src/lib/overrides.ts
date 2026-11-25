// Typed fetchers for the per-title art overrides (REST contract: docs/api.md "Per-title art overrides").
// The picker (components/OverridePicker.vue) only talks to this file.
import { del, get, put } from '@/lib/api'
import { useAuthStore } from '@/stores/auth'

export type MediaType = 'movie' | 'tv'
export type ImageKind = 'poster' | 'logo' | 'backdrop' | 'episode'

export type OverrideSource = 'auto' | 'tmdb' | 'fanart' | 'upload'
export type CandidateSource = 'tmdb' | 'fanart'

export interface Override {
  media_type: MediaType
  tmdb_id: number
  kind: ImageKind
  season: number | null
  episode: number | null
  lang: string | null
  source: OverrideSource
  value: string
  fanart_id: string | null
  no_badges: boolean
  /** Unix seconds. */
  updated_at: number
  /** Public URL, or a root-relative admin path (uploads) that needs the bearer token. null for `auto`. */
  thumb_url: string | null
}

export interface Candidate {
  source: CandidateSource
  value: string
  fanart_id: string | null
  /** null = language-less. */
  lang: string | null
  /** TMDB vote_average. */
  votes: number | null
  vote_count: number | null
  /** fanart.tv likes. */
  likes: number | null
  width: number | null
  height: number | null
  thumb_url: string
  full_url: string
  auto: boolean
  current: boolean
}

export interface AutoPick {
  source: CandidateSource
  value: string
  fanart_id: string | null
}

export interface Candidates {
  media_type: MediaType
  tmdb_id: number
  kind: ImageKind
  season: number | null
  episode: number | null
  lang: string
  current: Override | null
  auto: AutoPick | null
  candidates: Candidate[]
  /** TMDB/fanart failures; the list can still be partly filled. */
  errors: string[]
}

export interface PurgeResult {
  ids: string[]
  files_deleted: number
  meta_deleted: number
  errors: string[]
  external_cache_only: boolean
}

export interface UploadResult {
  sha256: string
  format: 'png' | 'jpeg' | 'webp'
  width: number
  height: number
  bytes: number
  created: boolean
  url: string
}

/** What to send in the PUT body. `value` is left out for `auto`. */
export interface OverrideBody {
  source: OverrideSource
  value?: string
  fanart_id?: string
  no_badges?: boolean
}

/** Which image of a title an override or candidate list is about. */
export interface OverrideTarget {
  mediaType: MediaType
  tmdbId: number
  kind: ImageKind
  season?: number | null
  episode?: number | null
}

export const MAX_UPLOAD_BYTES = 15 * 1024 * 1024
export const UPLOAD_TYPES = ['image/png', 'image/jpeg', 'image/webp'] as const

const BASE = '/api/admin'

/** `?season=&episode=&lang=` for a target; only what the key needs is sent. */
export function targetQuery(t: Pick<OverrideTarget, 'kind' | 'season' | 'episode'>, lang?: string | null): string {
  const qs = new URLSearchParams()
  if (t.kind === 'episode') {
    qs.set('season', String(t.season ?? 0))
    qs.set('episode', String(t.episode ?? 0))
  }
  // Backdrops and stills are language-agnostic; the backend refuses a language for them on write routes.
  if (lang) qs.set('lang', lang)
  const s = qs.toString()
  return s ? `?${s}` : ''
}

const seg = (v: number | string) => encodeURIComponent(String(v))
const route = (t: OverrideTarget) => `${BASE}/overrides/${seg(t.mediaType)}/${seg(t.tmdbId)}/${seg(t.kind)}`
export const kindHasLanguage = (kind: ImageKind) => kind === 'poster' || kind === 'logo'

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
    // unreadable body
  }
  return `Request failed (${res.status})`
}

async function ok(res: Response): Promise<Response> {
  if (!res.ok) throw new Error(await errorMessage(res))
  return res
}

/** The override key of a cached image id (`imdb/tt0111161`, `tmdb/episode-1399-S1E2`, …). */
export async function resolveTarget(idType: string, idValue: string, kind: ImageKind): Promise<OverrideTarget> {
  const res = await ok(await get(`${BASE}/resolve/${seg(idType)}/${seg(idValue)}?kind=${seg(kind)}`))
  const body: { media_type: MediaType; tmdb_id: number; kind: ImageKind; season: number | null; episode: number | null } =
    await res.json()
  return { mediaType: body.media_type, tmdbId: body.tmdb_id, kind: body.kind, season: body.season, episode: body.episode }
}

export async function getCandidates(t: OverrideTarget, lang?: string | null): Promise<Candidates> {
  const res = await ok(await get(`${route(t)}/candidates${targetQuery(t, kindHasLanguage(t.kind) ? lang : null)}`))
  return res.json()
}

export async function putOverride(
  t: OverrideTarget,
  body: OverrideBody,
  lang?: string | null,
): Promise<{ override: Override; purge: PurgeResult; shadowed_by?: Override[] }> {
  const res = await ok(await put(`${route(t)}${targetQuery(t, kindHasLanguage(t.kind) ? lang : null)}`, body))
  return res.json()
}

export async function deleteOverride(
  t: OverrideTarget,
  lang?: string | null,
): Promise<{ deleted: boolean; purge: PurgeResult | null }> {
  const res = await ok(await del(`${route(t)}${targetQuery(t, kindHasLanguage(t.kind) ? lang : null)}`))
  return res.json()
}

/** Client-side mirror of the server's upload rules; returns a message, or null when the file is acceptable. */
export function checkUploadFile(file: Pick<File, 'type' | 'size' | 'name'>): string | null {
  const type = file.type.toLowerCase()
  // Some drag sources give no MIME type: judge by the extension then (the server sniffs the bytes anyway).
  const byName = /\.(png|jpe?g|webp)$/i.test(file.name)
  if (type ? !(UPLOAD_TYPES as readonly string[]).includes(type) : !byName) {
    return 'Only PNG, JPEG and WebP images can be uploaded.'
  }
  if (file.size > MAX_UPLOAD_BYTES) {
    return `This file is ${formatBytes(file.size)}. Uploads are limited to 15 MB.`
  }
  if (file.size === 0) return 'This file is empty.'
  return null
}

/**
 * Uploads a file as the raw request body (not multipart). Uses `fetch` directly because the shared helpers
 * always send JSON; it adds the bearer token and retries once after a token refresh, like they do.
 */
export async function uploadImage(file: File): Promise<UploadResult> {
  const base = import.meta.env.VITE_API_URL || ''
  const auth = useAuthStore()
  const send = () => {
    const headers = new Headers()
    if (auth.token) headers.set('Authorization', `Bearer ${auth.token}`)
    headers.set('Content-Type', file.type || 'application/octet-stream')
    return fetch(`${base}${BASE}/uploads`, { method: 'POST', headers, body: file })
  }
  let res = await send()
  if (res.status === 401 && auth.token && (await auth.refresh())) res = await send()
  await ok(res)
  return res.json()
}

// --- display helpers ---

export function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  if (n >= 1024) return `${Math.round(n / 1024)} KB`
  return `${n} B`
}

const LANG_NAMES = (() => {
  try {
    return new Intl.DisplayNames(['en'], { type: 'language' })
  } catch {
    return null
  }
})()

/**
 * Sentence for a PUT response whose row is overridden by more specific language rows
 * (`shadowed_by`), or '' when nothing shadows it.
 */
export function shadowNote(shadowedBy: Pick<Override, 'lang'>[] | undefined): string {
  const langs = (shadowedBy ?? []).map((o) => o.lang).filter((l): l is string => !!l)
  if (!langs.length) return ''
  const names = langs.map((l) => languageLabel(l)).join(', ')
  return langs.length === 1
    ? ` The override for ${names} still wins for those requests; clear it to use this one there too.`
    : ` The overrides for ${names} still win for those requests; clear them to use this one there too.`
}

/** "English (en)", "No language" for null. */
export function languageLabel(lang: string | null | undefined): string {
  if (!lang) return 'No language'
  let name: string | undefined
  try {
    name = LANG_NAMES?.of(lang)
  } catch {
    name = undefined
  }
  return name && name !== lang ? `${name} (${lang})` : lang
}

export function sizeLabel(c: Pick<Candidate, 'width' | 'height'>): string | null {
  return c.width && c.height ? `${c.width}×${c.height}` : null
}

/** "3.3 from 3 votes" (TMDB), "4 likes" (fanart), null when the source gave nothing. */
export function scoreLabel(c: Pick<Candidate, 'votes' | 'vote_count' | 'likes'> & { source: string }): string | null {
  if (c.source === 'fanart') {
    return c.likes == null ? null : `${c.likes} ${c.likes === 1 ? 'like' : 'likes'}`
  }
  if (c.votes == null) return null
  const n = c.vote_count
  return n == null ? `${c.votes.toFixed(1)} votes` : `${c.votes.toFixed(1)} from ${n} ${n === 1 ? 'vote' : 'votes'}`
}

export const sourceLabel = (s: OverrideSource | CandidateSource) =>
  ({ tmdb: 'TMDB', fanart: 'Fanart', upload: 'Upload', auto: 'Automatic' })[s]

/** Same art? (an override row compared with a candidate) */
export function sameArt(a: { source: string; value: string }, b: { source: string; value: string }): boolean {
  return a.source === b.source && a.value === b.value
}
