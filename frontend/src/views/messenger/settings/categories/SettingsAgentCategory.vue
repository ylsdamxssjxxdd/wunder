<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-agent"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-page-row-actions">
        <button class="messenger-settings-action ghost" type="button" @click="openAgentChat">
          <i class="fa-solid fa-comments" aria-hidden="true"></i>
          <span>{{ t('messenger.agent.openChat') }}</span>
        </button>
        <button class="messenger-settings-action ghost" type="button" @click="triggerAgentSettingsReload">
          <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
          <span>{{ t('common.refresh') }}</span>
        </button>
        <button
          v-if="!isSettingsDefaultAgentReadonly"
          class="messenger-settings-action"
          type="button"
          data-testid="settings-agent-save"
          @click="triggerAgentSettingsSave"
        >
          <i class="fa-solid fa-floppy-disk" aria-hidden="true"></i>
          <span>{{ t('portal.agent.save') }}</span>
        </button>
      </div>

      <div class="messenger-chat-settings-block">
        <AgentSettingsPanel
          ref="agentSettingsPanelRef"
          :agent-id="settingsAgentIdForPanel"
          :readonly="isSettingsDefaultAgentReadonly"
          :focus-target="agentSettingsFocusTarget"
          :focus-token="agentSettingsFocusToken"
          :preset-locked-fields="lockedFields"
          @saved="handleAgentSettingsSaved"
          @focus-consumed="handleAgentSettingsFocusConsumed"
        />
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';

import {
  ensureUserAgentPreset,
  presetLockedFields
} from '@/views/messenger/settings/userAgentPreset';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ 'open-agent-chat': [] }>();
const { t } = useI18n();

const lockedFields = computed(() => presetLockedFields.value);

const settingsAgentIdForPanel = props.controller.settingsAgentIdForPanel;
const isSettingsDefaultAgentReadonly = props.controller.isSettingsDefaultAgentReadonly;
const agentSettingsFocusTarget = props.controller.agentSettingsFocusTarget;
const agentSettingsFocusToken = props.controller.agentSettingsFocusToken;
const agentSettingsPanelRef = props.controller.agentSettingsPanelRef;
const triggerAgentSettingsReload = props.controller.triggerAgentSettingsReload;
const triggerAgentSettingsSave = props.controller.triggerAgentSettingsSave;
const handleAgentSettingsSaved = props.controller.handleAgentSettingsSaved;
const handleAgentSettingsFocusConsumed = props.controller.handleAgentSettingsFocusConsumed;
const AgentSettingsPanel = props.controller.AgentSettingsPanel;

const openAgentChat = () => {
  emit('open-agent-chat');
};

onMounted(() => {
  // 只读读取预设授权范围；失败时优雅降级为「全部可编辑」并给出说明。
  void ensureUserAgentPreset();
});
</script>
