<script setup lang="ts">
// louk-fork: one title: header, cached poster/backdrop/logo, then seasons -> episodes for shows.
import { computed, onBeforeUnmount, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryClient } from '@tanstack/vue-query'
import {
  ArrowLeft,
  Check,
  ChevronRight,
  Clapperboard,
  ExternalLink,
  Film,
  Image,
  Loader2,
  Pencil,
  RotateCw,
  Stamp,
  Trash2,
  Tv,
  TriangleAlert,
  Wallpaper,
} from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import RefreshButton from '@/components/RefreshButton.vue'
import ImageTile from '@/louk/components/ImageTile.vue'
import OverridePicker from '@/louk/components/OverridePicker.vue'
import SeasonList from '@/louk/components/SeasonList.vue'
import {
  fetchImageBlob,
  getTitle,
  imdbUrl,
  purgeImageVariant,
  purgeTitle,
  refetchTitle,
  titleLabel,
  tmdbUrl,
  type ImageKind,
  type MediaType,
  type TitleImage,
} from '@/louk/api'
import { relativeTime } from '@/louk/format'
import type { OverrideTarget } from '@/louk/overrides'

const route = useRoute()
const router = useRouter()
const queryClient = useQueryClient()

const mediaType = computed<MediaType | null>(() => {
  const t = String(route.params.mediaType)
  return t === 'movie' || t === 'tv' ? t : null
})
const tmdbId = computed(() => String(route.params.tmdbId))

const { data, isPending, isFetching, isError, error, refetch } = useQuery({
  queryKey: computed(() => ['louk', 'title', mediaType.value, tmdbId.value]),
  queryFn: () => getTitle(mediaType.value!, tmdbId.value),
  enabled: computed(() => mediaType.value !== null),
  retry: false,
})

// Also while "Try again" is running, so the page never looks frozen on a slow first open of a show.
const loading = computed(() => isPending.value || (isError.value && isFetching.value && !data.value))

const label = computed(() => (data.value ? titleLabel(data.value) : ''))
const resolved = computed(() => !!data.value?.name?.trim())
const isShow = computed(() => data.value?.media_type === 'tv')

const thumbFailed = ref(false)

const sections: { kind: Exclude<ImageKind, 'episode'>; title: string; icon: typeof Image; grid: string }[] = [
  { kind: 'poster', title: 'Posters', icon: Image, grid: 'grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))]' },
  { kind: 'backdrop', title: 'Backdrops', icon: Wallpaper, grid: 'grid-cols-[repeat(auto-fill,minmax(16rem,1fr))]' },
  { kind: 'logo', title: 'Logos', icon: Stamp, grid: 'grid-cols-[repeat(auto-fill,minmax(13rem,1fr))]' },
]

function imagesOf(kind: ImageKind): TitleImage[] {
  return (data.value?.images ?? []).filter((i) => i.kind === kind)
}

// Episode renders the season list does not show (e.g. episode names could not be fetched).
const looseEpisodes = computed(() => {
  const inSeasons = new Set(
    (data.value?.seasons ?? []).flatMap((s) => s.episodes.flatMap((e) => e.cached.map((c) => c.cache_value))),
  )
  return imagesOf('episode').filter((i) => !inSeasons.has(i.cache_value))
})

const seasonKey = computed(() => (data.value?.seasons ?? []).map((s) => s.season).join(','))

function goBack() {
  if (window.history.state?.back) router.back()
  else router.push({ name: 'library' })
}

// --- status banner (success or failure of the last action) ---
const banner = ref<{ ok: boolean; text: string; action?: { label: string; run: () => Promise<void> } } | null>(null)
let bannerTimer: ReturnType<typeof setTimeout> | undefined
function say(ok: boolean, text: string, action?: { label: string; run: () => Promise<void> }) {
  banner.value = { ok, text, action }
  clearTimeout(bannerTimer)
  if (ok) bannerTimer = setTimeout(() => (banner.value = null), action ? 30000 : 8000)
}
onBeforeUnmount(() => clearTimeout(bannerTimer))

async function reload() {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ['louk', 'title', mediaType.value, tmdbId.value] }),
    queryClient.invalidateQueries({ queryKey: ['louk', 'titles'] }),
  ])
}

// --- one confirmation dialog for every purge / re-fetch ---
interface Confirm {
  title: string
  body: string
  label: string
  destructive: boolean
  run: () => Promise<void>
  done: string
}
const confirm = ref<Confirm | null>(null)
const confirmOpen = ref(false)
const confirmBusy = ref(false)
const confirmError = ref('')

function ask(c: Confirm) {
  confirm.value = c
  confirmError.value = ''
  confirmOpen.value = true
}

async function runConfirm() {
  const c = confirm.value
  if (!c || confirmBusy.value) return
  confirmBusy.value = true
  confirmError.value = ''
  try {
    await c.run()
    confirmOpen.value = false
    say(true, c.done)
    await reload()
  } catch (e) {
    confirmError.value = e instanceof Error ? e.message : 'The request failed'
    say(false, `${c.title} failed: ${confirmError.value}`)
    await reload() // part of the work may have been done
  } finally {
    confirmBusy.value = false
  }
}

function askPurgeImage(img: TitleImage) {
  ask({
    title: `Purge ${img.kind}`,
    body: 'Removes this cached render. It is generated again the next time something requests it.',
    label: 'Purge',
    destructive: true,
    run: () => purgeImageVariant(img),
    done: `Purged ${img.kind} ${img.cache_value}.`,
  })
}

function askPurgeTitle() {
  const t = data.value
  if (!t) return
  const n = t.aliases.length
  ask({
    title: 'Purge all images',
    body: `Removes every cached image of ${label.value}, under all ${n} ${n === 1 ? 'id' : 'ids'} it was requested by. They are generated again on the next request. ${RATINGS_NOTE}`,
    label: 'Purge all',
    destructive: true,
    run: () => purgeTitle(t.media_type, t.tmdb_id),
    done: `Purged every cached image of ${label.value}.`,
  })
}

const kindPlural: Record<ImageKind, string> = { poster: 'posters', backdrop: 'backdrops', logo: 'logos', episode: 'episodes' }

// Both the purge and the re-fetch dialogs say this: it mirrors upstream's own purge/fetch buttons.
const RATINGS_NOTE =
  'Drawing images again can cost a few rating lookups (MDBList/OMDb quota), the same as the other purge and re-fetch buttons in the dashboard.'

// Episode stills are not re-fetched per title (the backend refuses it), so there is no 'episode' here.
function askRefetch(kind?: Exclude<ImageKind, 'episode'>) {
  const t = data.value
  if (!t) return
  const what = kind ? kindPlural[kind] : 'images'
  ask({
    title: `Re-fetch ${what}`,
    body: `Draws the ${what} of ${label.value} that are not cached, using the global settings. Images that are already cached are left as they are; purge them first to draw them again. ${RATINGS_NOTE}`,
    label: 'Re-fetch',
    destructive: false,
    run: () => refetchTitle(t.media_type, t.tmdb_id, kind),
    done: `Re-fetched the ${what} of ${label.value}.`,
  })
}

// --- art override picker (louk-fork: P3.3) ---
const pickerOpen = ref(false)
const pickerTarget = ref<OverrideTarget | null>(null)
const pickerHeading = ref('')
const pickerRendered = ref<TitleImage | null>(null)
// Bumped after every override change: the tiles ask the server again instead of showing the browser's copy.
const bust = ref(0)

const APPS_NOTE = 'Apps show the new art once their own caches expire.'

function openPicker(kind: Exclude<ImageKind, 'episode'>) {
  const t = data.value
  if (!t) return
  pickerTarget.value = { mediaType: t.media_type, tmdbId: t.tmdb_id, kind }
  pickerHeading.value = label.value
  pickerRendered.value = imagesOf(kind)[0] ?? null
  pickerOpen.value = true
}

function openEpisodePicker(ep: { season: number; episode: number; name: string; rendered: TitleImage | null }) {
  const t = data.value
  if (!t) return
  pickerTarget.value = { mediaType: t.media_type, tmdbId: t.tmdb_id, kind: 'episode', season: ep.season, episode: ep.episode }
  pickerHeading.value = `${label.value}, ${ep.season === 0 ? 'specials' : `season ${ep.season}`} episode ${ep.episode} (${ep.name})`
  pickerRendered.value = ep.rendered
  pickerOpen.value = true
}

async function onOverrideChanged(change: { kind: ImageKind; message: string; redrawable: boolean }) {
  bust.value = Date.now()
  const text = `${change.message} The cached ${change.kind === 'episode' ? 'still' : change.kind} was removed and is drawn again on the next request. ${APPS_NOTE}`
  const kind = change.kind
  say(
    true,
    text,
    change.redrawable && kind !== 'episode'
      ? {
          label: 'Draw it now',
          run: async () => {
            const t = data.value
            if (t) await refetchTitle(t.media_type, t.tmdb_id, kind)
          },
        }
      : undefined,
  )
  await reload()
}

const bannerBusy = ref(false)
async function runBannerAction() {
  const a = banner.value?.action
  if (!a || bannerBusy.value) return
  bannerBusy.value = true
  try {
    await a.run()
    bust.value = Date.now()
    say(true, `Drew the image again. ${APPS_NOTE}`)
  } catch (e) {
    say(false, `Drawing the image failed: ${e instanceof Error ? e.message : 'the request failed'}`)
  } finally {
    bannerBusy.value = false
    await reload()
  }
}

// --- image viewer ---
const viewing = ref<TitleImage | null>(null)
const viewOpen = ref(false)
const viewUrl = ref<string | null>(null)
const viewLoading = ref(false)

function dropViewUrl() {
  if (viewUrl.value) URL.revokeObjectURL(viewUrl.value)
  viewUrl.value = null
}

async function openView(img: TitleImage) {
  viewing.value = img
  viewOpen.value = true
  viewLoading.value = true
  dropViewUrl()
  try {
    viewUrl.value = URL.createObjectURL(await fetchImageBlob(img.admin_image_url))
  } catch {
    viewUrl.value = null
  } finally {
    viewLoading.value = false
  }
}

function closeView() {
  viewOpen.value = false
  dropViewUrl()
}
onBeforeUnmount(dropViewUrl)

const viewSize = computed(() => (viewing.value?.kind === 'poster' ? 'max-w-md' : 'max-w-3xl'))
</script>

<template>
  <div class="space-y-6">
    <div>
      <Button variant="ghost" size="sm" class="-ml-2 text-muted-foreground" @click="goBack">
        <ArrowLeft />
        Library
      </Button>
    </div>

    <div v-if="mediaType === null" role="alert" class="rounded-lg border border-dashed px-6 py-16 text-center">
      <p class="font-medium">This is not a title address</p>
      <p class="mt-1 text-sm text-muted-foreground">A title page needs a movie or show and a TMDB id.</p>
    </div>

    <div v-else-if="loading" class="space-y-6" aria-busy="true" aria-label="Loading title">
      <div class="flex gap-5">
        <Skeleton class="h-36 w-24 shrink-0 rounded-lg" />
        <div class="min-w-0 flex-1 space-y-3">
          <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
            <h1 class="font-mono text-xl font-semibold tracking-tight text-muted-foreground">tmdb {{ tmdbId }}</h1>
            <Badge variant="secondary">
              <component :is="mediaType === 'tv' ? Tv : Film" aria-hidden="true" />
              {{ mediaType === 'tv' ? 'Show' : 'Movie' }}
            </Badge>
          </div>
          <Skeleton class="h-4 w-1/3" />
          <Skeleton class="h-4 w-2/3" />
        </div>
      </div>
      <p
        v-if="mediaType === 'tv'"
        role="status"
        class="flex items-start gap-2 rounded-md border bg-muted/40 px-3 py-2 text-sm"
        data-testid="loading-note"
      >
        <Loader2 class="mt-0.5 size-4 shrink-0 animate-spin" aria-hidden="true" />
        <span>Fetching episode names from TMDB, the first time can take up to a minute.</span>
      </p>
      <Skeleton class="h-48 w-full rounded-lg" />
    </div>

    <div
      v-else-if="isError"
      role="alert"
      class="flex flex-col items-center gap-3 rounded-lg border border-destructive/40 bg-destructive/5 px-6 py-12 text-center"
    >
      <TriangleAlert class="size-6 text-destructive" aria-hidden="true" />
      <div class="space-y-1">
        <p class="font-medium">Could not load this title</p>
        <p class="text-sm text-muted-foreground">{{ error?.message }}</p>
      </div>
      <div class="flex gap-2">
        <Button variant="outline" size="sm" @click="refetch()">Try again</Button>
        <Button variant="ghost" size="sm" @click="router.push({ name: 'library' })">Back to the library</Button>
      </div>
    </div>

    <template v-else-if="data">
      <header class="flex flex-wrap gap-x-5 gap-y-4">
        <div class="h-36 w-24 shrink-0 overflow-hidden rounded-lg border bg-muted">
          <img
            v-if="data.poster_thumb_url && !thumbFailed"
            :src="data.poster_thumb_url"
            alt=""
            class="size-full object-cover"
            @error="thumbFailed = true"
          />
          <div v-else class="flex size-full items-center justify-center text-muted-foreground">
            <component :is="isShow ? Tv : Film" class="size-7 opacity-60" aria-hidden="true" />
          </div>
        </div>
        <div class="min-w-0 flex-1 basis-72 space-y-2">
          <div class="flex flex-wrap items-center gap-x-3 gap-y-1">
            <h1 class="text-2xl font-semibold tracking-tight break-words" :class="resolved ? '' : 'font-mono text-xl text-muted-foreground'">
              {{ label }}
            </h1>
            <span v-if="data.year" class="text-lg text-muted-foreground tabular-nums">{{ data.year }}</span>
            <Badge variant="secondary">
              <component :is="isShow ? Tv : Film" aria-hidden="true" />
              {{ isShow ? 'Show' : 'Movie' }}
            </Badge>
          </div>
          <p v-if="!resolved" class="text-sm text-muted-foreground">
            <template v-if="(data.resolved_at ?? 0) < 0">
              The last TMDB lookup for this title failed. It is tried again once a day.
            </template>
            <template v-else>
              The name and poster have not been looked up yet. They are fetched in the background; refresh in a minute.
            </template>
          </p>
          <ul class="flex flex-wrap gap-2 text-xs" aria-label="Ids">
            <li>
              <a :href="tmdbUrl(data)" target="_blank" rel="noopener noreferrer" class="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-2 py-1 font-mono outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50">
                tmdb {{ data.tmdb_id }}
                <ExternalLink class="size-3" aria-hidden="true" />
                <span class="sr-only">(opens TMDB)</span>
              </a>
            </li>
            <li v-if="data.imdb_id">
              <a :href="imdbUrl(data.imdb_id)" target="_blank" rel="noopener noreferrer" class="inline-flex items-center gap-1 rounded-md border bg-muted/40 px-2 py-1 font-mono outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50">
                imdb {{ data.imdb_id }}
                <ExternalLink class="size-3" aria-hidden="true" />
                <span class="sr-only">(opens IMDb)</span>
              </a>
            </li>
            <li v-if="data.tvdb_id" class="inline-flex items-center rounded-md border bg-muted/40 px-2 py-1 font-mono">
              tvdb {{ data.tvdb_id }}
            </li>
          </ul>
          <p class="text-sm text-muted-foreground">Last requested {{ relativeTime(data.last_requested) }}</p>
        </div>
        <div class="flex items-start gap-2">
          <RefreshButton :fetching="isFetching" @refresh="refetch()" />
          <Button variant="outline" size="sm" @click="askRefetch()">
            <RotateCw />
            Re-fetch all
          </Button>
          <Button variant="outline" size="sm" class="text-destructive hover:text-destructive" @click="askPurgeTitle">
            <Trash2 />
            Purge all
          </Button>
        </div>
      </header>

      <div
        v-if="banner"
        :role="banner.ok ? 'status' : 'alert'"
        class="flex items-start gap-2 rounded-md border px-3 py-2 text-sm"
        :class="banner.ok ? 'bg-muted/40' : 'border-destructive/40 bg-destructive/5'"
        data-testid="banner"
      >
        <Check v-if="banner.ok" class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <TriangleAlert v-else class="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
        <span class="min-w-0 flex-1 break-words">{{ banner.text }}</span>
        <Button
          v-if="banner.action"
          variant="outline"
          size="sm"
          class="-my-1 shrink-0"
          :disabled="bannerBusy"
          data-testid="banner-action"
          @click="runBannerAction"
        >
          <Loader2 v-if="bannerBusy" class="animate-spin" />
          {{ banner.action.label }}
        </Button>
      </div>

      <Collapsible v-if="data.aliases.length" class="rounded-lg border">
        <CollapsibleTrigger class="group flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm outline-none hover:bg-accent/50 focus-visible:ring-[3px] focus-visible:ring-ring/50">
          <ChevronRight class="size-4 text-muted-foreground transition-transform group-data-[state=open]:rotate-90" aria-hidden="true" />
          <span class="font-medium">Requested as</span>
          <span class="text-muted-foreground tabular-nums">{{ data.aliases.length }} {{ data.aliases.length === 1 ? 'id' : 'ids' }}</span>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <ul class="flex flex-wrap gap-1.5 border-t px-3 py-3">
            <li
              v-for="a in data.aliases"
              :key="`${a.id_type}/${a.id_value}`"
              class="rounded-md border bg-muted/40 px-2 py-0.5 font-mono text-xs"
            >
              {{ a.id_type }}/{{ a.id_value }}
            </li>
          </ul>
        </CollapsibleContent>
      </Collapsible>

      <section v-for="s in sections" :key="s.kind" :aria-labelledby="`sec-${s.kind}`" class="space-y-3">
        <div class="flex flex-wrap items-center gap-2 border-b pb-2">
          <component :is="s.icon" class="size-4 text-muted-foreground" aria-hidden="true" />
          <h2 :id="`sec-${s.kind}`" class="font-medium">{{ s.title }}</h2>
          <span class="text-sm text-muted-foreground tabular-nums">{{ imagesOf(s.kind).length }}</span>
          <div class="ml-auto flex gap-2">
            <Button variant="outline" size="sm" @click="askRefetch(s.kind)">
              <RotateCw />
              Re-fetch
            </Button>
            <Button variant="outline" size="sm" :data-testid="`override-${s.kind}`" @click="openPicker(s.kind)">
              <Pencil />
              Override
            </Button>
          </div>
        </div>
        <ul v-if="imagesOf(s.kind).length" class="grid gap-4" :class="s.grid">
          <li v-for="img in imagesOf(s.kind)" :key="`${img.cache_value}:${bust}`" class="min-w-0">
            <ImageTile :image="img" :bust="bust" @view="openView" @purge="askPurgeImage" />
          </li>
        </ul>
        <p v-else class="py-2 text-sm text-muted-foreground">
          No {{ kindPlural[s.kind] }} cached. One is rendered the first time something requests it.
        </p>
      </section>

      <section v-if="isShow" aria-labelledby="sec-episodes" class="space-y-3">
        <div class="flex flex-wrap items-center gap-2 border-b pb-2">
          <Clapperboard class="size-4 text-muted-foreground" aria-hidden="true" />
          <h2 id="sec-episodes" class="font-medium">Seasons and episodes</h2>
          <span class="text-sm text-muted-foreground tabular-nums">{{ imagesOf('episode').length }} cached</span>
        </div>
        <SeasonList
          v-if="data.seasons && data.seasons.length"
          :key="seasonKey"
          :seasons="data.seasons"
          @view="openView"
          @purge="askPurgeImage"
          @override="openEpisodePicker"
        />
        <p v-else class="py-2 text-sm text-muted-foreground">
          The season list has not been fetched from TMDB yet. Refresh in a moment.
        </p>
        <div v-if="looseEpisodes.length" class="space-y-3">
          <h3 class="text-sm font-medium">Other episode images</h3>
          <ul class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-4">
            <li v-for="img in looseEpisodes" :key="`${img.cache_value}:${bust}`" class="min-w-0">
              <ImageTile :image="img" :bust="bust" @view="openView" @purge="askPurgeImage" />
            </li>
          </ul>
        </div>
      </section>
    </template>

    <Dialog :open="confirmOpen" @update:open="(v: boolean) => { if (!v && !confirmBusy) confirmOpen = false }">
      <DialogContent class="max-w-md">
        <DialogHeader>
          <DialogTitle>{{ confirm?.title }}</DialogTitle>
          <DialogDescription>{{ confirm?.body }}</DialogDescription>
        </DialogHeader>
        <p v-if="confirmError" role="alert" class="text-sm text-destructive">{{ confirmError }}</p>
        <div class="flex justify-end gap-2">
          <Button variant="outline" :disabled="confirmBusy" @click="confirmOpen = false">Cancel</Button>
          <Button :variant="confirm?.destructive ? 'destructive' : 'default'" :disabled="confirmBusy" data-testid="confirm-run" @click="runConfirm">
            <Loader2 v-if="confirmBusy" class="animate-spin" />
            {{ confirm?.label }}
          </Button>
        </div>
      </DialogContent>
    </Dialog>

    <OverridePicker
      v-model:open="pickerOpen"
      :target="pickerTarget"
      :heading="pickerHeading"
      :rendered="pickerRendered"
      @changed="onOverrideChanged"
    />

    <Dialog :open="viewOpen" @update:open="(v: boolean) => { if (!v) closeView() }">
      <DialogContent :class="viewSize">
        <DialogHeader>
          <DialogTitle class="min-w-0 pr-8 font-mono text-sm break-all">{{ viewing?.cache_value }}</DialogTitle>
          <DialogDescription class="sr-only">Cached {{ viewing?.kind }} of {{ label }}</DialogDescription>
        </DialogHeader>
        <div class="flex min-h-[200px] min-w-0 items-center justify-center">
          <Skeleton v-if="viewLoading" class="h-[270px] w-full max-w-md rounded-md" />
          <img v-else-if="viewUrl" :src="viewUrl" :alt="`${viewing?.kind} ${viewing?.cache_value}`" class="max-h-[70vh] max-w-full rounded-md object-contain" />
          <p v-else class="text-sm text-muted-foreground">Could not load this image.</p>
        </div>
      </DialogContent>
    </Dialog>
  </div>
</template>
