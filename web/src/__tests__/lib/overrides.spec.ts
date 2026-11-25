import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const mockAuthStore = { token: 'test-token', refresh: vi.fn(), logout: vi.fn() }
vi.mock('@/stores/auth', () => ({ useAuthStore: () => mockAuthStore }))

import {
  MAX_UPLOAD_BYTES,
  checkUploadFile,
  deleteOverride,
  getCandidates,
  putOverride,
  resolveTarget,
  scoreLabel,
  shadowNote,
  targetQuery,
  uploadImage,
} from '@/lib/overrides'

function res(status: number, body: unknown = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(typeof body === 'string' ? body : JSON.stringify(body)),
  }
}

const logo = { mediaType: 'tv', tmdbId: 62017, kind: 'logo' } as const
const still = { mediaType: 'tv', tmdbId: 62017, kind: 'episode', season: 1, episode: 2 } as const

describe('overrides api', () => {
  const fetchMock = vi.fn()
  beforeEach(() => {
    setActivePinia(createPinia())
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
    mockAuthStore.token = 'test-token'
  })
  afterEach(() => vi.unstubAllGlobals())

  it('builds the key query: season and episode for stills, lang only when given', () => {
    expect(targetQuery(logo)).toBe('')
    expect(targetQuery(logo, 'en')).toBe('?lang=en')
    expect(targetQuery(still)).toBe('?season=1&episode=2')
    expect(targetQuery({ kind: 'episode', season: 0, episode: 5 })).toBe('?season=0&episode=5')
  })

  it('getCandidates calls the candidates route, with lang for logos and posters only', async () => {
    fetchMock.mockResolvedValue(res(200, { candidates: [] }))
    await getCandidates(logo, 'pt-BR')
    expect(fetchMock.mock.calls[0]![0]).toBe('/api/admin/overrides/tv/62017/logo/candidates?lang=pt-BR')
    await getCandidates({ mediaType: 'tv', tmdbId: 62017, kind: 'backdrop' }, 'en')
    expect(fetchMock.mock.calls[1]![0]).toBe('/api/admin/overrides/tv/62017/backdrop/candidates')
    await getCandidates(still)
    expect(fetchMock.mock.calls[2]![0]).toBe('/api/admin/overrides/tv/62017/episode/candidates?season=1&episode=2')
  })

  it('putOverride sends the JSON body of the contract', async () => {
    fetchMock.mockResolvedValue(res(200, { override: {}, purge: {} }))
    await putOverride(logo, { source: 'tmdb', value: '/gd.png', no_badges: false })
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toBe('/api/admin/overrides/tv/62017/logo')
    expect(init.method).toBe('PUT')
    expect(JSON.parse(init.body)).toEqual({ source: 'tmdb', value: '/gd.png', no_badges: false })

    await putOverride(still, { source: 'auto', no_badges: true })
    expect(fetchMock.mock.calls[1]![0]).toBe('/api/admin/overrides/tv/62017/episode?season=1&episode=2')
    expect(JSON.parse(fetchMock.mock.calls[1]![1].body)).toEqual({ source: 'auto', no_badges: true })
  })

  it('putOverride surfaces the server error message', async () => {
    fetchMock.mockResolvedValue(res(400, { error: 'no upload with sha256 abc' }))
    await expect(putOverride(logo, { source: 'upload', value: 'abc' })).rejects.toThrow('no upload with sha256 abc')
  })

  it('deleteOverride calls DELETE on the key', async () => {
    fetchMock.mockResolvedValue(res(200, { deleted: true, purge: null }))
    await deleteOverride(still)
    expect(fetchMock.mock.calls[0]![0]).toBe('/api/admin/overrides/tv/62017/episode?season=1&episode=2')
    expect(fetchMock.mock.calls[0]![1].method).toBe('DELETE')
    await deleteOverride({ mediaType: 'movie', tmdbId: 603, kind: 'poster' }, 'pt')
    expect(fetchMock.mock.calls[1]![0]).toBe('/api/admin/overrides/movie/603/poster?lang=pt')
  })

  it('uploadImage posts the raw file as the body with the bearer token', async () => {
    const result = { sha256: 'a'.repeat(64), format: 'png', width: 8, height: 8, bytes: 3, created: true, url: '/x' }
    fetchMock.mockResolvedValue(res(200, result))
    const file = new File([new Uint8Array([1, 2, 3])], 'logo.png', { type: 'image/png' })
    await expect(uploadImage(file)).resolves.toEqual(result)
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toBe('/api/admin/uploads')
    expect(init.method).toBe('POST')
    expect(init.body).toBe(file)
    expect(init.body).not.toBeInstanceOf(FormData)
    expect(new Headers(init.headers).get('Authorization')).toBe('Bearer test-token')
  })

  it('uploadImage shows the server rejection', async () => {
    fetchMock.mockResolvedValue(res(400, { error: 'only PNG, JPEG and WebP images are accepted' }))
    const file = new File(['x'], 'a.png', { type: 'image/png' })
    await expect(uploadImage(file)).rejects.toThrow('only PNG, JPEG and WebP images are accepted')
    fetchMock.mockResolvedValue(res(413, { error: 'uploads are limited to 15 MiB' }))
    await expect(uploadImage(file)).rejects.toThrow('uploads are limited to 15 MiB')
  })

  it('checkUploadFile mirrors the server rules', () => {
    expect(checkUploadFile({ name: 'a.png', type: 'image/png', size: 10 })).toBeNull()
    expect(checkUploadFile({ name: 'a.webp', type: 'image/webp', size: MAX_UPLOAD_BYTES })).toBeNull()
    expect(checkUploadFile({ name: 'a.gif', type: 'image/gif', size: 10 })).toMatch(/PNG, JPEG and WebP/)
    expect(checkUploadFile({ name: 'a.svg', type: 'image/svg+xml', size: 10 })).toMatch(/PNG, JPEG and WebP/)
    expect(checkUploadFile({ name: 'a.png', type: 'image/png', size: MAX_UPLOAD_BYTES + 1 })).toMatch(/limited to 15 MB/)
    expect(checkUploadFile({ name: 'a.png', type: 'image/png', size: 0 })).toMatch(/empty/)
    // No MIME type (some drag sources): the extension decides, the server sniffs.
    expect(checkUploadFile({ name: 'logo.webp', type: '', size: 10 })).toBeNull()
    expect(checkUploadFile({ name: 'logo.JPG', type: '', size: 10 })).toBeNull()
    expect(checkUploadFile({ name: 'logo.gif', type: '', size: 10 })).toMatch(/PNG, JPEG and WebP/)
  })

  it('shadowNote names the language rows that still win', () => {
    expect(shadowNote(undefined)).toBe('')
    expect(shadowNote([])).toBe('')
    expect(shadowNote([{ lang: 'en' }])).toMatch(/^ The override for .*en.* still wins for those requests; clear it/)
    expect(shadowNote([{ lang: 'en' }, { lang: 'de' }])).toMatch(/overrides for .*en.*de.* still win/)
  })

  it('resolveTarget maps a cached image id to its override key', async () => {
    fetchMock.mockResolvedValue(res(200, { media_type: 'tv', tmdb_id: 62017, kind: 'episode', season: 1, episode: 2 }))
    await expect(resolveTarget('imdb', 'episode-tt1740299-S1E2', 'episode')).resolves.toEqual(still)
    expect(fetchMock.mock.calls[0]![0]).toBe('/api/admin/resolve/imdb/episode-tt1740299-S1E2?kind=episode')
    fetchMock.mockResolvedValue(res(404, { error: 'not found' }))
    await expect(resolveTarget('imdb', 'tt0', 'logo')).rejects.toThrow('not found')
  })

  it('formats scores', () => {
    expect(scoreLabel({ source: 'tmdb', votes: 3.334, vote_count: 3, likes: null })).toBe('3.3 from 3 votes')
    expect(scoreLabel({ source: 'fanart', votes: null, vote_count: null, likes: 1 })).toBe('1 like')
    expect(scoreLabel({ source: 'fanart', votes: null, vote_count: null, likes: null })).toBeNull()
  })
})
