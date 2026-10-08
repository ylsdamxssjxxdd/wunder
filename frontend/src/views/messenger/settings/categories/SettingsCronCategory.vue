<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-cron"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('chat.features.cron') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.cron.hint') }}</div>
      </div>
      <div class="messenger-chat-settings-block">
        <AgentCronPanel
          :agent-id="settingsAgentIdForApi"
          :active="isActive"
          @changed="handleCronPanelChanged"
        />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onActivated, onDeactivated, ref } from 'vue';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

/** 分类离开视口（KeepAlive 停用）时停掉面板轮询。 */
const isActive = ref(true);
onActivated(() => {
  isActive.value = true;
});
onDeactivated(() => {
  isActive.value = false;
});

const settingsAgentIdForApi = props.controller.settingsAgentIdForApi;
const handleCronPanelChanged = props.controller.handleCronPanelChanged;
const AgentCronPanel = props.controller.AgentCronPanel;
</script>
