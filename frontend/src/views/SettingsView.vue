<template>
  <div class="settings-view">
    <el-card>
      <h3>{{ t('settings.title') }}</h3>
      <p>{{ t('settings.placeholder') }}</p>
    </el-card>

    <el-card v-if="desktopMode" class="settings-desktop-card">
      <template #header>
        <div class="settings-desktop-title">{{ t('desktop.settings.title') }}</div>
      </template>

      <el-form label-position="top">
        <el-form-item :label="t('desktop.settings.toolCallMode')">
          <el-radio-group v-model="toolCallMode">
            <el-radio-button label="tool_call">tool_call</el-radio-button>
            <el-radio-button label="function_call">function_call</el-radio-button>
          </el-radio-group>
          <p class="settings-desktop-hint">{{ t('desktop.settings.toolCallHint') }}</p>
        </el-form-item>

        <el-form-item :label="t('desktop.settings.tools')">
          <p class="settings-desktop-hint">{{ t('desktop.settings.toolsHint') }}</p>
          <el-button type="primary" @click="openToolsManager">
            {{ t('desktop.settings.openTools') }}
          </el-button>
        </el-form-item>

        <el-form-item :label="t('desktop.settings.system')">
          <p class="settings-desktop-hint">{{ t('desktop.settings.systemHint') }}</p>
          <el-button type="primary" plain @click="openSystemSettings">
            {{ t('desktop.settings.openSystem') }}
          </el-button>
        </el-form-item>

        <el-form-item :label="t('desktop.settings.update')">
          <p class="settings-desktop-hint">{{ t('desktop.settings.updateHint') }}</p>
          <el-button type="primary" plain :disabled="!updateAvailable" @click="checkDesktopUpdate">
            {{ t('desktop.settings.checkUpdate') }}
          </el-button>
        </el-form-item>

        <el-form-item :label="t('desktop.settings.devtools')">
          <p class="settings-desktop-hint">{{ t('desktop.settings.devtoolsHint') }}</p>
          <el-button type="primary" plain :disabled="!devtoolsAvailable" @click="toggleDevTools">
            {{ t('desktop.settings.openDevtools') }}
          </el-button>
        </el-form-item>
      </el-form>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import {
  getDesktopToolCallMode,
  isDesktopModeEnabled,
  setDesktopToolCallMode,
  type DesktopToolCallMode
} from '@/config/desktop';
import { confirmWithFallback } from '@/utils/confirm';

const { t } = useI18n();
const router = useRouter();

const desktopMode = computed(() => isDesktopModeEnabled());
const devtoolsAvailable = computed(() => {
  if (!desktopMode.value || typeof window === 'undefined') {
    return false;
  }
  return Boolean((window as any).wunderDesktop?.toggleDevTools);
});
const updateAvailable = computed(() => {
  if (!desktopMode.value || typeof window === 'undefined') {
    return false;
  }
  return Boolean((window as any).wunderDesktop?.checkForUpdates);
});
const toolCallMode = ref<DesktopToolCallMode>(getDesktopToolCallMode());

watch(toolCallMode, (value) => {
  if (!desktopMode.value) {
    return;
  }
  setDesktopToolCallMode(value);
});

const openToolsManager = () => {
  if (!desktopMode.value) {
    return;
  }
  router.push('/desktop/tools');
};

const openSystemSettings = () => {
  if (!desktopMode.value) {
    return;
  }
  router.push('/desktop/system');
};

const toggleDevTools = async () => {
  if (!devtoolsAvailable.value) {
    return;
  }
  const api = (window as any).wunderDesktop;
  await api?.toggleDevTools?.();
};

const checkDesktopUpdate = async () => {
  if (!updateAvailable.value || typeof window === 'undefined') {
    ElMessage.warning(t('desktop.settings.updateUnsupported'));
    return;
  }

  const api = (window as any).wunderDesktop;
  const checkingMessage = ElMessage({
    type: 'info',
    message: t('desktop.settings.checkingUpdate'),
    duration: 0,
    showClose: true
  });

  try {
    const state = await api?.checkForUpdates?.();
    checkingMessage.close();

    const phase = String(state?.phase || '').trim().toLowerCase();
    const latestVersion = String(state?.latestVersion || state?.currentVersion || '-');

    if (phase === 'not-available' || phase === 'idle') {
      ElMessage.success(t('desktop.settings.updateNotAvailable'));
      return;
    }
    if (phase === 'unsupported') {
      ElMessage.warning(t('desktop.settings.updateUnsupported'));
      return;
    }
    if (phase === 'error') {
      const reason = String(state?.message || '').trim() || t('common.unknown');
      ElMessage.error(t('desktop.settings.updateCheckFailed', { reason }));
      return;
    }
    if (phase === 'downloading' || phase === 'available' || phase === 'checking') {
      ElMessage.info(t('desktop.settings.updateDownloading'));
      return;
    }
    if (phase !== 'downloaded') {
      ElMessage.info(t('desktop.settings.updateUnknownState'));
      return;
    }

    const confirmed = await confirmWithFallback(
      t('desktop.settings.updateReadyConfirm', { version: latestVersion }),
      t('desktop.settings.update'),
      {
        type: 'warning',
        confirmButtonText: t('desktop.settings.installNow'),
        cancelButtonText: t('common.cancel')
      }
    );
    if (!confirmed) {
      ElMessage.info(t('desktop.settings.updateReadyLater'));
      return;
    }

    const installResult = await api?.installUpdate?.();
    const installOk = typeof installResult === 'boolean' ? installResult : Boolean(installResult?.ok);
    if (!installOk) {
      ElMessage.warning(t('desktop.settings.updateInstallFailed'));
      return;
    }
    ElMessage.success(t('desktop.settings.updateInstalling'));
  } catch (error) {
    checkingMessage.close();
    const reason = String((error as { message?: unknown })?.message || '').trim() || t('common.unknown');
    ElMessage.error(t('desktop.settings.updateCheckFailed', { reason }));
  }
};
</script>

<style scoped>
.settings-view {
  display: grid;
  gap: 16px;
}

.settings-desktop-title {
  font-size: 15px;
  font-weight: 700;
}

.settings-desktop-hint {
  margin: 10px 0 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--portal-muted);
}

:root[data-user-theme='light'] .settings-desktop-hint {
  color: #64748b;
}
</style>
