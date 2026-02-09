<template>
  <div class="inquiry-panel">
    <div class="inquiry-panel-header">
      <div class="inquiry-panel-title">
        {{ t('chat.inquiry.title') }}
        <span class="inquiry-panel-mode">{{ modeLabel }}</span>
      </div>
      <div class="inquiry-panel-question">{{ panel.question }}</div>
    </div>
    <div class="inquiry-panel-routes">
      <button
        v-for="(route, index) in panel.routes"
        :key="`${index}-${route.label}`"
        type="button"
        :class="[
          'inquiry-panel-route',
          { active: selectedIndices.includes(index), recommended: route.recommended }
        ]"
        @click="toggleSelection(index)"
      >
        <div class="inquiry-route-main">
          <span class="inquiry-route-label">{{ route.label }}</span>
          <span v-if="route.recommended" class="inquiry-route-tag">
            {{ t('chat.inquiry.recommended') }}
          </span>
        </div>
        <div v-if="route.description" class="inquiry-route-desc">{{ route.description }}</div>
      </button>
    </div>
    <div class="inquiry-panel-hint">{{ modeHint }}</div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps({
  panel: {
    type: Object,
    required: true
  }
});

const emit = defineEmits(['update:selected']);
const { t } = useI18n();

const selectedIndices = ref([]);
const isMultiple = computed(() => props.panel?.multiple === true);
const modeLabel = computed(() =>
  isMultiple.value ? t('chat.inquiry.mode.multi') : t('chat.inquiry.mode.single')
);
const modeHint = computed(() =>
  isMultiple.value ? t('chat.inquiry.hint.multi') : t('chat.inquiry.hint.single')
);

const emitSelection = () => {
  emit('update:selected', selectedIndices.value.slice());
};

const resetSelection = () => {
  selectedIndices.value = [];
  emitSelection();
};

const toggleSelection = (index) => {
  if (!isMultiple.value) {
    if (selectedIndices.value.length === 1 && selectedIndices.value[0] === index) {
      selectedIndices.value = [];
    } else {
      selectedIndices.value = [index];
    }
    emitSelection();
    return;
  }
  const next = new Set(selectedIndices.value);
  if (next.has(index)) {
    next.delete(index);
  } else {
    next.add(index);
  }
  selectedIndices.value = Array.from(next);
  emitSelection();
};

watch(
  () => props.panel,
  () => {
    resetSelection();
  }
);
</script>

