<script setup lang="ts">
// louk-fork: seasons -> episodes of a show, with TMDB stills and the cached renders per episode.
import { reactive } from 'vue'
import { ChevronRight, Eye, Trash2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Badge } from '@/components/ui/badge'
import type { EpisodeEntry, SeasonEntry, TitleImage } from '@/louk/api'
import { overrideLabel, type Override } from '@/louk/overrides'
import { relativeTime } from '@/louk/format'

const props = defineProps<{ seasons: SeasonEntry[]; overrides?: Override[] }>()
const emit = defineEmits<{
  view: [image: TitleImage]
  purge: [image: TitleImage]
  override: [episode: { season: number; episode: number; name: string; rendered: TitleImage | null }]
}>()

function cachedCount(s: SeasonEntry): number {
  return s.episodes.filter((e) => e.cached.length > 0).length
}

// Open the seasons that hold cached renders; if none do, open the first one.
const open = reactive<Record<number, boolean>>({})
const anyCached = props.seasons.some((s) => cachedCount(s) > 0)
props.seasons.forEach((s, i) => {
  open[s.season] = anyCached ? cachedCount(s) > 0 : i === 0
})

function episodeOverrides(season: number, episode: number): Override[] {
  return (props.overrides ?? []).filter((o) => o.kind === 'episode' && o.season === season && o.episode === episode)
}

function seasonLabel(s: SeasonEntry): string {
  return s.season === 0 ? 'Specials' : `Season ${s.season}`
}

function episodeName(e: EpisodeEntry): string {
  return e.name?.trim() || `Episode ${e.episode}`
}
</script>

<template>
  <div class="space-y-2" data-testid="season-list">
    <Collapsible
      v-for="s in seasons"
      :key="s.season"
      v-model:open="open[s.season]"
      class="rounded-lg border"
      data-testid="season"
    >
      <CollapsibleTrigger
        class="group flex w-full items-center gap-2 rounded-lg px-3 py-2.5 text-left outline-none hover:bg-accent/50 focus-visible:ring-[3px] focus-visible:ring-ring/50"
      >
        <ChevronRight
          class="size-4 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-90"
          aria-hidden="true"
        />
        <span class="font-medium">{{ seasonLabel(s) }}</span>
        <span class="ml-auto text-sm text-muted-foreground tabular-nums">
          {{ cachedCount(s) }} of {{ s.episodes.length }} cached
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ul class="divide-y border-t">
          <li
            v-for="e in s.episodes"
            :key="e.episode"
            class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2"
            data-testid="episode"
          >
            <div class="h-[52px] w-[92px] shrink-0 overflow-hidden rounded bg-muted">
              <img
                v-if="e.still_thumb_url"
                :src="e.still_thumb_url"
                alt=""
                loading="lazy"
                decoding="async"
                class="size-full object-cover"
              />
            </div>
            <div class="min-w-0 flex-1 basis-40">
              <p class="truncate text-sm" :class="e.cached.length ? 'font-medium' : 'text-muted-foreground'">
                <span class="mr-1.5 font-mono text-xs text-muted-foreground">E{{ e.episode }}</span>{{ episodeName(e) }}
              </p>
              <p v-if="!e.cached.length" class="text-xs text-muted-foreground">Not cached</p>
              <Badge
                v-for="o in episodeOverrides(s.season, e.episode)"
                :key="`${o.season}-${o.episode}-${o.lang ?? ''}`"
                variant="secondary"
                class="mt-0.5"
                data-testid="override-chip"
              >
                Override active: {{ overrideLabel(o) }}
              </Badge>
            </div>
            <ul v-if="e.cached.length" class="flex flex-wrap items-center gap-1.5">
              <li
                v-for="img in e.cached"
                :key="img.cache_value"
                class="flex items-center rounded-md border bg-muted/40 pl-2"
              >
                <span class="max-w-40 truncate font-mono text-xs" :title="img.cache_value">{{ img.cache_value }}</span>
                <span class="mx-1.5 text-xs text-muted-foreground">{{ relativeTime(img.created_at) }}</span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="size-7 text-muted-foreground"
                  :aria-label="`View episode ${img.cache_value}`"
                  @click="emit('view', img)"
                >
                  <Eye />
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  class="size-7 text-muted-foreground hover:text-destructive"
                  :aria-label="`Purge episode ${img.cache_value}`"
                  @click="emit('purge', img)"
                >
                  <Trash2 />
                </Button>
              </li>
            </ul>
            <Button
              variant="outline"
              size="sm"
              :aria-label="`Override the still of ${seasonLabel(s)} ${episodeName(e)}`"
              data-testid="override-episode"
              @click="emit('override', { season: s.season, episode: e.episode, name: episodeName(e), rendered: e.cached[0] ?? null })"
            >
              Override
            </Button>
          </li>
        </ul>
      </CollapsibleContent>
    </Collapsible>
  </div>
</template>
