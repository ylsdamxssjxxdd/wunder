<template>
  <div class="tt-json-tree">
    <div v-if="branch" class="tt-json-row">
      <button class="tt-json-toggle" type="button" @click="open = !open">
        <span class="tt-json-punct">{{ array ? '[' : '{' }}</span>
      </button>
      <span v-if="!open" class="tt-json-ellipsis">…</span>
      <span v-if="!open" class="tt-json-count">{{ entries(parsed).length }}</span>
      <span v-if="!open" class="tt-json-punct">{{ array ? ']' : '}' }}</span>
      <span v-if="!open && label !== null" class="tt-json-label">"{{ label }}"</span>
    </div>
    <template v-if="branch && open">
      <div v-for="entry in entries(parsed)" :key="entry.key" class="tt-json-child">
        <TrajectoryJsonTree
          :data="entry.value"
          :label="entry.key"
          :collapsed-string-lines="collapsedStringLines"
        />
      </div>
      <div class="tt-json-row">
        <span class="tt-json-punct">{{ array ? ']' : '}' }}</span>
      </div>
    </template>
    <div v-if="!branch" class="tt-json-row">
      <span v-if="label !== null" class="tt-json-label">"{{ label }}"</span>
      <span
        v-if="isLongString(parsed)"
        class="tt-json-string"
        :class="{ 'tt-json-string-open': stringOpen }"
        :style="stringStyle"
        @click="stringOpen = !stringOpen"
      >"{{ parsed }}"</span>
      <span v-else :class="valueClass(parsed)">{{ valueText(parsed) }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'

const props = withDefaults(defineProps<{
  data: unknown
  label?: string | null
  collapsedStringLines?: number
}>(), {
  label: null,
  collapsedStringLines: 12,
})

const open = ref(true)
const stringOpen = ref(false)

const parsed = computed(() => props.data)
const branch = computed(() => props.data !== null && typeof props.data === 'object')
const array = computed(() => Array.isArray(props.data))

interface JsonEntry { key: string; value: unknown }

function entries(value: unknown): JsonEntry[] {
  if (Array.isArray(value)) {
    return value.map((item, index) => ({ key: String(index), value: item }))
  }
  if (value !== null && typeof value === 'object') {
    return Object.entries(value as Record<string, unknown>)
      .map(([key, item]) => ({ key, value: item }))
  }
  return []
}

function isLongString(value: unknown): boolean {
  if (typeof value !== 'string') return false
  return value.length > 1024 || value.split('\n').length > props.collapsedStringLines
}

const stringStyle = computed(() =>
  stringOpen.value ? undefined : { maxHeight: `${props.collapsedStringLines * 16}px` })

function valueClass(value: unknown): string {
  if (value === null || value === undefined) return 'tt-json-keyword'
  if (typeof value === 'string') return 'tt-json-string'
  if (typeof value === 'number') return 'tt-json-number'
  if (typeof value === 'boolean') return 'tt-json-keyword'
  return 'tt-json-punct'
}

function valueText(value: unknown): string {
  if (value === null) return 'null'
  if (value === undefined) return 'undefined'
  if (typeof value === 'string') return JSON.stringify(value)
  return String(value)
}
</script>
