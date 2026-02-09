<template>
  <div class="plan-floating" :class="{ expanded }">
    <div class="plan-panel" :class="{ expanded }" :aria-hidden="expanded ? 'false' : 'true'">
      <div class="plan-board">
        <div v-if="planExplanation" class="plan-explanation">{{ planExplanation }}</div>
        <div class="plan-steps">
          <div
            v-for="(item, index) in steps"
            :key="`${index}-${item.step}`"
            :class="['plan-step', `plan-step--${item.status}`]"
          >
            <span class="plan-index">{{ index + 1 }}</span>
            <div class="plan-text">{{ item.step }}</div>
            <span class="plan-status">{{ formatPlanStatus(item.status) }}</span>
          </div>
        </div>
      </div>
    </div>
    <button
      class="plan-tab"
      type="button"
      :aria-expanded="expanded ? 'true' : 'false'"
      :aria-label="t('chat.workflow.plan.title')"
      @click="toggleExpanded"
      @keydown.enter.prevent="toggleExpanded"
      @keydown.space.prevent="toggleExpanded"
    >
      <span class="plan-tab-title">{{ t('chat.workflow.plan.title') }}</span>
      <span class="plan-tab-divider" />
      <span class="plan-tab-progress">{{ progressLabel }}</span>
      <span v-if="currentStep" class="plan-tab-step" :title="currentStep.step">
        {{ currentStep.step }}
      </span>
      <i class="fa-solid fa-chevron-up plan-tab-icon" :class="{ expanded }" aria-hidden="true"></i>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps({
  plan: {
    type: Object,
    default: null
  },
  expanded: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:expanded']);
const { t } = useI18n();

const steps = computed(() => (Array.isArray(props.plan?.steps) ? props.plan.steps : []));
const planExplanation = computed(() => String(props.plan?.explanation || '').trim());

const currentIndex = computed(() => {
  const list = steps.value;
  if (!list.length) return -1;
  const inProgressIndex = list.findIndex((item) => item?.status === 'in_progress');
  if (inProgressIndex >= 0) return inProgressIndex;
  const pendingIndex = list.findIndex((item) => item?.status === 'pending');
  if (pendingIndex >= 0) return pendingIndex;
  return list.length - 1;
});

const currentStep = computed(() => {
  const index = currentIndex.value;
  return index >= 0 ? steps.value[index] : null;
});

const statusLabel = computed(() =>
  currentStep.value ? formatPlanStatus(currentStep.value.status) : ''
);

const progressLabel = computed(() => {
  if (!steps.value.length || currentIndex.value < 0) return '';
  const ratio = `${currentIndex.value + 1}/${steps.value.length}`;
  return statusLabel.value ? `${ratio} ${statusLabel.value}` : ratio;
});

const toggleExpanded = () => {
  emit('update:expanded', !props.expanded);
};

const formatPlanStatus = (status) => {
  if (status === 'completed') return t('chat.workflow.plan.status.completed');
  if (status === 'in_progress') return t('chat.workflow.plan.status.inProgress');
  return t('chat.workflow.plan.status.pending');
};
</script>
