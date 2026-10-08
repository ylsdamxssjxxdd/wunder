<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-prompts"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.cat.prompts.title') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.prompts.hint') }}</div>
      </div>
      <div class="messenger-chat-settings-block">
        <UserPromptSettingsPanel />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const UserPromptSettingsPanel = props.controller.UserPromptSettingsPanel;
</script>
