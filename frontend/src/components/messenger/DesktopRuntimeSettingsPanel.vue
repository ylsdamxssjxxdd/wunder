<template>
  <section v-if="desktopLocalMode" class="messenger-settings-card desktop-runtime-settings-panel">
    <div class="messenger-settings-head">
      <div>
        <div class="messenger-settings-title">{{ t('desktop.system.runtimeTitle') }}</div>
        <div class="messenger-settings-subtitle">{{ t('desktop.system.runtimeHint') }}</div>
      </div>
    </div>

    <div v-if="launchAtLoginSupported" class="messenger-settings-row">
      <div>
        <div class="messenger-settings-label">{{ t('desktop.system.startAtLogin') }}</div>
        <div class="messenger-settings-hint">{{ t('desktop.system.startAtLoginHint') }}</div>
      </div>
      <select
        v-model="launchAtLoginValue"
        class="messenger-settings-select"
        :disabled="launchAtLoginLoading"
        @change="handleLaunchAtLoginChange"
      >
        <option value="off">{{ t('common.disable') }}</option>
        <option value="on">{{ t('common.enable') }}</option>
      </select>
    </div>

    <div v-if="windowCloseBehaviorSupported" class="messenger-settings-row">
      <div>
        <div class="messenger-settings-label">{{ t('messenger.settings.windowCloseBehavior') }}</div>
        <div class="messenger-settings-hint">
          {{ t('messenger.settings.windowCloseBehaviorHint') }}
        </div>
      </div>
      <select
        v-model="windowCloseBehavior"
        class="messenger-settings-select"
        :disabled="windowCloseBehaviorLoading"
        @change="handleWindowCloseBehaviorChange"
      >
        <option value="tray">{{ t('messenger.settings.windowCloseBehaviorHide') }}</option>
        <option value="quit">{{ t('messenger.settings.windowCloseBehaviorQuit') }}</option>
      </select>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';

type WindowCloseBehavior = 'tray' | 'quit';
type DesktopLaunchAtLoginState = {
  supported: boolean;
  enabled: boolean;
};
type DesktopRuntimeBridge = {
  getWindowCloseBehavior?: () => Promise<string | null> | string | null;
  setWindowCloseBehavior?: (behavior: string) => Promise<string | null> | string | null;
  getLaunchAtLogin?: () => Promise<unknown> | unknown;
  setLaunchAtLogin?: (enabled: boolean) => Promise<unknown> | unknown;
};

withDefaults(
  defineProps<{
    desktopLocalMode?: boolean;
  }>(),
  {
    desktopLocalMode: true
  }
);

const { t } = useI18n();

const windowCloseBehavior = ref<WindowCloseBehavior>('tray');
const windowCloseBehaviorLoading = ref(false);
const launchAtLoginEnabled = ref(false);
const launchAtLoginLoading = ref(false);
const launchAtLoginSupported = ref(false);
let disposed = false;

const launchAtLoginValue = computed({
  get: () => (launchAtLoginEnabled.value ? 'on' : 'off'),
  set: (value: string) => {
    launchAtLoginEnabled.value = value === 'on';
  }
});

const windowCloseBehaviorSupported = computed(() => {
  const bridge = getDesktopRuntimeBridge();
  return Boolean(
    bridge &&
      typeof bridge.getWindowCloseBehavior === 'function' &&
      typeof bridge.setWindowCloseBehavior === 'function'
  );
});

function normalizeWindowCloseBehavior(value: unknown): WindowCloseBehavior {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'quit') return 'quit';
  return 'tray';
}

function normalizeLaunchAtLoginState(value: unknown): DesktopLaunchAtLoginState {
  if (typeof value === 'boolean') {
    return {
      supported: true,
      enabled: value
    };
  }
  if (value && typeof value === 'object') {
    const source = value as Record<string, unknown>;
    return {
      supported: source.supported !== false,
      enabled: source.enabled === true
    };
  }
  return {
    supported: false,
    enabled: false
  };
}

function getDesktopRuntimeBridge(): DesktopRuntimeBridge | null {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopRuntimeBridge }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
}

async function loadLaunchAtLoginState() {
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.getLaunchAtLogin !== 'function') {
    launchAtLoginSupported.value = false;
    return;
  }
  launchAtLoginLoading.value = true;
  try {
    const state = normalizeLaunchAtLoginState(await bridge.getLaunchAtLogin());
    if (disposed) return;
    launchAtLoginSupported.value = state.supported;
    launchAtLoginEnabled.value = state.enabled;
  } catch (error) {
    if (disposed) return;
    console.error(error);
    launchAtLoginSupported.value = false;
    ElMessage.error(t('desktop.system.startAtLoginLoadFailed'));
  } finally {
    if (!disposed) {
      launchAtLoginLoading.value = false;
    }
  }
}

async function handleLaunchAtLoginChange() {
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.setLaunchAtLogin !== 'function' || launchAtLoginLoading.value) {
    return;
  }
  launchAtLoginLoading.value = true;
  try {
    const state = normalizeLaunchAtLoginState(await bridge.setLaunchAtLogin(launchAtLoginEnabled.value));
    if (disposed) return;
    launchAtLoginSupported.value = state.supported;
    launchAtLoginEnabled.value = state.enabled;
  } catch (error) {
    if (disposed) return;
    console.error(error);
    ElMessage.error(t('desktop.system.startAtLoginSaveFailed'));
    await loadLaunchAtLoginState();
  } finally {
    if (!disposed) {
      launchAtLoginLoading.value = false;
    }
  }
}

async function loadWindowCloseBehavior() {
  if (!windowCloseBehaviorSupported.value) {
    return;
  }
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.getWindowCloseBehavior !== 'function') {
    return;
  }
  windowCloseBehaviorLoading.value = true;
  try {
    const rawBehavior = await bridge.getWindowCloseBehavior();
    if (disposed) return;
    const normalized = normalizeWindowCloseBehavior(rawBehavior);
    windowCloseBehavior.value = normalized;
    const source = String(rawBehavior || '')
      .trim()
      .toLowerCase();
    if ((source === 'ask' || source === 'hide') && typeof bridge.setWindowCloseBehavior === 'function') {
      await bridge.setWindowCloseBehavior(normalized);
    }
  } catch (error) {
    if (disposed) return;
    console.error(error);
    windowCloseBehavior.value = 'tray';
  } finally {
    if (!disposed) {
      windowCloseBehaviorLoading.value = false;
    }
  }
}

async function handleWindowCloseBehaviorChange() {
  if (!windowCloseBehaviorSupported.value || windowCloseBehaviorLoading.value) {
    return;
  }
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.setWindowCloseBehavior !== 'function') {
    return;
  }
  const target = normalizeWindowCloseBehavior(windowCloseBehavior.value);
  windowCloseBehaviorLoading.value = true;
  try {
    const next = await bridge.setWindowCloseBehavior(target);
    if (disposed) return;
    windowCloseBehavior.value = normalizeWindowCloseBehavior(next || target);
  } catch (error) {
    if (disposed) return;
    console.error(error);
    await loadWindowCloseBehavior();
  } finally {
    if (!disposed) {
      windowCloseBehaviorLoading.value = false;
    }
  }
}

onMounted(() => {
  void loadLaunchAtLoginState();
  void loadWindowCloseBehavior();
});

onBeforeUnmount(() => {
  disposed = true;
});
</script>

<style scoped>
.desktop-runtime-settings-panel {
  min-height: 0;
}

.desktop-runtime-settings-row--block {
  align-items: flex-start;
}

.desktop-runtime-settings-label-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  flex: 1 1 220px;
}

.desktop-runtime-settings-editor {
  display: flex;
  flex: 1 1 520px;
  min-width: 0;
  flex-direction: column;
  gap: 10px;
}

.desktop-runtime-settings-input-row {
  display: flex;
  min-width: 0;
}

.desktop-runtime-settings-input-row :deep(.el-input) {
  flex: 1 1 auto;
  min-width: 0;
}

.desktop-runtime-settings-action-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.desktop-runtime-settings-candidates {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border: 1px dashed #d8dee8;
  border-radius: 12px;
  background: #f8fafc;
}

.desktop-runtime-settings-candidates-title {
  font-size: 12px;
  font-weight: 700;
  color: #4b5563;
}

.desktop-runtime-settings-candidate {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.desktop-runtime-settings-candidate-main {
  display: flex;
  min-width: 0;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 4px;
}

.desktop-runtime-settings-candidate-path {
  color: #1f2937;
  font-size: 13px;
  word-break: break-all;
}

.desktop-runtime-settings-candidate-meta {
  color: #6b7280;
  font-size: 12px;
}

.desktop-runtime-settings-path-picker {
  display: flex;
  min-height: 320px;
  flex-direction: column;
  gap: 10px;
}

.desktop-runtime-settings-path-picker-toolbar {
  display: flex;
  justify-content: flex-start;
}

.desktop-runtime-settings-path-picker-current {
  font-size: 12px;
  color: #6b7280;
  word-break: break-all;
}

.desktop-runtime-settings-path-picker-roots {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.desktop-runtime-settings-path-picker-root {
  border: 1px solid #d7d7d7;
  border-radius: 999px;
  background: #f8f8f8;
  color: #4f4f4f;
  padding: 6px 12px;
  cursor: pointer;
}

.desktop-runtime-settings-path-picker-list {
  display: flex;
  min-height: 220px;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
}

.desktop-runtime-settings-path-picker-item {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid #e5e7eb;
  border-radius: 10px;
  background: #ffffff;
  color: #374151;
  padding: 10px 12px;
  text-align: left;
  cursor: pointer;
}

.desktop-runtime-settings-path-picker-empty {
  color: #8a8a8a;
  font-size: 12px;
  padding: 12px 4px;
}

.desktop-runtime-settings-panel :deep(.el-input__wrapper) {
  background: #f8f8f8;
  border: 1px solid #d9d9d9;
  border-radius: 10px;
  box-shadow: none;
  min-height: 38px;
}

.desktop-runtime-settings-panel :deep(.el-input__wrapper.is-focus) {
  border-color: rgba(var(--ui-accent-rgb), 0.52);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.12);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-candidates) {
  border-color: var(--tech-blue-border);
  background: rgba(9, 17, 30, 0.88);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-candidates-title),
:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-candidate-path) {
  color: var(--tech-blue-text);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-candidate-meta),
:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-path-picker-current),
:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-path-picker-empty) {
  color: var(--tech-blue-muted);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-path-picker-root),
:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-path-picker-item) {
  border-color: var(--tech-blue-border);
  background: var(--tech-blue-surface-3);
  color: var(--tech-blue-text);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-panel .el-input__wrapper) {
  border-color: var(--tech-blue-border);
  background: var(--tech-blue-surface-3);
  color: var(--tech-blue-text);
  box-shadow: none;
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-panel .el-input__wrapper.is-focus) {
  border-color: var(--tech-blue-border-strong);
  box-shadow:
    0 0 0 2px rgba(var(--ui-accent-rgb), 0.18),
    inset 0 0 0 1px rgba(var(--ui-accent-rgb), 0.2);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .desktop-runtime-settings-panel .el-input__inner) {
  color: var(--tech-blue-text);
}
</style>
