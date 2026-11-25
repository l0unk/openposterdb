import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { QueryClient, VueQueryPlugin } from '@tanstack/vue-query'
import { createRouter, createMemoryHistory } from 'vue-router'
import ImageListView from '@/components/ImageListView.vue'

const body = {
  items: [
    { cache_key: 'imdb/tt0111161_t_de@ir.pbc.zm', release_date: null, created_at: 1710000000, updated_at: 1710100000 },
    { cache_key: 'imdb/episode-tt0944947-S1E2@ir.zm', release_date: null, created_at: 1710000000, updated_at: 1710100000 },
  ],
  total: 2,
  page: 1,
  page_size: 50,
}

describe('ImageListView title link', () => {
  it('ImageListView row links to alias route', async () => {
    const router = createRouter({
      history: createMemoryHistory(),
      routes: [
        { path: '/', component: { template: '<div />' } },
        { path: '/admin/library/alias/:idType/:idValue', name: 'library-alias', component: { template: '<div />' } },
      ],
    })
    const listFn = vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(body) })
    const wrapper = mount(ImageListView, {
      props: { kind: 'poster', listFn, imageFn: vi.fn(), fetchFn: vi.fn(), deleteFn: vi.fn(), clearAllFn: vi.fn() },
      global: { plugins: [createPinia(), router, [VueQueryPlugin, { queryClient: new QueryClient() }]] },
    })
    await flushPromises()

    const links = wrapper.findAll('a[aria-label="Open title page"]')
    expect(links).toHaveLength(2)
    // the variant and size suffix are stripped: the alias is the plain id as requested
    expect(links[0]!.attributes('href')).toBe('/admin/library/alias/imdb/tt0111161')
    expect(links[1]!.attributes('href')).toBe('/admin/library/alias/imdb/episode-tt0944947-S1E2')
  })
})
