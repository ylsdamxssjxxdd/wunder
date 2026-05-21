<template>
  <section
    v-if="desktopLocalMode"
    class="messenger-settings-card desktop-runtime-preferences-panel"
  >
    <div class="desktop-runtime-preferences-head">
      <div>
        <div class="messenger-settings-title">{{ t('desktop.system.runtimeTitle') }}</div>
        <div class="messenger-settings-subtitle">{{ t('desktop.system.runtimeHint') }}</div>
      </div>
      <el-button
        v-if="supplementImportSupported"
        class="desktop-runtime-preferences-btn desktop-runtime-preferences-import-btn"
        :loading="importingSupplement"
        @click="handleImportSupplementPackage"
      >
        {{ t('desktop.system.pythonSupplementImport') }}
      </el-button>
    </div>

    <div class="desktop-runtime-preferences-block desktop-runtime-preferences-tool-grid">
      <label
        v-for="tool in runtimeToolFields"
        :key="tool.key"
        class="desktop-runtime-preferences-field"
      >
        <span class="desktop-runtime-preferences-field-label">{{ tool.label }}</span>
        <div class="desktop-runtime-preferences-editor">
          <div class="desktop-runtime-preferences-input-row">
            <el-input
              v-model="tool.draft.value"
              clearable
              class="desktop-runtime-preferences-input"
              :placeholder="tool.placeholder.value"
              @blur="() => handleRuntimeToolPathInputCommit(tool.key)"
              @keyup.enter="() => handleRuntimeToolPathInputCommit(tool.key)"
            />
            <div class="desktop-runtime-preferences-inline-actions">
              <el-button
                class="desktop-runtime-preferences-btn"
                :loading="pickingToolKey === tool.key"
                @click="openRuntimeToolPathPicker(tool.key)"
              >
                {{ t('desktop.common.browse') }}
              </el-button>
            </div>
          </div>
        </div>
      </label>
    </div>

    <div v-if="launchAtLoginSupported || windowCloseBehaviorSupported" class="desktop-runtime-preferences-controls">
      <div v-if="launchAtLoginSupported" class="desktop-runtime-preferences-row">
        <div class="desktop-runtime-preferences-row-main">
          <div class="desktop-runtime-preferences-row-title">{{ t('desktop.system.startAtLogin') }}</div>
          <div class="desktop-runtime-preferences-row-hint">{{ t('desktop.system.startAtLoginHint') }}</div>
        </div>
        <div class="desktop-runtime-preferences-row-control desktop-runtime-preferences-row-control--switch">
          <el-switch
            v-model="launchAtLoginEnabled"
            :loading="launchAtLoginLoading"
            :disabled="launchAtLoginLoading"
            @change="handleLaunchAtLoginChange"
          />
        </div>
      </div>

      <div v-if="windowCloseBehaviorSupported" class="desktop-runtime-preferences-row">
        <div class="desktop-runtime-preferences-row-main">
          <div class="desktop-runtime-preferences-row-title">{{ t('messenger.settings.windowCloseBehavior') }}</div>
          <div class="desktop-runtime-preferences-row-hint">
            {{ t('messenger.settings.windowCloseBehaviorHint') }}
          </div>
        </div>
        <div class="desktop-runtime-preferences-row-control">
          <el-select
            v-model="windowCloseBehavior"
            class="desktop-runtime-preferences-select"
            :disabled="windowCloseBehaviorLoading"
            @change="handleWindowCloseBehaviorChange"
          >
            <el-option :label="t('messenger.settings.windowCloseBehaviorHide')" value="tray" />
            <el-option :label="t('messenger.settings.windowCloseBehaviorQuit')" value="quit" />
          </el-select>
        </div>
      </div>
    </div>

  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, type ComputedRef, type Ref } from 'vue';
import { ElMessage } from 'element-plus';

import {
  fetchDesktopSettings,
  updateDesktopSettings,
  type DesktopSettingsData
} from '@/api/desktop';
import { useI18n } from '@/i18n';

type WindowCloseBehavior = 'tray' | 'quit';
type DesktopLaunchAtLoginState = {
  supported: boolean;
  enabled: boolean;
};
type DesktopRuntimeBridge = {
  getPythonRuntimeInfo?: () => Promise<unknown> | unknown;
  getWindowCloseBehavior?: () => Promise<string | null> | string | null;
  setWindowCloseBehavior?: (behavior: string) => Promise<string | null> | string | null;
  getLaunchAtLogin?: () => Promise<unknown> | unknown;
  setLaunchAtLogin?: (enabled: boolean) => Promise<unknown> | unknown;
  importSupplementPackage?: () => Promise<unknown> | unknown;
  choosePythonInterpreter?: (defaultPath?: string) => Promise<string | null> | string | null;
  chooseRuntimeExecutable?: (payload?: {
    defaultPath?: string;
    title?: string;
  }) => Promise<string | null> | string | null;
};
type DesktopSupplementImportResult = {
  supported?: boolean;
  canceled?: boolean;
  installed?: boolean;
  install_root?: string;
  package_path?: string;
  imported_paths?: string[];
  runtime_paths?: Partial<Record<`${RuntimeToolKey}_path`, string>>;
};
type RuntimeToolKey = 'python' | 'pip' | 'git' | 'rg';
type RuntimeToolField = {
  key: RuntimeToolKey;
  label: string;
  draft: Ref<string>;
  configured: Ref<string>;
  placeholder: ComputedRef<string>;
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

const loading = ref(false);
const savingRuntimeToolPath = ref(false);
const pickingToolKey = ref<RuntimeToolKey | ''>('');
const importingSupplement = ref(false);
const pythonPathDraft = ref('');
const configuredPythonPath = ref('');
const pipPathDraft = ref('');
const configuredPipPath = ref('');
const gitPathDraft = ref('');
const configuredGitPath = ref('');
const rgPathDraft = ref('');
const configuredRgPath = ref('');
const pythonRuntimeMode = ref<'auto' | 'system' | 'custom'>('auto');
const pythonRuntimeBin = ref('');
const pipRuntimeBin = ref('');
const gitRuntimeBin = ref('');
const rgRuntimeBin = ref('');
const bundledDefaultPythonPath = ref('');
const bundledDefaultPythonExists = ref(false);
const detectedPythonPaths = ref<string[]>([]);
const windowCloseBehavior = ref<WindowCloseBehavior>('tray');
const windowCloseBehaviorLoading = ref(false);
const launchAtLoginEnabled = ref(false);
const launchAtLoginLoading = ref(false);
const launchAtLoginSupported = ref(false);
let disposed = false;

const pythonPathDirty = computed(
  () =>
    normalizePathForCompare(pythonPathDraft.value) !==
    normalizePathForCompare(configuredPythonPath.value)
);
const runtimeToolDirtyMap = {
  python: pythonPathDirty,
  pip: computed(
    () =>
      normalizePathForCompare(pipPathDraft.value) !==
      normalizePathForCompare(configuredPipPath.value)
  ),
  git: computed(
    () =>
      normalizePathForCompare(gitPathDraft.value) !==
      normalizePathForCompare(configuredGitPath.value)
  ),
  rg: computed(
    () =>
      normalizePathForCompare(rgPathDraft.value) !==
      normalizePathForCompare(configuredRgPath.value)
  )
};
const windowCloseBehaviorSupported = computed(() => {
  const bridge = getDesktopRuntimeBridge();
  return Boolean(
    bridge &&
      typeof bridge.getWindowCloseBehavior === 'function' &&
      typeof bridge.setWindowCloseBehavior === 'function'
  );
});
const supplementImportSupported = computed(() => {
  const bridge = getDesktopRuntimeBridge();
  return Boolean(bridge && typeof bridge.importSupplementPackage === 'function');
});
const pythonPathPlaceholder = computed(() => {
  if (pythonRuntimeMode.value === 'system') {
    return t('desktop.system.pythonInterpreterPathPlaceholder');
  }
  if (bundledDefaultPythonExists.value && bundledDefaultPythonPath.value) {
    return bundledDefaultPythonPath.value;
  }
  if (detectedPythonPaths.value.length) {
    return detectedPythonPaths.value[0];
  }
  if (bundledDefaultPythonPath.value) {
    return bundledDefaultPythonPath.value;
  }
  if (pythonRuntimeBin.value) {
    return pythonRuntimeBin.value;
  }
  return t('desktop.system.pythonInterpreterPathPlaceholder');
});
const pipPathPlaceholder = computed(() => {
  if (pipRuntimeBin.value) return pipRuntimeBin.value;
  return t('desktop.system.runtimeToolPathPlaceholder');
});
const gitPathPlaceholder = computed(() => {
  if (gitRuntimeBin.value) return gitRuntimeBin.value;
  return t('desktop.system.runtimeToolPathPlaceholder');
});
const rgPathPlaceholder = computed(() => {
  if (rgRuntimeBin.value) return rgRuntimeBin.value;
  return t('desktop.system.runtimeToolPathPlaceholder');
});
const runtimeToolFields = computed<RuntimeToolField[]>(() => [
  {
    key: 'python',
    label: t('desktop.system.pythonInterpreterPath'),
    draft: pythonPathDraft,
    configured: configuredPythonPath,
    placeholder: pythonPathPlaceholder
  },
  {
    key: 'pip',
    label: t('desktop.system.pipPath'),
    draft: pipPathDraft,
    configured: configuredPipPath,
    placeholder: pipPathPlaceholder
  },
  {
    key: 'git',
    label: t('desktop.system.gitPath'),
    draft: gitPathDraft,
    configured: configuredGitPath,
    placeholder: gitPathPlaceholder
  },
  {
    key: 'rg',
    label: t('desktop.system.rgPath'),
    draft: rgPathDraft,
    configured: configuredRgPath,
    placeholder: rgPathPlaceholder
  }
]);

function normalizePathForCompare(value: string): string {
  let normalized = String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/+$/, '');
  if (normalized === '/') return normalized;
  if (/^[A-Za-z]:$/.test(normalized)) normalized += '/';
  normalized = normalized.replace(/\/{2,}/g, '/');
  if (typeof window !== 'undefined' && navigator.userAgent.toLowerCase().includes('windows')) {
    normalized = normalized.toLowerCase();
  }
  return normalized;
}

function normalizePythonRuntimeMode(value: unknown, pythonPath = configuredPythonPath.value): 'auto' | 'system' | 'custom' {
  if (String(pythonPath || '').trim()) {
    return 'custom';
  }
  const mode = String(value || '')
    .trim()
    .toLowerCase();
  return mode === 'system' ? 'system' : 'auto';
}

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

function resolveErrorMessage(error: unknown, fallback: string): string {
  const responseMessage = (error as { response?: { data?: { message?: string } } })?.response?.data
    ?.message;
  const detailMessage = (error as { response?: { data?: { detail?: string } } })?.response?.data
    ?.detail;
  const message = (error as { message?: string })?.message;
  return String(responseMessage || detailMessage || message || fallback);
}

function applySettingsData(data: DesktopSettingsData | Record<string, unknown> | undefined) {
  const source = (data || {}) as Record<string, unknown>;
  configuredPythonPath.value = String(source.python_path || '').trim();
  pythonPathDraft.value = configuredPythonPath.value;
  configuredPipPath.value = String(source.pip_path || '').trim();
  pipPathDraft.value = configuredPipPath.value;
  configuredGitPath.value = String(source.git_path || '').trim();
  gitPathDraft.value = configuredGitPath.value;
  configuredRgPath.value = String(source.rg_path || '').trim();
  rgPathDraft.value = configuredRgPath.value;
  pythonRuntimeMode.value = normalizePythonRuntimeMode(source.python_runtime_mode, configuredPythonPath.value);
}

function applyFallbackPythonRuntimeInfo() {
  if (configuredPythonPath.value) {
    pythonRuntimeBin.value = configuredPythonPath.value;
  } else {
    pythonRuntimeBin.value = '';
  }
  bundledDefaultPythonPath.value = '';
  bundledDefaultPythonExists.value = false;
  detectedPythonPaths.value = pythonRuntimeBin.value ? [pythonRuntimeBin.value] : [];
  pipRuntimeBin.value = configuredPipPath.value;
  gitRuntimeBin.value = configuredGitPath.value;
  rgRuntimeBin.value = configuredRgPath.value;
}

function resolveInitialPickerPath(toolKey: RuntimeToolKey): string | undefined {
  const candidates = resolveToolPickerCandidates(toolKey).map((item) => String(item || '').trim());
  for (const item of candidates) {
    if (!item) continue;
    const normalized = item.replace(/[\\/]+$/, '');
    if (!normalized) continue;
    const slashIndex = Math.max(normalized.lastIndexOf('/'), normalized.lastIndexOf('\\'));
    if (slashIndex > 0) {
      if (slashIndex === 2 && /^[A-Za-z]:[\\/]/.test(normalized)) {
        return normalized.slice(0, 3);
      }
      return normalized.slice(0, slashIndex);
    }
  }
  return undefined;
}

function resolveToolPickerCandidates(toolKey: RuntimeToolKey): string[] {
  if (toolKey === 'python') {
    return [pythonPathDraft.value, configuredPythonPath.value, pythonRuntimeBin.value];
  }
  if (toolKey === 'pip') {
    return [pipPathDraft.value, configuredPipPath.value, pipRuntimeBin.value];
  }
  if (toolKey === 'git') {
    return [gitPathDraft.value, configuredGitPath.value, gitRuntimeBin.value];
  }
  return [rgPathDraft.value, configuredRgPath.value, rgRuntimeBin.value];
}

function resolveToolDraft(toolKey: RuntimeToolKey): Ref<string> {
  if (toolKey === 'python') return pythonPathDraft;
  if (toolKey === 'pip') return pipPathDraft;
  if (toolKey === 'git') return gitPathDraft;
  return rgPathDraft;
}

function resolveToolPickerTitle(toolKey: RuntimeToolKey): string {
  if (toolKey === 'python') return t('desktop.system.pythonPathPickerTitle');
  if (toolKey === 'pip') return t('desktop.system.pipPathPickerTitle');
  if (toolKey === 'git') return t('desktop.system.gitPathPickerTitle');
  return t('desktop.system.rgPathPickerTitle');
}

async function loadSettings() {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    if (disposed) return;
    applySettingsData((response?.data?.data || {}) as DesktopSettingsData);
  } catch (error) {
    if (disposed) return;
    console.error(error);
    ElMessage.error(resolveErrorMessage(error, t('desktop.common.loadFailed')));
  } finally {
    if (!disposed) {
      loading.value = false;
    }
  }
}

async function loadPythonRuntimeInfo() {
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.getPythonRuntimeInfo !== 'function') {
    applyFallbackPythonRuntimeInfo();
    return;
  }
  try {
    const payload = (await bridge.getPythonRuntimeInfo()) as Record<string, unknown> | null;
    if (disposed) return;
    const source = payload && typeof payload === 'object' ? payload : {};
    pythonRuntimeBin.value = String(source.bin || '').trim();
    bundledDefaultPythonPath.value = String(source.bundled_default_bin || '').trim();
    bundledDefaultPythonExists.value = source.bundled_default_exists === true;
    detectedPythonPaths.value = Array.isArray(source.detected_bins)
      ? source.detected_bins.map((item) => String(item || '').trim()).filter(Boolean)
      : [];
    pipRuntimeBin.value = String(source.pip_bin || '').trim();
    gitRuntimeBin.value = String(source.git_bin || '').trim();
    rgRuntimeBin.value = String(source.rg_bin || '').trim();
    if (pythonRuntimeBin.value && !detectedPythonPaths.value.includes(pythonRuntimeBin.value)) {
      detectedPythonPaths.value = [pythonRuntimeBin.value, ...detectedPythonPaths.value];
    }
    if (!pythonRuntimeBin.value) {
      applyFallbackPythonRuntimeInfo();
    }
  } catch (error) {
    if (disposed) return;
    console.error(error);
    applyFallbackPythonRuntimeInfo();
  }
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

async function saveRuntimeToolPath(toolKey: RuntimeToolKey, nextPath: string, silent = true) {
  if (savingRuntimeToolPath.value) {
    return;
  }
  savingRuntimeToolPath.value = true;
  try {
    const trimmedPath = String(nextPath || '').trim();
    const payload: Record<string, unknown> = {};
    if (toolKey === 'python') {
      payload.python_path = trimmedPath;
      payload.python_runtime_mode = trimmedPath ? 'custom' : 'system';
    } else {
      payload[`${toolKey}_path`] = trimmedPath;
    }
    const response = await updateDesktopSettings(payload);
    if (disposed) return;
    applySettingsData((response?.data?.data || {}) as DesktopSettingsData);
    await loadPythonRuntimeInfo();
    if (!silent) {
      ElMessage.success(t('desktop.common.saveSuccess'));
    }
  } catch (error) {
    if (disposed) return;
    console.error(error);
    ElMessage.error(resolveErrorMessage(error, t('desktop.common.saveFailed')));
  } finally {
    if (!disposed) {
      savingRuntimeToolPath.value = false;
    }
  }
}

async function handleRuntimeToolPathInputCommit(toolKey: RuntimeToolKey) {
  if (!runtimeToolDirtyMap[toolKey].value || savingRuntimeToolPath.value) {
    return;
  }
  await saveRuntimeToolPath(toolKey, resolveToolDraft(toolKey).value, true);
}

async function handleImportSupplementPackage() {
  const bridge = getDesktopRuntimeBridge();
  if (!bridge || typeof bridge.importSupplementPackage !== 'function' || importingSupplement.value) {
    return;
  }
  try {
    importingSupplement.value = true;
    await nextTick();
    const result = (await bridge.importSupplementPackage()) as DesktopSupplementImportResult | null;
    if (disposed || !result || result.canceled) {
      return;
    }
    const runtimePaths = result.runtime_paths || {};
    if (runtimePaths.python_path) pythonPathDraft.value = String(runtimePaths.python_path || '').trim();
    if (runtimePaths.pip_path) pipPathDraft.value = String(runtimePaths.pip_path || '').trim();
    if (runtimePaths.git_path) gitPathDraft.value = String(runtimePaths.git_path || '').trim();
    if (runtimePaths.rg_path) rgPathDraft.value = String(runtimePaths.rg_path || '').trim();
    const refreshedSettings = await fetchDesktopSettings();
    if (disposed) return;
    applySettingsData((refreshedSettings?.data?.data || {}) as DesktopSettingsData);
    await loadPythonRuntimeInfo();
    const installRoot = String(result.install_root || '').trim();
    if (installRoot) {
      ElMessage.success(t('desktop.system.pythonSupplementImportSuccess', { path: installRoot }));
      return;
    }
    ElMessage.success(
      t('desktop.system.pythonSupplementImportSuccess', {
        path: t('desktop.system.pythonSupplementInstallRoot')
      })
    );
  } catch (error) {
    if (disposed) return;
    console.error(error);
    ElMessage.error(resolveErrorMessage(error, t('desktop.system.pythonSupplementImportFailed')));
  } finally {
    if (!disposed) {
      importingSupplement.value = false;
    }
  }
}

async function openRuntimeToolPathPicker(toolKey: RuntimeToolKey) {
  if (pickingToolKey.value) {
    return;
  }
  const bridge = getDesktopRuntimeBridge();
  const canPickPython = toolKey === 'python' && typeof bridge?.choosePythonInterpreter === 'function';
  const canPickExecutable = typeof bridge?.chooseRuntimeExecutable === 'function';
  if (!bridge || (!canPickPython && !canPickExecutable)) {
    ElMessage.error(t('desktop.system.pythonPathPickerLoadFailed'));
    return;
  }
  pickingToolKey.value = toolKey;
  try {
    const defaultPath = resolveInitialPickerPath(toolKey);
    const picked =
      toolKey === 'python' && canPickPython
        ? await bridge.choosePythonInterpreter?.(defaultPath)
        : await bridge.chooseRuntimeExecutable?.({
            defaultPath,
            title: resolveToolPickerTitle(toolKey)
          });
    if (disposed) return;
    const nextPath = String(picked || '').trim();
    if (!nextPath) {
      return;
    }
    resolveToolDraft(toolKey).value = nextPath;
    await saveRuntimeToolPath(toolKey, nextPath, true);
  } catch (error) {
    if (disposed) return;
    console.error(error);
    ElMessage.error(resolveErrorMessage(error, t('desktop.system.pythonPathPickerLoadFailed')));
  } finally {
    if (!disposed) {
      pickingToolKey.value = '';
    }
  }
}

async function initializePanel() {
  await loadSettings();
  await Promise.all([
    loadPythonRuntimeInfo(),
    loadLaunchAtLoginState(),
    loadWindowCloseBehavior()
  ]);
}

onMounted(() => {
  void initializePanel();
});

onBeforeUnmount(() => {
  disposed = true;
});
</script>

<style scoped>
.desktop-runtime-preferences-panel {
  position: relative;
  min-height: 0;
  display: grid;
  gap: 14px;
}

.desktop-runtime-preferences-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.desktop-runtime-preferences-import-btn {
  flex: 0 0 auto;
}

.desktop-runtime-preferences-block {
  border: 1px solid var(--portal-border, #d8dee8);
  border-radius: 14px;
  background: var(--portal-surface, #f8fafc);
  padding: 12px;
  min-width: 0;
}

.desktop-runtime-preferences-field {
  display: grid;
  gap: 8px;
  min-width: 0;
}

.desktop-runtime-preferences-tool-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.desktop-runtime-preferences-field-label {
  color: var(--portal-text, #1f2937);
  font-size: 12px;
  font-weight: 600;
}

.desktop-runtime-preferences-editor {
  display: grid;
  gap: 10px;
}

.desktop-runtime-preferences-input-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
}

.desktop-runtime-preferences-inline-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: flex-end;
}

.desktop-runtime-preferences-input,
.desktop-runtime-preferences-select {
  width: 100%;
}

.desktop-runtime-preferences-select {
  min-width: 220px;
}

.desktop-runtime-preferences-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(220px, 280px);
  gap: 14px;
  align-items: center;
  min-width: 0;
}

.desktop-runtime-preferences-row-main {
  display: grid;
  gap: 4px;
  min-width: 0;
}

.desktop-runtime-preferences-row-title {
  color: var(--portal-text, #1f2937);
  font-size: 13px;
  font-weight: 600;
}

.desktop-runtime-preferences-row-hint {
  color: var(--portal-muted, #6b7280);
  font-size: 12px;
  line-height: 1.6;
}

.desktop-runtime-preferences-controls {
  border: 1px solid var(--portal-border, #d8dee8);
  border-radius: 14px;
  background: var(--portal-surface, #f8fafc);
  padding: 0 12px;
  overflow: hidden;
}

.desktop-runtime-preferences-controls .desktop-runtime-preferences-row {
  padding: 12px 0;
}

.desktop-runtime-preferences-controls .desktop-runtime-preferences-row + .desktop-runtime-preferences-row {
  border-top: 1px solid var(--portal-border, #e5e7eb);
}

.desktop-runtime-preferences-row-control {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  min-width: 0;
}

.desktop-runtime-preferences-row-control--switch {
  min-height: 40px;
}

.desktop-runtime-preferences-btn {
  border-radius: 10px;
  border: 1px solid var(--portal-border, #d8dee8);
  background: #ffffff;
  color: #4b5563;
  box-shadow: none;
}

.desktop-runtime-preferences-btn:hover:not(:disabled) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  background: var(--ui-accent-soft-2, rgba(var(--ui-accent-rgb), 0.08));
  color: var(--ui-accent-deep, #1d4ed8);
}

.desktop-runtime-preferences-panel :deep(.el-input__wrapper),
.desktop-runtime-preferences-panel :deep(.el-select__wrapper) {
  background: var(--portal-surface, #f8fafc);
  border: 1px solid var(--portal-border, #d8dee8);
  border-radius: 12px;
  box-shadow: none;
  min-height: 40px;
}

.desktop-runtime-preferences-panel :deep(.el-input__wrapper:hover),
.desktop-runtime-preferences-panel :deep(.el-select__wrapper:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.35);
}

.desktop-runtime-preferences-panel :deep(.el-input__wrapper.is-focus),
.desktop-runtime-preferences-panel :deep(.el-select__wrapper.is-focused) {
  border-color: rgba(var(--ui-accent-rgb), 0.55);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.12);
}

.desktop-runtime-preferences-panel :deep(.el-input__inner),
.desktop-runtime-preferences-panel :deep(.el-select__selected-item),
.desktop-runtime-preferences-panel :deep(.el-select__placeholder) {
  color: var(--portal-text, #1f2937);
}

@media (max-width: 900px) {
  .desktop-runtime-preferences-head {
    display: grid;
    grid-template-columns: 1fr;
  }

  .desktop-runtime-preferences-import-btn {
    justify-self: flex-start;
  }

  .desktop-runtime-preferences-tool-grid {
    grid-template-columns: 1fr;
  }

  .desktop-runtime-preferences-input-row {
    grid-template-columns: 1fr;
  }

  .desktop-runtime-preferences-inline-actions {
    justify-content: flex-start;
  }

  .desktop-runtime-preferences-row {
    grid-template-columns: 1fr;
  }

  .desktop-runtime-preferences-row-control {
    justify-content: flex-start;
  }

  .desktop-runtime-preferences-select {
    min-width: 0;
  }
}
</style>
