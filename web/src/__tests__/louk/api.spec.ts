import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const mockAuthStore = { token: 'test-token', refresh: vi.fn(), logout: vi.fn() }
vi.mock('@/stores/auth', () => ({ useAuthStore: () => mockAuthStore }))

import {
  buildTitlesQuery,
  fetchImageBlob,
  getTitle,
  listTitles,
  purgeImageVariant,
  purgeTitle,
  refetchTitle,
  resolveAlias,
  titleLabel,
} from '@/louk/api'

function res(status: number, body: unknown = {}) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
    text: () => Promise.resolve(typeof body === 'string' ? body : JSON.stringify(body)),
    blob: () => Promise.resolve(new Blob(['x'])),
  }
}

describe('louk api', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.restoreAllMocks()
    mockAuthStore.token = 'test-token'
  })

  it('louk api builds query string', () => {
    expect(buildTitlesQuery()).toBe('')
    expect(buildTitlesQuery({ q: '  ', type: '', page: 1 })).toBe('')
    expect(buildTitlesQuery({ q: 'the matrix', type: 'movie', sort: 'name', page: 3, perPage: 48 })).toBe(
      '?q=the+matrix&type=movie&sort=name&page=3&per_page=48',
    )
  })

  it('listTitles calls the list endpoint with the bearer token', async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(200, { items: [], total: 0 }))
    vi.stubGlobal('fetch', fetchMock)

    const out = await listTitles({ q: 'ma', type: 'tv', perPage: 48 })

    expect(out).toEqual({ items: [], total: 0 })
    const [url, options] = fetchMock.mock.calls[0]
    expect(url).toBe('/api/admin/louk/titles?q=ma&type=tv&per_page=48')
    expect(options.headers.get('Authorization')).toBe('Bearer test-token')
  })

  it('getTitle, purgeTitle and refetchTitle use the title paths', async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(200, { name: 'x' }))
    vi.stubGlobal('fetch', fetchMock)

    await getTitle('tv', 1399)
    await purgeTitle('tv', 1399)
    await refetchTitle('tv', 1399, 'poster')
    await refetchTitle('movie', '603')

    const calls = fetchMock.mock.calls.map(([url, o]) => [o.method ?? 'GET', url])
    expect(calls).toEqual([
      ['GET', '/api/admin/louk/titles/tv/1399'],
      ['POST', '/api/admin/louk/titles/tv/1399/purge'],
      ['POST', '/api/admin/louk/titles/tv/1399/refetch?kind=poster'],
      ['POST', '/api/admin/louk/titles/movie/603/refetch'],
    ])
  })

  it('purgeTitle and refetchTitle throw the backend message when the body says ok:false', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(res(200, { ok: false, errors: ['imdb/tt1 poster: disk error', 'tmdb/movie-1 logo: boom'] }))
        .mockResolvedValueOnce(
          res(200, {
            ok: false,
            results: [
              { kind: 'poster', ok: true, error: null },
              { kind: 'logo', ok: false, error: 'no logo on TMDB' },
            ],
          }),
        )
        .mockResolvedValueOnce(res(200, { ok: true, results: [] })),
    )
    await expect(purgeTitle('movie', 1)).rejects.toThrow('imdb/tt1 poster: disk error; tmdb/movie-1 logo: boom')
    await expect(refetchTitle('movie', 1)).rejects.toThrow('logo: no logo on TMDB')
    await expect(refetchTitle('movie', 1, 'logo')).resolves.toBeUndefined()
  })

  it('getTitle gives up after the timeout with an explanation', async () => {
    vi.stubGlobal('fetch', vi.fn(() => new Promise(() => {})))
    await expect(getTitle('tv', 1399, 20)).rejects.toThrow('still busy with this title')
  })

  it('resolveAlias returns the target, null on 404 and throws on other errors', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(res(200, { media_type: 'tv', tmdb_id: 1399 }))
      .mockResolvedValueOnce(res(404, { error: 'not found' }))
      .mockResolvedValueOnce(res(500, { error: 'db is locked' }))
    vi.stubGlobal('fetch', fetchMock)

    expect(await resolveAlias('imdb', 'episode-tt0944947-S1E2')).toEqual({ media_type: 'tv', tmdb_id: 1399 })
    expect(fetchMock.mock.calls[0]![0]).toBe('/api/admin/louk/titles/by-alias/imdb/episode-tt0944947-S1E2')
    expect(await resolveAlias('imdb', 'tt404')).toBeNull()
    await expect(resolveAlias('imdb', 'tt1')).rejects.toThrow('db is locked')
  })

  it('errors carry the server message or the status', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(res(500, 'boom')).mockResolvedValueOnce({ ...res(502), text: () => Promise.resolve('') }))
    await expect(listTitles()).rejects.toThrow('boom')
    await expect(listTitles()).rejects.toThrow('Request failed (502)')
  })

  it('purgeImageVariant uses the per-kind admin endpoint with scope=variant', async () => {
    const fetchMock = vi.fn().mockResolvedValue(res(200))
    vi.stubGlobal('fetch', fetchMock)

    await purgeImageVariant({ kind: 'backdrop', id_type: 'tmdb', cache_value: 'movie-603@ir.zl' })
    await purgeImageVariant({ kind: 'poster', id_type: 'imdb', cache_value: 'tt1_t@ir' })

    expect(fetchMock.mock.calls[0]![0]).toBe('/api/admin/backdrops/tmdb/movie-603%40ir.zl?scope=variant')
    expect(fetchMock.mock.calls[0]![1].method).toBe('DELETE')
    expect(fetchMock.mock.calls[1]![0]).toBe('/api/admin/posters/imdb/tt1_t%40ir?scope=variant')
  })

  it('fetchImageBlob returns the bytes and throws when the image is gone', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(res(200)).mockResolvedValueOnce(res(404, { error: 'gone' })))
    expect(await fetchImageBlob('/api/admin/posters/imdb/tt1/image')).toBeInstanceOf(Blob)
    await expect(fetchImageBlob('/x')).rejects.toThrow('gone')
  })

  it('titleLabel falls back to the id for unresolved titles', () => {
    expect(titleLabel({ name: null, media_type: 'movie', tmdb_id: 7 })).toBe('tmdb 7')
    expect(titleLabel({ name: 'Heat', media_type: 'movie', tmdb_id: 949 })).toBe('Heat')
  })
})
