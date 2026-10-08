<template>
  <div class="composer-status-bar">
    <span class="composer-status-item" data-testid="composer-workspace-name" :title="displayName">
      <i class="fa-solid fa-folder-open composer-status-icon" aria-hidden="true"></i>
      <span class="composer-status-text">{{ displayName }}</span>
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { workspaceDisplayNameOverride } from '@/views/messenger/workspaceDisplayName';
import { useI18n } from '@/i18n';

/**
 * 输入卡工具栏左组的第一项：**只**保留「这次消息发到哪个工作目录」这一条本地上下文。
 *
 * 对齐桌面 composer.slint:517-525：桌面把工作目录名从框下状态行挪进工具栏，
 * 这里跟随同一位置（不再单独占一行）；B6 合并结论不变：
 * 云端在线/就绪与上下文占用属于**状态**，统一由壳体底部的
 * `MessengerStatusBar` 显示（带标签的占用只有那一处）。
 */
const props = defineProps({
  workspaceName: {
    type: String,
    default: ''
  }
});

const { t } = useI18n();

const displayName = computed(
  () =>
    String(props.workspaceName || '').trim() ||
    String(workspaceDisplayNameOverride.value || '').trim() ||
    t('messenger.workspace.defaultName')
);
</script>

<style scoped>
/* 工具栏左组的第一项（非按钮）：与 30px 高的按钮行居中对齐、可省略。 */
.composer-status-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  box-sizing: border-box;
  flex: 0 1 auto;
  min-width: 0;
  max-width: 200px;
  height: 30px;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 12px;
  line-height: 1.3;
}

.composer-status-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
}

.composer-status-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-status-icon {
  flex-shrink: 0;
  color: var(--mz-primary, #c96443);
  font-size: 12px;
}

/* 窄输入卡先收工作目录名，保证右组（模型/占用/发送）不被挤出。 */
@container (max-width: 560px) {
  .composer-status-bar {
    max-width: 128px;
  }
}

@container (max-width: 420px) {
  .composer-status-bar {
    max-width: 88px;
  }
}
</style>
