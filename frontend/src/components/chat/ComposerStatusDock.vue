<template>
  <div v-if="hasPlan" class="status-dock" role="status">
    <div class="status-dock-pill">
      <span v-if="loading" class="status-dock-spinner" aria-hidden="true">
        <i class="fa-solid fa-circle-notch"></i>
      </span>
      <span class="status-dock-steps">
        {{ t('chat.activity.steps', { done: planProgress.done, total: planProgress.total }) }}
      </span>
    </div>
    <transition name="status-dock-fade">
      <div v-if="hasPlan" class="status-dock-pop" role="presentation">
        <button
          class="status-dock-pop-close"
          type="button"
          :title="t('chat.workflow.plan.remove')"
          :aria-label="t('chat.workflow.plan.remove')"
          @click="handleDismiss"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
        <div v-if="planExplanation" class="status-dock-pop-explain">{{ planExplanation }}</div>
        <div class="status-dock-pop-steps">
          <div
            v-for="(step, index) in planSteps"
            :key="`${index}-${step.text}`"
            :class="['status-dock-step', `is-${step.status}`]"
          >
            <svg class="status-dock-step-icon" viewBox="0 0 20 20" aria-hidden="true">
              <circle cx="10" cy="10" r="8.25" />
              <path
                v-if="step.status === 'completed'"
                d="M6.3 10.3 8.9 12.9 13.7 7.4"
              />
              <path
                v-else-if="step.status === 'in_progress'"
                d="M8.2 7.4v5.2M11.8 7.4v5.2"
              />
            </svg>
            <span class="status-dock-step-text">{{ step.text }}</span>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { summarizePlanProgress } from '@/components/chat/composerActivity';

/**
 * 输入区上方的计划状态条：只有「计划面板」写过步骤才出现，居中胶囊给出
 * 步骤进度，步骤卡常驻在胶囊上方。运行阶段提示不在这里——每个智能体气泡
 * 自己已经带运行信息。目标条（MessageGoalBar）仍在其下，两者不合并。
 */
const props = withDefaults(defineProps<{
  loading?: boolean;
  plan?: unknown;
}>(), {
  loading: false,
  plan: null
});

const emit = defineEmits<{
  (event: 'remove'): void;
}>();

const { t } = useI18n();

const planProgress = computed(() => summarizePlanProgress(props.plan));

const hasPlan = computed(() => planProgress.value.total > 0);

const planSteps = computed(() => {
  const steps = Array.isArray((props.plan as { steps?: unknown } | null)?.steps)
    ? (props.plan as { steps: Array<{ step?: unknown; status?: unknown }> }).steps
    : [];
  return steps.slice(0, 32).map((step) => ({
    text: String(step?.step ?? '').trim(),
    status: String(step?.status ?? '').trim().toLowerCase()
  }));
});

const planExplanation = computed(() =>
  String((props.plan as { explanation?: unknown } | null)?.explanation ?? '').trim()
);

const handleDismiss = () => {
  emit('remove');
};
</script>

<style scoped>
.status-dock {
  position: relative;
  display: flex;
  justify-content: center;
  margin: 0 0 8px;
  min-height: 26px;
}

.status-dock-pill {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: min(640px, calc(100vw - 48px));
  padding: 4px 14px;
  border: 1px solid rgba(15, 23, 42, 0.1);
  border-radius: 999px;
  background: #ffffff;
  color: #4b5563;
  font-size: 12px;
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  box-shadow: 0 1px 4px rgba(15, 23, 42, 0.06);
}

.status-dock-spinner {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  color: #6b7280;
  font-size: 12px;
}

.status-dock-spinner .fa-circle-notch {
  animation: status-dock-spin 1.1s linear infinite;
}

.status-dock-steps {
  flex: 0 0 auto;
  font-variant-numeric: tabular-nums;
}

/* 计划步骤卡：有步骤时常驻在胶囊上方，浮在消息流之上，细环三态图标读作进度。 */
.status-dock-pop {
  position: absolute;
  left: 50%;
  bottom: calc(100% + 8px);
  transform: translateX(-50%);
  width: min(560px, calc(100vw - 48px));
  max-height: min(360px, 46vh);
  display: flex;
  flex-direction: column;
  padding: 12px 16px;
  border: 1px solid rgba(15, 23, 42, 0.08);
  border-radius: 14px;
  background: #ffffff;
  box-shadow: 0 14px 36px rgba(15, 23, 42, 0.16);
  z-index: 7;
}

.status-dock-pop-close {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 22px;
  height: 22px;
  display: none;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #6b7280;
  font-size: 12px;
  cursor: pointer;
}

.status-dock-pop:hover .status-dock-pop-close {
  display: inline-flex;
}

.status-dock-pop-close:hover {
  background: rgba(15, 23, 42, 0.06);
  color: #d64545;
}

.status-dock-pop-explain {
  flex: 0 0 auto;
  padding-right: 22px;
  font-size: 12px;
  line-height: 1.6;
  color: #6b7280;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.status-dock-pop-steps {
  flex: 1 1 auto;
  min-height: 0;
  margin-top: 2px;
  overflow-y: auto;
}

.status-dock-step {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 5px 0;
  font-size: 14px;
  line-height: 1.55;
  color: #3f3f46;
}

.status-dock-step-icon {
  flex: 0 0 auto;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.status-dock-step.is-completed .status-dock-step-icon {
  color: #16a34a;
}

.status-dock-step.is-in_progress .status-dock-step-icon {
  color: #c2810b;
}

.status-dock-step.is-pending .status-dock-step-icon {
  color: #b6bcc6;
}

.status-dock-step.is-in_progress {
  color: #1f2937;
}

.status-dock-step-text {
  flex: 1 1 auto;
  min-width: 0;
}

.status-dock-fade-enter-active,
.status-dock-fade-leave-active {
  transition: opacity 0.16s ease, transform 0.16s ease;
}

.status-dock-fade-enter-from,
.status-dock-fade-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(4px);
}

@keyframes status-dock-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .status-dock-spinner .fa-circle-notch {
    animation: none;
  }

  .status-dock-fade-enter-active,
  .status-dock-fade-leave-active {
    transition: none;
  }
}
</style>
