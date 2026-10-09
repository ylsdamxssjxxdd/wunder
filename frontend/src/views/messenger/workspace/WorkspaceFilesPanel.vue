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
      <span class="workspace-files-head-title">{{ t('messenger.filesArea.group') }}</span>
      <div v-if="statsVisible" class="workspace-files-usage" :title="statsTitle">
        <span class="workspace-files-usage-value">{{ t('messenger.filesArea.statsUsed', { size: statsUsedLabel }) }}</span>
        <span class="workspace-files-usage-sep">·</span>
        <span>{{ t('messenger.filesArea.statsFiles', { count: stats?.files || 0 }) }}</span>
        <!-- No quota is configured (quota_bytes === null): never fake a denominator. -->
        <span v-if="statsHasQuota" class="workspace-files-usage-track" aria-hidden="true">
          <span class="workspace-files-usage-bar" :style="{ width: `${statsRatio}%` }"></span>
        </span>
        <span v-if="stats?.truncated" class="workspace-files-usage-flag">
          {{ t('messenger.filesArea.statsTruncated') }}
        </span>
      </div>
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
      :rows="rows"
      :active-path="activePath"
      :highlighted-path="highlightedPath"
      :loading="rootLoading"
      :error="rootError"
      :selection-mode="selectionMode"
      :selected-paths="selectedList"
      @toggle-dir="handleToggleDirectory"
      @load-more="handleLoadMore"
      @activate="handleActivateRow"
      @open="handleOpenRow"
      @toggle-select="toggleSelection"
      @exit-selection="exitSelectionMode"
      @retry="handleRefresh"
      @command="handleRowCommand"
    />

    <div v-if="uploadTasks.length" class="workspace-files-uploads">
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
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { useI18n } from '@/i18n';
import {
  resolveWorkspacePreviewTooLargeHint,
  resolveWorkspaceResourcePreviewKind
} from '@/utils/workspaceResourcePreview';
import WorkspaceNewFileDialog, {
  type WorkspaceNewFileTemplate
} from '@/components/chat/WorkspaceNewFileDialog.vue';
import WorkspaceFilePreviewDialog from './WorkspaceFilePreviewDialog.vue';
import WorkspaceFileTree from './WorkspaceFileTree.vue';
import {
  clearWorkspaceRoot,
  createWorkspaceDirectory,
  createWorkspaceFile,
  deleteWorkspacePath,
  deleteWorkspacePaths,
  downloadWorkspaceDirectoryArchive,
  downloadWorkspacePath,
  fetchWorkspacePathBlob,
  fetchWorkspaceStatsSnapshot,
  isWorkspaceEndpointMissing,
  moveWorkspacePath,
  readWorkspaceFileContent,
  resolveWorkspaceErrorMessage,
  writeWorkspaceFileContent,
  type WorkspaceStatsSnapshot
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
  workspaceParentPath,
  type WorkspaceVisibleRow
} from './workspaceFileModel';
import { useWorkspaceFileTree } from './workspaceFileTree';
import { createWorkspaceUploadQueue, type WorkspaceUploadTask } from './workspaceUploadQueue';

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
const UPLOAD_MAX_FILES_PER_DROP = 200;
const STATS_REFRESH_DEBOUNCE_MS = 1500;
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

const tree = useWorkspaceFileTree({
  onError: (message) => {
    if (message) ElMessage.error(message);
  }
});
const {
  rows,
  activePath,
  highlightedPath,
  selectedList,
  selectionMode,
  toggleDirectory,
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
const busy = ref(false);
const refreshing = ref(false);
const dragActive = ref(false);
const newFileDialog = reactive({ visible: false, directory: '' });

const rootLoading = computed(() => Boolean(tree.directories.get('')?.loading));
const rootError = computed(() => String(tree.directories.get('')?.error || ''));

// ------------------------------------------------------------------ uploads

let statsTimer: number | null = null;

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
  scheduleStatsRefresh();
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

const handleDragEnter = () => {
  dragActive.value = true;
};

const handleDragOver = (event: DragEvent) => {
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

// -------------------------------------------------------------------- stats

const stats = ref<WorkspaceStatsSnapshot | null>(null);
const statsDisabled = ref(false);

const statsVisible = computed(() => !statsDisabled.value && Boolean(stats.value));
const statsHasQuota = computed(() => {
  const quota = Number(stats.value?.quotaBytes);
  return Number.isFinite(quota) && quota > 0;
});
const statsUsedLabel = computed(() => formatWorkspaceBytes(stats.value?.usedBytes || 0));
const statsRatio = computed(() => {
  if (!statsHasQuota.value) return 0;
  const quota = Number(stats.value?.quotaBytes);
  return Math.max(0, Math.min(100, Math.round(((stats.value?.usedBytes || 0) / quota) * 100)));
});
const statsTitle = computed(() => {
  if (!stats.value) return '';
  const parts = [t('messenger.filesArea.statsUsed', { size: statsUsedLabel.value })];
  parts.push(t('messenger.filesArea.statsFiles', { count: stats.value.files }));
  if (Number.isFinite(Number(stats.value.quotaBytes))) {
    parts.push(t('messenger.filesArea.statsQuota', { size: formatWorkspaceBytes(stats.value.quotaBytes) }));
  }
  if (stats.value.truncated) parts.push(t('messenger.filesArea.statsTruncated'));
  return parts.join('  ·  ');
});

const loadStats = async () => {
  if (statsDisabled.value) return;
  try {
    const snapshot = await fetchWorkspaceStatsSnapshot('');
    stats.value = snapshot;
  } catch (error) {
    // 404/501: the endpoint is not deployed — hide the bar for good, no retry loop.
    if (isWorkspaceEndpointMissing(error)) {
      statsDisabled.value = true;
      stats.value = null;
    }
  }
};

/** Coalesce stats refreshes; never more than one request per debounce window. */
const scheduleStatsRefresh = () => {
  if (statsDisabled.value) return;
  if (statsTimer) window.clearTimeout(statsTimer);
  statsTimer = window.setTimeout(() => {
    statsTimer = null;
    void loadStats();
  }, STATS_REFRESH_DEBOUNCE_MS);
};

// --------------------------------------------------------------- tree wiring

const handleActivateRow = (row: WorkspaceVisibleRow) => {
  setActivePath(row.path);
  if (row.kind === 'dir') {
    uploadTarget.value = row.path;
    void toggleDirectory(row.path);
    return;
  }
  uploadTarget.value = workspaceParentPath(row.path);
};

const handleOpenRow = (row: WorkspaceVisibleRow) => {
  if (row.kind === 'dir') {
    uploadTarget.value = row.path;
    void toggleDirectory(row.path);
    return;
  }
  void openPreview(row.path, row.name, row.size);
};

const handleToggleDirectory = (path: string) => {
  uploadTarget.value = normalizeWorkspaceRelativePath(path);
  void toggleDirectory(path);
};

const handleLoadMore = (path: string) => {
  void loadMore(path);
};

const handleRefresh = async () => {
  if (refreshing.value) return;
  refreshing.value = true;
  try {
    await Promise.all([refreshDirectory(''), loadStats()]);
    tree.expandedPaths.forEach((path) => {
      void refreshDirectory(path);
    });
  } finally {
    refreshing.value = false;
  }
};

/** Menu target: a directory row creates inside it, anything else uses the current folder. */
const createTargetPath = (row: WorkspaceVisibleRow | null): string =>
  row && row.kind === 'dir' ? row.path : uploadTarget.value;

const handleCreateEntry = async (kind: 'dir' | 'file', row: WorkspaceVisibleRow | null) => {
  if (busy.value) return;
  const directory = createTargetPath(row);
  if (kind === 'file') {
    // 文件类型交给专用对话框选择（文本 / Markdown / Word / 表格 / 演示文稿 / 流程图）。
    newFileDialog.directory = directory;
    newFileDialog.visible = true;
    return;
  }
  try {
    const { value } = await ElMessageBox.prompt(
      t('messenger.filesArea.newDirPrompt'),
      t('messenger.filesArea.newDir'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        inputPlaceholder: t('messenger.filesArea.namePlaceholder'),
        inputValidator: (input: string) =>
          isValidWorkspaceEntryName(input) ? true : t('messenger.filesArea.invalidName')
      }
    );
    const name = String(value || '').trim();
    if (!name) return;
    busy.value = true;
    await createWorkspaceDirectory(directory, name);
    await refreshDirectory(directory);
    await revealPath(joinWorkspacePath(directory, name));
    scheduleStatsRefresh();
    ElMessage.success(t('messenger.filesArea.newDirSuccess'));
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    if (error instanceof Error || (error as { response?: unknown })?.response) {
      ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
    }
  } finally {
    busy.value = false;
  }
};

/** 「新建文件」对话框确认：按所选类型写入带模板内容的新文件。 */
const handleNewFileConfirm = async (payload: { name: string; content: string; typeId: string }) => {
  const name = String(payload.name || '').trim();
  newFileDialog.visible = false;
  if (!name || !isValidWorkspaceEntryName(name)) {
    if (name) ElMessage.warning(t('messenger.filesArea.invalidName'));
    return;
  }
  const directory = newFileDialog.directory;
  busy.value = true;
  try {
    await createWorkspaceFile(directory, name, payload.content || '');
    await refreshDirectory(directory);
    await revealPath(joinWorkspacePath(directory, name));
    scheduleStatsRefresh();
    ElMessage.success(t('messenger.filesArea.newFileSuccess'));
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    busy.value = false;
  }
};

// -------------------------------------------------------------- row commands

const handleRowCommand = async (command: string, row: WorkspaceVisibleRow | null) => {
  // 空白区菜单没有宿主行：创建落在当前目录，其余三项承接自已下线的头部 ⋯。
  switch (command) {
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
      await toggleDirectory(row.path);
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
  try {
    const { value } = await ElMessageBox.prompt(t('messenger.filesArea.renamePrompt'), t('common.edit'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      inputValue: row.name,
      inputValidator: (input: string) =>
        isValidWorkspaceEntryName(input) ? true : t('messenger.filesArea.invalidName')
    });
    const nextName = String(value || '').trim();
    if (!nextName || nextName === row.name) return;
    busy.value = true;
    const parent = workspaceParentPath(row.path);
    await moveWorkspacePath(row.path, joinWorkspacePath(parent, nextName));
    removePaths([row.path]);
    await refreshDirectory(parent);
    ElMessage.success(t('messenger.filesArea.renameSuccess'));
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    if ((error as { response?: unknown })?.response || error instanceof Error) {
      ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
    }
  } finally {
    busy.value = false;
  }
};

const confirmDelete = async (count: number, name?: string) => {
  const message = name
    ? t('messenger.filesArea.deleteConfirm', { name })
    : t('messenger.filesArea.batchDeleteConfirm', { count });
  await ElMessageBox.confirm(message, t('common.delete'), {
    confirmButtonText: t('common.delete'),
    cancelButtonText: t('common.cancel'),
    type: 'warning',
    confirmButtonClass: 'el-button--danger'
  });
};

const deleteEntry = async (row: WorkspaceVisibleRow) => {
  try {
    await confirmDelete(1, row.name);
  } catch (error) {
    return;
  }
  busy.value = true;
  try {
    await deleteWorkspacePath(row.path);
    removePaths([row.path]);
    await refreshDirectory(workspaceParentPath(row.path));
    scheduleStatsRefresh();
    ElMessage.success(t('messenger.filesArea.deleteSuccess'));
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    busy.value = false;
  }
};

const handleBatchDelete = async () => {
  const paths = selectedList.value.slice();
  if (!paths.length) return;
  try {
    await confirmDelete(paths.length);
  } catch (error) {
    return;
  }
  busy.value = true;
  try {
    const outcome = await deleteWorkspacePaths(paths);
    removePaths(paths);
    exitSelectionMode();
    const directories = new Set(paths.map((path) => workspaceParentPath(path)));
    for (const directory of directories) {
      await refreshDirectory(directory);
    }
    scheduleStatsRefresh();
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
    path: row.path,
    name: row.name,
    isDir: row.kind === 'dir'
  });
  if (ok) {
    ElMessage.success(t('messenger.filesArea.quoteSuccess'));
  } else {
    ElMessage.warning(t('messenger.filesArea.quoteFailed'));
  }
};

// ------------------------------------------------------------------ preview

type PreviewKind = 'text' | 'image' | 'svg' | 'pdf' | 'audio' | 'video' | 'onlyoffice' | 'drawio' | 'unsupported';

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

const releasePreviewUrl = () => {
  if (preview.previewUrl) {
    URL.revokeObjectURL(preview.previewUrl);
    preview.previewUrl = '';
  }
};

const openPreview = async (path: string, name: string, size: number) => {
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

const closePreview = () => {
  if (preview.saving) return;
  preview.visible = false;
  releasePreviewUrl();
};

const handlePreviewDownload = async () => {
  if (!preview.path) return;
  await runAction(() => downloadWorkspacePath(preview.path), t('messenger.filesArea.downloadFailed'));
};

const handlePreviewSave = async (content: string) => {
  if (!preview.path || preview.saving) return;
  preview.saving = true;
  try {
    await writeWorkspaceFileContent(preview.path, content);
    preview.content = content;
    await refreshDirectory(workspaceParentPath(preview.path));
    ElMessage.success(t('messenger.filesArea.saved'));
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('messenger.filesArea.saveFailed')));
  } finally {
    preview.saving = false;
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
  scheduleStatsRefresh();
};

// ------------------------------------------------------------ clear workspace

const handleClearWorkspace = async () => {
  const phrase = t('messenger.filesArea.clearPhrase');
  try {
    await ElMessageBox.prompt(
      `${t('messenger.filesArea.clearWarning')} ${t('messenger.filesArea.clearPrompt', { phrase })}`,
      t('messenger.filesArea.clear'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        type: 'warning',
        inputPlaceholder: phrase,
        inputValidator: (input: string) =>
          String(input || '').trim() === phrase ? true : t('messenger.filesArea.clearMismatch')
      }
    );
  } catch (error) {
    return;
  }
  busy.value = true;
  try {
    await clearWorkspaceRoot();
    // Root and every expanded directory changed; reset to a single fresh load.
    tree.reset();
    uploadTarget.value = '';
    await Promise.all([loadDirectory('', { force: true }), loadStats()]);
    ElMessage.success(t('messenger.filesArea.clearSuccess'));
  } catch (error) {
    ElMessage.error(resolveWorkspaceErrorMessage(error, t('common.requestFailed')));
  } finally {
    busy.value = false;
  }
};

// -------------------------------------------------------- cross-module wiring

/** Esc leaves selection mode from anywhere in the file area (plan §6.1). */
const handleWindowKeydown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape') return;
  if (selectionMode.value) exitSelectionMode();
};

// Timeline / patch labels (B3) can ask the file area to reveal a path.
watch(
  () => pendingWorkspaceReveal.value?.token,
  async () => {
    const request = pendingWorkspaceReveal.value;
    if (!request) return;
    const found = await revealPath(request.path);
    if (!found) ElMessage.info(t('messenger.filesArea.revealMissing'));
    clearWorkspaceReveal(request.token);
  }
);

onMounted(() => {
  window.addEventListener('keydown', handleWindowKeydown);
  void loadDirectory('');
  void loadStats();
});

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleWindowKeydown);
  if (statsTimer) window.clearTimeout(statsTimer);
  statsTimer = null;
  uploadQueue.dispose();
  releasePreviewUrl();
});
</script>
