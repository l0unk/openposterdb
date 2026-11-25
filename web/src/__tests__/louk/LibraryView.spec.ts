import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createRouter, createMemoryHistory } from 'vue-router'
import { summary } from './fixtures'

vi.mock('@/louk/api', async (orig) => ({
  ...(await orig<typeof import('@/louk/api')>()),
  listTitles: vi.fn(),
}))

import { listTitles } from '@/louk/api'
import LibraryView from '@/louk/views/LibraryView.vue'

const items = [
  summary(),
  summary({ media_type: 'tv', tmdb_id: 1399, name: 'Game of Thrones', year: 2011, image_counts: { poster: 1, logo: 0, backdrop: 1, episode: 4 } }),
  summary({ tmdb_id: 77, name: null, year: null, poster_thumb_url: null, imdb_id: null }),
]

async function mountView(path = '/admin/library') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/library', name: 'library', component: LibraryView },
      { path: '/admin/library/:mediaType/:tmdbId', name: 'library-title', component: { template: '<div />' } },
    ],
  })
  router.push(path)
  await router.isReady()
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = mount(LibraryView, { global: { plugins: [createPinia(), router, [VueQueryPlugin, { queryClient }]] } })
  await flushPromises()
  return { wrapper, router }
}

describe('LibraryView', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(listTitles).mockReset()
  })

  it('shows a loading skeleton first', async () => {
    vi.mocked(listTitles).mockReturnValue(new Promise(() => {}))
    const { wrapper } = await mountView()
    expect(wrapper.find('[aria-busy="true"]').exists()).toBe(true)
  })

  it('LibraryView renders items and filters by type', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items, total: 3 })
    const { wrapper, router } = await mountView()

    const cards = wrapper.findAll('[data-testid="title-card"]')
    expect(cards).toHaveLength(3)
    expect(cards[0]!.text()).toContain('The Matrix')
    expect(cards[0]!.text()).toContain('1999')
    expect(cards[0]!.attributes('href')).toBe('/admin/library/movie/603')
    expect(cards[1]!.text()).toContain('Game of Thrones')
    expect(cards[1]!.text()).toContain('Show')
    // unresolved title: shows its id, a placeholder, and no <img>
    expect(cards[2]!.text()).toContain('tmdb 77')
    expect(cards[2]!.find('[data-testid="poster-placeholder"]').exists()).toBe(true)
    expect(cards[2]!.find('img').exists()).toBe(false)
    expect(cards[0]!.find('img').attributes('loading')).toBe('lazy')
    expect(wrapper.text()).toContain('3 titles')
    expect(vi.mocked(listTitles)).toHaveBeenLastCalledWith({ q: '', type: '', sort: 'last', page: 1, perPage: 48 })

    const shows = wrapper.findAll('button').find((b) => b.text() === 'Shows')!
    await shows.trigger('click')
    await flushPromises()

    expect(router.currentRoute.value.query.type).toBe('tv')
    expect(vi.mocked(listTitles)).toHaveBeenLastCalledWith({ q: '', type: 'tv', sort: 'last', page: 1, perPage: 48 })
    expect(shows.attributes('aria-pressed')).toBe('true')
  })

  it('sorts by name', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items, total: 3 })
    const { wrapper } = await mountView()
    await wrapper.findAll('button').find((b) => b.text() === 'Name')!.trigger('click')
    await flushPromises()
    expect(vi.mocked(listTitles)).toHaveBeenLastCalledWith(expect.objectContaining({ sort: 'name' }))
  })

  it('debounces the search box by 250 ms and labels it', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items, total: 3 })
    const { wrapper } = await mountView()
    expect(wrapper.find('label[for="library-search"]').exists()).toBe(true)
    vi.mocked(listTitles).mockClear()

    await wrapper.find('#library-search').setValue('matrix')
    await new Promise((r) => setTimeout(r, 100))
    expect(listTitles).not.toHaveBeenCalled()
    await new Promise((r) => setTimeout(r, 250))
    await flushPromises()
    expect(vi.mocked(listTitles)).toHaveBeenLastCalledWith(expect.objectContaining({ q: 'matrix', page: 1 }))
  })

  it('reads filters from the URL and pages', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items, total: 100 })
    const { wrapper } = await mountView('/admin/library?q=ga&type=tv&page=2')
    expect(vi.mocked(listTitles)).toHaveBeenLastCalledWith({ q: 'ga', type: 'tv', sort: 'last', page: 2, perPage: 48 })
    expect(wrapper.text()).toContain('Page 2 of 3')
  })

  it('explains an empty library on a fresh install', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items: [], total: 0 })
    const { wrapper } = await mountView()
    const empty = wrapper.find('[data-testid="library-empty"]')
    expect(empty.text()).toContain('within about a minute')
    expect(empty.text()).toContain('indexed in the background')
    expect(empty.text()).toContain('over the first day')
  })

  it('offers to clear the filters when nothing matches', async () => {
    vi.mocked(listTitles).mockResolvedValue({ items: [], total: 0 })
    const { wrapper, router } = await mountView('/admin/library?q=zzz')
    expect(wrapper.text()).toContain('No titles match')
    await wrapper.findAll('button').find((b) => b.text().includes('Clear search'))!.trigger('click')
    await flushPromises()
    expect(router.currentRoute.value.query.q).toBeUndefined()
  })

  it('shows the error and retries', async () => {
    vi.mocked(listTitles).mockRejectedValueOnce(new Error('db is locked')).mockResolvedValue({ items, total: 3 })
    const { wrapper } = await mountView()
    expect(wrapper.find('[role="alert"]').text()).toContain('db is locked')
    await wrapper.findAll('button').find((b) => b.text() === 'Try again')!.trigger('click')
    await flushPromises()
    expect(wrapper.findAll('[data-testid="title-card"]')).toHaveLength(3)
  })
})
