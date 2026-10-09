<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-general"
  >
    <!-- 外观 / 行为 / 版本：直接复用既有 MessengerSettingsPanel（不重写业务逻辑）。
         账号资料编辑已拆到独立的「账号」分类（SettingsAccountCategory）。 -->
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.appearanceGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.appearanceHint') }}</div>
      </div>
      <MessengerSettingsPanel
        mode="general"
        :username="currentUsername"
        :user-id="currentUserId"
        :language-label="currentLanguageLabel"
        :send-key="messengerSendKey"
        :theme-palette="themeStore.palette"
        :ui-font-size="uiFontSize"
        :username-saving="usernameSaving"
        :devtools-available="debugToolsAvailable"
        :profile-avatar-icon="currentUserAvatarIcon"
        :profile-avatar-color="currentUserAvatarColor"
        :profile-avatar-options="profileAvatarOptions"
        :profile-avatar-colors="profileAvatarColors"
        @toggle-language="toggleLanguage"
        @toggle-devtools="openDebugTools"
        @update:send-key="updateSendKey"
        @update:theme-palette="updateThemePalette"
        @update:ui-font-size="updateUiFontSize"
        @update:username="updateCurrentUsername"
        @update:profile-avatar-icon="updateCurrentUserAvatarIcon"
        @update:profile-avatar-color="updateCurrentUserAvatarColor"
      />
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.general.dataGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.general.dataHint') }}</div>
      </div>
      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-file-arrow-down messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settingsPage.exportDiagnostics') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.exportDiagnosticsHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <button
            class="messenger-settings-action ghost"
            type="button"
            data-testid="settings-export-diagnostics"
            @click="handleExportDiagnostics"
          >
            {{ t('common.export') }}
          </button>
        </div>
      </div>
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
        <div class="messenger-settings-page-row-actions">
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
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { ElMessage } from 'element-plus';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';
import { confirmWithFallback } from '@/utils/confirm';
import { exportClientDiagnostics } from '@/utils/clientDiagnostics';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const currentUsername = props.controller.currentUsername;
const currentUserId = props.controller.currentUserId;
const currentLanguageLabel = props.controller.currentLanguageLabel;
const messengerSendKey = props.controller.messengerSendKey;
const themeStore = props.controller.themeStore;
const uiFontSize = props.controller.uiFontSize;
const usernameSaving = props.controller.usernameSaving;
const debugToolsAvailable = props.controller.debugToolsAvailable;
const currentUserAvatarIcon = props.controller.currentUserAvatarIcon;
const currentUserAvatarColor = props.controller.currentUserAvatarColor;
const profileAvatarOptions = props.controller.profileAvatarOptions;
const profileAvatarColors = props.controller.profileAvatarColors;
const toggleLanguage = props.controller.toggleLanguage;
const openDebugTools = props.controller.openDebugTools;
const updateSendKey = props.controller.updateSendKey;
const updateThemePalette = props.controller.updateThemePalette;
const updateUiFontSize = props.controller.updateUiFontSize;
const updateCurrentUsername = props.controller.updateCurrentUsername;
const updateCurrentUserAvatarIcon = props.controller.updateCurrentUserAvatarIcon;
const updateCurrentUserAvatarColor = props.controller.updateCurrentUserAvatarColor;
const settingsLogoutDisabled = props.controller.settingsLogoutDisabled;
const handleSettingsLogout = props.controller.handleSettingsLogout;
const MessengerSettingsPanel = props.controller.MessengerSettingsPanel;

const handleExportDiagnostics = () => {
  const filename = exportClientDiagnostics({
    language: String(currentLanguageLabel?.value || ''),
    themePalette: String(themeStore?.palette || ''),
    uiFontSize: Number(uiFontSize?.value || 0),
    sendKey: String(messengerSendKey?.value || ''),
    settingsCategory: 'general',
    section: String(props.controller.sessionHub?.activeSection || ''),
    authenticated: Boolean(props.controller.authStore?.isAuthenticated)
  });
  ElMessage.success(t('messenger.settingsPage.exportDiagnosticsDone', { name: filename }));
};

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
  if (!confirmed) return;
  void handleSettingsLogout?.();
};
</script>
