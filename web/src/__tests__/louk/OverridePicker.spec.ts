import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import type { Candidate, Candidates, Override } from '@/louk/overrides'

vi.mock('@/louk/overrides', async (orig) => ({
  ...(await orig<typeof import('@/louk/overrides')>()),
  getCandidates: vi.fn(),
  putOverride: vi.fn(),
  deleteOverride: vi.fn(),
  uploadImage: vi.fn(),
}))
vi.mock('@/louk/api', async (orig) => ({
  ...(await orig<typeof import('@/louk/api')>()),
  fetchImageBlob: vi.fn(),
}))

import { deleteOverride, getCandidates, putOverride, uploadImage, MAX_UPLOAD_BYTES } from '@/louk/overrides'
import { fetchImageBlob } from '@/louk/api'
import OverridePicker from '@/louk/components/OverridePicker.vue'

// The real dialog renders in a portal; inline it so the test can see it.
const stubs = {
  Dialog: { template: '<div v-if="open"><slot /></div>', props: ['open'] },
  DialogContent: { template: '<div role="dialog"><slot /></div>' },
  DialogHeader: { template: '<div><slot /></div>' },
  DialogTitle: { template: '<h2><slot /></h2>' },
  DialogDescription: { template: '<p><slot /></p>' },
}

const logoTarget = { mediaType: 'tv', tmdbId: 62017, kind: 'logo' } as const
const stillTarget = { mediaType: 'tv', tmdbId: 62017, kind: 'episode', season: 1, episode: 2 } as const

function cand(over: Partial<Candidate>): Candidate {
  const value = over.value ?? '/a.png'
  return {
    source: 'tmdb',
    value,
    fanart_id: null,
    lang: 'en',
    votes: 3.3,
    vote_count: 3,
    likes: null,
    width: 2505,
    height: 1299,
    thumb_url: `https://image.tmdb.org/t/p/w300${value}`,
    full_url: `https://image.tmdb.org/t/p/original${value}`,
    auto: false,
    current: false,
    ...over,
  }
}

// The Man in the High Castle: three equal English logos, the automatic pick is the third; one fanart logo.
function highCastle(over: Partial<Candidates> = {}): Candidates {
  return {
    media_type: 'tv',
    tmdb_id: 62017,
    kind: 'logo',
    season: null,
    episode: null,
    lang: 'en',
    current: null,
    auto: { source: 'tmdb', value: '/c.png', fanart_id: null },
    candidates: [
      cand({ value: '/a.png' }),
      cand({ value: '/b.png' }),
      cand({ value: '/c.png', auto: true }),
      cand({
        source: 'fanart',
        value: 'https://assets.fanart.tv/fanart/tv/1/hdtvlogo/x.png',
        fanart_id: '12345',
        lang: 'de',
        votes: null,
        vote_count: null,
        likes: 4,
        width: null,
        height: null,
        thumb_url: 'https://assets.fanart.tv/preview/tv/1/hdtvlogo/x.png',
        full_url: 'https://assets.fanart.tv/fanart/tv/1/hdtvlogo/x.png',
      }),
    ],
    errors: [],
    ...over,
  }
}

function override(over: Partial<Override> = {}): Override {
  return {
    media_type: 'tv',
    tmdb_id: 62017,
    kind: 'logo',
    season: null,
    episode: null,
    lang: null,
    source: 'tmdb',
    value: '/a.png',
    fanart_id: null,
    no_badges: false,
    updated_at: 1,
    thumb_url: 'https://image.tmdb.org/t/p/w300/a.png',
    ...over,
  }
}

async function mountPicker(target: typeof logoTarget | typeof stillTarget = logoTarget, rendered = null as null | { admin_image_url: string }) {
  const wrapper = mount(OverridePicker, {
    props: { open: true, target, heading: 'The Man in the High Castle', rendered: rendered as never },
    global: { stubs },
  })
  await flushPromises()
  return wrapper
}

const tiles = (w: ReturnType<typeof mount>) => w.findAll('[data-testid="candidate"]')

describe('OverridePicker', () => {
  beforeEach(() => {
    vi.mocked(getCandidates).mockReset().mockResolvedValue(highCastle())
    vi.mocked(putOverride).mockReset().mockResolvedValue({ override: override(), purge: {} as never })
    vi.mocked(deleteOverride).mockReset().mockResolvedValue({ deleted: true, purge: null })
    vi.mocked(uploadImage).mockReset()
    vi.mocked(fetchImageBlob).mockReset().mockResolvedValue(new Blob(['x']))
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: vi.fn(() => 'blob:local'), revokeObjectURL: vi.fn() }))
  })

  it('shows a loading state while the candidates are fetched', async () => {
    let resolve!: (c: Candidates) => void
    vi.mocked(getCandidates).mockReturnValue(new Promise((r) => (resolve = r)))
    const w = await mountPicker()
    expect(w.find('[data-testid="loading"]').exists()).toBe(true)
    resolve(highCastle())
    await flushPromises()
    expect(w.find('[data-testid="loading"]').exists()).toBe(false)
    expect(tiles(w)).toHaveLength(4)
  })

  it('lists every candidate with the automatic and current markers', async () => {
    vi.mocked(getCandidates).mockResolvedValue(
      highCastle({
        current: override({ value: '/a.png' }),
        candidates: highCastle().candidates.map((c) => (c.value === '/a.png' ? { ...c, current: true } : c)),
      }),
    )
    const w = await mountPicker()
    const t = tiles(w)
    expect(t).toHaveLength(4)
    expect(t[0]!.find('[data-testid="mark-current"]').exists()).toBe(true)
    expect(t[0]!.find('[data-testid="mark-auto"]').exists()).toBe(false)
    expect(t[2]!.find('[data-testid="mark-auto"]').exists()).toBe(true)
    expect(t[2]!.text()).toContain('Automatic pick')
    expect(t[1]!.find('[data-testid="mark-auto"]').exists()).toBe(false)
    // thumbnails, language, votes and size are visible; the label is descriptive for screen readers
    expect(t[0]!.find('img').attributes('src')).toBe('https://image.tmdb.org/t/p/w300/a.png')
    expect(t[0]!.text()).toContain('TMDB')
    expect(t[0]!.text()).toContain('English (en)')
    expect(t[0]!.text()).toContain('3.3 from 3 votes')
    expect(t[0]!.text()).toContain('2505×1299')
    expect(t[0]!.attributes('aria-label')).toContain('current override')
    expect(t[2]!.attributes('aria-label')).toContain('automatic pick')
    expect(t[3]!.text()).toContain('4 likes')
    expect(t[3]!.text()).toContain('German (de)')
  })

  it('keeps the actions in a footer outside the scrolling body, and the grid is one tab stop', async () => {
    const w = await mountPicker()
    const footer = w.find('[data-testid="footer"]')
    expect(footer.find('[data-testid="save"]').exists()).toBe(true)
    expect(footer.find('[data-testid="no-badges"]').exists()).toBe(true)
    expect(w.find('[data-testid="body"]').find('[data-testid="footer"]').exists()).toBe(false)
    const t = tiles(w)
    expect(t.filter((b) => b.attributes('tabindex') === '0')).toHaveLength(1)
    expect(t[0]!.attributes('tabindex')).toBe('0')
    await t[2]!.trigger('focus')
    expect(tiles(w)[2]!.attributes('tabindex')).toBe('0')
    expect(tiles(w)[0]!.attributes('tabindex')).toBe('-1')
  })

  it('hides "0.0 from 0 votes"', async () => {
    vi.mocked(getCandidates).mockResolvedValue(highCastle({ candidates: [cand({ votes: 0, vote_count: 0 })] }))
    const w = await mountPicker()
    expect(tiles(w)[0]!.text()).toContain('no votes')
    expect(tiles(w)[0]!.text()).not.toContain('0.0')
  })

  it('filters by language and by source', async () => {
    const w = await mountPicker()
    const select = w.find('select')
    await select.setValue('de')
    expect(tiles(w)).toHaveLength(1)
    expect(tiles(w)[0]!.text()).toContain('Fanart')
    await select.setValue('all')
    const fanartButton = w.findAll('button').find((b) => b.text() === 'Fanart')!
    await fanartButton.trigger('click')
    expect(tiles(w)).toHaveLength(1)
    const tmdbButton = w.findAll('button').find((b) => b.text() === 'TMDB')!
    await tmdbButton.trigger('click')
    expect(tiles(w)).toHaveLength(3)
  })

  it('previews before and after, then sends the exact PUT body for a TMDB candidate', async () => {
    const w = await mountPicker(logoTarget, { admin_image_url: '/api/admin/logos/imdb/tt1/image' })
    expect(fetchImageBlob).toHaveBeenCalledWith('/api/admin/logos/imdb/tt1/image')
    expect(w.find('[data-testid="before"] img').exists()).toBe(true)
    expect(w.find('[data-testid="after"] img').exists()).toBe(false)
    expect(w.find('[data-testid="save"]').attributes('disabled')).toBeDefined()

    await tiles(w)[1]!.trigger('click')
    expect(tiles(w)[1]!.attributes('aria-pressed')).toBe('true')
    expect(w.find('[data-testid="after"] img').attributes('src')).toBe('https://image.tmdb.org/t/p/w500/b.png')

    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledTimes(1)
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'tmdb', value: '/b.png', no_badges: false }, undefined)
    expect(w.emitted('update:open')![0]).toEqual([false])
    const changed = w.emitted('changed')![0]![0] as { kind: string; message: string; redrawable: boolean }
    expect(changed.kind).toBe('logo')
    expect(changed.redrawable).toBe(true)
  })

  it('says when a language-specific override still wins over the saved one', async () => {
    vi.mocked(putOverride).mockResolvedValue({
      override: override(),
      purge: {} as never,
      shadowed_by: [override({ lang: 'en' })],
    })
    const w = await mountPicker(logoTarget)
    await tiles(w)[1]!.trigger('click')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    const changed = w.emitted('changed')![0]![0] as { message: string }
    expect(changed.message).toMatch(/override for .*en.* still wins for those requests/)
  })

  it('sends fanart_id for a fanart candidate, with the badge switch', async () => {
    const w = await mountPicker()
    await tiles(w)[3]!.trigger('click')
    await w.find('[data-testid="no-badges"]').trigger('click')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(
      logoTarget,
      { source: 'fanart', value: 'https://assets.fanart.tv/fanart/tv/1/hdtvlogo/x.png', fanart_id: '12345', no_badges: true },
      undefined,
    )
  })

  it('"no badges only" keeps the automatic art with source auto', async () => {
    const w = await mountPicker()
    await w.find('[data-testid="no-badges"]').trigger('click')
    expect(w.find('[data-testid="no-badges"]').attributes('aria-checked')).toBe('true')
    expect(w.find('[data-testid="save"]').text()).toContain('Save badge setting')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'auto', no_badges: true }, undefined)
    expect(deleteOverride).not.toHaveBeenCalled()
  })

  it('turning badges off on an existing override keeps its art', async () => {
    vi.mocked(getCandidates).mockResolvedValue(
      highCastle({
        current: override({ value: '/b.png' }),
        candidates: highCastle().candidates.map((c) => (c.value === '/b.png' ? { ...c, current: true } : c)),
      }),
    )
    const w = await mountPicker()
    await w.find('[data-testid="no-badges"]').trigger('click')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'tmdb', value: '/b.png', no_badges: true }, undefined)
  })

  it('clear override sends DELETE for the row in effect', async () => {
    vi.mocked(getCandidates).mockResolvedValue(highCastle({ current: override({ lang: 'en' }) }))
    const w = await mountPicker()
    await w.find('[data-testid="clear"]').trigger('click')
    await flushPromises()
    expect(deleteOverride).toHaveBeenCalledWith(logoTarget, 'en')
    expect(w.emitted('changed')).toHaveLength(1)
    expect(w.emitted('update:open')![0]).toEqual([false])
  })

  it('has no clear button without an override', async () => {
    const w = await mountPicker()
    expect(w.find('[data-testid="clear"]').exists()).toBe(false)
  })

  it('uploads a file as chosen art and sends it as source upload', async () => {
    vi.mocked(uploadImage).mockResolvedValue({ sha256: 'f'.repeat(64), format: 'png', width: 800, height: 310, bytes: 5, created: true, url: '/u' })
    const w = await mountPicker()
    const file = new File(['12345'], 'mine.png', { type: 'image/png' })
    const input = w.find('[data-testid="file-input"]')
    Object.defineProperty(input.element, 'files', { value: [file], configurable: true })
    await input.trigger('change')
    await flushPromises()
    expect(uploadImage).toHaveBeenCalledWith(file)
    expect(tiles(w)).toHaveLength(5)
    expect(tiles(w)[0]!.attributes('aria-pressed')).toBe('true')
    expect(w.find('[data-testid="after"] img').attributes('src')).toBe('blob:local')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'upload', value: 'f'.repeat(64), no_badges: false }, undefined)
  })

  it('shows the server message when an upload is rejected', async () => {
    vi.mocked(uploadImage).mockRejectedValue(new Error('image is 9000x10; at most 8000 px per side are accepted'))
    const w = await mountPicker()
    const input = w.find('[data-testid="file-input"]')
    Object.defineProperty(input.element, 'files', { value: [new File(['x'], 'wide.png', { type: 'image/png' })], configurable: true })
    await input.trigger('change')
    await flushPromises()
    expect(w.find('[data-testid="upload-error"]').text()).toContain('at most 8000 px per side')
    expect(tiles(w)).toHaveLength(4)
  })

  it('refuses wrong types and files over 15 MB before uploading', async () => {
    const w = await mountPicker()
    const input = w.find('[data-testid="file-input"]')
    const gif = new File(['x'], 'a.gif', { type: 'image/gif' })
    Object.defineProperty(input.element, 'files', { value: [gif], configurable: true })
    await input.trigger('change')
    await flushPromises()
    expect(w.find('[data-testid="upload-error"]').text()).toContain('PNG, JPEG and WebP')

    const big = new File(['x'], 'big.png', { type: 'image/png' })
    Object.defineProperty(big, 'size', { value: MAX_UPLOAD_BYTES + 1 })
    Object.defineProperty(input.element, 'files', { value: [big], configurable: true })
    await input.trigger('change')
    await flushPromises()
    expect(w.find('[data-testid="upload-error"]').text()).toContain('limited to 15 MB')
    expect(uploadImage).not.toHaveBeenCalled()
  })

  it('accepts a dropped file', async () => {
    vi.mocked(uploadImage).mockResolvedValue({ sha256: 'e'.repeat(64), format: 'webp', width: 10, height: 10, bytes: 5, created: false, url: '/u' })
    const w = await mountPicker()
    const file = new File(['12345'], 'drop.webp', { type: 'image/webp' })
    await w.find('[role="dialog"]').trigger('drop', { dataTransfer: { files: [file] } })
    await flushPromises()
    expect(uploadImage).toHaveBeenCalledWith(file)
  })

  it('shows a save error and keeps the dialog open', async () => {
    vi.mocked(putOverride).mockRejectedValue(new Error('no upload with sha256 abc'))
    const w = await mountPicker()
    await tiles(w)[0]!.trigger('click')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="action-error"]').text()).toContain('no upload with sha256 abc')
    expect(w.emitted('update:open')).toBeUndefined()
    expect(w.emitted('changed')).toBeUndefined()
  })

  it('shows an error with retry when loading fails', async () => {
    vi.mocked(getCandidates).mockRejectedValueOnce(new Error('TMDB is down')).mockResolvedValue(highCastle())
    const w = await mountPicker()
    expect(w.find('[data-testid="load-error"]').text()).toContain('TMDB is down')
    await w.find('[data-testid="load-error"] button').trigger('click')
    await flushPromises()
    expect(tiles(w)).toHaveLength(4)
  })

  it('still allows an upload when there are no candidates', async () => {
    vi.mocked(getCandidates).mockResolvedValue(highCastle({ candidates: [], auto: null }))
    const w = await mountPicker()
    expect(w.find('[data-testid="empty"]').text()).toContain('Upload your own')
    expect(w.find('[data-testid="upload-button"]').attributes('disabled')).toBeUndefined()
  })

  it('reports partial source failures', async () => {
    vi.mocked(getCandidates).mockResolvedValue(highCastle({ errors: ['fanart: timed out'] }))
    const w = await mountPicker()
    expect(w.find('[data-testid="source-errors"]').text()).toContain('fanart: timed out')
    expect(tiles(w)).toHaveLength(4)
  })

  it('episode still override passes season and episode, and offers no language scope', async () => {
    vi.mocked(getCandidates).mockResolvedValue(
      highCastle({ kind: 'episode', season: 1, episode: 2, candidates: [cand({ value: '/s1e2.jpg', auto: true })] }),
    )
    const w = await mountPicker(stillTarget)
    expect(getCandidates).toHaveBeenCalledWith(stillTarget, undefined)
    expect(w.find('[data-testid="scope"]').exists()).toBe(false)
    expect(w.text()).toContain('Override the episode still')
    await tiles(w)[0]!.trigger('click')
    expect(w.find('[data-testid="after"] img').attributes('src')).toBe('https://image.tmdb.org/t/p/w780/s1e2.jpg')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(stillTarget, { source: 'tmdb', value: '/s1e2.jpg', no_badges: false }, undefined)
    expect((w.emitted('changed')![0]![0] as { redrawable: boolean }).redrawable).toBe(false)
  })

  it('a language scope is sent as lang on save and reloads the candidates', async () => {
    const w = await mountPicker()
    await w.find('[data-testid="scope"]').setValue('de')
    await flushPromises()
    expect(getCandidates).toHaveBeenLastCalledWith(logoTarget, 'de')
    await tiles(w)[0]!.trigger('click')
    await w.find('[data-testid="save"]').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'tmdb', value: '/a.png', no_badges: false }, 'de')
  })
})
