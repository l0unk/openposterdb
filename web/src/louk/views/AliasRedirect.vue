<script setup lang="ts">
// louk-fork: turns "this id" (as shown in the id-only cache lists) into the title page it belongs to.
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { Loader2 } from 'lucide-vue-next'
import { Button } from '@/components/ui/button'
import { resolveAlias, titleRoute } from '@/louk/api'

const route = useRoute()
const router = useRouter()
const state = ref<'loading' | 'missing' | 'failed'>('loading')
const message = ref('')

async function resolve() {
  state.value = 'loading'
  const idType = String(route.params.idType)
  const idValue = String(route.params.idValue)
  try {
    const target = await resolveAlias(idType, idValue)
    if (target) await router.replace(titleRoute(target))
    else state.value = 'missing'
  } catch (e) {
    message.value = e instanceof Error ? e.message : 'The request failed'
    state.value = 'failed'
  }
}

onMounted(resolve)
</script>

<template>
  <div class="flex flex-col items-center gap-3 px-6 py-20 text-center" :aria-busy="state === 'loading'">
    <template v-if="state === 'loading'">
      <Loader2 class="size-6 animate-spin text-muted-foreground" aria-hidden="true" />
      <p class="text-sm text-muted-foreground" role="status">Finding the title…</p>
    </template>
    <template v-else-if="state === 'missing'">
      <p class="font-medium">This id is not in the library yet</p>
      <p class="max-w-md text-sm text-muted-foreground">
        <span class="font-mono">{{ route.params.idType }}/{{ route.params.idValue }}</span>
        has not been matched to a title. Older cache entries are indexed in the background, so try again in a while.
      </p>
      <Button variant="outline" size="sm" @click="router.push({ name: 'library' })">Open the library</Button>
    </template>
    <template v-else>
      <p class="font-medium" role="alert">Could not look up this id</p>
      <p class="text-sm text-muted-foreground">{{ message }}</p>
      <Button variant="outline" size="sm" @click="resolve">Try again</Button>
    </template>
  </div>
</template>
