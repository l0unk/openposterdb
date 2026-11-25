<script setup lang="ts">
// Per-title art override picker, opened from a row of the cache browser (ImageListView).
// Resolves the row's id to its title, lists the TMDB + fanart.tv candidates, and saves,
// uploads or clears the override. All requests go through '@/lib/overrides'.
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Loader2, Upload } from 'lucide-vue-next'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Skeleton } from '@/components/ui/skeleton'
import {
  checkUploadFile,
  deleteOverride,
  formatBytes,
  getCandidates,
  kindHasLanguage,
  languageLabel,
  putOverride,
  resolveTarget,
  sameArt,
  scoreLabel,
  shadowNote,
  sourceLabel,
  uploadImage,
  type Candidates,
  type ImageKind,
  type Override,
  type OverrideBody,
  type OverrideTarget,
} from '@/lib/overrides'

const props = defineProps<{
  open: boolean
  kind: ImageKind
  /** The cached image's id, e.g. `imdb` + `tt0111161` (no variant suffix). */
  idType: string
  idValue: string
}>()
const emit = defineEmits<{
  'update:open': [open: boolean]
  changed: [message: string]
}>()

/** A candidate, the current override or an upload of this session, as one tile. */
interface Tile {
  key: string
  source: 'tmdb' | 'fanart' | 'upload'
  value: string
  fanart_id: string | null
  /** Image URL for the tile; null when it needs the admin token (a stored upload). */
  thumb: string | null
  label: string
  auto: boolean
  current: boolean
}

const kindWord = { poster: 'poster', logo: 'logo', backdrop: 'backdrop', episode: 'episode still' } as const

const target = ref<OverrideTarget | null>(null)
const data = ref<Candidates | null>(null)
const loading = ref(false)
const loadError = ref('')
let loadSeq = 0

const selectedKey = ref<string | null>(null)
const noBadges = ref(false)
const badgesTouched = ref(false)
/** Language the override applies to (posters and logos); '' = every language. */
const scope = ref('')
const scopeTouched = ref(false)

const saving = ref(false)
const actionError = ref('')
const uploading = ref(false)
const uploadError = ref('')
const uploads = ref<Tile[]>([])
const fileInput = ref<HTMLInputElement | null>(null)

function dropUploads() {
  uploads.value.forEach((u) => u.thumb && URL.revokeObjectURL(u.thumb))
  uploads.value = []
}
onBeforeUnmount(dropUploads)

const hasLanguage = computed(() => kindHasLanguage(props.kind))
const current = computed<Override | null>(() => data.value?.current ?? null)

const tiles = computed<Tile[]>(() => {
  const cur = current.value
  const list: Tile[] = (data.value?.candidates ?? []).map((c) => ({
    key: `${c.source}:${c.value}`,
    source: c.source,
    value: c.value,
    fanart_id: c.fanart_id,
    thumb: c.thumb_url,
    label: [sourceLabel(c.source), languageLabel(c.lang), scoreLabel(c)].filter(Boolean).join(', '),
    auto: c.auto,
    current: c.current,
  }))
  const session = uploads.value.map((u) => ({
    ...u,
    current: !!cur && cur.source === 'upload' && cur.value === u.value,
  }))
  const out: Tile[] = [...session]
  if (cur && cur.source !== 'auto' && ![...session, ...list].some((t) => sameArt(t, cur))) {
    out.push({
      key: `${cur.source}:${cur.value}`,
      source: cur.source,
      value: cur.value,
      fanart_id: cur.fanart_id,
      thumb: cur.source === 'upload' ? null : cur.thumb_url,
      label: cur.source === 'upload' ? 'Your upload' : `${sourceLabel(cur.source)}, not in the current list`,
      auto: false,
      current: true,
    })
  }
  out.push(...list)
  return out
})

const selected = computed(() => tiles.value.find((t) => t.key === selectedKey.value) ?? null)
const badgesChanged = computed(() => noBadges.value !== (current.value?.no_badges ?? false))
const artChanged = computed(() => !!selected.value && !selected.value.current)
const canSave = computed(() => !!target.value && !saving.value && !uploading.value && (artChanged.value || badgesChanged.value))

const scopeOptions = computed(() => {
  const set = new Set<string>(['en'])
  for (const c of data.value?.candidates ?? []) if (c.lang) set.add(c.lang)
  if (current.value?.lang) set.add(current.value.lang)
  if (scope.value) set.add(scope.value)
  return [...set].sort((a, b) => (a === 'en' ? -1 : b === 'en' ? 1 : a.localeCompare(b)))
})

async function load() {
  const seq = ++loadSeq
  loading.value = true
  loadError.value = ''
  try {
    if (!target.value) target.value = await resolveTarget(props.idType, props.idValue, props.kind)
    const res = await getCandidates(target.value, hasLanguage.value ? scope.value || undefined : undefined)
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

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return
    loadSeq++
    target.value = null
    data.value = null
    selectedKey.value = null
    noBadges.value = false
    badgesTouched.value = false
    scope.value = ''
    scopeTouched.value = false
    actionError.value = ''
    uploadError.value = ''
    dropUploads()
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

function onBadges(e: Event) {
  noBadges.value = (e.target as HTMLInputElement).checked
  badgesTouched.value = true
}

function choose(t: Tile) {
  selectedKey.value = t.key === selectedKey.value ? null : t.key
}

async function onFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
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
    const tile: Tile = {
      key: `upload:${r.sha256}`,
      source: 'upload',
      value: r.sha256,
      fanart_id: null,
      thumb: URL.createObjectURL(file),
      label: `Your upload, ${r.width}×${r.height}, ${formatBytes(r.bytes)}`,
      auto: false,
      current: false,
    }
    uploads.value = [tile, ...uploads.value.filter((u) => u.key !== tile.key)]
    selectedKey.value = tile.key
  } catch (e) {
    uploadError.value = e instanceof Error ? e.message : 'The upload failed'
  } finally {
    uploading.value = false
  }
}

async function save() {
  const t = target.value
  if (!t || !canSave.value) return
  saving.value = true
  actionError.value = ''
  try {
    const pick = selected.value
    const cur = current.value
    const lang = hasLanguage.value ? (pick ? scope.value : (cur?.lang ?? scope.value)) || undefined : undefined
    let body: OverrideBody
    if (pick) {
      body = { source: pick.source, value: pick.value, no_badges: noBadges.value }
      if (pick.source === 'fanart' && pick.fanart_id) body.fanart_id = pick.fanart_id
    } else if (cur && cur.source !== 'auto') {
      body = { source: cur.source, value: cur.value, no_badges: noBadges.value }
      if (cur.fanart_id) body.fanart_id = cur.fanart_id
    } else if (noBadges.value) {
      body = { source: 'auto', no_badges: true }
    } else {
      // Back to fully automatic: nothing left to store.
      await deleteOverride(t, cur?.lang ?? lang)
      done(`Cleared the ${kindWord[props.kind]} override.`)
      return
    }
    const res = await putOverride(t, body, lang)
    const what = pick
      ? `Set the ${kindWord[props.kind]} to ${pick.label}${noBadges.value ? ', without badges' : ''}.`
      : `Turned the rating badges ${noBadges.value ? 'off' : 'on'} for this ${kindWord[props.kind]}.`
    done(what + shadowNote(res?.shadowed_by))
  } catch (e) {
    actionError.value = e instanceof Error ? e.message : 'The request failed'
  } finally {
    saving.value = false
  }
}

async function clear() {
  const t = target.value
  const cur = current.value
  if (!t || !cur || saving.value) return
  saving.value = true
  actionError.value = ''
  try {
    await deleteOverride(t, hasLanguage.value ? cur.lang : null)
    done(`Cleared the ${kindWord[props.kind]} override. The automatic pick is used again.`)
  } catch (e) {
    actionError.value = e instanceof Error ? e.message : 'The request failed'
  } finally {
    saving.value = false
  }
}

function done(message: string) {
  emit('changed', message)
  emit('update:open', false)
}

function onOpenChange(v: boolean) {
  if (!v && saving.value) return
  emit('update:open', v)
}

const tileAspect = computed(() => (props.kind === 'poster' ? 'aspect-[2/3]' : 'aspect-video'))
const gridClass = computed(() =>
  props.kind === 'poster' ? 'grid-cols-[repeat(auto-fill,minmax(7rem,1fr))]' : 'grid-cols-[repeat(auto-fill,minmax(11rem,1fr))]',
)
</script>

<template>
  <Dialog :open="open" @update:open="onOpenChange">
    <DialogContent class="flex max-h-[90vh] flex-col gap-4 sm:max-w-4xl" data-testid="override-picker">
      <DialogHeader>
        <DialogTitle>Override {{ kindWord[kind] }}</DialogTitle>
        <DialogDescription class="break-all font-mono text-xs">
          {{ idType }}/{{ idValue }}<template v-if="target"> → {{ target.mediaType }}/{{ target.tmdbId }}<template v-if="target.kind === 'episode'"> S{{ target.season }}E{{ target.episode }}</template></template>
        </DialogDescription>
      </DialogHeader>

      <div class="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
        <label v-if="hasLanguage" class="flex items-center gap-2">
          Applies to
          <select class="rounded-md border bg-background px-2 py-1" :value="scope" data-testid="override-scope" @change="onScope">
            <option value="">every language</option>
            <option v-for="l in scopeOptions" :key="l" :value="l">{{ languageLabel(l) }} requests</option>
          </select>
        </label>
        <label class="flex items-center gap-2">
          <input type="checkbox" :checked="noBadges" data-testid="override-no-badges" @change="onBadges" />
          No rating badges
        </label>
        <Button variant="outline" size="sm" :disabled="uploading || saving || !target" @click="fileInput?.click()">
          <Loader2 v-if="uploading" class="size-4 animate-spin mr-1" />
          <Upload v-else class="size-4 mr-1" />
          Upload image
        </Button>
        <input ref="fileInput" type="file" accept="image/png,image/jpeg,image/webp" class="hidden" data-testid="override-file" @change="onFile" />
      </div>
      <p v-if="uploadError" class="text-sm text-destructive">{{ uploadError }}</p>
      <p v-if="data?.errors.length" class="text-sm text-muted-foreground">Some sources failed: {{ data.errors.join('; ') }}</p>

      <div class="min-h-[160px] overflow-y-auto">
        <div v-if="loading && !data" :class="['grid gap-3', gridClass]">
          <Skeleton v-for="i in 6" :key="i" :class="['w-full rounded-md', tileAspect]" />
        </div>
        <div v-else-if="loadError" class="space-y-2 text-sm">
          <p class="text-destructive">{{ loadError }}</p>
          <Button variant="outline" size="sm" @click="load">Retry</Button>
        </div>
        <p v-else-if="tiles.length === 0" class="text-sm text-muted-foreground">
          No candidates found. You can upload an image instead.
        </p>
        <div v-else :class="['grid gap-3', gridClass]">
          <button
            v-for="t in tiles"
            :key="t.key"
            type="button"
            :aria-pressed="t.key === selectedKey"
            :aria-label="[t.label, t.auto ? 'automatic pick' : '', t.current ? 'current override' : ''].filter(Boolean).join(', ')"
            :class="[
              'group flex flex-col gap-1 rounded-md border p-1 text-left text-xs transition-colors',
              t.key === selectedKey ? 'border-primary ring-2 ring-primary' : 'hover:border-foreground/40',
            ]"
            data-testid="override-tile"
            @click="choose(t)"
          >
            <div :class="['flex w-full items-center justify-center overflow-hidden rounded bg-neutral-900', tileAspect]">
              <img v-if="t.thumb" :src="t.thumb" alt="" loading="lazy" class="max-h-full max-w-full object-contain" />
              <span v-else class="text-muted-foreground">{{ t.label }}</span>
            </div>
            <span class="line-clamp-2 text-muted-foreground">{{ t.label }}</span>
            <span class="flex flex-wrap gap-1">
              <Badge v-if="t.auto" variant="secondary">Automatic pick</Badge>
              <Badge v-if="t.current">Current</Badge>
            </span>
          </button>
        </div>
      </div>

      <p v-if="actionError" class="text-sm text-destructive">{{ actionError }}</p>
      <div class="flex flex-wrap justify-end gap-2">
        <Button v-if="current" variant="outline" class="mr-auto text-destructive hover:text-destructive" :disabled="saving" @click="clear">
          Clear override
        </Button>
        <Button variant="outline" :disabled="saving" @click="onOpenChange(false)">Cancel</Button>
        <Button :disabled="!canSave" @click="save">
          <Loader2 v-if="saving" class="size-4 animate-spin mr-1" />
          Save
        </Button>
      </div>
    </DialogContent>
  </Dialog>
</template>
