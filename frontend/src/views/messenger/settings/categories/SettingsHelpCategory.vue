<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-help"
  >
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.help.shortcutGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.help.shortcutHint') }}</div>
      </div>
      <div
        v-for="item in shortcuts"
        :key="item.key"
        class="messenger-settings-row"
      >
        <div class="messenger-settings-page-row-main">
          <i :class="item.icon" class="messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ item.label }}</div>
            <div class="messenger-settings-hint">{{ item.hint }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <kbd class="messenger-settings-page-kbd">{{ item.keys }}</kbd>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settingsPage.help.aboutGroup') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settingsPage.help.aboutHint') }}</div>
      </div>
      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-circle-info messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.versionNumber') }}</div>
            <div class="messenger-settings-hint">{{ appVersion }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="messenger-settings-page-badge is-muted">
            {{ t('messenger.settingsPage.help.runtimeWeb') }}
          </span>
        </div>
      </div>
      <div class="messenger-settings-row">
        <div class="messenger-settings-page-row-main">
          <i class="fa-solid fa-rotate messenger-settings-page-row-icon" aria-hidden="true"></i>
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.checkUpdate') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settingsPage.help.updateHint') }}</div>
          </div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="messenger-settings-page-badge is-muted">{{ t('messenger.settingsPage.help.updateManaged') }}</span>
        </div>
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
            data-testid="settings-help-export-diagnostics"
            @click="handleExportDiagnostics"
          >
            {{ t('common.export') }}
          </button>
        </div>
      </div>
    </section>

    <section class="messenger-settings-card messenger-settings-card--manual">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settings.helpManual') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settings.helpManualHint') }}</div>
      </div>
      <div class="messenger-settings-page-manual">
        <MessengerHelpManualPanel @loading-change="handleHelpManualLoadingChange" />
      </div>
    </section>

    <HoneycombWaitingOverlay
      :visible="Boolean(showHelpManualWaitingOverlay?.value)"
      :title="t('messenger.waiting.title')"
      :target-name="t('messenger.settings.helpManual')"
      :phase-label="t('messenger.waiting.phase.loading')"
      :summary-label="t('messenger.waiting.summary.helpManual')"
      :progress="42"
      :teleport-to-body="false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { ElMessage } from 'element-plus';

import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { APP_VERSION } from '@/config/appVersion';
import { useI18n } from '@/i18n';
import { exportClientDiagnostics } from '@/utils/clientDiagnostics';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();

const appVersion = APP_VERSION;

const shortcuts = computed(() => [
  {
    key: 'send',
    icon: 'fa-solid fa-paper-plane',
    label: t('messenger.settingsPage.help.shortcutSend'),
    hint: formatSendKeyHint(),
    keys: formatSendKeyKeys()
  },
  {
    key: 'escape',
    icon: 'fa-solid fa-xmark',
    label: t('messenger.settingsPage.help.shortcutEscape'),
    hint: t('messenger.settingsPage.help.shortcutEscapeHint'),
    keys: 'Esc'
  },
  {
    key: 'settings',
    icon: 'fa-solid fa-gear',
    label: t('messenger.settingsPage.help.shortcutSettings'),
    hint: t('messenger.settingsPage.help.shortcutSettingsHint'),
    keys: t('messenger.settingsPage.help.shortcutSettingsKeys')
  }
]);

function formatSendKeyHint(): string {
  const mode = String(props.controller.messengerSendKey?.value || 'enter');
  if (mode === 'none') return t('messenger.settings.sendKeyNone');
  return t('messenger.settingsPage.help.shortcutSendHint');
}

function formatSendKeyKeys(): string {
  const mode = String(props.controller.messengerSendKey?.value || 'enter');
  if (mode === 'ctrl_enter') return 'Ctrl + Enter';
  if (mode === 'none') return t('messenger.settingsPage.help.shortcutButtonOnly');
  return 'Enter';
}

const handleExportDiagnostics = () => {
  const themeStore = props.controller.themeStore;
  const filename = exportClientDiagnostics({
    language: String(props.controller.currentLanguageLabel?.value || ''),
    themePalette: String(themeStore?.palette || ''),
    uiFontSize: Number(props.controller.uiFontSize?.value || 0),
    sendKey: String(props.controller.messengerSendKey?.value || ''),
    settingsCategory: 'help',
    section: String(props.controller.sessionHub?.activeSection || ''),
    authenticated: Boolean(props.controller.authStore?.isAuthenticated)
  });
  ElMessage.success(t('messenger.settingsPage.exportDiagnosticsDone', { name: filename }));
};

const handleHelpManualLoadingChange = props.controller.handleHelpManualLoadingChange;
const showHelpManualWaitingOverlay = props.controller.showHelpManualWaitingOverlay;
const MessengerHelpManualPanel = props.controller.MessengerHelpManualPanel;
const HoneycombWaitingOverlay = props.controller.HoneycombWaitingOverlay;
</script>
