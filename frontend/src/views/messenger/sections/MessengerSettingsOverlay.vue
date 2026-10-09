<template>
  <div class="messenger-settings-overlay">
    <!--
      对齐桌面端：设置以浮动窗口呼出（标题栏 + 关闭按钮），不再整页覆盖。
      窗口内部沿用「左导航 + 右卡片内容」的设置页布局。
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

// Esc 关闭窗口：监听挂在 window 上，焦点在输入框内时同样生效。
const handleWindowKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
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
