import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import type { Candidate, Candidates, Override } from '@/lib/overrides'

vi.mock('@/lib/overrides', async (orig) => ({
  ...(await orig<typeof import('@/lib/overrides')>()),
  resolveTarget: vi.fn(),
  getCandidates: vi.fn(),
  putOverride: vi.fn(),
  deleteOverride: vi.fn(),
  uploadImage: vi.fn(),
}))

import { deleteOverride, getCandidates, putOverride, resolveTarget, uploadImage } from '@/lib/overrides'
import OverridePicker from '@/components/OverridePicker.vue'

// The real dialog renders in a portal; inline it so the test can see it.
const stubs = {
  Dialog: { template: '<div v-if="open"><slot /></div>', props: ['open'] },
  DialogContent: { template: '<div role="dialog"><slot /></div>' },
  DialogHeader: { template: '<div><slot /></div>' },
  DialogTitle: { template: '<h2><slot /></h2>' },
  DialogDescription: { template: '<p><slot /></p>' },
}

const logoTarget = { mediaType: 'tv', tmdbId: 62017, kind: 'logo', season: null, episode: null } as const

function cand(over: Partial<Candidate>): Candidate {
  const value = over.value ?? '/a.png'
  return {
    source: 'tmdb',
    value,
    fanart_id: null,
    lang: 'en',
    votes: 0,
    vote_count: 0,
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

// The Man in the High Castle: three English logos with equal votes (one is the automatic pick) and one fanart logo.
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
        value: 'https://assets.fanart.tv/fanart/tv/295829/hdtvlogo/x.png',
        fanart_id: '12345',
        lang: 'de',
        votes: null,
        vote_count: null,
        likes: 4,
        width: null,
        height: null,
        thumb_url: 'https://assets.fanart.tv/preview/tv/295829/hdtvlogo/x.png',
        full_url: 'https://assets.fanart.tv/fanart/tv/295829/hdtvlogo/x.png',
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

async function mountPicker() {
  const wrapper = mount(OverridePicker, {
    props: { open: true, kind: 'logo', idType: 'imdb', idValue: 'tt1740299' },
    global: { stubs },
  })
  await flushPromises()
  return wrapper
}

const tiles = (w: Awaited<ReturnType<typeof mountPicker>>) => w.findAll('[data-testid="override-tile"]')
const button = (w: Awaited<ReturnType<typeof mountPicker>>, text: string) =>
  w.findAll('button').find((b) => b.text().trim() === text)!

describe('OverridePicker', () => {
  beforeEach(() => {
    vi.mocked(resolveTarget).mockReset().mockResolvedValue(logoTarget)
    vi.mocked(getCandidates).mockReset().mockResolvedValue(highCastle())
    vi.mocked(putOverride).mockReset().mockResolvedValue({ override: override(), purge: {} as never, shadowed_by: [] })
    vi.mocked(deleteOverride).mockReset().mockResolvedValue({ deleted: true, purge: null })
    vi.mocked(uploadImage).mockReset()
  })

  it('resolves the cached id and lists the candidates with the automatic pick marked', async () => {
    const w = await mountPicker()
    expect(resolveTarget).toHaveBeenCalledWith('imdb', 'tt1740299', 'logo')
    expect(getCandidates).toHaveBeenCalledWith(logoTarget, undefined)
    expect(w.text()).toContain('tv/62017')
    expect(tiles(w)).toHaveLength(4)
    expect(tiles(w)[2]!.text()).toContain('Automatic pick')
    expect(button(w, 'Save').attributes('disabled')).toBeDefined()
    expect(w.findAll('button').some((b) => b.text().trim() === 'Clear override')).toBe(false)
  })

  it('saves the chosen candidate for every language and closes', async () => {
    const w = await mountPicker()
    await tiles(w)[0]!.trigger('click')
    await button(w, 'Save').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'tmdb', value: '/a.png', no_badges: false }, undefined)
    expect(w.emitted('changed')![0]![0]).toMatch(/^Set the logo to TMDB/)
    expect(w.emitted('update:open')![0]).toEqual([false])
  })

  it('sends the fanart id, the language scope and the no-badges flag', async () => {
    const w = await mountPicker()
    await w.find('[data-testid="override-scope"]').setValue('en')
    await flushPromises()
    expect(getCandidates).toHaveBeenLastCalledWith(logoTarget, 'en')
    await w.find('[data-testid="override-no-badges"]').setValue(true)
    await tiles(w)[3]!.trigger('click')
    await button(w, 'Save').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(
      logoTarget,
      {
        source: 'fanart',
        value: 'https://assets.fanart.tv/fanart/tv/295829/hdtvlogo/x.png',
        fanart_id: '12345',
        no_badges: true,
      },
      'en',
    )
  })

  it('turns badges off without changing the art (source auto)', async () => {
    const w = await mountPicker()
    await w.find('[data-testid="override-no-badges"]').setValue(true)
    await button(w, 'Save').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'auto', no_badges: true }, undefined)
  })

  it('clears the override in effect', async () => {
    vi.mocked(getCandidates).mockResolvedValue(
      highCastle({ current: override({ lang: 'en' }), candidates: [cand({ value: '/a.png', current: true })] }),
    )
    const w = await mountPicker()
    expect(tiles(w)[0]!.text()).toContain('Current')
    await button(w, 'Clear override').trigger('click')
    await flushPromises()
    expect(deleteOverride).toHaveBeenCalledWith(logoTarget, 'en')
    expect(w.emitted('changed')![0]![0]).toMatch(/Cleared the logo override/)
  })

  it('uploads a file and selects it', async () => {
    vi.stubGlobal('URL', { ...URL, createObjectURL: () => 'blob:x', revokeObjectURL: () => {} })
    vi.mocked(uploadImage).mockResolvedValue({
      sha256: 'a'.repeat(64),
      format: 'png',
      width: 800,
      height: 310,
      bytes: 2048,
      created: true,
      url: '/api/admin/uploads/' + 'a'.repeat(64),
    })
    const w = await mountPicker()
    const input = w.find('[data-testid="override-file"]')
    const file = new File([new Uint8Array([1, 2, 3])], 'logo.png', { type: 'image/png' })
    Object.defineProperty(input.element, 'files', { value: [file] })
    await input.trigger('change')
    await flushPromises()
    expect(uploadImage).toHaveBeenCalledWith(file)
    expect(tiles(w)[0]!.text()).toContain('Your upload, 800×310')
    expect(tiles(w)[0]!.attributes('aria-pressed')).toBe('true')
    await button(w, 'Save').trigger('click')
    await flushPromises()
    expect(putOverride).toHaveBeenCalledWith(logoTarget, { source: 'upload', value: 'a'.repeat(64), no_badges: false }, undefined)
    vi.unstubAllGlobals()
  })

  it('refuses a non-image file before uploading', async () => {
    const w = await mountPicker()
    const input = w.find('[data-testid="override-file"]')
    Object.defineProperty(input.element, 'files', { value: [new File(['x'], 'a.gif', { type: 'image/gif' })] })
    await input.trigger('change')
    await flushPromises()
    expect(uploadImage).not.toHaveBeenCalled()
    expect(w.text()).toContain('Only PNG, JPEG and WebP images can be uploaded.')
  })

  it('shows a load error with a retry', async () => {
    vi.mocked(resolveTarget).mockRejectedValueOnce(new Error('no movie or tv show found'))
    const w = await mountPicker()
    expect(w.text()).toContain('no movie or tv show found')
    await button(w, 'Retry').trigger('click')
    await flushPromises()
    expect(tiles(w)).toHaveLength(4)
  })

  it('keeps the dialog open when saving fails', async () => {
    vi.mocked(putOverride).mockRejectedValue(new Error('no upload with sha256 abc'))
    const w = await mountPicker()
    await tiles(w)[1]!.trigger('click')
    await button(w, 'Save').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('no upload with sha256 abc')
    expect(w.emitted('update:open')).toBeUndefined()
  })
})
