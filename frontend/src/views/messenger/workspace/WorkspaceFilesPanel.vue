<template>
  <div
    class="workspace-files"
    :class="{ 'is-dragover': dragActive, 'is-selecting': selectionMode }"
    @dragenter.prevent="handleDragEnter"
    @dragover.prevent="handleDragOver"
    @dragleave="handleDragLeave"
    @drop.prevent="handleDrop"
  >
    <div class="messenger-sidebar-files-head">
      <template v-if="!searchOpen">
        <span class="workspace-files-head-title">{{ t('messenger.filesArea.group') }}</span>
        <div v-if="!selectionMode" class="workspace-files-actions" role="toolbar" :aria-label="t('messenger.filesArea.group')">
          <template v-if="!remoteInterlinkActive">
            <input
              ref="uploadInputRef"
              class="workspace-files-upload-input"
              type="file"
              multiple
              tabindex="-1"
              @change="handleUploadInputChange"
            />
            <button
              class="workspace-files-action workspace-files-action--icon"
              type="button"
              :title="t('messenger.filesArea.upload')"
              :aria-label="t('messenger.filesArea.upload')"
              @click="openUploadPicker"
            >
              <i class="fa-solid fa-arrow-up-from-bracket" aria-hidden="true"></i>
            </button>
            <button
              class="workspace-files-action workspace-files-action--icon"
              type="button"
              :title="t('messenger.filesArea.search')"
              :aria-label="t('messenger.filesArea.search')"
              @click="openSearch"
            >
              <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
            </button>
            <button
              class="workspace-files-action workspace-files-action--icon"
              type="button"
              :disabled="busy"
              :aria-busy="archiving || undefined"
              :title="t('messenger.filesArea.archiveCurrent')"
              :aria-label="t('messenger.filesArea.archiveCurrent')"
              @click="handleDownloadCurrentDirectory"
            >
              <i
                class="fa-solid"
                :class="archiving ? 'fa-spinner fa-spin' : 'fa-file-zipper'"
                aria-hidden="true"
              ></i>
            </button>
            <button
              class="workspace-files-action workspace-files-action--icon"
              type="button"
              :title="t('messenger.filesArea.select')"
              :aria-label="t('messenger.filesArea.select')"
              @click="enterSelectionMode"
            >
              <i class="fa-solid fa-list-check" aria-hidden="true"></i>
            </button>
          </template>
          <button
            class="workspace-files-action workspace-files-action--icon"
            type="button"
            :disabled="refreshing"
            :title="t('common.refresh')"
            :aria-label="t('common.refresh')"
            @click="handleRefresh({ forceTree: true })"
          >
            <i class="fa-solid fa-rotate-right" :class="{ 'fa-spin': refreshing }" aria-hidden="true"></i>
          </button>
        </div>
      </template>
      <div v-else class="workspace-files-search" role="search">
        <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
        <input
          ref="searchInputRef"
          v-model="searchQuery"
          class="workspace-files-search-input"
          type="search"
          :placeholder="t('messenger.filesArea.searchPlaceholder')"
          :aria-label="t('messenger.filesArea.search')"
          @keydown.enter.prevent="runSearchNow"
          @keydown.esc.stop.prevent="exitSearch"
        />
        <button
          class="workspace-files-action workspace-files-action--icon"
          type="button"
          :title="t('common.cancel')"
          :aria-label="t('common.cancel')"
          @click="exitSearch"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
    </div>

    <div v-if="archiving" class="workspace-files-busy-note" role="status">
      <i class="fa-solid fa-spinner fa-spin" aria-hidden="true"></i>
      <span>{{ t('messenger.filesArea.archivingNote', { name: archiveTargetName }) }}</span>
    </div>

    <div v-if="remoteOperationState" class="workspace-files-remote-note" role="status">
      {{ remoteOperationState }}
      <button type="button" @click="cancelRemoteOperation">{{ t('common.cancel') }}</button>
    </div>
    <div v-if="remoteTruncated" class="workspace-files-remote-note" aria-live="polite">
      {{ t('interlink.workspace.treeTruncated') }}
    </div>

    <div v-if="selectionMode" class="workspace-files-selection">
      <span class="workspace-files-selection-count">
        {{ t('messenger.filesArea.selectedCount', { count: selectedList.length }) }}
      </span>
      <button type="button" @click="exitSelectionMode">{{ t('common.cancel') }}</button>
      <button type="button" :disabled="!selectedList.length" @click="handleBatchArchive">
        {{ t('messenger.filesArea.batchDownload') }}
      </button>
      <button type="button" class="is-danger" :disabled="!selectedList.length" @click="handleBatchDelete">
        {{ t('common.delete') }}
      </button>
    </div>

    <WorkspaceFileTree
      :rows="visibleFileRows"
      :active-path="searchActive ? '' : activePath"
      :highlighted-path="searchActive ? '' : highlightedPath"
      :loading="visibleFilesLoading"
      :error="visibleFilesError"
      :selection-mode="!searchActive && selectionMode"
      :selected-paths="selectedList"
      :read-only="!searchActive && remoteInterlinkActive && !remoteWritable"
      :remote="!searchActive && remoteInterlinkActive"
      :empty-text="visibleEmptyText"
      :empty-hint="visibleEmptyHint"
      @toggle-dir="handleToggleDirectory"
      @load-more="handleLoadMore"
      @activate="handleActivateRow"
      @open="handleOpenRow"
      @toggle-select="toggleSelection"
      @exit-selection="handleExitSelection"
      @retry="handleRetry"
      @command="handleRowCommand"
    />

    <div v-if="!remoteInterlinkActive && uploadTasks.length" class="workspace-files-uploads">
      <div class="workspace-files-uploads-head">
        <span>{{ t('messenger.filesArea.uploadingCount', { count: uploadTasks.length }) }}</span>
        <button type="button" @click="uploadQueue.clearSettled()">
          {{ t('messenger.filesArea.uploadClear') }}
        </button>
        <button type="button" @click="uploadQueue.cancelAll()">{{ t('common.cancel') }}</button>
      </div>
      <div v-for="task in uploadTasks" :key="task.id" class="workspace-files-upload-row">
        <span class="workspace-files-upload-name" :title="task.name">{{ task.name }}</span>
        <span class="workspace-files-upload-track" aria-hidden="true">
          <span
            class="workspace-files-upload-bar"
            :class="{ 'is-error': task.status === 'failed' }"
            :style="{ width: `${uploadPercent(task)}%` }"
          ></span>
        </span>
        <span class="workspace-files-upload-state">
          <template v-if="task.status === 'failed'">
            <button type="button" :title="t('messenger.filesArea.uploadRetry')" @click="uploadQueue.retry(task.id)">
              <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            </button>
          </template>
          <template v-else-if="task.status === 'uploading' || task.status === 'queued'">
            <button type="button" :title="t('common.cancel')" @click="uploadQueue.cancel(task.id)">
              <i class="fa-solid fa-xmark" aria-hidden="true"></i>
            </button>
          </template>
          <template v-else-if="task.status === 'done'">
            <i class="fa-solid fa-check" aria-hidden="true"></i>
          </template>
          <template v-else>
            <i class="fa-solid fa-ban" aria-hidden="true"></i>
          </template>
        </span>
      </div>
    </div>
  </div>

  <WorkspaceFilePreviewDialog
    :visible="preview.visible"
    :loading="preview.loading"
    :title="preview.title"
    :path="preview.path"
    :meta-label="preview.metaLabel"
    :hint="preview.hint"
    :error="preview.error"
    :too-large="preview.tooLarge"
    :editable="preview.editable"
    :saving="preview.saving"
    :preview-kind="preview.previewKind"
    :preview-url="preview.previewUrl"
    :content="preview.content"
    @close="closePreview"
    @download="handlePreviewDownload"
    @save="handlePreviewSave"
  />

  <component
    :is="OnlyOfficeEditorDialog"
    v-if="officeDialog.visible"
    :visible="officeDialog.visible"
    :path="officeDialog.path"
    preserve-sidebar
    :sidebar-visible="false"
    @update:visible="handleOfficeVisibleChange"
    @saved="handleEditorSaved"
  />

  <component
    :is="DrawioEditorDialog"
    v-if="drawioDialog.visible"
    :visible="drawioDialog.visible"
    :path="drawioDialog.path"
    preserve-sidebar
    :sidebar-visible="false"
    @update:visible="handleDrawioVisibleChange"
    @saved="handleEditorSaved"
  />

  <WorkspaceNewFileDialog
    v-model:visible="newFileDialog.visible"
    :file-type-options="workspaceNewFileTemplates"
    @confirm="handleNewFileConfirm"
  />
</template>

<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import type { InterlinkShadowWorkspace } from '@/api/interlink';
import {
  extractWorkspaceResourceExtension,
  normalizeWorkspacePreviewBlob,
  resolveWorkspacePreviewTooLargeHint,
  resolveWorkspacePreviewUnsupportedHint,
  resolveWorkspaceResourcePreviewKind
} from '@/utils/workspaceResourcePreview';
import { saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';
import { mzConfirm, mzPrompt } from '@/utils/mzDialog';
import WorkspaceNewFileDialog, {
  type WorkspaceNewFileTemplate
} from '@/components/chat/WorkspaceNewFileDialog.vue';
import WorkspaceFilePreviewDialog from './WorkspaceFilePreviewDialog.vue';
import WorkspaceFileTree from './WorkspaceFileTree.vue';
import { useInterlinkNodeTarget } from '@/views/messenger/interlink/interlinkNodeTarget';
import { useInterlinkNodes } from '@/views/messenger/interlink/useInterlinkNodes';
import { useInterlinkShadow } from '@/views/messenger/interlink/useInterlinkShadow';
import { listRemoteDirectory, readRemoteFile, runInterlinkCommand } from '@/views/messenger/interlink/interlinkCommandRunner';
import {
  DISABLED_REASON_KEY,
  REMOTE_PREVIEW_MAX_BYTES,
  groupShadowTreeByDirectory,
  normalizeRemoteDirectoryEntries,
  resolveDisabledReason,
  sortEntriesByName
} from '@/views/messenger/interlink/interlinkNodeModel';
import {
  clearWorkspaceRoot,
  createWorkspaceDirectory,
  createWorkspaceFile,
  deleteWorkspacePath,
  deleteWorkspacePaths,
  downloadWorkspaceDirectoryArchive,
  downloadWorkspacePath,
  fetchWorkspaceDirectory,
  fetchWorkspaceContentDirectory,
  fetchWorkspacePathBlob,
  isWorkspaceRequestCancelled,
  moveWorkspacePath,
  readWorkspaceFileContent,
  resolveWorkspaceErrorMessage,
  searchWorkspaceEntries,
  writeWorkspaceFileContent
} from './workspaceFileApi';
import {
  clearWorkspaceReveal,
  pendingWorkspaceReveal,
  queueWorkspaceChatReference
} from './workspaceChatReference';
import {
  formatWorkspaceBytes,
  isValidWorkspaceEntryName,
  joinWorkspacePath,
  normalizeWorkspaceRelativePath,
  workspaceFileExtension,
  workspaceParentPath,
  type WorkspaceEntry,
  type WorkspaceVisibleRow
} from './workspaceFileModel';
import { useWorkspaceFileTree } from './workspaceFileTree';
import { createWorkspaceDirectoryRefresh } from './workspaceDirectoryRefresh';
import { createWorkspaceUploadQueue, type WorkspaceUploadTask } from './workspaceUploadQueue';
import { onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { buildWorkspaceHtmlPreviewDocument } from '@/components/chat/workspaceHtmlPreview';

// The heavy editors are loaded only when the user actually opens such a file.
const OnlyOfficeEditorDialog = defineAsyncComponent(
  () => import('@/components/chat/OnlyOfficeEditorDialog.vue')
);
const DrawioEditorDialog = defineAsyncComponent(
  () => import('@/components/chat/DrawioEditorDialog.vue')
);

const { t } = useI18n();

/** Text/code editing is capped; larger files are download-only (plan §6.3). */
const WORKSPACE_TEXT_EDIT_MAX_BYTES = 2 * 1024 * 1024;
const HTML_PREVIEW_EXTENSIONS = new Set(['html', 'htm', 'xhtml']);
const UPLOAD_MAX_FILES_PER_DROP = 200;
const SEARCH_DEBOUNCE_MS = 250;
const SEARCH_RESULT_LIMIT = 200;
const METAFILE_EXTENSIONS = new Set(['wmf', 'emf']);

// 「新建文件」对话框的六种类型（与旧工作区面板同一套模板）。
const WORKSPACE_DOC_ICON_BASE = `${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}doc-icons`;
const WORKSPACE_TEXT_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/txt.png`;
const WORKSPACE_WORD_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/docx.png`;
const WORKSPACE_EXCEL_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/xlsx.png`;
const WORKSPACE_PPT_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/pptx.png`;
const WORKSPACE_DIAGRAM_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/processon_flow.png`;

const workspaceNewFileTemplates = computed<WorkspaceNewFileTemplate[]>(() => [
  {
    id: 'text',
    label: t('workspace.createFile.type.text'),
    extension: 'txt',
    extensionLabel: '.txt',
    icon: WORKSPACE_TEXT_FILE_ICON,
    hint: t('workspace.createFile.typeHint.text'),
    defaultName: 'untitled.txt',
    content: ''
  },
  {
    id: 'markdown',
    label: t('workspace.createFile.type.markdown'),
    extension: 'md',
    extensionLabel: '.md',
    icon: WORKSPACE_TEXT_FILE_ICON,
    hint: t('workspace.createFile.typeHint.markdown'),
    defaultName: 'notes.md',
    content: '# Title\n'
  },
  {
    id: 'word',
    label: t('workspace.createFile.type.word'),
    extension: 'docx',
    extensionLabel: '.docx',
    icon: WORKSPACE_WORD_FILE_ICON,
    hint: t('workspace.createFile.typeHint.word'),
    defaultName: 'document.docx',
    content: ''
  },
  {
    id: 'sheet',
    label: t('workspace.createFile.type.sheet'),
    extension: 'xlsx',
    extensionLabel: '.xlsx',
    icon: WORKSPACE_EXCEL_FILE_ICON,
    hint: t('workspace.createFile.typeHint.sheet'),
    defaultName: 'sheet.xlsx',
    content: ''
  },
  {
    id: 'slides',
    label: t('workspace.createFile.type.slides'),
    extension: 'pptx',
    extensionLabel: '.pptx',
    icon: WORKSPACE_PPT_FILE_ICON,
    hint: t('workspace.createFile.typeHint.slides'),
    defaultName: 'slides.pptx',
    content: ''
  },
  {
    id: 'diagram',
    label: t('workspace.createFile.type.flowchart'),
    extension: 'drawio',
    extensionLabel: '.drawio',
    icon: WORKSPACE_DIAGRAM_FILE_ICON,
    hint: t('workspace.createFile.typeHint.flowchart'),
    defaultName: 'flowchart.drawio',
    content: '<mxfile><diagram name="Flowchart"></diagram></mxfile>'
  }
]);

// ---------------------------------------------------------- 互通远程节点（§6.3）
// 目标工作区由路由 `?node=&ws=` 决定（侧栏工作区行是唯一入口），面板只跟随；
// 云端与远程共用同一棵目录树、同一套行渲染，差异只在数据源与只读状态。

const interlink = useInterlinkNodes({});
const {
  target: interlinkTarget,
  deviceId: remoteInterlinkDeviceId,
  workspaceId: interlinkWorkspaceId,
  isRemote: interlinkIsRemote
} = useInterlinkNodeTarget();

/** 选中本地设备＝云端目录整块让位给影子视图（默认云端行为零变化）。 */
const remoteInterlinkActive = computed(() => interlinkIsRemote.value);
const remoteInterlinkNode = computed(() => {
  const deviceId = remoteInterlinkDeviceId.value;
  if (!deviceId) return null;
  return interlink.nodes.value.find((item) => item.node_id === deviceId) || null;
});

const listBlockedReason = computed(() => {
  if (!remoteInterlinkActive.value) return '';
  const reason = resolveDisabledReason(remoteInterlinkNode.value, 'workspace.list');
  return reason ? t(DISABLED_REASON_KEY[reason]) : '';
});

const remoteWritable = computed(() => !resolveDisabledReason(remoteInterlinkNode.value, 'workspace.write'));
const remoteMutation = async (kind: string, args: Record<string, unknown>) => {
  const target = interlinkTarget.value;
  const workspace = interlinkWorkspaceId.value;
  const abort = directoryAbort;
  const blocked = resolveDisabledReason(remoteInterlinkNode.value, kind);
  if (blocked) throw new Error(t(DISABLED_REASON_KEY[blocked]));
  const outcome = await runInterlinkCommand({
    to: interlinkTarget.value, kind,
    args: { ...args, ...(interlinkWorkspaceId.value ? { workspace_id: interlinkWorkspaceId.value } : {}) },
    signal: directoryAbort.signal,
    timeoutMs: 135_000,
    onState: ({ record }) => { if (abort === directoryAbort) remoteOperationState.value = record?.approval_state === 'pending' ? t('interlink.operation.waitApproval') : String(record?.status || ''); }
  });
  if (abort !== directoryAbort || target !== interlinkTarget.value || workspace !== interlinkWorkspaceId.value) throw new Error(t('common.cancel'));
  remoteOperationState.value = '';
  if (outcome.status !== 'succeeded') throw new Error(outcome.errorSummary || outcome.errorCode || outcome.status);
};
const remoteOperationState = ref('');

const readBlockedReason = computed(() => {
  if (!remoteInterlinkActive.value) return '';
  const reason = resolveDisabledReason(remoteInterlinkNode.value, 'workspace.read');
  return reason ? t(DISABLED_REASON_KEY[reason]) : '';
});

// 每次切换目标都换一个 controller：旧命令轮询不会继续回填状态。
let directoryAbort = createAbortHost();
const cancelRemoteOperation = () => {
  directoryAbort.controller.abort();
  directoryAbort = createAbortHost();
  remoteOperationState.value = '';
  busy.value = false;
  preview.saving = false;
};

const isCurrentWorkspace = (request: ReturnType<typeof createAbortHost>): boolean =>
  request === directoryAbort && !request.signal.aborted;

function createAbortHost(): { controller: AbortController; signal: AbortSignal } {
  const controller = new AbortController();
  return { controller, signal: controller.signal };
}

const tree = useWorkspaceFileTree({
  onError: (message) => {
    if (message) ElMessage.error(message);
  },
  loadPage: async (path, pageOptions) => {
    if (!remoteInterlinkActive.value) {
      return pageOptions.refreshTree
        ? fetchWorkspaceContentDirectory(path, { ...pageOptions, refreshTree: true })
        : fetchWorkspaceDirectory(path, pageOptions);
    }
    const signal = directoryAbort.signal;
    const outcome = await listRemoteDirectory({
      to: interlinkTarget.value,
      workspaceId: interlinkWorkspaceId.value || undefined,
      path,
      offset: pageOptions.offset,
      limit: pageOptions.limit,
      signal
    });
    if (outcome.failure) {
      const thrown = new Error(outcome.failure) as Error & { code?: string };
      // 切换目标导致的主动取消不渲染成错误。
      if (signal.aborted) thrown.code = 'ERR_CANCELED';
      throw thrown;
    }
    return {
      path,
      entries: normalizeRemoteDirectoryEntries(outcome.entries),
      total: outcome.total,
      offset: pageOptions.offset,
      limit: pageOptions.limit
    };
  }
});
const {
  rows,
  activePath,
  highlightedPath,
  selectedList,
  selectionMode,
  loadMore,
  refreshDirectory,
  loadDirectory,
  revealPath,
  removePaths,
  setActivePath,
  toggleSelection,
  exitSelectionMode
} = tree;

const uploadTarget = ref('');
const uploadInputRef = ref<HTMLInputElement | null>(null);
const busy = ref(false);
const refreshing = ref(false);
/** 服务端打包当前目录期间为 true：按钮转圈并在标题下挂一条状态说明。 */
const archiving = ref(false);
/** 当前目录的显示名（根目录用分组名），用于打包确认与打包中提示。 */
const archiveTargetName = computed(() => {
  const segment = uploadTarget.value.split('/').filter(Boolean).pop();
  return segment || t('messenger.filesArea.group');
});
const dragActive = ref(false);
const newFileDialog = reactive({ visible: false, directory: '' });
let workspaceRefreshStop: (() => void) | null = null;

const rootLoading = computed(() => Boolean(tree.directories.get('')?.loading));
const rootError = computed(() => String(tree.directories.get('')?.error || ''));

// ----------------------------------------------------------------- shadow seed
// 影子投影（§6.1）负责两件事：离线时仍有可浏览的目录快照；在线时先给内容再换实时列表。

const shadowApi = useInterlinkShadow();

const remoteShadowWorkspace = computed<InterlinkShadowWorkspace | null>(() => {
  if (!remoteInterlinkActive.value) return null;
  const list = shadowApi.shadow.value?.workspaces || [];
  if (!list.length) return null;
  const wanted = interlinkWorkspaceId.value;
  if (wanted) {
    // A missing selected named workspace must not display a different local
    // directory as its fallback. The live listing may arrive later, but until
    // then the panel stays empty instead of making an unsafe visual switch.
    return list.find((item) => item.workspace_id === wanted) || null;
  }
  return list.find((item) => !item.workspace_id) || null;
});

const remoteTruncated = computed(() => remoteShadowWorkspace.value?.truncated === true);
const remoteEmptyHint = computed(() =>
  remoteInterlinkActive.value ? t('interlink.workspace.emptyShadow') : ''
);

const filesLoading = computed(() => {
  if (!remoteInterlinkActive.value) return rootLoading.value;
  return (shadowApi.loading.value || rootLoading.value) && !rows.value.length;
});

const filesError = computed(() => {
  if (!remoteInterlinkActive.value) return rootError.value;
  if (rootError.value || rows.value.length) return rootError.value;
  return String(shadowApi.error.value || '');
});

// ------------------------------------------------------------------- search
// 文件名搜索走服务端递归索引；搜索期间工具栏收起、头部换成搜索框，结果平铺进同一个行视图。

const searchOpen = ref(false);
const searchQuery = ref('');
const searchLoading = ref(false);
const searchError = ref('');
const searchResults = ref<WorkspaceEntry[]>([]);
const searchInputRef = ref<HTMLInputElement | null>(null);
let searchTimer: number | null = null;
let searchAbort: AbortController | null = null;
let searchSerial = 0;

const clearSearchTimer = () => {
  if (searchTimer !== null) window.clearTimeout(searchTimer);
  searchTimer = null;
};

const openSearch = () => {
  if (remoteInterlinkActive.value) return;
  searchOpen.value = true;
  void nextTick(() => searchInputRef.value?.focus());
};

const exitSearch = () => {
  if (!searchOpen.value) return;
  searchOpen.value = false;
  searchQuery.value = '';
  searchResults.value = [];
  searchError.value = '';
  searchLoading.value = false;
  searchSerial += 1;
  clearSearchTimer();
  searchAbort?.abort();
  searchAbort = null;
};

const runSearch = async (): Promise<void> => {
  const keyword = searchQuery.value.trim();
  if (!searchOpen.value) return;
  if (!keyword) {
    searchResults.value = [];
    searchError.value = '';
    searchLoading.value = false;
    return;
  }
  const serial = (searchSerial += 1);
  searchAbort?.abort();
  const controller = new AbortController();
  searchAbort = controller;
  searchLoading.value = true;
  searchError.value = '';
  try {
    const entries = await searchWorkspaceEntries(keyword, {
      limit: SEARCH_RESULT_LIMIT,
      signal: controller.signal
    });
    if (serial !== searchSerial) return;
    searchResults.value = entries;
  } catch (error) {
    if (serial !== searchSerial || isWorkspaceRequestCancelled(error)) return;
    searchResults.value = [];
    searchError.value = resolveWorkspaceErrorMessage(error, t('common.requestFailed'));
  } finally {
    if (serial === searchSerial) searchLoading.value = false;
  }
};

/** 回车立即搜索，跳过防抖等待。 */
const runSearchNow = () => {
  clearSearchTimer();
  void runSearch();
};

watch(searchQuery, () => {
  if (!searchOpen.value) return;
  clearSearchTimer();
  if (searchQuery.value.trim()) searchLoading.value = true;
  searchTimer = window.setTimeout(() => {
    searchTimer = null;
    void runSearch();
  }, SEARCH_DEBOUNCE_MS);
});

/** 关键词为空时仍显示目录树，只有真正有查询才切换到平铺结果。 */
const searchActive = computed(() => searchOpen.value && searchQuery.value.trim().length > 0);

/** 结果平铺：目录不再有展开箭头，父子目录用行尾路径提示区分同名项。 */
const searchRows = computed<WorkspaceVisibleRow[]>(() =>
  searchResults.value.map((entry) => ({
    key: `search:${entry.path}`,
    path: entry.path,
    name: entry.name,
    pathHint: workspaceParentPath(entry.path),
    kind: entry.kind,
    size: entry.size,
    updatedTime: entry.updatedTime,
    depth: 0,
    expanded: false,
    loading: false,
    expandable: false
  }))
);

const visibleFileRows = computed(() => (searchActive.value ? searchRows.value : rows.value));
const visibleFilesLoading = computed(() =>
  searchActive.value ? searchLoading.value : filesLoading.value
);
const visibleFilesError = computed(() => (searchActive.value ? searchError.value : filesError.value));
const visibleEmptyText = computed(() =>
  searchActive.value ? t('messenger.filesArea.searchEmpty') : ''
);
const visibleEmptyHint = computed(() =>
  searchActive.value ? t('messenger.filesArea.searchEmptyHint') : remoteEmptyHint.value
);

const handleExitSelection = () => {
  if (searchOpen.value) {
    exitSearch();
    return;
  }
  exitSelectionMode();
};

const handleRetry = () => {
  if (searchActive.value) {
    void runSearch();
    return;
  }
  void handleRefresh();
};

const seedTreeFromShadow = (): void => {
  const keepExpanded = Array.from(tree.expandedPaths);
  tree.reset();
  const entries = remoteShadowWorkspace.value?.tree || [];
  const groups = groupShadowTreeByDirectory(entries);
  groups.forEach((list, parent) => {
    tree.patchDirectoryEntries(parent, sortEntriesByName(list));
  });
  // 根节点必须存在，否则行铺平函数走不到任何目录。
  if (!groups.has('')) tree.patchDirectoryEntries('', []);
  keepExpanded.forEach((path) => {
    if (groups.has(path)) tree.expandedPaths.add(path);
  });
};

/** 进入一个远程工作区：先铺影子快照，在线时再把根目录换成实时列表。 */
const enterRemoteWorkspace = async (): Promise<void> => {
  if (!remoteInterlinkActive.value) return;
  const target = interlinkTarget.value;
  const deviceId = remoteInterlinkDeviceId.value;
  const workspaceId = interlinkWorkspaceId.value;
  exitSelectionMode();
  remoteOperationState.value = '';
  directoryAbort.controller.abort();
  directoryAbort = createAbortHost();
  const request = directoryAbort;
  closePreview();
  await shadowApi.load(deviceId || '');
  // The target can change while the bounded shadow request is in flight. A
  // late completion must not seed its directory into the newly selected local
  // workspace.
  if (
    !remoteInterlinkActive.value ||
    request !== directoryAbort ||
    request.signal.aborted ||
    target !== interlinkTarget.value ||
    deviceId !== remoteInterlinkDeviceId.value ||
    workspaceId !== interlinkWorkspaceId.value
  ) return;
  seedTreeFromShadow();
  if (!listBlockedReason.value) void tree.loadDirectory('', { force: true });
};

// ------------------------------------------------------------------ uploads

const uploadQueue = createWorkspaceUploadQueue({
  concurrency: 2,
  onUploaded: (task) => {
    void handleUploadFinished(task);
  },
  onDrained: () => {
    const tasks = uploadQueue.tasks;
    const failed = tasks.filter((task) => task.status === 'failed').length;
    const done = tasks.filter((task) => task.status === 'done').length;
    if (failed > 0) {
      // Failed rows stay visible so the user can retry them.
      ElMessage.error(t('messenger.filesArea.uploadFailedCount', { count: failed }));
      uploadQueue.clearSucceeded();
      return;
    }
    if (done > 0) {
      ElMessage.success(t('messenger.filesArea.uploadDone', { count: done }));
      uploadQueue.clearSucceeded();
    }
  }
});
const uploadTasks = computed(() => uploadQueue.tasks);

const uploadPercent = (task: WorkspaceUploadTask): number => {
  if (task.status === 'done') return 100;
  if (!task.size) return task.status === 'uploading' ? 30 : 0;
  return Math.max(2, Math.min(100, Math.round((task.loaded / task.size) * 100)));
};

const handleUploadFinished = async (task: WorkspaceUploadTask) => {
  const uploadedPaths = task.uploadedPaths.length
    ? task.uploadedPaths
    : [joinWorkspacePath(task.targetPath, task.name)];
  const directoriesToRefresh = new Set<string>();
  uploadedPaths.forEach((path) => directoriesToRefresh.add(workspaceParentPath(path)));
  for (const directory of directoriesToRefresh) {
    await refreshDirectory(directory);
  }
  const first = uploadedPaths[0];
  if (first) {
    highlightedPath.value = first;
    window.setTimeout(() => {
      if (highlightedPath.value === first) highlightedPath.value = '';
    }, 2400);
  }
};

const enqueueFiles = (files: File[], targetPath: string) => {
  if (!files.length) return;
  // A new batch starts from a clean slate; finished rows from the last batch
  // would otherwise pile up in a 240px-wide column.
  uploadQueue.clearSettled();
  const result = uploadQueue.enqueue(
    files.map((file) => ({ file })),
    targetPath
  );
  if (result.rejected > 0) {
    ElMessage.warning(t('messenger.filesArea.uploadQueueFull', { count: result.rejected }));
  }
};

const openUploadPicker = () => {
  if (remoteInterlinkActive.value) return;
  uploadInputRef.value?.click();
};

const handleUploadInputChange = (event: Event) => {
  const input = event.target as HTMLInputElement | null;
  const files = Array.from(input?.files || []);
  // Clear the input before queueing so retrying the same local file opens a
  // new upload request instead of being ignored by the browser change event.
  if (input) input.value = '';
  enqueueFiles(files.slice(0, UPLOAD_MAX_FILES_PER_DROP), uploadTarget.value);
};

const handleDragEnter = () => {
  // 远程节点是只读投影，不接受拖拽上传（写入要走 workspace.write + 本地审批）。
  if (remoteInterlinkActive.value) return;
  dragActive.value = true;
};

const handleDragOver = (event: DragEvent) => {
  if (remoteInterlinkActive.value) return;
  dragActive.value = true;
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
};

const handleDragLeave = (event: DragEvent) => {
  const related = event.relatedTarget as Node | null;
  const current = event.currentTarget as Node | null;
  if (related && current && current.contains(related)) return;
  dragActive.value = false;
};

const handleDrop = async (event: DragEvent) => {
  dragActive.value = false;
  if (remoteInterlinkActive.value) return;
  const transfer = event.dataTransfer;
  if (!transfer) return;
  const collected: File[] = [];
  let sawDirectory = false;

  type DroppedEntry = {
    isDirectory: boolean;
    isFile: boolean;
    file?: (callback: (file: File) => void) => void;
    createReader?: () => { readEntries: (callback: (items: unknown[]) => void) => void };
  };

  const entries = Array.from(transfer.items || [])
    .map((item) => (typeof item.webkitGetAsEntry === 'function' ? item.webkitGetAsEntry() : null))
    .filter(Boolean) as unknown as DroppedEntry[];

  if (entries.length) {
    await collectDroppedEntries(entries, collected, 0, () => {
      sawDirectory = true;
    });
  } else {
    collected.push(...Array.from(transfer.files || []));
  }

  if (!collected.length && sawDirectory) {
    ElMessage.info(t('messenger.filesArea.folderUnsupported'));
    return;
  }
  enqueueFiles(collected.slice(0, UPLOAD_MAX_FILES_PER_DROP), uploadTarget.value);
};

/** Bounded depth-first walk over a dropped directory tree. */
const collectDroppedEntries = async (
  entries: Array<{
    isDirectory: boolean;
    isFile: boolean;
    file?: (callback: (file: File) => void) => void;
    createReader?: () => { readEntries: (callback: (items: unknown[]) => void) => void };
  }>,
  output: File[],
  depth: number,
  markDirectory: () => void
): Promise<void> => {
  if (depth > 6 || output.length >= UPLOAD_MAX_FILES_PER_DROP) return;
  for (const entry of entries) {
    if (output.length >= UPLOAD_MAX_FILES_PER_DROP) return;
    if (entry.isFile && entry.file) {
      const file = await new Promise<File | null>((resolve) => {
        entry.file?.((value) => resolve(value || null));
      });
      if (file) output.push(file);
      continue;
    }
    if (entry.isDirectory) {
      markDirectory();
      const reader = entry.createReader?.();
      if (!reader) continue;
      const children = await new Promise<unknown[]>((resolve) => {
        reader.readEntries((items) => resolve(items || []));
      });
      if (children.length) {
        await collectDroppedEntries(
          children as Parameters<typeof collectDroppedEntries>[0],
          output,
          depth + 1,
          markDirectory
        );
      }
    }
  }
};

// --------------------------------------------------------------- tree wiring

/**
 * 展开/收起一个目录。云端直接翻树；远程在线展开时把影子快照换成实时列表，
 * 离线或无能力时只翻种子内容，不发命令。
 */
const toggleDirectoryPath = async (path: string): Promise<void> => {
  if (!remoteInterlinkActive.value) {
    await tree.toggleDirectory(path);
    return;
  }
  const wasExpanded = tree.isExpanded(path);
  const seeded = Boolean(tree.getDirectory(path)?.loaded);
  if (listBlockedReason.value) {
    if (!seeded) {
      ElMessage.warning(listBlockedReason.value);
      return;
    }
    if (!wasExpanded) await tree.toggleDirectory(path);
    return;
  }
  await tree.toggleDirectory(path);
  if (!wasExpanded && seeded) await tree.refreshDirectory(path);
};

const handleActivateRow = (row: WorkspaceVisibleRow) => {
  setActivePath(row.path);
  // 搜索结果平铺，点选只记高亮，不展开目录也不改上传目标。
  if (searchActive.value) return;
  if (row.kind === 'dir') {
    uploadTarget.value = row.path;
    void toggleDirectoryPath(row.path);
    return;
  }
  uploadTarget.value = workspaceParentPath(row.path);
};

const handleOpenRow = (row: WorkspaceVisibleRow) => {
  if (searchActive.value) {
    if (row.kind === 'dir') {
      exitSearch();
      void revealPath(row.path);
    } else {
      void openPreview(row.path, row.name, row.size);
    }
    return;
  }
  if (row.kind === 'dir') {
    uploadTarget.value = row.path;
    void toggleDirectoryPath(row.path);
    return;
  }
  void openPreview(row.path, row.name, row.size);
};

const handleToggleDirectory = (path: string) => {
  if (searchActive.value) {
    exitSearch();
    void revealPath(path);
    return;
  }
  uploadTarget.value = normalizeWorkspaceRelativePath(path);
  void toggleDirectoryPath(path);
};

const handleLoadMore = (path: string) => {
  if (searchActive.value) return;
  void loadMore(path);
};

const handleRefresh = async (options: { forceTree?: boolean } = {}) => {
  if (remoteInterlinkActive.value) {
    if (refreshing.value) return;
    refreshing.value = true;
    try {
      await enterRemoteWorkspace();
    } finally {
      refreshing.value = false;
    }
    return;
  }
  await directoryRefresh.request(options);
};

const enterSelectionMode = () => {
  if (remoteInterlinkActive.value) return;
  tree.clearSelection();
  selectionMode.value = true;
};

const handleDownloadCurrentDirectory = async () => {
  if (remoteInterlinkActive.value || archiving.value) return;
  const confirmed = await mzConfirm({
    title: t('messenger.filesArea.archiveCurrent'),
    message: t('messenger.filesArea.archiveConfirm', { name: archiveTargetName.value }),
    confirmText: t('common.download'),
    cancelText: t('common.cancel')
  });
  if (!confirmed) return;
  archiving.value = true;
  try {
    await runAction(
      () => downloadWorkspaceDirectoryArchive(uploadTarget.value),
      t('messenger.filesArea.downloadFailed')
    );
  } finally {
    archiving.value = false;
  }
};

const directoryRefresh = createWorkspaceDirectoryRefresh(tree, {
  onBusy: value => { refreshing.value = value; }
});

const refreshFromWorkspaceEvent = (event: Event) => {
  if (remoteInterlinkActive.value) return;
  const detail = (event as CustomEvent<Record<string, unknown>>).detail || {};
  void directoryRefresh.handleEvent(detail);
};

/** Menu target: a directory row creates inside it, anything else uses the current folder. */
const createTargetPath = (row: WorkspaceVisibleRow | null): string =>
  row && row.kind === 'dir' ? row.path : uploadTarget.value;

const handleCreateEntry = async (kind: 'dir' | 'file', row: WorkspaceVisibleRow | null) => {
  if (busy.value) return;
  const request = directoryAbort;
  const directory = createTargetPath(row);
  if (kind === 'file') {
    // 文件类型交给专用对话框选择（文本 / Markdown / Word / 表格 / 演示文稿 / 流程图）。
    newFileDialog.directory = directory;
    newFileDialog.visible = true;
    return;
  }
  try {
    const value = await mzPrompt({
      title: t('messenger.filesArea.newDir'),
      message: t('messenger.filesArea.newDirPrompt'),
      placeholder: t('messenger.filesArea.namePlaceholder'),
      validator: (input: string) =>
        isValidWorkspaceEntryName(input) ? true : t('messenger.filesArea.invalidName')
    });
    const name = String(value || '').trim();
    if (!name || !isCurrentWorkspace(request)) return;
    busy.value = true;
    if (remoteInterlinkActive.value) await remoteMutation('workspace.mkdir', { path: joinWorkspacePath(directory, name) });
    else await createWorkspaceDirectory(directory, name);
    if (!isCurrentWorkspace(request)) return;
    await refreshDirectory(directory);
    await revealPath(joinWorkspacePath(directory, name));
    ElMessage.success(t('messenger.filesArea.newDirSuccess'));
  } catch (error) {
    if (isCurrentWorkspace(request) && (error instanceof Error || (error as { response?: unknown })?.response)) {
      ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
    }
  } finally {
    if (isCurrentWorkspace(request)) busy.value = false;
  }
};

/** 「新建文件」对话框确认：按所选类型写入带模板内容的新文件。 */
const handleNewFileConfirm = async (payload: { name: string; content: string; typeId: string }) => {
  if (!newFileDialog.visible) return;
  const request = directoryAbort;
  const name = String(payload.name || '').trim();
  newFileDialog.visible = false;
  if (!name || !isValidWorkspaceEntryName(name)) {
    if (name) ElMessage.warning(t('messenger.filesArea.invalidName'));
    return;
  }
  const directory = newFileDialog.directory;
  busy.value = true;
  try {
    if (remoteInterlinkActive.value) await remoteMutation('workspace.write', { path: joinWorkspacePath(directory, name), content: payload.content || '' });
    else await createWorkspaceFile(directory, name, payload.content || '');
    if (!isCurrentWorkspace(request)) return;
    await refreshDirectory(directory);
    await revealPath(joinWorkspacePath(directory, name));
    ElMessage.success(t('messenger.filesArea.newFileSuccess'));
  } catch (error) {
    if (isCurrentWorkspace(request)) ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    if (isCurrentWorkspace(request)) busy.value = false;
  }
};

// -------------------------------------------------------------- row commands

/** 搜索结果里会改动工作区的命令；执行后重跑一次搜索，避免列表留旧行。 */
const SEARCH_RERUN_COMMANDS = new Set(['new-dir', 'new-file', 'rename', 'delete']);

const handleRowCommand = async (command: string, row: WorkspaceVisibleRow | null) => {
  if (searchActive.value) {
    if (command === 'refresh') {
      void runSearch();
      return;
    }
    if (command === 'toggle' && row?.kind === 'dir') {
      exitSearch();
      void revealPath(row.path);
      return;
    }
  }
  await runRowCommand(command, row);
  if (searchActive.value && SEARCH_RERUN_COMMANDS.has(command)) void runSearch();
};

const runRowCommand = async (command: string, row: WorkspaceVisibleRow | null) => {
  if (remoteInterlinkActive.value) {
    if (command === 'refresh') { await handleRefresh({ forceTree: true }); return; }
    if (command === 'new-dir' || command === 'new-file') {
      await handleCreateEntry(command === 'new-dir' ? 'dir' : 'file', row); return;
    }
    if (!row) return;
    if (command === 'toggle') await toggleDirectoryPath(row.path);
    else if (command === 'preview') await openPreview(row.path, row.name, row.size);
    else if (command === 'download') await downloadRemoteFile(row.path, row.name);
    else if (command === 'rename' && remoteWritable.value) await renameEntry(row);
    else if (command === 'delete' && remoteWritable.value) await deleteEntry(row);
    return;
  }
  // 空白区菜单没有宿主行：创建落在当前目录，其余三项承接自已下线的头部 ⋯。
  switch (command) {
    case 'refresh':
      if (!row) {
        await handleRefresh({ forceTree: true });
      } else {
        await refreshDirectory(
          row.kind === 'dir' ? row.path : workspaceParentPath(row.path),
          { refreshTree: true }
        );
      }
      return;
    case 'new-dir':
      await handleCreateEntry('dir', row);
      return;
    case 'new-file':
      await handleCreateEntry('file', row);
      return;
    case 'archive-root':
      await runAction(
        () => downloadWorkspaceDirectoryArchive(uploadTarget.value),
        t('messenger.filesArea.downloadFailed')
      );
      return;
    case 'clear':
      await handleClearWorkspace();
      return;
    default:
      break;
  }
  if (!row) return;
  switch (command) {
    case 'toggle':
      await toggleDirectoryPath(row.path);
      return;
    case 'preview':
      await openPreview(row.path, row.name, row.size);
      return;
    case 'download':
      await runAction(() => downloadWorkspacePath(row.path), t('messenger.filesArea.downloadFailed'));
      return;
    case 'archive':
      await runAction(
        () => downloadWorkspaceDirectoryArchive(row.path),
        t('messenger.filesArea.downloadFailed')
      );
      return;
    case 'rename':
      await renameEntry(row);
      return;
    case 'quote':
      quoteEntryToChat(row);
      return;
    case 'delete':
      await deleteEntry(row);
      return;
    default:
      return;
  }
};

const runAction = async (action: () => Promise<void>, fallback: string) => {
  busy.value = true;
  try {
    await action();
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    ElMessage.error(resolveWorkspaceErrorMessage(error, fallback));
  } finally {
    busy.value = false;
  }
};

const renameEntry = async (row: WorkspaceVisibleRow) => {
  const request = directoryAbort;
  try {
    const value = await mzPrompt({
      title: t('common.edit'),
      message: t('messenger.filesArea.renamePrompt'),
      inputValue: row.name,
      validator: (input: string) =>
        isValidWorkspaceEntryName(input) ? true : t('messenger.filesArea.invalidName')
    });
    const nextName = String(value || '').trim();
    if (!nextName || nextName === row.name || !isCurrentWorkspace(request)) return;
    busy.value = true;
    const parent = workspaceParentPath(row.path);
    if (remoteInterlinkActive.value) await remoteMutation('workspace.move', { path: row.path, destination: joinWorkspacePath(parent, nextName) });
    else await moveWorkspacePath(row.path, joinWorkspacePath(parent, nextName));
    if (!isCurrentWorkspace(request)) return;
    removePaths([row.path]);
    await refreshDirectory(parent);
    ElMessage.success(t('messenger.filesArea.renameSuccess'));
  } catch (error) {
    if (isCurrentWorkspace(request) && ((error as { response?: unknown })?.response || error instanceof Error)) {
      ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
    }
  } finally {
    if (isCurrentWorkspace(request)) busy.value = false;
  }
};

const confirmDelete = async (count: number, name?: string): Promise<boolean> => {
  const message = name
    ? t('messenger.filesArea.deleteConfirm', { name })
    : t('messenger.filesArea.batchDeleteConfirm', { count });
  return mzConfirm({
    title: t('common.delete'),
    message,
    confirmText: t('common.delete'),
    cancelText: t('common.cancel'),
    tone: 'danger'
  });
};

const deleteEntry = async (row: WorkspaceVisibleRow) => {
  const request = directoryAbort;
  if (!(await confirmDelete(1, row.name)) || !isCurrentWorkspace(request)) return;
  busy.value = true;
  try {
    if (remoteInterlinkActive.value) await remoteMutation('workspace.delete', { path: row.path });
    else await deleteWorkspacePath(row.path);
    if (!isCurrentWorkspace(request)) return;
    removePaths([row.path]);
    await refreshDirectory(workspaceParentPath(row.path));
    ElMessage.success(t('messenger.filesArea.deleteSuccess'));
  } catch (error) {
    if (isCurrentWorkspace(request)) ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    if (isCurrentWorkspace(request)) busy.value = false;
  }
};

const handleBatchDelete = async () => {
  const paths = selectedList.value.slice();
  if (!paths.length) return;
  if (!(await confirmDelete(paths.length))) return;
  busy.value = true;
  try {
    const outcome = await deleteWorkspacePaths(paths);
    removePaths(paths);
    exitSelectionMode();
    const directories = new Set(paths.map((path) => workspaceParentPath(path)));
    for (const directory of directories) {
      await refreshDirectory(directory);
    }
    if (outcome.ok) {
      ElMessage.success(t('messenger.filesArea.deleteSuccess'));
    } else {
      ElMessage.warning(
        t('messenger.filesArea.partialDelete', { done: outcome.succeeded, failed: outcome.failed })
      );
    }
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    busy.value = false;
  }
};

/** Bounded download fan-out (plan §6.6: download concurrency <= 3). */
const DOWNLOAD_CONCURRENCY = 3;

const downloadSelection = async (paths: string[]) => {
  const kindByPath = new Map(rows.value.map((row) => [row.path, row.kind]));
  let cursor = 0;
  let done = 0;
  const workers = Array.from({ length: Math.min(DOWNLOAD_CONCURRENCY, paths.length) }, async () => {
    for (;;) {
      const index = cursor;
      cursor += 1;
      const path = paths[index];
      if (!path) return;
      try {
        // Directories are packaged as a zip; files are streamed as-is.
        if (kindByPath.get(path) === 'dir') {
          await downloadWorkspaceDirectoryArchive(path);
        } else {
          await downloadWorkspacePath(path);
        }
        done += 1;
      } catch (error) {
        // Keep going; a single failure should not abort the whole batch.
      }
    }
  });
  await Promise.all(workers);
  return done;
};

const handleBatchArchive = async () => {
  const paths = selectedList.value.slice();
  if (!paths.length) return;
  busy.value = true;
  try {
    const done = await downloadSelection(paths);
    if (done > 0) {
      ElMessage.success(t('messenger.filesArea.batchDownloadDone', { count: done }));
    } else {
      ElMessage.error(t('messenger.filesArea.downloadFailed'));
    }
  } finally {
    busy.value = false;
  }
};

const quoteEntryToChat = (row: WorkspaceVisibleRow) => {
  const ok = queueWorkspaceChatReference({
    path: row.path
  });
  if (ok) {
    ElMessage.success(t('messenger.filesArea.quoteSuccess'));
  } else {
    ElMessage.warning(t('messenger.filesArea.quoteFailed'));
  }
};

// ------------------------------------------------------------------ preview

type PreviewKind = 'text' | 'html' | 'image' | 'svg' | 'pdf' | 'audio' | 'video' | 'model' | 'onlyoffice' | 'drawio' | 'unsupported';

const preview = reactive({
  visible: false,
  loading: false,
  title: '',
  path: '',
  metaLabel: '',
  hint: '',
  error: '',
  tooLarge: false,
  editable: false,
  saving: false,
  previewKind: 'text' as PreviewKind,
  previewUrl: '',
  content: ''
});

const officeDialog = reactive({ visible: false, path: '' });
const drawioDialog = reactive({ visible: false, path: '' });
let previewHtmlObjectUrls: string[] = [];

const releasePreviewUrl = () => {
  if (preview.previewUrl) {
    URL.revokeObjectURL(preview.previewUrl);
    preview.previewUrl = '';
  }
  previewHtmlObjectUrls.forEach((url) => URL.revokeObjectURL(url));
  previewHtmlObjectUrls = [];
};

let previewAbort: AbortController | null = null;
let previewSerial = 0;

const openPreview = async (path: string, name: string, size: number) => {
  if (remoteInterlinkActive.value) {
    await openRemoteFilePreview(path, name, size);
    return;
  }
  const kind = resolveWorkspaceResourcePreviewKind(name, 0) as PreviewKind;
  if (kind === 'onlyoffice') {
    officeDialog.path = path;
    officeDialog.visible = true;
    return;
  }
  if (kind === 'drawio') {
    drawioDialog.path = path;
    drawioDialog.visible = true;
    return;
  }

  releasePreviewUrl();
  Object.assign(preview, {
    visible: true,
    loading: true,
    title: name,
    path,
    metaLabel: `${path}  ·  ${formatWorkspaceBytes(size)}`,
    hint: '',
    error: '',
    tooLarge: false,
    editable: false,
    saving: false,
    previewKind: kind,
    previewUrl: '',
    content: ''
  });

  try {
    if (kind === 'text') {
      const extension = extractWorkspaceResourceExtension(name);
      if (HTML_PREVIEW_EXTENSIONS.has(extension)) {
        const source = await fetchWorkspacePathBlob(path);
        const previewDocument = await buildWorkspaceHtmlPreviewDocument({
          rawHtml: await source.blob.text(),
          entryPath: path,
          fetchResource: async (relativePath) => {
            const resource = await fetchWorkspacePathBlob(relativePath);
            return resource.blob;
          }
        });
        preview.previewKind = 'html';
        previewHtmlObjectUrls = previewDocument.objectUrls;
        preview.previewUrl = URL.createObjectURL(
          new Blob([previewDocument.html], { type: 'text/html; charset=utf-8' })
        );
        return;
      }
      if (size > WORKSPACE_TEXT_EDIT_MAX_BYTES) {
        preview.tooLarge = true;
        preview.hint = resolveWorkspacePreviewTooLargeHint();
        return;
      }
      const payload = await readWorkspaceFileContent(path, WORKSPACE_TEXT_EDIT_MAX_BYTES);
      if (payload.size > WORKSPACE_TEXT_EDIT_MAX_BYTES) {
        preview.tooLarge = true;
        preview.hint = resolveWorkspacePreviewTooLargeHint();
        return;
      }
      preview.content = payload.content;
      preview.editable = true;
      preview.metaLabel = `${path}  ·  ${formatWorkspaceBytes(payload.size || size)}`;
      return;
    }
    if (kind === 'unsupported') {
      preview.hint = t('workspace.preview.unsupportedHint');
      return;
    }
    const extension = name.split('.').pop()?.toLowerCase() || '';
    const blob = await fetchWorkspacePathBlob(path, {
      preview: METAFILE_EXTENSIONS.has(extension) ? 'png' : undefined
    });
    preview.previewUrl = URL.createObjectURL(blob.blob);
  } catch (error) {
    preview.error = resolveWorkspaceErrorMessage(error, t('messenger.filesArea.openFailed'));
  } finally {
    preview.loading = false;
  }
};

/**
 * 远程只读预览：内容走一条 L0 `workspace.read` 命令；编辑器类文件
 * （OnlyOffice/drawio 依赖云端文件端点）降级为提示。
 */
const openRemoteFilePreview = async (path: string, name: string, size: number) => {
  const blocked = readBlockedReason.value;
  if (blocked) {
    ElMessage.warning(blocked);
    return;
  }

  const extension = extractWorkspaceResourceExtension(name) || workspaceFileExtension(name);
  previewSerial += 1;
  const serial = previewSerial;
  releasePreviewUrl();
  Object.assign(preview, {
    visible: true,
    loading: true,
    title: name,
    path,
    metaLabel: `${path}  ·  ${formatWorkspaceBytes(size)}`,
    hint: '',
    error: '',
    tooLarge: false,
    editable: false,
    saving: false,
    previewKind: 'unsupported' as PreviewKind,
    previewUrl: '',
    content: ''
  });

  if (Number(size) > REMOTE_PREVIEW_MAX_BYTES) {
    preview.loading = false;
    preview.tooLarge = true;
    preview.hint = resolveWorkspacePreviewTooLargeHint();
    return;
  }

  const declaredKind = resolveWorkspaceResourcePreviewKind(name, size);
  const isEditorOnly = declaredKind === 'onlyoffice' || declaredKind === 'drawio';

  const controller = new AbortController();
  previewAbort = controller;
  const outcome = await readRemoteFile({
    to: interlinkTarget.value,
    workspaceId: interlinkWorkspaceId.value || undefined,
    path,
    signal: controller.signal
  });

  if (serial !== previewSerial) return;
  previewAbort = null;
  preview.loading = false;

  if (!outcome.payload) {
    preview.error = outcome.failure || t('messenger.filesArea.openFailed');
    preview.previewKind = 'unsupported';
    return;
  }

  if (outcome.payload.truncated) {
    preview.hint = t('interlink.workspace.truncatedHint', {
      size: formatWorkspaceBytes(outcome.payload.size)
    });
  }

  if (isEditorOnly) {
    preview.previewKind = 'unsupported';
    preview.hint = t('interlink.workspace.editorUnsupported');
    return;
  }

  if (declaredKind === 'text' || declaredKind === 'unsupported') {
    if (typeof outcome.payload.text === 'string') {
      preview.previewKind = 'text';
      preview.content = outcome.payload.text;
      preview.editable = remoteWritable.value && !outcome.payload.truncated;
      return;
    }
    preview.previewKind = 'unsupported';
    preview.hint = resolveWorkspacePreviewUnsupportedHint();
    return;
  }

  const bytes = outcome.payload.bytes;
  if (!bytes) {
    preview.error = t('messenger.filesArea.openFailed');
    preview.previewKind = 'unsupported';
    return;
  }

  const blob = normalizeWorkspacePreviewBlob(new Blob([bytes]), declaredKind, extension);
  preview.previewUrl = URL.createObjectURL(blob);
  preview.previewKind = declaredKind as PreviewKind;
};

/** Read a remote file once and save the exact bytes. A truncated command result
 * is never presented as a complete download. */
const downloadRemoteFile = async (path: string, name: string) => {
  const blocked = readBlockedReason.value;
  if (blocked) {
    ElMessage.warning(blocked);
    return;
  }
  busy.value = true;
  const request = directoryAbort;
  try {
    const outcome = await readRemoteFile({
      to: interlinkTarget.value,
      workspaceId: interlinkWorkspaceId.value || undefined,
      path,
      signal: request.signal
    });
    if (request !== directoryAbort || request.signal.aborted) return;
    if (!outcome.payload) throw new Error(outcome.failure || outcome.errorCode || 'READ_FAILED');
    if (outcome.payload.truncated) {
      ElMessage.warning(t('interlink.workspace.truncatedHint', { size: formatWorkspaceBytes(outcome.payload.size) }));
      return;
    }
    const blob = outcome.payload.bytes
      ? new Blob([outcome.payload.bytes])
      : new Blob([outcome.payload.text], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    saveObjectUrlAsFile(url, name || 'download');
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (error) {
    if (request === directoryAbort && !request.signal.aborted) {
      ElMessage.error(resolveWorkspaceErrorMessage(error, t('messenger.filesArea.downloadFailed')));
    }
  } finally {
    if (request === directoryAbort) busy.value = false;
  }
};

const closePreview = () => {
  if (preview.saving) return;
  previewAbort?.abort();
  previewAbort = null;
  previewSerial += 1;
  preview.visible = false;
  releasePreviewUrl();
};

const handlePreviewDownload = async () => {
  if (!preview.path) return;
  // 远程文件不落云端存储：已取回的内容直接另存为本地文件。
  if (remoteInterlinkActive.value) {
    if (preview.previewUrl) {
      saveObjectUrlAsFile(preview.previewUrl, preview.title || 'download');
      return;
    }
    if (preview.content) {
      const blob = new Blob([preview.content], { type: 'text/plain;charset=utf-8' });
      const textUrl = URL.createObjectURL(blob);
      saveObjectUrlAsFile(textUrl, preview.title || 'download');
      window.setTimeout(() => URL.revokeObjectURL(textUrl), 1000);
      return;
    }
    ElMessage.warning(t('interlink.workspace.downloadUnavailable'));
    return;
  }
  await runAction(() => downloadWorkspacePath(preview.path), t('messenger.filesArea.downloadFailed'));
};

const handlePreviewSave = async (content: string) => {
  if (!preview.path || preview.saving) return;
  const request = directoryAbort;
  preview.saving = true;
  try {
    if (remoteInterlinkActive.value) await remoteMutation('workspace.write', { path: preview.path, content });
    else await writeWorkspaceFileContent(preview.path, content);
    if (!isCurrentWorkspace(request)) return;
    preview.content = content;
    await refreshDirectory(workspaceParentPath(preview.path));
    ElMessage.success(t('messenger.filesArea.saved'));
  } catch (error) {
    if (isCurrentWorkspace(request)) ElMessage.error(resolveWorkspaceErrorMessage(error, t('messenger.filesArea.saveFailed')));
  } finally {
    if (isCurrentWorkspace(request)) preview.saving = false;
  }
};

const handleOfficeVisibleChange = (visible: boolean) => {
  officeDialog.visible = visible;
  if (!visible) officeDialog.path = '';
};

const handleDrawioVisibleChange = (visible: boolean) => {
  drawioDialog.visible = visible;
  if (!visible) drawioDialog.path = '';
};

const handleEditorSaved = async (payload?: { path?: string }) => {
  const path = String(payload?.path || officeDialog.path || drawioDialog.path || '');
  if (!path) return;
  await refreshDirectory(workspaceParentPath(path));
};

// ------------------------------------------------------------ clear workspace

const handleClearWorkspace = async () => {
  if (busy.value) return;
  const confirmed = await mzConfirm({
    title: t('messenger.filesArea.clear'),
    message: t('messenger.filesArea.clearWarning'),
    confirmText: t('common.clear'),
    cancelText: t('common.cancel'),
    tone: 'danger'
  });
  if (!confirmed) return;
  busy.value = true;
  try {
    await clearWorkspaceRoot();
    // Root and every expanded directory changed; reset to a single fresh load.
    tree.reset();
    uploadTarget.value = '';
    await loadDirectory('', { force: true });
    ElMessage.success(t('messenger.filesArea.clearSuccess'));
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    busy.value = false;
  }
};

// -------------------------------------------------------- cross-module wiring

/** Esc leaves search or selection mode from anywhere in the file area (plan §6.1). */
const handleWindowKeydown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape') return;
  if (searchOpen.value) {
    exitSearch();
    return;
  }
  if (selectionMode.value) exitSelectionMode();
};

// Timeline / patch labels (B3) can ask the file area to reveal a path.
watch(
  () => pendingWorkspaceReveal.value?.token,
  async () => {
    const request = pendingWorkspaceReveal.value;
    if (!request) return;
    // 远程投影里没有云端路径可定位，直接消费掉请求，不触发云端目录分页拉取。
    if (remoteInterlinkActive.value) {
      clearWorkspaceReveal(request.token);
      ElMessage.info(t('messenger.filesArea.revealMissing'));
      return;
    }
    const found = await revealPath(request.path);
    if (!found) ElMessage.info(t('messenger.filesArea.revealMissing'));
    clearWorkspaceReveal(request.token);
  }
);

onMounted(() => {
  window.addEventListener('keydown', handleWindowKeydown);
  workspaceRefreshStop = onWorkspaceRefresh(refreshFromWorkspaceEvent);
  if (remoteInterlinkActive.value) {
    void enterRemoteWorkspace();
    return;
  }
  void loadDirectory('');
});

// 目标工作区变化（设备或工作区参数任一变化）：重铺影子快照并尝试实时根目录。
watch(
  () =>
    remoteInterlinkActive.value
      ? `${remoteInterlinkDeviceId.value || ''}\n${interlinkWorkspaceId.value || ''}`
      : '',
  () => {
    directoryRefresh.reset();
    directoryAbort.controller.abort();
    directoryAbort = createAbortHost();
    shadowApi.reset();
    tree.reset();
    busy.value = false;
    uploadTarget.value = '';
    remoteOperationState.value = '';
    preview.saving = false;
    closePreview();
    officeDialog.visible = false;
    drawioDialog.visible = false;
    newFileDialog.visible = false;
    exitSearch();
    if (remoteInterlinkActive.value) {
      void enterRemoteWorkspace();
    } else {
      void loadDirectory('');
    }
  }
);

// A deep link may load before the device catalog, or the device may reconnect.
// Refresh only when listing becomes available; status polling must not rebuild
// a tree that is already usable.
watch(listBlockedReason, (reason, previous) => {
  if (remoteInterlinkActive.value && previous && !reason && !shadowApi.loading.value) {
    void tree.loadDirectory('', { force: true });
  }
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWindowKeydown);
  workspaceRefreshStop?.();
  workspaceRefreshStop = null;
  directoryRefresh.dispose();
  uploadQueue.dispose();
  clearSearchTimer();
  searchSerial += 1;
  searchAbort?.abort();
  searchAbort = null;
  previewAbort?.abort();
  previewAbort = null;
  directoryAbort.controller.abort();
  releasePreviewUrl();
});
</script>
