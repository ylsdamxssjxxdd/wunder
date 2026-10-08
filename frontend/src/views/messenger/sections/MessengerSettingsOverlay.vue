<template>
  <section
    class="messenger-settings-overlay"
    data-testid="messenger-settings"
    role="dialog"
    aria-modal="true"
    :aria-label="t('messenger.sidebar.settings')"
  >
    <!--
      全屏设置页（方案 §九）：左导航（返回应用 / 搜索设置 / 12 分类）+ 右卡片内容。
      分类内容按需挂载，覆盖层本身不随分类切换重建。
    -->
    <MessengerSettingsPage
      :controller="controller"
      @close="emit('close')"
      @open-agent-chat="emit('open-agent-chat')"
    />
  </section>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import MessengerSettingsPage from '@/views/messenger/settings/MessengerSettingsPage.vue';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ close: []; 'open-agent-chat': [] }>();

const t = props.controller.t;
</script>
