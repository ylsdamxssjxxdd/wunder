<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-agent"
  >
    <section class="messenger-settings-card">
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

import {
  ensureUserAgentPreset,
  presetLockedFields
} from '@/views/messenger/settings/userAgentPreset';

const props = defineProps<{ controller: MessengerControllerContext }>();
const lockedFields = computed(() => presetLockedFields.value);

const settingsAgentIdForPanel = props.controller.settingsAgentIdForPanel;
const isSettingsDefaultAgentReadonly = props.controller.isSettingsDefaultAgentReadonly;
const agentSettingsFocusTarget = props.controller.agentSettingsFocusTarget;
const agentSettingsFocusToken = props.controller.agentSettingsFocusToken;
const agentSettingsPanelRef = props.controller.agentSettingsPanelRef;
const handleAgentSettingsSaved = props.controller.handleAgentSettingsSaved;
const handleAgentSettingsFocusConsumed = props.controller.handleAgentSettingsFocusConsumed;
const AgentSettingsPanel = props.controller.AgentSettingsPanel;

onMounted(() => {
  // 只读读取预设授权范围；失败时优雅降级为「全部可编辑」并给出说明。
  void ensureUserAgentPreset();
});
</script>
