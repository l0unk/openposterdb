<script setup lang="ts">
// louk-fork: Library, a poster wall of every title the server has rendered images for.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { keepPreviousData, useQuery } from '@tanstack/vue-query'
import { Library, Search, TriangleAlert } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import RefreshButton from '@/components/RefreshButton.vue'
import SegmentedControl from '@/louk/components/SegmentedControl.vue'
import TitleCard from '@/louk/components/TitleCard.vue'
import { LIBRARY_PAGE_SIZE, listTitles, type TitleSort, type TitleTypeFilter } from '@/louk/api'

const route = useRoute()
const router = useRouter()

function queryString(v: unknown): string {
  return typeof v === 'string' ? v : ''
}

// Filters live in the URL so "back" from a title page returns to the same wall.
const q = computed(() => queryString(route.query.q))
const type = computed<TitleTypeFilter>(() => {
  const t = queryString(route.query.type)
  return t === 'movie' || t === 'tv' ? t : ''
})
const sort = computed<TitleSort>(() => (queryString(route.query.sort) === 'name' ? 'name' : 'last'))
const page = computed(() => {
  const p = Number(route.query.page)
  return Number.isInteger(p) && p > 0 ? p : 1
})

function update(next: { q?: string; type?: string; sort?: string; page?: number }) {
  const merged = {
    q: q.value,
    type: type.value as string,
    sort: sort.value as string,
    page: page.value,
    ...next,
  }
  router.replace({
    query: {
      q: merged.q || undefined,
      type: merged.type || undefined,
      sort: merged.sort === 'last' ? undefined : merged.sort,
      page: merged.page > 1 ? String(merged.page) : undefined,
    },
  })
}

// Search box: debounced 250 ms before it reaches the URL (and so the query).
const search = ref(q.value)
let timer: ReturnType<typeof setTimeout> | undefined
watch(search, (v) => {
  clearTimeout(timer)
  timer = setTimeout(() => {
    if (v.trim() !== q.value) update({ q: v.trim(), page: 1 })
  }, 250)
})
watch(q, (v) => {
  if (v !== search.value.trim()) search.value = v
})
onBeforeUnmount(() => clearTimeout(timer))

const typeModel = computed<TitleTypeFilter>({
  get: () => type.value,
  set: (v) => update({ type: v, page: 1 }),
})
const sortModel = computed<TitleSort>({
  get: () => sort.value,
  set: (v) => update({ sort: v, page: 1 }),
})

const { data, isPending, isFetching, isError, error, refetch } = useQuery({
  queryKey: computed(() => ['louk', 'titles', q.value, type.value, sort.value, page.value]),
  queryFn: () =>
    listTitles({ q: q.value, type: type.value, sort: sort.value, page: page.value, perPage: LIBRARY_PAGE_SIZE }),
  placeholderData: keepPreviousData,
})

const totalPages = computed(() => (data.value ? Math.max(1, Math.ceil(data.value.total / LIBRARY_PAGE_SIZE)) : 1))
const filtered = computed(() => !!q.value || !!type.value)

function clearFilters() {
  search.value = ''
  update({ q: '', type: '', page: 1 })
}
</script>

<template>
  <div class="space-y-5">
    <div class="flex flex-wrap items-end gap-x-4 gap-y-3">
      <div class="min-w-48 flex-1 basis-64 space-y-1.5 sm:max-w-sm">
        <label for="library-search" class="text-sm font-medium">Search titles</label>
        <div class="relative">
          <Search class="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
          <Input id="library-search" v-model="search" type="search" placeholder="Name of a movie or show" class="pl-8" autocomplete="off" />
        </div>
      </div>
      <div class="space-y-1.5">
        <span class="block text-sm font-medium">Type</span>
        <SegmentedControl
          v-model="typeModel"
          label="Filter by type"
          :options="[
            { value: '', label: 'All' },
            { value: 'movie', label: 'Movies' },
            { value: 'tv', label: 'Shows' },
          ]"
        />
      </div>
      <div class="space-y-1.5">
        <span class="block text-sm font-medium">Sort by</span>
        <SegmentedControl
          v-model="sortModel"
          label="Sort order"
          :options="[
            { value: 'last', label: 'Last requested' },
            { value: 'name', label: 'Name' },
          ]"
        />
      </div>
      <div class="ml-auto">
        <RefreshButton :fetching="isFetching" @refresh="refetch()" />
      </div>
    </div>

    <div v-if="isPending" class="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-x-4 gap-y-6" aria-busy="true" aria-label="Loading titles">
      <div v-for="i in 12" :key="i" class="space-y-2">
        <Skeleton class="aspect-[2/3] w-full rounded-lg" />
        <Skeleton class="h-4 w-3/4" />
        <Skeleton class="h-3 w-1/2" />
      </div>
    </div>

    <div v-else-if="isError && !data" role="alert" class="flex flex-col items-center gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-6 py-12 text-center">
      <TriangleAlert class="size-6 text-destructive" aria-hidden="true" />
      <div class="space-y-1">
        <p class="font-medium">Could not load the library</p>
        <p class="text-sm text-muted-foreground">{{ error?.message }}</p>
      </div>
      <Button variant="outline" size="sm" @click="refetch()">Try again</Button>
    </div>

    <template v-else-if="data">
      <div v-if="data.items.length === 0" class="flex flex-col items-center gap-3 rounded-lg border border-dashed px-6 py-16 text-center" data-testid="library-empty">
        <Library class="size-8 text-muted-foreground" aria-hidden="true" />
        <template v-if="filtered">
          <p class="font-medium">No titles match</p>
          <p class="max-w-md text-sm text-muted-foreground">Nothing in the library fits this search and filter.</p>
          <Button variant="outline" size="sm" @click="clearFilters">Clear search and filter</Button>
        </template>
        <template v-else>
          <p class="font-medium">No titles yet</p>
          <p class="max-w-md text-sm text-muted-foreground">
            A title shows up here the first time an image is requested for it. Images that were already cached
            before this page existed are indexed in the background, a few at a time, so older titles appear
            gradually over the next hours.
          </p>
        </template>
      </div>

      <template v-else>
        <ul
          class="grid grid-cols-[repeat(auto-fill,minmax(9.5rem,1fr))] gap-x-4 gap-y-6 transition-opacity"
          :class="isFetching ? 'opacity-70' : ''"
          aria-label="Titles"
        >
          <li v-for="t in data.items" :key="`${t.media_type}-${t.tmdb_id}`" class="min-w-0">
            <TitleCard :title="t" />
          </li>
        </ul>
        <div class="flex items-center justify-between border-t pt-4">
          <p class="text-sm text-muted-foreground" aria-live="polite">
            {{ data.total }} {{ data.total === 1 ? 'title' : 'titles' }}
          </p>
          <div class="flex items-center gap-2">
            <Button variant="outline" size="sm" :disabled="page <= 1" @click="update({ page: page - 1 })">Previous</Button>
            <span class="text-sm">Page {{ page }} of {{ totalPages }}</span>
            <Button variant="outline" size="sm" :disabled="page >= totalPages" @click="update({ page: page + 1 })">Next</Button>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>
