<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-account"
  >
    <!-- 账号：编辑资料（昵称 / 头像 / 密码），复用 MessengerSettingsPanel 的资料页分支。 -->
    <MessengerSettingsPanel
      mode="profile"
      :username="currentUsername"
      :user-id="currentUserId"
      :language-label="currentLanguageLabel"
      :send-key="messengerSendKey"
      :theme-palette="themeStore.palette"
      :ui-font-size="uiFontSize"
      :username-saving="usernameSaving"
      :profile-avatar-icon="currentUserAvatarIcon"
      :profile-avatar-color="currentUserAvatarColor"
      :profile-avatar-options="profileAvatarOptions"
      :profile-avatar-colors="profileAvatarColors"
      @toggle-language="toggleLanguage"
      @update:send-key="updateSendKey"
      @update:theme-palette="updateThemePalette"
      @update:ui-font-size="updateUiFontSize"
      @update:username="updateCurrentUsername"
      @update:profile-avatar-icon="updateCurrentUserAvatarIcon"
      @update:profile-avatar-color="updateCurrentUserAvatarColor"
    />

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.agent.runtime.trendTitle') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.agent.runtime.trendMeta') }}</div>
      </div>
      <AgentDailyTrendChart :agent-id="settingsRuntimeAgentIdForApi" :active="isActive" />
    </section>

    <section class="messenger-settings-card messenger-settings-card--danger">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.sessionGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.sessionHint') }}</div>
      </div>
      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-right-from-bracket messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.site.logout') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.general.logoutHint') }}</div>
          </div>
        </div>
        <button
          class="messenger-settings-action danger"
          type="button"
          data-testid="settings-logout"
          :disabled="Boolean(settingsLogoutDisabled?.value)"
          @click="handleLogout"
        >
          {{ t('messenger.site.logout') }}
        </button>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onActivated, onDeactivated, ref } from 'vue';

import AgentDailyTrendChart from '@/components/messenger/AgentDailyTrendChart.vue';
import { useI18n } from '@/i18n';
import { confirmWithFallback } from '@/utils/confirm';
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const isActive = ref(true);
onActivated(() => {
  isActive.value = true;
});
onDeactivated(() => {
  isActive.value = false;
});

const settingsRuntimeAgentIdForApi = props.controller.settingsRuntimeAgentIdForApi;
const currentUsername = props.controller.currentUsername;
const currentUserId = props.controller.currentUserId;
const currentLanguageLabel = props.controller.currentLanguageLabel;
const messengerSendKey = props.controller.messengerSendKey;
const themeStore = props.controller.themeStore;
const uiFontSize = props.controller.uiFontSize;
const usernameSaving = props.controller.usernameSaving;
const currentUserAvatarIcon = props.controller.currentUserAvatarIcon;
const currentUserAvatarColor = props.controller.currentUserAvatarColor;
const profileAvatarOptions = props.controller.profileAvatarOptions;
const profileAvatarColors = props.controller.profileAvatarColors;
const toggleLanguage = props.controller.toggleLanguage;
const updateSendKey = props.controller.updateSendKey;
const updateThemePalette = props.controller.updateThemePalette;
const updateUiFontSize = props.controller.updateUiFontSize;
const updateCurrentUsername = props.controller.updateCurrentUsername;
const updateCurrentUserAvatarIcon = props.controller.updateCurrentUserAvatarIcon;
const updateCurrentUserAvatarColor = props.controller.updateCurrentUserAvatarColor;
const MessengerSettingsPanel = props.controller.MessengerSettingsPanel;
const settingsLogoutDisabled = props.controller.settingsLogoutDisabled;
const handleSettingsLogout = props.controller.handleSettingsLogout;

const handleLogout = async () => {
  const confirmed = await confirmWithFallback(
    t('messenger.settingsPage.general.logoutConfirm'),
    t('common.confirm'),
    {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    }
  );
  if (confirmed) void handleSettingsLogout?.();
};
</script>
