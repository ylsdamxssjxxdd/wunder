<template>
  <div class="inquiry-panel">
    <div class="inquiry-panel-header">
      <div class="inquiry-panel-title">
        问询面板
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
          <span v-if="route.recommended" class="inquiry-route-tag">推荐</span>
        </div>
        <div v-if="route.description" class="inquiry-route-desc">{{ route.description }}</div>
      </button>
    </div>
    <div class="inquiry-panel-actions">
      <button class="inquiry-panel-btn secondary" type="button" @click="dismissPanel">
        暂不选择
      </button>
      <button
        class="inquiry-panel-btn primary"
        type="button"
        :disabled="!canSubmit"
        @click="submitSelection"
      >
        发送选择
      </button>
    </div>
    <div class="inquiry-panel-hint">{{ modeHint }}，也可以直接输入消息继续。</div>
  </div>
</template>

<script setup>
import { computed, ref, watch } from 'vue';

const props = defineProps({
  panel: {
    type: Object,
    required: true
  }
});

const emit = defineEmits(['select', 'dismiss']);

const selectedIndices = ref([]);
const isMultiple = computed(() => props.panel?.multiple === true);
const canSubmit = computed(() => selectedIndices.value.length > 0);
const modeLabel = computed(() => (isMultiple.value ? '多选' : '单选'));
const modeHint = computed(() =>
  isMultiple.value ? '多选，可选择多个选项' : '单选，再次点击已选项可取消'
);

const resetSelection = () => {
  selectedIndices.value = [];
};

const toggleSelection = (index) => {
  if (!isMultiple.value) {
    if (selectedIndices.value.length === 1 && selectedIndices.value[0] === index) {
      selectedIndices.value = [];
    } else {
      selectedIndices.value = [index];
    }
    return;
  }
  const next = new Set(selectedIndices.value);
  if (next.has(index)) {
    next.delete(index);
  } else {
    next.add(index);
  }
  selectedIndices.value = Array.from(next);
};

const submitSelection = () => {
  if (!canSubmit.value) return;
  const selected = selectedIndices.value.slice();
  emit('select', { selected });
};

const dismissPanel = () => {
  emit('dismiss');
};

watch(
  () => props.panel,
  () => {
    resetSelection();
  }
);
</script>

