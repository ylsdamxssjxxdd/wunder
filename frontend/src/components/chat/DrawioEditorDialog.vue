<template>
  <el-dialog
    v-model="dialogVisible"
    width="calc(100vw - 16px)"
    top="8px"
    class="workspace-dialog workspace-dialog--drawio"
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
            :title="t('common.save')"
            :aria-label="t('common.save')"
            :disabled="loading || saving || !editorReady"
            @click="requestSave"
          >
            <i class="fa-solid fa-floppy-disk" :class="{ 'fa-spin': saving }" aria-hidden="true"></i>
          </button>
          <button
            class="workspace-onlyoffice-icon-btn"
            type="button"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            :disabled="loading || saving"
            @click="handleRefresh"
          >
            <i class="fa-solid fa-rotate" :class="{ 'fa-spin': loading }" aria-hidden="true"></i>
          </button>
          <button
            class="workspace-onlyoffice-icon-btn"
            type="button"
            :title="t('common.close')"
            :aria-label="t('common.close')"
            @click="close()"
          >
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </template>
    <div v-if="errorText" class="workspace-preview-hint">{{ errorText }}</div>
    <div class="workspace-onlyoffice-frame workspace-drawio-frame">
      <div v-if="loading || saving" class="workspace-empty">
        {{ saving ? t('workspace.drawio.saving') : t('workspace.drawio.loading') }}
      </div>
      <iframe
        v-if="iframeUrl"
        ref="iframeRef"
        class="workspace-drawio-editor"
        :src="iframeUrl"
        allow="clipboard-read; clipboard-write"
      ></iframe>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';

import {
  fetchWunderWorkspaceContent,
  fetchWunderWorkspaceDrawioConfig,
  saveWunderWorkspaceFile
} from '@/api/workspace';
import type { QueryParams } from '@/api/types';
import { getCurrentLanguage, useI18n } from '@/i18n';

const props = defineProps<{
  visible: boolean;
  path: string;
  agentId?: string;
  containerId?: number | string;
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (event: 'saved', payload: { path: string }): void;
  (event: 'fallback', payload: { path: string; message?: string }): void;
}>();

type DrawioMessage = {
  event?: string;
  xml?: string;
  modified?: boolean;
  exit?: boolean;
};

type PersistOptions = {
  notify?: boolean;
  showSaving?: boolean;
};

type CloseOptions = {
  flushPending?: boolean;
};

const AUTO_SAVE_DEBOUNCE_MS = 1200;

const { t } = useI18n();
const iframeRef = ref<HTMLIFrameElement | null>(null);
const loading = ref(false);
const saving = ref(false);
const editorReady = ref(false);
const iframeUrl = ref('');
const currentXml = ref('');
const errorText = ref('');
let editorOrigin = '';
let autosaveTimer: number | null = null;
let pendingAutosaveXml = '';
let pendingNotifyRefresh = false;
let pendingNotifyRefreshPath = '';
let saveQueue: Promise<void> = Promise.resolve();
let visibleSaveRequests = 0;
let closeInProgress = false;

const dialogVisible = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const normalizedContainerId = computed(() => {
  const parsed = Number.parseInt(String(props.containerId ?? ''), 10);
  if (!Number.isFinite(parsed)) return 0;
  return Math.min(10, Math.max(0, parsed));
});

const requestParams = (): QueryParams => {
  const params: QueryParams = {
    path: props.path,
    container_id: normalizedContainerId.value,
    lang: getCurrentLanguage()
  };
  const agentId = String(props.agentId || '').trim();
  if (agentId) {
    params.agent_id = agentId;
  }
  return params;
};

const postToEditor = (payload: Record<string, unknown>) => {
  const frameWindow = iframeRef.value?.contentWindow;
  if (!frameWindow || !editorOrigin) return;
  frameWindow.postMessage(JSON.stringify(payload), editorOrigin);
};

const clearAutosaveTimer = () => {
  if (autosaveTimer !== null) {
    window.clearTimeout(autosaveTimer);
    autosaveTimer = null;
  }
};

const scheduleAutosave = (xml: string) => {
  pendingAutosaveXml = xml;
  clearAutosaveTimer();
  autosaveTimer = window.setTimeout(() => {
    autosaveTimer = null;
    void flushAutosave({ notify: false, showSaving: false });
  }, AUTO_SAVE_DEBOUNCE_MS);
};

const flushAutosave = async (options: PersistOptions = {}) => {
  if (!pendingAutosaveXml) return true;
  const nextXml = pendingAutosaveXml;
  pendingAutosaveXml = '';
  clearAutosaveTimer();
  const persisted = await persistXml(nextXml, options);
  if (!persisted && !pendingAutosaveXml && props.visible) {
    pendingAutosaveXml = nextXml;
  }
  return persisted;
};

const beginSavingIndicator = (enabled: boolean) => {
  if (!enabled) return;
  visibleSaveRequests += 1;
  saving.value = true;
};

const endSavingIndicator = (enabled: boolean) => {
  if (!enabled) return;
  visibleSaveRequests = Math.max(0, visibleSaveRequests - 1);
  saving.value = visibleSaveRequests > 0;
};

const markPendingRefresh = (path: string) => {
  pendingNotifyRefresh = true;
  pendingNotifyRefreshPath = path;
};

const clearPendingRefresh = () => {
  pendingNotifyRefresh = false;
  pendingNotifyRefreshPath = '';
};

const emitSavedRefresh = (path: string) => {
  if (!path) return;
  emit('saved', { path });
  clearPendingRefresh();
};

const emitPendingRefresh = () => {
  if (!pendingNotifyRefresh) return;
  emitSavedRefresh(pendingNotifyRefreshPath || props.path);
};

const loadEditor = async () => {
  if (!props.visible || !props.path) return;
  loading.value = true;
  saving.value = false;
  editorReady.value = false;
  iframeUrl.value = '';
  editorOrigin = '';
  errorText.value = '';
  pendingAutosaveXml = '';
  clearPendingRefresh();
  clearAutosaveTimer();
  try {
    const params = requestParams();
    const { data: config } = await fetchWunderWorkspaceDrawioConfig(params);
    const maxBytes = Number(config?.max_file_bytes);
    const { data: contentPayload } = await fetchWunderWorkspaceContent({
      ...params,
      include_content: true,
      max_bytes: Number.isFinite(maxBytes) && maxBytes > 0 ? maxBytes : 50 * 1024 * 1024
    });
    const url = String(config?.editor_url || '').trim();
    const xml = typeof contentPayload?.content === 'string' ? contentPayload.content : '';
    if (!url) {
      throw new Error(t('workspace.drawio.configFailed'));
    }
    if (contentPayload?.truncated) {
      throw new Error(t('workspace.drawio.tooLarge'));
    }
    currentXml.value = xml;
    iframeUrl.value = url;
    editorOrigin = new URL(url, window.location.href).origin;
  } catch (error) {
    const source = error as {
      response?: { data?: { detail?: string; error?: string } | string };
      message?: string;
    };
    const data = source.response?.data;
    const message =
      (typeof data === 'object' && (data.detail || data.error)) ||
      (typeof data === 'string' ? data : '') ||
      source.message ||
      t('workspace.drawio.openFailed');
    errorText.value = message;
    emit('fallback', { path: props.path, message });
    emit('update:visible', false);
  } finally {
    loading.value = false;
  }
};

const handleEditorMessage = async (event: MessageEvent) => {
  if (!props.visible || !editorOrigin || event.origin !== editorOrigin) return;
  let message: DrawioMessage | null = null;
  if (typeof event.data === 'string') {
    try {
      message = JSON.parse(event.data) as DrawioMessage;
    } catch {
      return;
    }
  } else if (event.data && typeof event.data === 'object') {
    message = event.data as DrawioMessage;
  }
  if (!message?.event) return;
  if (message.event === 'init') {
    editorReady.value = true;
    const initialXml = currentXml.value || '<mxfile><diagram name="Page-1"></diagram></mxfile>';
    currentXml.value = initialXml;
    postToEditor({
      action: 'load',
      xml: initialXml,
      autosave: 1,
      title: props.path.split('/').pop() || 'diagram.drawio'
    });
    return;
  }
  if (message.event === 'autosave' && typeof message.xml === 'string') {
    scheduleAutosave(message.xml);
    return;
  }
  if (message.event === 'save' && typeof message.xml === 'string') {
    pendingAutosaveXml = '';
    clearAutosaveTimer();
    const persisted = await persistXml(message.xml, { notify: true, showSaving: true });
    if (persisted) {
      postToEditor({ action: 'status', messageKey: 'allChangesSaved', modified: false });
    }
    return;
  }
  if (message.event === 'export' && typeof message.xml === 'string') {
    pendingAutosaveXml = '';
    clearAutosaveTimer();
    await persistXml(message.xml, { notify: true, showSaving: true });
    return;
  }
  if (message.event === 'exit') {
    let persisted = true;
    if (typeof message.xml === 'string' && (message.modified === undefined || message.modified)) {
      pendingAutosaveXml = '';
      clearAutosaveTimer();
      persisted = await persistXml(message.xml, { notify: true, showSaving: true });
    } else {
      persisted = await flushAutosave({ notify: true, showSaving: true });
    }
    if (!persisted) return;
    emitPendingRefresh();
    await close({ flushPending: false });
  }
};

const persistXml = async (xml: string, options: PersistOptions = {}) => {
  const targetPath = props.path;
  if (!targetPath) return false;
  const params = requestParams();
  const notifySaved = options.notify !== false;
  const showSaving = options.showSaving !== false;
  beginSavingIndicator(showSaving);
  const operation = saveQueue
    .then(async () => {
      if (xml === currentXml.value) {
        if (notifySaved) {
          emitSavedRefresh(targetPath);
        }
        return true;
      }
      try {
        await saveWunderWorkspaceFile({
          ...params,
          path: targetPath,
          content: xml
        });
        if (props.path === targetPath) {
          currentXml.value = xml;
        }
        if (notifySaved) {
          emitSavedRefresh(targetPath);
        } else {
          markPendingRefresh(targetPath);
        }
        return true;
      } catch (error) {
        const source = error as {
          response?: { data?: { detail?: string; error?: string } | string };
          message?: string;
        };
        const data = source.response?.data;
        errorText.value =
          (typeof data === 'object' && (data.detail || data.error)) ||
          (typeof data === 'string' ? data : '') ||
          source.message ||
          t('workspace.drawio.saveFailed');
        return false;
      }
    })
    .finally(() => {
      endSavingIndicator(showSaving);
    });
  saveQueue = operation.then(() => undefined, () => undefined);
  try {
    return await operation;
  } catch {
    return false;
  }
};

const close = async (options: CloseOptions = {}) => {
  if (closeInProgress) return;
  closeInProgress = true;
  try {
    if (options.flushPending !== false) {
      const persisted = await flushAutosave({ notify: false, showSaving: true });
      await saveQueue;
      if (!persisted && pendingAutosaveXml) {
        return;
      }
    }
    emitPendingRefresh();
    emit('update:visible', false);
  } finally {
    closeInProgress = false;
  }
};

const requestSave = () => {
  if (!editorReady.value || saving.value) return;
  postToEditor({ action: 'save' });
};

const handleRefresh = async () => {
  await loadEditor();
};

const handleClosed = () => {
  clearAutosaveTimer();
  loading.value = false;
  saving.value = false;
  visibleSaveRequests = 0;
  closeInProgress = false;
  editorReady.value = false;
  iframeUrl.value = '';
  editorOrigin = '';
  errorText.value = '';
  pendingAutosaveXml = '';
  emitPendingRefresh();
};

watch(
  () => [props.visible, props.path, props.agentId, props.containerId],
  () => {
    if (props.visible) {
      void loadEditor();
    }
  },
  { immediate: true }
);

window.addEventListener('message', handleEditorMessage);

onBeforeUnmount(() => {
  clearAutosaveTimer();
  window.removeEventListener('message', handleEditorMessage);
});
</script>
