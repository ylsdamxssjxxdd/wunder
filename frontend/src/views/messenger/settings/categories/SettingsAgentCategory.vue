<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-agent"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head messenger-settings-group-head--row">
        <div>
          <div class="messenger-settings-title">{{ t('messenger.settingsPage.agent.presetGroup') }}</div>
          <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.agent.presetHint') }}</div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <button class="messenger-settings-action ghost" type="button" @click="openAgentChat">
            <i class="fa-solid fa-comments" aria-hidden="true"></i>
            <span>{{ t('messenger.agent.openChat') }}</span>
          </button>
        </div>
      </div>

      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-shield-halved messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">
              {{ presetState.available ? presetState.presetName || t('messenger.settingsPage.agent.presetUnnamed') : t('messenger.settingsPage.agent.presetUnknown') }}
            </div>
            <div class="messenger-settings-hint">
              {{ presetState.available ? t('messenger.settingsPage.agent.presetBound') : t('messenger.settingsPage.agent.presetUnavailable') }}
            </div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span v-if="presetState.loading" class="messenger-settings-hint">{{ t('common.loading') }}</span>
          <span
            v-else-if="presetState.available && lockedFields.length"
            class="messenger-settings-page-lock"
          >
            <i class="fa-solid fa-lock" aria-hidden="true"></i>
            {{ t('messenger.settingsPage.lockedFields', { count: lockedFields.length }) }}
          </span>
          <span v-else-if="presetState.available" class="messenger-settings-page-badge">
            {{ t('messenger.settingsPage.agent.presetAllCustomizable') }}
          </span>
          <span v-else class="messenger-settings-page-badge is-muted">
            {{ t('messenger.settingsPage.agent.presetDegraded') }}
          </span>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head messenger-settings-group-head--row">
        <div>
          <div class="messenger-settings-title">{{ t('chat.features.agentSettings') }}</div>
          <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.agent.formHint') }}</div>
        </div>
        <div class="messenger-settings-page-row-actions">
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
  presetLockedFields,
  userAgentPresetState
} from '@/views/messenger/settings/userAgentPreset';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{ 'open-agent-chat': [] }>();
const { t } = useI18n();

const presetState = computed(() => userAgentPresetState.value);
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
