import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createRouter, createMemoryHistory } from 'vue-router'
import { showDetail, summary } from './fixtures'
import type { TitleDetail } from '@/louk/api'

vi.mock('@/louk/overrides', async (orig) => ({
  ...(await orig<typeof import('@/louk/overrides')>()),
  getCandidates: vi.fn(),
}))

vi.mock('@/louk/api', async (orig) => ({
  ...(await orig<typeof import('@/louk/api')>()),
  getTitle: vi.fn(),
  purgeTitle: vi.fn(),
  refetchTitle: vi.fn(),
  purgeImageVariant: vi.fn(),
  fetchImageBlob: vi.fn(),
}))

import { fetchImageBlob, getTitle, purgeImageVariant, purgeTitle, refetchTitle } from '@/louk/api'
import { getCandidates } from '@/louk/overrides'
import TitleView from '@/louk/views/TitleView.vue'

// Dialogs render in a portal in the real app; inline them so the test can see them.
const stubs = {
  Dialog: { template: '<div v-if="open"><slot /></div>', props: ['open'] },
  DialogContent: { template: '<div role="dialog"><slot /></div>' },
  DialogHeader: { template: '<div><slot /></div>' },
  DialogTitle: { template: '<h2><slot /></h2>' },
  DialogDescription: { template: '<p><slot /></p>' },
}

async function mountView(path: string) {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/library', name: 'library', component: { template: '<div />' } },
      { path: '/admin/library/:mediaType/:tmdbId', name: 'library-title', component: TitleView },
    ],
  })
  router.push(path)
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(TitleView, { global: { plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]], stubs } })
  await flushPromises()
  return wrapper
}

const buttonByText = (w: ReturnType<typeof mount>, text: string) => w.findAll('button').find((b) => b.text().includes(text))!

describe('TitleView', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(getTitle).mockReset()
    vi.mocked(purgeTitle).mockReset().mockResolvedValue()
    vi.mocked(refetchTitle).mockReset().mockResolvedValue()
    vi.mocked(purgeImageVariant).mockReset().mockResolvedValue()
    vi.mocked(fetchImageBlob).mockReset().mockResolvedValue(new Blob(['x']))
    vi.stubGlobal('URL', Object.assign(URL, { createObjectURL: vi.fn(() => 'blob:x'), revokeObjectURL: vi.fn() }))
  })

  it('TitleView groups episodes by season', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    expect(vi.mocked(getTitle)).toHaveBeenCalledWith('tv', '1399')
    expect(wrapper.find('h1').text()).toBe('Game of Thrones')
    expect(wrapper.text()).toContain('2011')

    const seasons = wrapper.findAll('[data-testid="season"]')
    expect(seasons).toHaveLength(2)
    expect(seasons[0]!.text()).toContain('Season 1')
    expect(seasons[0]!.text()).toContain('1 of 2 cached')
    expect(seasons[0]!.text()).toContain('Winter Is Coming')
    expect(seasons[0]!.text()).toContain('The Kingsroad')
    expect(seasons[0]!.text()).not.toContain('The North Remembers')
    expect(seasons[1]!.text()).toContain('Season 2')
    expect(seasons[1]!.text()).toContain('The North Remembers')
    expect(seasons[0]!.findAll('[data-testid="episode"]')).toHaveLength(2)
    // season 1 has an uncached episode, shown dimmed
    expect(seasons[0]!.text()).toContain('Not cached')
  })

  it('lists poster and backdrop images and an empty logo section', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    expect(fetchImageBlob).toHaveBeenCalledWith('/api/admin/posters/imdb/tt0944947@ir.pbc.zm/image')
    expect(wrapper.findAll('[data-testid="image-tile"]')).toHaveLength(2)
    expect(wrapper.text()).toContain('No logos cached')
    // the override buttons are live (P3.3)
    const overrides = wrapper.findAll('button').filter((b) => b.text().includes('Override'))
    expect(overrides.length).toBeGreaterThan(0)
    expect(overrides.every((b) => b.attributes('disabled') === undefined)).toBe(true)
  })

  it('opens the override picker for a kind and for one episode', async () => {
    vi.mocked(getCandidates).mockReset().mockResolvedValue({
      media_type: 'tv', tmdb_id: 1399, kind: 'logo', season: null, episode: null, lang: 'en',
      current: null, auto: null, candidates: [], errors: [],
    })
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    await wrapper.find('[data-testid="override-logo"]').trigger('click')
    await flushPromises()
    expect(getCandidates).toHaveBeenLastCalledWith({ mediaType: 'tv', tmdbId: 1399, kind: 'logo' }, undefined)
    expect(wrapper.find('[data-testid="override-picker"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('Override the logo')

    // close it again (Cancel), then the second episode of season 1
    await buttonByText(wrapper, 'Cancel').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="override-picker"]').exists()).toBe(false)
    const ep = wrapper.findAll('[data-testid="episode"]')[1]!
    await ep.find('[data-testid="override-episode"]').trigger('click')
    await flushPromises()
    expect(getCandidates).toHaveBeenLastCalledWith(
      { mediaType: 'tv', tmdbId: 1399, kind: 'episode', season: 1, episode: 2 },
      undefined,
    )
    expect(wrapper.text()).toContain('season 1 episode 2 (The Kingsroad)')
  })

  it('shows an unresolved title by its id', async () => {
    const t: TitleDetail = { ...summary({ tmdb_id: 77, name: null, year: null, poster_thumb_url: null, imdb_id: null }), aliases: [], images: [] }
    vi.mocked(getTitle).mockResolvedValue(t)
    const wrapper = await mountView('/admin/library/movie/77')
    expect(wrapper.find('h1').text()).toBe('tmdb 77')
    expect(wrapper.text()).toContain('have not been looked up yet')
    expect(wrapper.find('header img').exists()).toBe(false)
  })

  it('asks for confirmation, purges every alias and reports success', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    await buttonByText(wrapper, 'Purge all').trigger('click')
    expect(wrapper.find('[role="dialog"]').text()).toContain('all 3 ids')
    expect(purgeTitle).not.toHaveBeenCalled()

    await wrapper.find('[data-testid="confirm-run"]').trigger('click')
    await flushPromises()

    expect(purgeTitle).toHaveBeenCalledWith('tv', 1399)
    expect(wrapper.find('[data-testid="banner"]').text()).toContain('Purged every cached image of Game of Thrones')
    expect(vi.mocked(getTitle).mock.calls.length).toBeGreaterThan(1) // reloaded
  })

  it('purges one image variant after confirming', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    await wrapper.find('button[aria-label="Purge poster tt0944947@ir.pbc.zm"]').trigger('click')
    await wrapper.find('[data-testid="confirm-run"]').trigger('click')
    await flushPromises()

    expect(purgeImageVariant).toHaveBeenCalledWith(expect.objectContaining({ kind: 'poster', id_type: 'imdb', cache_value: 'tt0944947@ir.pbc.zm' }))
  })

  it('reports a failed re-fetch and keeps the dialog open', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    vi.mocked(refetchTitle).mockRejectedValue(new Error('TMDB is unreachable'))
    const wrapper = await mountView('/admin/library/tv/1399')

    await wrapper.find('#sec-backdrop').element.closest('section')!.querySelector<HTMLButtonElement>('button')!.click()
    await flushPromises()
    await wrapper.find('[data-testid="confirm-run"]').trigger('click')
    await flushPromises()

    expect(refetchTitle).toHaveBeenCalledWith('tv', 1399, 'backdrop')
    expect(wrapper.find('[role="dialog"]').text()).toContain('TMDB is unreachable')
    expect(wrapper.find('[data-testid="banner"]').attributes('role')).toBe('alert')
  })

  it('has no episode re-fetch button and warns about rating lookups in both dialogs', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/1399')

    const episodes = wrapper.find('#sec-episodes').element.closest('section')!
    expect(Array.from(episodes.querySelectorAll('button')).some((b) => b.textContent?.includes('Re-fetch'))).toBe(false)

    await buttonByText(wrapper, 'Purge all').trigger('click')
    expect(wrapper.find('[role="dialog"]').text()).toContain('MDBList/OMDb quota')
    await buttonByText(wrapper, 'Cancel').trigger('click')

    await buttonByText(wrapper, 'Re-fetch all').trigger('click')
    const text = wrapper.find('[role="dialog"]').text()
    expect(text).toContain('MDBList/OMDb quota')
    expect(text).toContain('that are not cached')
  })

  it('shows the backend message when a purge reports ok:false', async () => {
    vi.mocked(getTitle).mockResolvedValue(showDetail())
    vi.mocked(purgeTitle).mockRejectedValue(new Error('imdb/tt0944947 poster: disk error'))
    const wrapper = await mountView('/admin/library/tv/1399')

    await buttonByText(wrapper, 'Purge all').trigger('click')
    await wrapper.find('[data-testid="confirm-run"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[role="dialog"]').text()).toContain('disk error')
    expect(wrapper.find('[data-testid="banner"]').attributes('role')).toBe('alert')
  })

  it('shows the header and the TMDB wait message while a show loads, then the retry on failure', async () => {
    let fail!: (e: Error) => void
    vi.mocked(getTitle)
      .mockImplementationOnce(() => new Promise((_, reject) => (fail = reject)))
      .mockImplementationOnce(() => new Promise(() => {}))
    const wrapper = await mountView('/admin/library/tv/1399')

    expect(wrapper.find('h1').text()).toBe('tmdb 1399')
    expect(wrapper.find('[data-testid="loading-note"]').text()).toContain('Fetching episode names from TMDB')

    fail(new Error('still busy'))
    await flushPromises()
    expect(wrapper.find('[data-testid="loading-note"]').exists()).toBe(false)
    expect(wrapper.find('[role="alert"]').text()).toContain('still busy')

    await buttonByText(wrapper, 'Try again').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="loading-note"]').exists()).toBe(true)
  })

  it('does not show the TMDB wait message for a movie', async () => {
    vi.mocked(getTitle).mockImplementation(() => new Promise(() => {}))
    const wrapper = await mountView('/admin/library/movie/603')
    expect(wrapper.find('h1').text()).toBe('tmdb 603')
    expect(wrapper.find('[data-testid="loading-note"]').exists()).toBe(false)
  })

  it('shows an error with a retry', async () => {
    vi.mocked(getTitle).mockRejectedValueOnce(new Error('not found')).mockResolvedValue(showDetail())
    const wrapper = await mountView('/admin/library/tv/999')
    expect(wrapper.find('[role="alert"]').text()).toContain('not found')
    await buttonByText(wrapper, 'Try again').trigger('click')
    await flushPromises()
    expect(wrapper.find('h1').text()).toBe('Game of Thrones')
  })
})
