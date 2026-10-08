<template>
  <div class="messenger-status-bar">
    <span class="messenger-status-item">
      <span class="messenger-status-dot" :class="{ 'is-offline': !online }" aria-hidden="true"></span>
      {{ online ? t('messenger.status.online') : t('messenger.status.offline') }}
    </span>
    <span class="messenger-status-sep" aria-hidden="true">·</span>
    <span
      class="messenger-status-item messenger-status-usage"
      data-testid="messenger-status-usage"
      :title="usageTitle"
    >
      <ContextUsageIcon :ratio="usage.ratio" />
      <span class="messenger-status-usage-track" aria-hidden="true">
        <span
          class="messenger-status-usage-fill"
          :class="usage.level"
          :style="{ width: usageFillWidth }"
        ></span>
      </span>
      <span class="messenger-status-usage-text">{{ usage.percentText }}</span>
    </span>
    <span v-if="usage.overflow" class="messenger-status-overflow">
      {{ t('chat.composer.status.contextOverflow') }}
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import ContextUsageIcon from '@/components/chat/ContextUsageIcon.vue';
import { useSessionContextUsage } from '@/views/messenger/sessionContextUsage';

/**
 * 壳体底部状态栏（B6 合并后的**唯一**状态面）：云端在线/就绪 + 上下文占用。
 *
 * 合并前这里显示「工作区 · 在线 · 上下文 12k / 128k」，输入卡内又有一份
 * 「工作目录 · 在线 · 进度条 + 百分比」，同一屏两处重复。现在：
 * 状态（在线、占用图标/进度条/百分比/溢出提示）只在这里，
 * 工作目录名与模型/审批/历史等操作入口留在输入卡与左栏。
 */
const props = defineProps<{ controller: MessengerControllerContext }>();
const t = props.controller.t;

const browsingOnline = ref(typeof navigator === 'undefined' ? true : navigator.onLine !== false);
const bootLoading = props.controller.bootLoading;
const online = computed(() => browsingOnline.value && !Boolean(bootLoading?.value));
const markOnline = () => {
  browsingOnline.value = true;
};
const markOffline = () => {
  browsingOnline.value = false;
};
onMounted(() => {
  window.addEventListener('online', markOnline);
  window.addEventListener('offline', markOffline);
});
onBeforeUnmount(() => {
  window.removeEventListener('online', markOnline);
  window.removeEventListener('offline', markOffline);
});

const usage = useSessionContextUsage({
  scope: () => `${String(props.controller.activeSessionId?.value || '')}:${String(modelName())}`,
  messages: () => props.controller.agentRenderableContextMessages?.value || [],
  session: () => props.controller.activeSessionRecord?.value || null,
  loading: () => Boolean(props.controller.agentSessionLoading?.value),
  modelName
});

function modelName(): string {
  return String(props.controller.agentHeaderModelDisplayName?.value || '');
}

const usageFillWidth = computed(() => {
  const ratio = Number(usage.value.ratio);
  if (!Number.isFinite(ratio) || ratio < 0) return '0%';
  return `${Math.min(100, Math.round(ratio * 100))}%`;
});

const usageTitle = computed(() => {
  const counts = String(usage.value.counts || '').trim();
  return counts ? `${t('profile.stats.contextTokens')} ${counts}` : t('profile.stats.contextTokens');
});
</script>

<style scoped>
.messenger-status-usage {
  gap: 6px;
}

.messenger-status-usage-track {
  display: inline-block;
  width: 72px;
  height: 4px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--mz-border, #e8e6e3);
}

.messenger-status-usage-fill {
  display: block;
  height: 100%;
  border-radius: 999px;
  background: var(--mz-text-muted, #8a8f99);
  transition: width 220ms ease;
}

.messenger-status-usage-fill.is-warning {
  background: var(--mz-warning, #c98a2b);
}

.messenger-status-usage-fill.is-danger {
  background: var(--mz-danger, #d04a43);
}

.messenger-status-overflow {
  color: var(--mz-danger, #d04a43);
}

@media (prefers-reduced-motion: reduce) {
  .messenger-status-usage-fill {
    transition: none;
  }
}

@media (max-width: 560px) {
  .messenger-status-usage-track {
    width: 44px;
  }
}
</style>
