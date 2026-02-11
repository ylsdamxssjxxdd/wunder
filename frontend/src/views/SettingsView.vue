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
      </el-form>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRouter } from 'vue-router';

import { useI18n } from '@/i18n';
import {
  getDesktopToolCallMode,
  isDesktopModeEnabled,
  setDesktopToolCallMode,
  type DesktopToolCallMode
} from '@/config/desktop';

const { t } = useI18n();
const router = useRouter();

const desktopMode = computed(() => isDesktopModeEnabled());
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
  color: var(--dark-muted);
}

:root[data-user-theme='light'] .settings-desktop-hint {
  color: #64748b;
}
</style>
