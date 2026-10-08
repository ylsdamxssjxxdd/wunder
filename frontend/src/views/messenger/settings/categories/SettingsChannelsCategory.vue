<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-channels"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('chat.features.channels') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.channels.hint') }}</div>
      </div>
      <div class="messenger-chat-settings-block messenger-channel-panel-wrap">
        <UserChannelSettingsPanel
          mode="page"
          :agent-id="settingsAgentIdForApi"
          :active="isActive"
          @changed="handleChannelChanged"
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

const isActive = ref(true);
onActivated(() => {
  isActive.value = true;
});
onDeactivated(() => {
  isActive.value = false;
});

const settingsAgentIdForApi = props.controller.settingsAgentIdForApi;
const loadChannelBoundAgentIds = props.controller.loadChannelBoundAgentIds;
const UserChannelSettingsPanel = props.controller.UserChannelSettingsPanel;

const handleChannelChanged = () => {
  void loadChannelBoundAgentIds({ force: true });
};
</script>
