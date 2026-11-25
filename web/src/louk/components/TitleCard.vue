<script setup lang="ts">
// louk-fork: one poster in the Library wall.
import { computed, ref, watch } from 'vue'
import { Clapperboard, Film, Image, Stamp, Tv, Wallpaper } from 'lucide-vue-next'
import { titleLabel, titleRoute, type TitleSummary } from '@/louk/api'
import { relativeTime } from '@/louk/format'

const props = defineProps<{ title: TitleSummary }>()

const label = computed(() => titleLabel(props.title))
const resolved = computed(() => !!props.title.name?.trim())
const isShow = computed(() => props.title.media_type === 'tv')

const imageFailed = ref(false)
watch(() => props.title.poster_thumb_url, () => (imageFailed.value = false))
const showImage = computed(() => !!props.title.poster_thumb_url && !imageFailed.value)

const counts = computed(() => [
  { key: 'poster', label: 'poster', icon: Image, n: props.title.image_counts.poster },
  { key: 'backdrop', label: 'backdrop', icon: Wallpaper, n: props.title.image_counts.backdrop },
  { key: 'logo', label: 'logo', icon: Stamp, n: props.title.image_counts.logo },
  ...(isShow.value
    ? [{ key: 'episode', label: 'episode', icon: Clapperboard, n: props.title.image_counts.episode }]
    : []),
])
</script>

<template>
  <router-link
    :to="titleRoute(title)"
    class="group block rounded-lg outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
    data-testid="title-card"
  >
    <div
      class="relative aspect-[2/3] overflow-hidden rounded-lg border bg-muted transition-shadow group-hover:ring-2 group-hover:ring-ring/60 group-focus-visible:ring-2 group-focus-visible:ring-ring/60"
    >
      <img
        v-if="showImage"
        :src="title.poster_thumb_url!"
        alt=""
        loading="lazy"
        decoding="async"
        class="size-full object-cover"
        @error="imageFailed = true"
      />
      <div
        v-else
        class="flex size-full flex-col items-center justify-center gap-2 p-3 text-center text-muted-foreground"
        data-testid="poster-placeholder"
      >
        <component :is="isShow ? Tv : Film" class="size-8 opacity-60" aria-hidden="true" />
        <span class="font-mono text-xs break-all">tmdb {{ title.tmdb_id }}</span>
      </div>
      <span
        v-if="isShow"
        class="absolute top-1.5 left-1.5 inline-flex items-center gap-1 rounded-md bg-background/80 px-1.5 py-0.5 text-[11px] font-medium backdrop-blur-sm"
      >
        <Tv class="size-3" aria-hidden="true" />
        Show
      </span>
    </div>
    <div class="mt-2 space-y-1">
      <p
        class="truncate text-sm font-medium leading-tight"
        :class="resolved ? '' : 'font-mono text-muted-foreground'"
        :title="label"
      >
        {{ label }}
      </p>
      <p class="flex items-baseline justify-between gap-2 text-xs text-muted-foreground">
        <span>{{ resolved ? (title.year ?? 'Year unknown') : 'Not looked up yet' }}</span>
        <span class="shrink-0 tabular-nums">{{ relativeTime(title.last_requested) }}</span>
      </p>
      <ul class="flex gap-3 text-xs tabular-nums" aria-label="Cached images">
        <li
          v-for="c in counts"
          :key="c.key"
          class="inline-flex items-center gap-1"
          :class="c.n === 0 ? 'text-muted-foreground/50' : 'text-foreground/80'"
          :title="`${c.n} ${c.label}${c.n === 1 ? '' : 's'}`"
        >
          <component :is="c.icon" class="size-3.5" aria-hidden="true" />
          <span aria-hidden="true">{{ c.n }}</span>
          <span class="sr-only">{{ c.n }} {{ c.label }}{{ c.n === 1 ? '' : 's' }}</span>
        </li>
      </ul>
    </div>
  </router-link>
</template>
