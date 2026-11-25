<script setup lang="ts" generic="T extends string">
// louk-fork: a small labelled button group for filters (native buttons, aria-pressed).
defineProps<{
  label: string
  options: { value: T; label: string }[]
}>()
const model = defineModel<T>({ required: true })
</script>

<template>
  <div role="group" :aria-label="label" class="inline-flex rounded-md border bg-muted/40 p-0.5">
    <button
      v-for="opt in options"
      :key="opt.value"
      type="button"
      :aria-pressed="model === opt.value"
      class="rounded-[5px] px-3 py-1 text-sm font-medium text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-sm dark:aria-pressed:bg-input/50"
      @click="model = opt.value"
    >
      {{ opt.label }}
    </button>
  </div>
</template>
