<script setup lang="ts">
// louk-fork: one cached image of a title, with view and purge.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { Eye, ImageOff, Trash2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { fetchImageBlob, type TitleImage } from '@/louk/api'
import { withBust } from '@/louk/overrides'
import { relativeTime } from '@/louk/format'

// `bust`: bumped after an override change so the tile asks the server again instead of reusing the browser's copy.
// `also`: other renders with the same settings under other ids of the title (shown as one tile).
const props = defineProps<{ image: TitleImage; bust?: number; also?: TitleImage[] }>()
const emit = defineEmits<{ view: [image: TitleImage]; purge: [image: TitleImage] }>()

const root = ref<HTMLElement | null>(null)
const src = ref<string | null>(null)
const state = ref<'idle' | 'loading' | 'ready' | 'failed'>('idle')
let observer: IntersectionObserver | null = null
let gone = false

const aspect = computed(() => {
  if (props.image.kind === 'poster') return 'aspect-[2/3]'
  if (props.image.kind === 'logo') return 'aspect-[16/9]'
  return 'aspect-video'
})
const fit = computed(() => (props.image.kind === 'logo' ? 'object-contain p-4' : 'object-cover'))
// Logos are often black or white on transparent: a mid-grey checkerboard shows both.
const logoBacking =
  'background-color:#8b8b93;background-image:linear-gradient(45deg,#b5b5bd 25%,transparent 25%,transparent 75%,#b5b5bd 75%),linear-gradient(45deg,#b5b5bd 25%,transparent 25%,transparent 75%,#b5b5bd 75%);background-size:16px 16px;background-position:0 0,8px 8px'
const alsoText = computed(() => (props.also ?? []).map((i) => i.id_type).join(', '))

async function load() {
  if (state.value !== 'idle') return
  state.value = 'loading'
  try {
    const blob = await fetchImageBlob(withBust(props.image.admin_image_url, props.bust))
    if (gone) return
    src.value = URL.createObjectURL(blob)
    state.value = 'ready'
  } catch {
    if (!gone) state.value = 'failed'
  }
}

onMounted(() => {
  if (typeof IntersectionObserver === 'undefined' || !root.value) {
    load()
    return
  }
  observer = new IntersectionObserver(
    (entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        observer?.disconnect()
        load()
      }
    },
    { rootMargin: '200px' },
  )
  observer.observe(root.value)
})

onBeforeUnmount(() => {
  gone = true
  observer?.disconnect()
  if (src.value) URL.revokeObjectURL(src.value)
})
</script>

<template>
  <figure ref="root" class="space-y-2" data-testid="image-tile">
    <button
      type="button"
      class="relative block w-full overflow-hidden rounded-md border bg-muted/60 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
      :class="aspect"
      :style="image.kind === 'logo' ? logoBacking : undefined"
      :aria-label="`View ${image.kind} ${image.cache_value}`"
      :disabled="state === 'failed'"
      @click="emit('view', image)"
    >
      <img v-if="src" :src="src" :alt="`${image.kind} ${image.cache_value}`" class="size-full" :class="fit" />
      <Skeleton v-else-if="state !== 'failed'" class="size-full rounded-none" />
      <span
        v-else
        class="flex size-full flex-col items-center justify-center gap-1 text-xs text-muted-foreground"
        role="img"
        aria-label="Image failed to load"
      >
        <ImageOff class="size-5" aria-hidden="true" />
        Failed to load
      </span>
    </button>
    <figcaption class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <p class="truncate font-mono text-xs" :title="image.cache_value">{{ image.cache_value }}</p>
        <p class="text-xs whitespace-nowrap text-muted-foreground">Cached {{ relativeTime(image.created_at) }}</p>
        <p v-if="alsoText" class="text-xs text-muted-foreground" data-testid="also-cached">Also cached under {{ alsoText }}</p>
      </div>
      <div class="flex shrink-0 gap-0.5">
        <Button
          variant="ghost"
          size="icon-sm"
          class="text-muted-foreground"
          :aria-label="`View ${image.kind} ${image.cache_value}`"
          :disabled="state === 'failed'"
          @click="emit('view', image)"
        >
          <Eye />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          class="text-muted-foreground hover:text-destructive"
          :aria-label="`Purge ${image.kind} ${image.cache_value}`"
          @click="emit('purge', image)"
        >
          <Trash2 />
        </Button>
      </div>
    </figcaption>
  </figure>
</template>
