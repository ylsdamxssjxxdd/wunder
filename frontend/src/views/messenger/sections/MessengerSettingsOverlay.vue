<template>
  <div class="messenger-settings-overlay" @pointerdown.self="handleOverlayPointerDown">
    <!--
      对齐桌面端：设置以浮动窗口呼出（标题栏 + 关闭按钮），不再整页覆盖。
      窗口内部沿用「左导航 + 右卡片内容」的设置页布局。
      点窗口外的遮罩即关闭，与桌面版一致。
    -->
    <section
      class="messenger-settings-window"
      data-testid="messenger-settings"
      role="dialog"
      aria-modal="true"
      :aria-label="t('messenger.sidebar.settings')"
    >
      <header class="messenger-settings-window-head">
        <i class="fa-solid fa-gear messenger-settings-window-gear" aria-hidden="true"></i>
        <span class="messenger-settings-window-title">{{ t('messenger.sidebar.settings') }}</span>
        <button
          class="messenger-settings-window-close"
          type="button"
          data-testid="settings-close"
          :title="t('common.close')"
          :aria-label="t('common.close')"
          @click="emit('close')"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </header>
      <div class="messenger-settings-window-body">
        <MessengerSettingsPage
          :controller="controller"
          @close="emit('close')"
          @open-agent-chat="emit('open-agent-chat')"
        />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue';

import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import MessengerSettingsPage from '@/views/messenger/settings/MessengerSettingsPage.vue';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ close: []; 'open-agent-chat': [] }>();

const t = props.controller.t;

const hasVisibleModalDialog = () =>
  Array.from(document.querySelectorAll<HTMLElement>('.el-overlay-dialog[aria-modal="true"]'))
    .some((dialog) => dialog.getClientRects().length > 0);

// 点窗口外的遮罩关闭窗口（桌面端同款交互）。用 pointerdown 而不是 click：
// 从窗口内部拖拽选中文本、松手落在遮罩上时不会误关；只响应鼠标左键或触摸。
const handleOverlayPointerDown = (event: PointerEvent) => {
  if (event.button !== 0) return;
  // 二级弹窗（模型选择、确认框等）挂在 body 上，遮罩盖住本窗口时不应该
  // 连带关掉设置窗口；这里再兜一层，避免事件被代理到遮罩上。
  if (hasVisibleModalDialog()) return;
  emit('close');
};

// Esc 关闭窗口：监听挂在 window 上，焦点在输入框内时同样生效。
const handleWindowKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    // Element Plus dialogs are teleported outside this window. Let the top
    // dialog consume Escape before closing the settings window beneath it.
    if (hasVisibleModalDialog()) return;
    event.stopPropagation();
    emit('close');
  }
};

onMounted(() => {
  window.addEventListener('keydown', handleWindowKeydown, true);
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWindowKeydown, true);
});
</script>
