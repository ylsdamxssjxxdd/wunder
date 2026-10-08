<template>
  <div v-if="visible" class="status-dock" :class="{ 'has-pop': planOpen }" role="status" @mouseleave="planOpen = false">
    <div
      class="status-dock-pill"
      :class="{ 'has-plan': hasPlan }"
      @mouseenter="planOpen = hasPlan"
    >
      <span v-if="status.active" class="status-dock-spinner" aria-hidden="true">
        <i class="fa-solid fa-circle-notch"></i>
      </span>
      <span v-if="status.active" class="status-dock-label">{{ statusLabel }}</span>
      <template v-if="status.active && (planProgress.total > 0 || fileStats.files > 0)">
        <span class="status-dock-dot" aria-hidden="true">·</span>
      </template>
      <span v-if="planProgress.total > 0" class="status-dock-steps">
        {{ t('chat.activity.steps', { done: planProgress.done, total: planProgress.total }) }}
      </span>
      <template v-if="planProgress.total > 0 && fileStats.files > 0">
        <span class="status-dock-dot" aria-hidden="true">·</span>
      </template>
      <span v-if="fileStats.files > 0" class="status-dock-files">
        {{ t('chat.activity.filesChanged', { count: fileStats.files }) }}
        <span class="status-dock-add">+{{ fileStats.addedLines }}</span>
        <span class="status-dock-del">-{{ fileStats.deletedLines }}</span>
      </span>
    </div>
    <transition name="status-dock-fade">
      <div
        v-if="planOpen && planProgress.total > 0"
        class="status-dock-pop"
        @mouseenter="planOpen = true"
      >
        <div class="status-dock-pop-head">
          <span class="status-dock-pop-title">{{ t('chat.workflow.plan.title') }}</span>
          <button
            class="status-dock-pop-close"
            type="button"
            :title="t('chat.workflow.plan.remove')"
            :aria-label="t('chat.workflow.plan.remove')"
            @click="handleDismiss"
          >
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
        <div v-if="planExplanation" class="status-dock-pop-explain">{{ planExplanation }}</div>
        <div class="status-dock-pop-steps">
          <div
            v-for="(step, index) in planSteps"
            :key="`${index}-${step.text}`"
            :class="['status-dock-step', `is-${step.status}`]"
          >
            <span class="status-dock-step-icon" aria-hidden="true">
              <i v-if="step.status === 'completed'" class="fa-solid fa-circle-check"></i>
              <i v-else-if="step.status === 'in_progress'" class="fa-solid fa-circle-notch is-spin"></i>
              <i v-else class="fa-regular fa-circle"></i>
            </span>
            <span class="status-dock-step-text" :title="step.text">{{ step.text }}</span>
            <span class="status-dock-step-status">{{ formatPlanStatus(step.status) }}</span>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';
import {
  deriveActivityStatus,
  summarizeFileChanges,
  summarizePlanProgress
} from '@/components/chat/composerActivity';

/**
 * 输入区上方悬浮状态条（参考外部产品的输入区指示行）：
 * 运行状态 + 计划步骤进度 + 文件变更统计合并进一条居中胶囊，计划详情
 * 悬停胶囊呼出。目标条（MessageGoalBar）仍在其下，两者不合并。
 */
const props = withDefaults(defineProps<{
  loading?: boolean;
  messages?: unknown[];
  plan?: unknown;
}>(), {
  loading: false,
  messages: () => [],
  plan: null
});

const emit = defineEmits<{
  (event: 'remove'): void;
}>();

const { t } = useI18n();
const planOpen = ref(false);

const status = computed(() => deriveActivityStatus(props.messages, props.loading));
const fileStats = computed(() => summarizeFileChanges(props.messages));
const planProgress = computed(() => summarizePlanProgress(props.plan));

const statusLabel = computed(() =>
  t(status.value.labelKey, status.value.labelParams ?? {})
);

const visible = computed(() => status.value.active || planProgress.value.total > 0);

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

const formatPlanStatus = (value: string) => {
  if (value === 'completed') return t('chat.workflow.plan.status.completed');
  if (value === 'in_progress') return t('chat.workflow.plan.status.inProgress');
  return t('chat.workflow.plan.status.pending');
};

const handleDismiss = () => {
  planOpen.value = false;
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

/* 悬停桥：弹卡与胶囊之间留有 8px 视觉间隙，桥面让指针穿越时仍算停留在
   根元素内，避免 mouseleave 在抵达弹卡前把弹卡关掉。 */
.status-dock.has-pop::before {
  content: '';
  position: absolute;
  left: 50%;
  bottom: 100%;
  transform: translateX(-50%);
  width: min(420px, calc(100vw - 48px));
  height: 12px;
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

.status-dock-pill.has-plan {
  cursor: default;
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

.status-dock-label {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.status-dock-dot {
  flex: 0 0 auto;
  color: #9ca3af;
}

.status-dock-steps,
.status-dock-files {
  flex: 0 0 auto;
  font-variant-numeric: tabular-nums;
}

/* 参考图配色：增行红、删行绿，读作补丁摘要而不是普通计数。 */
.status-dock-add {
  margin-left: 6px;
  color: #d64545;
  font-weight: 600;
}

.status-dock-del {
  margin-left: 4px;
  color: #2f9e44;
  font-weight: 600;
}

.status-dock-pop {
  position: absolute;
  left: 50%;
  bottom: calc(100% + 8px);
  transform: translateX(-50%);
  width: min(420px, calc(100vw - 48px));
  max-height: min(380px, 52vh);
  display: flex;
  flex-direction: column;
  padding: 10px 12px;
  border: 1px solid rgba(15, 23, 42, 0.1);
  border-radius: 12px;
  background: #ffffff;
  box-shadow: 0 12px 32px rgba(15, 23, 42, 0.16);
  z-index: 7;
}

.status-dock-pop-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 0 0 auto;
}

.status-dock-pop-title {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: #1f2937;
}

.status-dock-pop-close {
  flex: 0 0 auto;
  width: 24px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: #6b7280;
  font-size: 13px;
  cursor: pointer;
}

.status-dock-pop-close:hover {
  background: rgba(15, 23, 42, 0.06);
  color: #d64545;
}

.status-dock-pop-explain {
  flex: 0 0 auto;
  margin-top: 6px;
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
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid rgba(15, 23, 42, 0.08);
  overflow-y: auto;
}

.status-dock-step {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 6px;
  border-radius: 8px;
  font-size: 12px;
  color: #374151;
}

.status-dock-step:hover {
  background: rgba(15, 23, 42, 0.04);
}

.status-dock-step-icon {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 14px;
  font-size: 13px;
}

.status-dock-step.is-completed .status-dock-step-icon {
  color: #2f9e44;
}

.status-dock-step.is-in_progress .status-dock-step-icon {
  color: #4f7cf0;
}

.status-dock-step.is-pending .status-dock-step-icon {
  color: #9ca3af;
}

.status-dock-step-icon .is-spin {
  animation: status-dock-spin 1.1s linear infinite;
}

.status-dock-step-text {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-dock-step-status {
  flex: 0 0 auto;
  color: #9ca3af;
  font-size: 11px;
}

.status-dock-step.is-in_progress .status-dock-step-status {
  color: #4f7cf0;
  font-weight: 600;
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
  .status-dock-spinner .fa-circle-notch,
  .status-dock-step-icon .is-spin {
    animation: none;
  }

  .status-dock-fade-enter-active,
  .status-dock-fade-leave-active {
    transition: none;
  }
}
</style>
