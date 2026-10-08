<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-memory"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.memory.title') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.memory.hint') }}</div>
      </div>
      <div class="messenger-chat-settings-block">
        <AgentMemoryPanel :agent-id="settingsAgentIdForApi" :active="isActive" />
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
const AgentMemoryPanel = props.controller.AgentMemoryPanel;
</script>
