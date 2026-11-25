<script setup lang="ts">
// louk-fork: the per-title art picker (P3.3). One dialog per poster/logo/backdrop/episode still:
// candidates from TMDB and fanart.tv, an upload, a "no badges" switch, and a before/after comparison.
// All requests go through '@/louk/overrides'; the title page owns refreshing after a change.
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { ArrowDown, ArrowRight, Check, ImageOff, Loader2, Sparkles, TriangleAlert, Upload } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import SegmentedControl from '@/louk/components/SegmentedControl.vue'
import { fetchImageBlob, type TitleImage } from '@/louk/api'
import {
  checkUploadFile,
  deleteOverride,
  getCandidates,
  kindHasLanguage,
  languageLabel,
  previewUrl,
  putOverride,
  sameArt,
  scoreLabel,
  sizeLabel,
  sourceLabel,
  uploadImage,
  formatBytes,
  type Candidate,
  type Candidates,
  type Override,
  type OverrideBody,
  type OverrideSource,
  type OverrideTarget,
} from '@/louk/overrides'

const props = defineProps<{
  open: boolean
  target: OverrideTarget | null
  /** What the dialog is about, in words: "The Man in the High Castle, season 1 episode 2". */
  heading: string
  /** The rendered image served now (a cached render of this kind), if there is one. */
  rendered: TitleImage | null
}>()
const emit = defineEmits<{
  'update:open': [open: boolean]
  changed: [change: { kind: OverrideTarget['kind']; message: string; redrawable: boolean }]
}>()

/** A candidate, the current override or an upload, as one tile. */
interface Tile {
  key: string
  source: 'tmdb' | 'fanart' | 'upload'
  value: string
  fanart_id: string | null
  lang: string | null
  votes: number | null
  vote_count: number | null
  likes: number | null
  width: number | null
  height: number | null
  /** Small image for the tile (public URL, object URL, or an admin path that needs a token). */
  thumb: string
  /** Larger image for the "after" side. */
  full: string
  auto: boolean
  current: boolean
  /** thumb/full is an admin path to fetch as a blob. */
  needsToken?: boolean
  note?: string
}

const kindWord = { poster: 'poster', logo: 'logo', backdrop: 'backdrop', episode: 'episode still' } as const

const data = ref<Candidates | null>(null)
const loading = ref(false)
const loadError = ref('')
let loadSeq = 0

const selectedKey = ref<string | null>(null)
const noBadges = ref(false)
const badgesTouched = ref(false)
const scope = ref('')
const scopeTouched = ref(false)
const langFilter = ref('all')
const sourceFilter = ref<'all' | 'tmdb' | 'fanart'>('all')
const backing = ref<'checks' | 'dark' | 'light'>('checks')

const saving = ref(false)
const actionError = ref('')
const uploading = ref(false)
const uploadError = ref('')
const dragging = ref(false)
const uploads = ref<Tile[]>([])
const fileInput = ref<HTMLInputElement | null>(null)

// Admin paths (uploads, the served render) need the bearer token, so they are fetched as blobs.
const blobUrls = reactive<Record<string, string | null>>({})
const madeUrls: string[] = []

async function ensureBlob(path: string) {
  if (path in blobUrls) return
  blobUrls[path] = null
  try {
    const url = URL.createObjectURL(await fetchImageBlob(path))
    madeUrls.push(url)
    blobUrls[path] = url
  } catch {
    blobUrls[path] = ''
  }
}

function dropUrls() {
  madeUrls.splice(0).forEach((u) => URL.revokeObjectURL(u))
  Object.keys(blobUrls).forEach((k) => delete blobUrls[k])
}

function dropUploads() {
  uploads.value.forEach((u) => URL.revokeObjectURL(u.thumb))
  uploads.value = []
}

onBeforeUnmount(() => {
  dropUrls()
  dropUploads()
})

const kind = computed(() => props.target?.kind ?? 'poster')
const current = computed<Override | null>(() => data.value?.current ?? null)
const hasLanguage = computed(() => kindHasLanguage(kind.value))

const tileOf = (c: Candidate): Tile => ({
  key: `${c.source}:${c.value}`,
  source: c.source,
  value: c.value,
  fanart_id: c.fanart_id,
  lang: c.lang,
  votes: c.votes,
  vote_count: c.vote_count,
  likes: c.likes,
  width: c.width,
  height: c.height,
  thumb: c.thumb_url,
  full: previewUrl(c, kind.value),
  auto: c.auto,
  current: c.current,
})

/** Every tile: the uploads of this session, the current override if the list does not hold it, then the candidates. */
const tiles = computed<Tile[]>(() => {
  const list = (data.value?.candidates ?? []).map(tileOf)
  const out: Tile[] = []
  const cur = current.value
  const sessionUploads = uploads.value.map((u) => ({
    ...u,
    current: !!cur && cur.source === 'upload' && cur.value === u.value && sameLang(cur),
  }))
  out.push(...sessionUploads)
  if (cur && cur.source !== 'auto' && cur.thumb_url) {
    const known = [...sessionUploads, ...list].some((t) => sameArt(t, cur))
    if (!known) {
      out.push({
        key: `${cur.source}:${cur.value}`,
        source: cur.source as 'tmdb' | 'fanart' | 'upload',
        value: cur.value,
        fanart_id: cur.fanart_id,
        lang: cur.lang,
        votes: null,
        vote_count: null,
        likes: null,
        width: null,
        height: null,
        thumb: cur.thumb_url,
        full: cur.thumb_url,
        auto: false,
        current: true,
        needsToken: cur.source === 'upload',
        note: cur.source === 'upload' ? 'Your upload' : 'Not in the current list',
      })
    }
  }
  out.push(...list)
  return out
})

function sameLang(o: Override) {
  return (o.lang ?? '') === scope.value
}

const languages = computed(() => {
  const set = new Set<string>()
  let none = false
  for (const t of tiles.value) {
    if (t.source === 'upload') continue
    if (t.lang) set.add(t.lang)
    else none = true
  }
  return { codes: [...set].sort((a, b) => (a === 'en' ? -1 : b === 'en' ? 1 : a.localeCompare(b))), none }
})
const sources = computed(() => new Set(tiles.value.filter((t) => t.source !== 'upload').map((t) => t.source)))

const shown = computed(() =>
  tiles.value.filter((t) => {
    if (t.source === 'upload') return true
    if (sourceFilter.value !== 'all' && t.source !== sourceFilter.value) return false
    if (langFilter.value === 'all') return true
    if (langFilter.value === '__none') return !t.lang
    return t.lang === langFilter.value
  }),
)

const scopeOptions = computed(() => {
  const set = new Set<string>(['en', ...languages.value.codes])
  if (current.value?.lang) set.add(current.value.lang)
  if (scope.value) set.add(scope.value)
  return [...set]
})

const selected = computed(() => tiles.value.find((t) => t.key === selectedKey.value) ?? null)
const currentTile = computed(() => tiles.value.find((t) => t.current) ?? null)
const autoTile = computed(() => tiles.value.find((t) => t.auto) ?? null)

const badgesChanged = computed(() => noBadges.value !== (current.value?.no_badges ?? false))
const artChanged = computed(() => !!selected.value && !selected.value.current)
const canSave = computed(() => !saving.value && !uploading.value && (artChanged.value || badgesChanged.value))
const saveLabel = computed(() => (artChanged.value ? `Use this ${kindWord[kind.value]}` : 'Save badge setting'))

// --- before / after ---
interface Side {
  src: string | null
  loading: boolean
  title: string
  detail: string
}

const before = computed<Side>(() => {
  const r = props.rendered
  if (r) {
    const url = blobUrls[r.admin_image_url]
    return {
      src: url || null,
      loading: url === null || url === undefined,
      title: 'Served now',
      detail: current.value ? 'The drawn image, with the current override' : 'The drawn image, from the automatic pick',
    }
  }
  const t = currentTile.value ?? autoTile.value
  if (t) {
    return {
      src: tileSrc(t, true),
      loading: false,
      title: t.current ? 'Current override' : 'Automatic pick',
      detail: 'Not drawn yet; this is the art it will use',
    }
  }
  return { src: null, loading: false, title: 'Served now', detail: 'Nothing is cached or chosen yet' }
})

const after = computed<Side>(() => {
  const t = selected.value
  if (!t) {
    return {
      src: null,
      loading: false,
      title: 'After',
      detail: badgesChanged.value
        ? noBadges.value
          ? 'Same art, rating badges off'
          : 'Same art, rating badges on'
        : `Pick a ${kindWord[kind.value]} below to preview it here`,
    }
  }
  return {
    src: tileSrc(t, true),
    loading: false,
    title: 'After',
    detail: `${tileSummary(t)}${noBadges.value ? ', no badges' : ''}`,
  }
})

function tileSrc(t: Tile, full: boolean): string | null {
  const path = full ? t.full : t.thumb
  return t.needsToken ? blobUrls[path] || null : path
}

// Tiles whose image sits behind the bearer token (the current upload) are fetched once they appear.
watch(tiles, (list) => list.forEach((t) => t.needsToken && void ensureBlob(t.thumb)), { immediate: true })

watch(
  () => props.rendered?.admin_image_url,
  (p) => {
    if (p && props.open) void ensureBlob(p)
  },
  { immediate: true },
)

function tileSummary(t: Tile): string {
  if (t.source === 'upload') return t.note ?? 'Your upload'
  return [sourceLabel(t.source), languageLabel(t.lang), scoreLabel(t), sizeLabel(t)].filter(Boolean).join(', ')
}

function tileAria(t: Tile): string {
  const flags = [t.auto ? 'automatic pick' : '', t.current ? 'current override' : '', t.key === selectedKey.value ? 'selected' : '']
  return [`${kindWord[kind.value]}`, tileSummary(t), ...flags].filter(Boolean).join(', ')
}

// --- loading ---
async function load() {
  const target = props.target
  if (!target) return
  const seq = ++loadSeq
  loading.value = true
  loadError.value = ''
  try {
    const res = await getCandidates(target, hasLanguage.value ? scope.value || undefined : undefined)
    if (seq !== loadSeq) return
    data.value = res
    if (!scopeTouched.value) scope.value = hasLanguage.value ? (res.current?.lang ?? '') : ''
    if (!badgesTouched.value) noBadges.value = res.current?.no_badges ?? false
  } catch (e) {
    if (seq !== loadSeq) return
    loadError.value = e instanceof Error ? e.message : 'The request failed'
  } finally {
    if (seq === loadSeq) loading.value = false
  }
}

function reset() {
  loadSeq++
  data.value = null
  loadError.value = ''
  selectedKey.value = null
  noBadges.value = false
  badgesTouched.value = false
  scope.value = ''
  scopeTouched.value = false
  langFilter.value = 'all'
  sourceFilter.value = 'all'
  actionError.value = ''
  uploadError.value = ''
  saving.value = false
  uploading.value = false
  dragging.value = false
  dropUploads()
  dropUrls()
  backing.value = kind.value === 'logo' ? 'checks' : 'dark'
}

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return
    reset()
    if (props.rendered) void ensureBlob(props.rendered.admin_image_url)
    void load()
  },
  { immediate: true },
)

function onScope(e: Event) {
  scope.value = (e.target as HTMLSelectElement).value
  scopeTouched.value = true
  badgesTouched.value = false
  void load()
}

function onBadges() {
  noBadges.value = !noBadges.value
  badgesTouched.value = true
}

// --- upload ---
function pickFile() {
  fileInput.value?.click()
}

async function takeFile(file: File | undefined) {
  if (!file || uploading.value || saving.value) return
  uploadError.value = ''
  const problem = checkUploadFile(file)
  if (problem) {
    uploadError.value = problem
    return
  }
  uploading.value = true
  try {
    const r = await uploadImage(file)
    const local = URL.createObjectURL(file)
    const tile: Tile = {
      key: `upload:${r.sha256}`,
      source: 'upload',
      value: r.sha256,
      fanart_id: null,
      lang: null,
      votes: null,
      vote_count: null,
      likes: null,
      width: r.width,
      height: r.height,
      thumb: local,
      full: local,
      auto: false,
      current: false,
      note: `Your upload, ${r.format.toUpperCase()} ${r.width}×${r.height}, ${formatBytes(r.bytes)}`,
    }
    uploads.value = [tile, ...uploads.value.filter((u) => u.key !== tile.key)]
    selectedKey.value = tile.key
  } catch (e) {
    uploadError.value = e instanceof Error ? e.message : 'The upload failed'
  } finally {
    uploading.value = false
  }
}

function onFileInput(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  void takeFile(file)
}

function onDrop(e: DragEvent) {
  dragging.value = false
  void takeFile(e.dataTransfer?.files?.[0])
}

// --- save / clear ---
function choose(t: Tile) {
  selectedKey.value = t.key === selectedKey.value ? null : t.key
}

async function save() {
  const target = props.target
  if (!target || !canSave.value) return
  saving.value = true
  actionError.value = ''
  try {
    const t = selected.value
    const cur = current.value
    const lang = hasLanguage.value ? (t ? scope.value : (cur?.lang ?? scope.value)) || undefined : undefined
    let body: OverrideBody
    if (t) {
      body = { source: t.source, value: t.value, no_badges: noBadges.value }
      if (t.source === 'fanart' && t.fanart_id) body.fanart_id = t.fanart_id
    } else if (cur && cur.source !== 'auto') {
      body = { source: cur.source, value: cur.value, no_badges: noBadges.value }
      if (cur.fanart_id) body.fanart_id = cur.fanart_id
    } else if (noBadges.value) {
      body = { source: 'auto' as OverrideSource, no_badges: true }
    } else {
      // Back to fully automatic: nothing left to store.
      await deleteOverride(target, cur?.lang ?? lang)
      done(`Cleared the ${kindWord[kind.value]} override.`)
      return
    }
    await putOverride(target, body, lang)
    const what = t
      ? `Set the ${kindWord[kind.value]} to ${tileSummary(t)}${noBadges.value ? ', without badges' : ''}.`
      : `Turned the rating badges ${noBadges.value ? 'off' : 'on'} for this ${kindWord[kind.value]}.`
    done(what)
  } catch (e) {
    actionError.value = e instanceof Error ? e.message : 'The request failed'
  } finally {
    saving.value = false
  }
}

async function clear() {
  const target = props.target
  const cur = current.value
  if (!target || !cur || saving.value) return
  saving.value = true
  actionError.value = ''
  try {
    await deleteOverride(target, hasLanguage.value ? cur.lang : null)
    done(`Cleared the ${kindWord[kind.value]} override. The automatic pick is used again.`)
  } catch (e) {
    actionError.value = e instanceof Error ? e.message : 'The request failed'
  } finally {
    saving.value = false
  }
}

function done(message: string) {
  const k = kind.value
  emit('changed', { kind: k, message, redrawable: k !== 'episode' })
  emit('update:open', false)
}

function onOpenChange(v: boolean) {
  if (!v && saving.value) return
  emit('update:open', v)
}

const CHECKS = {
  backgroundColor: '#26262b',
  backgroundImage:
    'linear-gradient(45deg,#3a3a41 25%,transparent 25%,transparent 75%,#3a3a41 75%),linear-gradient(45deg,#3a3a41 25%,transparent 25%,transparent 75%,#3a3a41 75%)',
  backgroundSize: '16px 16px',
  backgroundPosition: '0 0, 8px 8px',
}
const backingClass = computed(() => (backing.value === 'light' ? 'bg-neutral-100' : backing.value === 'dark' ? 'bg-neutral-950' : ''))
const backingStyle = computed(() => (backing.value === 'checks' ? CHECKS : undefined))

const failedThumbs = reactive<Record<string, boolean>>({})
const tileAspect = computed(() => (kind.value === 'poster' ? 'aspect-[2/3]' : 'aspect-video'))
const gridClass = computed(() =>
  kind.value === 'poster'
    ? 'grid-cols-[repeat(auto-fill,minmax(8rem,1fr))]'
    : 'grid-cols-[repeat(auto-fill,minmax(13rem,1fr))]',
)
const fitClass = computed(() => (kind.value === 'logo' ? 'object-contain p-3' : 'object-contain'))
const emptyAfterList = computed(() => !loading.value && !loadError.value && tiles.value.length === 0)
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent
      class="flex max-h-[92vh] flex-col gap-4 sm:max-w-5xl"
      data-testid="override-picker"
      @dragover.prevent="dragging = true"
      @dragleave.self="dragging = false"
      @drop.prevent="onDrop"
    >
      <DialogHeader>
        <DialogTitle>Override the {{ kindWord[kind] }}</DialogTitle>
        <DialogDescription>
          {{ heading }}. Pick another image, upload your own, or switch the rating badges off.
        </DialogDescription>
      </DialogHeader>

      <!-- before / after -->
      <div class="grid items-stretch gap-3 sm:grid-cols-[1fr_auto_1fr]" data-testid="compare">
        <figure
          v-for="(side, i) in [before, after]"
          :key="i"
          class="min-w-0 space-y-1.5 sm:row-start-1"
          :class="i === 1 ? 'order-3 sm:col-start-3' : 'order-1 sm:col-start-1'"
          :data-testid="i === 0 ? 'before' : 'after'"
        >
          <div
            class="relative flex h-40 items-center justify-center overflow-hidden rounded-md border sm:h-52"
            :class="backingClass"
            :style="backingStyle"
          >
            <Skeleton v-if="side.loading" class="size-full rounded-none" />
            <img
              v-else-if="side.src"
              :src="side.src"
              :alt="`${side.title}: ${side.detail}`"
              class="max-h-full max-w-full object-contain"
            />
            <p v-else class="px-4 text-center text-sm" :class="backing === 'light' ? 'text-neutral-600' : 'text-neutral-400'">
              {{ side.detail }}
            </p>
          </div>
          <figcaption class="text-sm">
            <span class="font-medium">{{ side.title }}</span>
            <span v-if="side.src || side.loading" class="text-muted-foreground"> · {{ side.detail }}</span>
          </figcaption>
        </figure>
        <div class="order-2 hidden items-center text-muted-foreground sm:col-start-2 sm:row-start-1 sm:flex" aria-hidden="true">
          <ArrowRight class="size-5" />
        </div>
        <div class="order-2 flex justify-center text-muted-foreground sm:hidden" aria-hidden="true">
          <ArrowDown class="size-5" />
        </div>
      </div>

      <!-- tools -->
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <label v-if="languages.codes.length + (languages.none ? 1 : 0) > 1" class="flex items-center gap-2 text-sm">
          <span class="text-muted-foreground">Language</span>
          <select
            v-model="langFilter"
            class="h-9 rounded-md border bg-transparent px-2 text-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
          >
            <option value="all">All</option>
            <option v-for="l in languages.codes" :key="l" :value="l">{{ languageLabel(l) }}</option>
            <option v-if="languages.none" value="__none">No language</option>
          </select>
        </label>
        <SegmentedControl
          v-if="sources.size > 1"
          v-model="sourceFilter"
          label="Source"
          :options="[
            { value: 'all', label: 'All' },
            { value: 'tmdb', label: 'TMDB' },
            { value: 'fanart', label: 'Fanart' },
          ]"
        />
        <SegmentedControl
          v-model="backing"
          label="Backing"
          :options="[
            { value: 'checks', label: 'Checks' },
            { value: 'dark', label: 'Dark' },
            { value: 'light', label: 'Light' },
          ]"
        />
        <div class="ml-auto flex items-center gap-2">
          <input
            ref="fileInput"
            type="file"
            accept="image/png,image/jpeg,image/webp"
            class="sr-only"
            tabindex="-1"
            aria-hidden="true"
            data-testid="file-input"
            @change="onFileInput"
          />
          <Button variant="outline" size="sm" :disabled="uploading || saving" data-testid="upload-button" @click="pickFile">
            <Loader2 v-if="uploading" class="animate-spin" />
            <Upload v-else />
            Upload your own
          </Button>
        </div>
      </div>
      <p v-if="uploadError" role="alert" class="-mt-2 flex items-start gap-2 text-sm text-destructive" data-testid="upload-error">
        <TriangleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        {{ uploadError }}
      </p>

      <!-- candidates -->
      <div
        class="relative min-h-40 min-w-0 flex-1 overflow-y-auto rounded-md border p-3"
        :class="dragging ? 'border-primary border-dashed bg-primary/5' : ''"
        :aria-busy="loading"
      >
        <p v-if="dragging" class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-md bg-background/80 text-sm font-medium">
          Drop a PNG, JPEG or WebP image to use it
        </p>

        <div v-if="loading && !data" role="status" class="space-y-3" data-testid="loading">
          <p class="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 class="size-4 animate-spin" aria-hidden="true" />
            Asking TMDB and fanart.tv for every {{ kindWord[kind] }}. This can take a few seconds.
          </p>
          <div class="grid gap-3" :class="gridClass">
            <Skeleton v-for="n in 6" :key="n" class="rounded-md" :class="tileAspect" />
          </div>
        </div>

        <div
          v-else-if="loadError"
          role="alert"
          class="flex flex-col items-center gap-3 px-4 py-8 text-center"
          data-testid="load-error"
        >
          <TriangleAlert class="size-6 text-destructive" aria-hidden="true" />
          <div class="space-y-1">
            <p class="font-medium">Could not load the candidates</p>
            <p class="text-sm text-muted-foreground">{{ loadError }}</p>
          </div>
          <Button variant="outline" size="sm" @click="load">Try again</Button>
        </div>

        <template v-else>
          <p
            v-if="data?.errors.length"
            role="status"
            class="mb-3 flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm"
            data-testid="source-errors"
          >
            <TriangleAlert class="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
            <span>Some candidates may be missing: {{ data.errors.join('; ') }}</span>
          </p>

          <p v-if="emptyAfterList" class="px-4 py-8 text-center text-sm text-muted-foreground" data-testid="empty">
            TMDB and fanart.tv have no {{ kindWord[kind] }} for this. Upload your own with the button above, or drop a file here.
          </p>

          <ul v-else class="grid gap-3" :class="gridClass" data-testid="candidates" aria-label="Candidates">
            <li v-for="t in shown" :key="t.key" class="min-w-0">
              <button
                type="button"
                class="group block w-full space-y-1.5 rounded-md p-1 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
                :class="t.key === selectedKey ? 'bg-accent ring-2 ring-primary' : t.current ? 'ring-1 ring-primary/60' : 'hover:bg-accent/50'"
                :aria-pressed="t.key === selectedKey"
                :aria-label="tileAria(t)"
                data-testid="candidate"
                @click="choose(t)"
              >
                <span
                  class="relative flex items-center justify-center overflow-hidden rounded border"
                  :class="[tileAspect, backingClass]"
                  :style="backingStyle"
                >
                  <img
                    v-if="!failedThumbs[t.key] && tileSrc(t, false)"
                    :src="tileSrc(t, false)!"
                    alt=""
                    loading="lazy"
                    decoding="async"
                    class="size-full"
                    :class="fitClass"
                    @error="failedThumbs[t.key] = true"
                  />
                  <span v-else class="flex flex-col items-center gap-1 text-xs text-neutral-400">
                    <ImageOff class="size-5" aria-hidden="true" />
                    No preview
                  </span>
                  <span
                    v-if="t.key === selectedKey"
                    class="absolute top-1 right-1 flex size-5 items-center justify-center rounded-full bg-primary text-primary-foreground"
                    aria-hidden="true"
                  >
                    <Check class="size-3.5" />
                  </span>
                </span>
                <span class="flex flex-wrap items-center gap-1">
                  <Badge v-if="t.current" variant="default" data-testid="mark-current">
                    <Check aria-hidden="true" />
                    Current override
                  </Badge>
                  <Badge v-if="t.auto" variant="outline" data-testid="mark-auto">
                    <Sparkles aria-hidden="true" />
                    Automatic pick
                  </Badge>
                </span>
                <span class="block space-y-0.5 text-xs">
                  <span class="flex flex-wrap items-center gap-x-1.5">
                    <span class="font-medium">{{ t.source === 'upload' ? 'Upload' : sourceLabel(t.source) }}</span>
                    <span v-if="t.source !== 'upload'" class="text-muted-foreground">{{ languageLabel(t.lang) }}</span>
                  </span>
                  <span v-if="t.source !== 'upload' && (scoreLabel(t) || sizeLabel(t))" class="block text-muted-foreground tabular-nums">
                    {{ [scoreLabel(t), sizeLabel(t)].filter(Boolean).join(' · ') }}
                  </span>
                  <span v-else-if="t.note" class="block text-muted-foreground">{{ t.note }}</span>
                </span>
              </button>
            </li>
          </ul>
          <p v-if="!emptyAfterList && shown.length === 0" class="px-4 py-8 text-center text-sm text-muted-foreground">
            Nothing matches these filters.
          </p>
        </template>
      </div>

      <!-- footer -->
      <p v-if="actionError" role="alert" class="flex items-start gap-2 text-sm text-destructive" data-testid="action-error">
        <TriangleAlert class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        {{ actionError }}
      </p>
      <div class="flex flex-wrap items-center gap-x-5 gap-y-3">
        <button
          type="button"
          role="switch"
          :aria-checked="noBadges"
          class="group flex items-center gap-2 rounded-md text-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
          data-testid="no-badges"
          @click="onBadges"
        >
          <span
            class="relative h-5 w-9 rounded-full border transition-colors"
            :class="noBadges ? 'border-primary bg-primary' : 'bg-input/50'"
            aria-hidden="true"
          >
            <span
              class="absolute top-0.5 left-0.5 size-3.5 rounded-full bg-foreground transition-transform"
              :class="noBadges ? 'translate-x-4 bg-primary-foreground' : ''"
            />
          </span>
          No rating badges
        </button>
        <label v-if="hasLanguage" class="flex items-center gap-2 text-sm">
          <span class="text-muted-foreground">Applies to</span>
          <select
            :value="scope"
            class="h-9 rounded-md border bg-transparent px-2 text-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 dark:bg-input/30"
            data-testid="scope"
            :disabled="saving"
            @change="onScope"
          >
            <option value="">Every language</option>
            <option v-for="l in scopeOptions" :key="l" :value="l">Only {{ languageLabel(l) }}</option>
          </select>
        </label>
        <div class="ml-auto flex flex-wrap items-center gap-2">
          <Button
            v-if="current"
            variant="outline"
            class="text-destructive hover:text-destructive"
            :disabled="saving || uploading"
            data-testid="clear"
            @click="clear"
          >
            Clear override
          </Button>
          <Button variant="outline" :disabled="saving" @click="onOpenChange(false)">Cancel</Button>
          <Button :disabled="!canSave" data-testid="save" @click="save">
            <Loader2 v-if="saving" class="animate-spin" />
            {{ saveLabel }}
          </Button>
        </div>
      </div>
      <p class="-mt-2 text-xs text-muted-foreground">
        Saving removes the cached renders of this {{ kindWord[kind] }}. It is drawn again the next time something requests it.
      </p>
    </DialogContent>
  </Dialog>
</template>
