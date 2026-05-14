<template>
  <el-dialog
    v-model="dialogVisible"
    width="calc(100vw - 8px)"
    top="4px"
    class="workspace-dialog workspace-dialog--onlyoffice"
    append-to-body
    destroy-on-close
    :show-close="false"
    @closed="handleClosed"
  >
    <template #header>
      <div class="workspace-onlyoffice-head">
        <div class="workspace-preview-meta workspace-onlyoffice-path" :title="path">{{ path }}</div>
        <div class="workspace-onlyoffice-actions">
          <button
            class="workspace-onlyoffice-icon-btn"
            type="button"
            :title="t('common.download')"
            :aria-label="t('common.download')"
            :disabled="loading"
            @click="handleDownload"
          >
            <i class="fa-solid fa-download" aria-hidden="true"></i>
          </button>
          <button
            class="workspace-onlyoffice-icon-btn"
            type="button"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            :disabled="loading"
            @click="handleRefresh"
          >
            <i class="fa-solid fa-rotate" :class="{ 'fa-spin': loading }" aria-hidden="true"></i>
          </button>
          <button
            class="workspace-onlyoffice-icon-btn"
            type="button"
            :title="t('common.close')"
            :aria-label="t('common.close')"
            @click="close"
          >
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </template>
    <div v-if="errorText" class="workspace-preview-hint">{{ errorText }}</div>
    <div class="workspace-onlyoffice-frame">
      <div v-if="loading" class="workspace-empty">{{ t('workspace.onlyoffice.loading') }}</div>
      <div ref="hostRef" class="workspace-onlyoffice-host"></div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import { downloadWunderWorkspaceFile, fetchWunderWorkspaceOnlyOfficeConfig } from '@/api/workspace';
import type { QueryParams } from '@/api/types';
import { getCurrentLanguage, useI18n } from '@/i18n';
import { getFilenameFromHeaders, saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';

const props = defineProps<{
  visible: boolean;
  path: string;
  userId?: string;
  agentId?: string;
  containerId?: number | string | null;
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (event: 'saved', payload: { path: string }): void;
  (event: 'fallback', payload: { path: string; message?: string }): void;
}>();

type OnlyOfficeWindow = Window & {
  DocsAPI?: {
    DocEditor: new (elementId: string, config: Record<string, unknown>) => {
      destroyEditor?: () => void;
    };
  };
};

const { t } = useI18n();
const hostRef = ref<HTMLElement | null>(null);
const loading = ref(false);
const errorText = ref('');
const editorInstance = ref<{ destroyEditor?: () => void } | null>(null);
let loadScriptTask: Promise<void> | null = null;
let refreshTimer: number | null = null;

const dialogVisible = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const normalizedContainerId = computed<number | null>(() => {
  if (props.containerId === null || props.containerId === undefined || String(props.containerId).trim() === '') {
    return null;
  }
  const parsed = Number.parseInt(String(props.containerId ?? ''), 10);
  if (!Number.isFinite(parsed)) return null;
  return Math.min(10, Math.max(0, parsed));
});

const requestParams = (): QueryParams => {
  const params: QueryParams = {
    path: props.path,
    lang: getCurrentLanguage()
  };
  if (normalizedContainerId.value !== null) {
    params.container_id = normalizedContainerId.value;
  }
  const userId = String(props.userId || '').trim();
  if (userId) {
    params.user_id = userId;
  }
  const agentId = String(props.agentId || '').trim();
  if (agentId) {
    params.agent_id = agentId;
  }
  return params;
};

const destroyEditor = () => {
  try {
    editorInstance.value?.destroyEditor?.();
  } catch {
    // OnlyOffice may already have removed the iframe during dialog teardown.
  }
  editorInstance.value = null;
  if (hostRef.value) {
    hostRef.value.innerHTML = '';
  }
};

const loadOnlyOfficeScript = (apiUrl: string): Promise<void> => {
  const targetUrl = String(apiUrl || '').trim();
  const win = window as OnlyOfficeWindow;
  if (win.DocsAPI?.DocEditor) {
    return Promise.resolve();
  }
  const existing = Array.from(document.querySelectorAll<HTMLScriptElement>('script[data-onlyoffice-api]'))
    .find((script) => script.dataset.onlyofficeApi === targetUrl);
  if (existing && loadScriptTask) {
    return loadScriptTask;
  }
  loadScriptTask = new Promise<void>((resolve, reject) => {
    const script = existing || document.createElement('script');
    script.dataset.onlyofficeApi = targetUrl;
    script.src = targetUrl;
    script.async = true;
    script.onload = () => resolve();
    script.onerror = () => reject(new Error(t('workspace.onlyoffice.scriptFailed')));
    if (!existing) {
      document.head.appendChild(script);
    }
  });
  return loadScriptTask;
};

const scheduleSavedRefresh = () => {
  if (refreshTimer) {
    window.clearTimeout(refreshTimer);
  }
  refreshTimer = window.setTimeout(() => {
    refreshTimer = null;
    emit('saved', { path: props.path });
  }, 800);
};

const openEditor = async () => {
  if (!props.visible || !props.path) return;
  loading.value = true;
  errorText.value = '';
  destroyEditor();
  try {
    const { data } = await fetchWunderWorkspaceOnlyOfficeConfig(requestParams());
    const apiUrl = String(data?.api_url || '').trim();
    const config = data?.config || {};
    if (!apiUrl || !config || typeof config !== 'object') {
      throw new Error(t('workspace.onlyoffice.configFailed'));
    }
    await loadOnlyOfficeScript(apiUrl);
    await nextTick();
    const host = hostRef.value;
    const win = window as OnlyOfficeWindow;
    if (!host || !win.DocsAPI?.DocEditor) {
      throw new Error(t('workspace.onlyoffice.scriptFailed'));
    }
    const editorId = `onlyoffice-${Math.random().toString(36).slice(2, 10)}`;
    host.innerHTML = `<div id="${editorId}" class="workspace-onlyoffice-editor"></div>`;
    editorInstance.value = new win.DocsAPI.DocEditor(editorId, config as Record<string, unknown>);
    scheduleSavedRefresh();
  } catch (error) {
    const source = error as {
      response?: { data?: { detail?: string; error?: string } };
      message?: string;
    };
    const message =
      source.response?.data?.detail ||
      source.response?.data?.error ||
      source.message ||
      t('workspace.onlyoffice.openFailed');
    errorText.value = message;
    emitFallback(message);
    emit('update:visible', false);
  } finally {
    loading.value = false;
  }
};

const handleRefresh = async () => {
  await openEditor();
};

const handleDownload = async () => {
  if (!props.path || loading.value) return;
  try {
    const response = await downloadWunderWorkspaceFile(requestParams());
    const objectUrl = URL.createObjectURL(response.data);
    const fallbackName = props.path.split('/').pop() || 'download';
    const filename = getFilenameFromHeaders(
      response.headers as Record<string, unknown>,
      fallbackName
    );
    saveObjectUrlAsFile(objectUrl, filename);
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
  } catch (error) {
    const source = error as {
      response?: { data?: { detail?: string; error?: string } };
      message?: string;
    };
    errorText.value =
      source.response?.data?.detail ||
      source.response?.data?.error ||
      source.message ||
      t('workspace.download.failed');
  }
};

const emitFallback = (message = '') => {
  emit('fallback', {
    path: props.path,
    message: String(message || '').trim()
  });
};

const close = () => {
  emit('update:visible', false);
};

const handleClosed = () => {
  destroyEditor();
  loading.value = false;
  errorText.value = '';
  emit('saved', { path: props.path });
};

watch(
  () => [props.visible, props.path, props.userId, props.agentId, props.containerId],
  () => {
    if (props.visible) {
      void openEditor();
    }
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  if (refreshTimer) {
    window.clearTimeout(refreshTimer);
    refreshTimer = null;
  }
  destroyEditor();
});
</script>
