<template>
  <div class="workspace-panel">
    <div class="workspace-header">
      <div class="workspace-title-row">
        <div class="workspace-title">{{ panelTitle }}</div>
        <div v-if="showContainerId" class="workspace-container-id">{{ normalizedContainerId }}</div>
      </div>
    </div>

    <div
      :class="[
        'workspace-upload-progress',
        { active: uploadProgress.active, indeterminate: uploadProgress.indeterminate }
      ]"
      aria-live="polite"
    >
      <div class="workspace-upload-bar">
        <div class="workspace-upload-bar-fill" :style="uploadProgressBarStyle"></div>
      </div>
      <div class="workspace-upload-text">{{ uploadProgressText }}</div>
    </div>

    <div
      ref="listRef"
      :class="[
        'workspace-list',
        {
          dragover: draggingOver,
          virtual: workspaceVirtual,
          refreshing: loading && displayEntries.length > 0
        }
      ]"
      @scroll="handleListScroll"
      @dragenter="handleListDragEnter"
      @dragover="handleListDragOver"
      @dragleave="handleListDragLeave"
      @drop="handleListDrop"
      @contextmenu.prevent="openContextMenu($event, null)"
    >
      <div v-if="loading && displayEntries.length === 0" class="workspace-skeleton" aria-hidden="true"></div>
      <div v-else-if="displayEntries.length === 0" class="workspace-empty">{{ emptyText }}</div>
      <template v-else>
        <div
          v-if="workspacePaddingTop"
          class="workspace-spacer"
          :style="{ height: `${workspacePaddingTop}px` }"
        ></div>
        <div
          v-for="item in workspaceEntries"
          :key="item.entry.path"
          :class="[
            'workspace-item',
            item.entry.type === 'dir' ? 'is-folder' : '',
            state.selectedPaths.has(item.entry.path) ? 'is-selected' : '',
            selectedEntry && selectedEntry.path === item.entry.path ? 'active' : ''
          ]"
          :style="{ '--workspace-indent': `${item.depth * 16}px` }"
          draggable="true"
          @click="handleWorkspaceItemClick($event, item.entry)"
          @dblclick="handleWorkspaceItemDoubleClick(item.entry)"
          @contextmenu.prevent.stop="openContextMenu($event, item.entry)"
          @dragstart="handleItemDragStart($event, item.entry)"
          @dragend="handleItemDragEnd"
          @dragenter="handleItemDragEnter($event, item.entry)"
          @dragover="handleItemDragOver($event, item.entry)"
          @dragleave="handleItemDragLeave($event, item.entry)"
          @drop="handleItemDrop($event, item.entry)"
        >
          <div class="workspace-item-main">
            <button
              class="workspace-item-caret"
              :class="{
                hidden: !isTreeView || item.entry.type !== 'dir',
                expanded: state.expanded.has(item.entry.path)
              }"
              type="button"
              :aria-label="t('workspace.action.expandDir')"
              @click.stop="toggleWorkspaceDirectory(item.entry)"
            >
              <i class="fa-solid fa-chevron-right workspace-caret-icon" aria-hidden="true"></i>
            </button>
            <span :class="['workspace-item-icon', item.icon.className]" :title="item.icon.label">
              <img
                class="workspace-item-icon-img"
                :src="item.icon.icon"
                :alt="item.icon.label"
              />
            </span>
            <div
              class="workspace-item-name"
              :title="state.renamingPath === item.entry.path ? '' : item.entry.name"
            >
              <input
                v-if="state.renamingPath === item.entry.path"
                v-model="state.renamingValue"
                class="workspace-item-rename"
                type="text"
                :data-rename-path="item.entry.path"
                @click.stop
                @keydown.enter.prevent="finishWorkspaceRename(item.entry, state.renamingValue)"
                @keydown.esc.prevent="cancelWorkspaceRename"
                @blur="finishWorkspaceRename(item.entry, state.renamingValue)"
              />
              <span v-else>{{ item.entry.name }}</span>
            </div>
          </div>
          <div class="workspace-item-meta">{{ getEntryMeta(item.entry) }}</div>
        </div>
        <div
          v-if="workspacePaddingBottom"
          class="workspace-spacer"
          :style="{ height: `${workspacePaddingBottom}px` }"
        ></div>
      </template>
    </div>

    <div class="workspace-toolbar workspace-toolbar-bottom">
      <div class="workspace-search">
        <i class="fa-solid fa-magnifying-glass workspace-search-icon" aria-hidden="true"></i>
        <input
          v-model="searchKeyword"
          type="text"
          :placeholder="t('workspace.search.placeholder')"
          @input="handleSearchInput"
          @keydown="handleSearchKeydown"
        />
      </div>
      <div class="workspace-selection-meta muted">{{ selectionMeta }}</div>
    </div>

    <input ref="uploadInputRef" type="file" multiple style="display: none" @change="handleUploadInput" />

    <Teleport to="body">
      <div
        v-show="contextMenu.visible"
        ref="menuRef"
        class="workspace-context-menu"
        :style="menuStyle"
        @contextmenu.prevent
      >
        <button class="workspace-menu-btn" @click="handleNewFile">
          {{ t('workspace.menu.newFile') }}
        </button>
        <button class="workspace-menu-btn" @click="handleContextMenuUpload">
          {{ t('common.upload') }}
        </button>
        <button class="workspace-menu-btn" @click="handleContextMenuRefresh">
          {{ t('common.refresh') }}
        </button>
        <button class="workspace-menu-btn" :disabled="!contextMenuCanEdit" @click="handleEdit">
          {{ t('common.edit') }}
        </button>
        <button class="workspace-menu-btn" :disabled="!contextMenuSingleEntry" @click="handleRename">
          {{ t('workspace.menu.rename') }}
        </button>
        <button class="workspace-menu-btn" @click="handleNewFolder">
          {{ t('workspace.menu.newFolder') }}
        </button>
        <button class="workspace-menu-btn" :disabled="!contextMenuSingleEntry" @click="handleDownload">
          {{ resourceActionLabel }}
        </button>
        <button class="workspace-menu-btn danger" :disabled="!contextMenuHasSelection" @click="handleDelete">
          {{ t('common.delete') }}
        </button>
      </div>
    </Teleport>

    <el-dialog
      v-model="preview.visible"
      :title="t('workspace.preview.dialogTitle')"
      width="720px"
      class="workspace-dialog"
      append-to-body
    >
      <div class="workspace-preview-title">
        {{ preview.entry?.name || t('workspace.preview.dialogTitle') }}
      </div>
      <div class="workspace-preview-meta">{{ previewMeta }}</div>
      <div v-if="preview.hint" class="workspace-preview-hint">{{ preview.hint }}</div>
      <div class="workspace-preview" :class="{ embed: preview.embed, 'is-svg': preview.type === 'svg' }">
        <div v-if="preview.loading" class="workspace-empty">
          {{ t('workspace.preview.loading') }}
        </div>
        <template v-else>
          <ZoomableImagePreview
            v-if="preview.embed && preview.type === 'image'"
            :image-url="preview.url"
            :alt="preview.entry?.name || t('workspace.preview.dialogTitle')"
            :active="preview.visible"
          />
          <iframe v-else-if="preview.embed && (preview.type === 'pdf' || preview.type === 'svg')" :src="preview.url" />
          <pre v-else class="workspace-preview-text">{{ preview.content }}</pre>
        </template>
      </div>
      <template #footer>
        <button class="workspace-btn secondary" @click="downloadPreview">
          {{ resourceActionLabel }}
        </button>
        <button class="workspace-btn secondary" @click="closePreview">
          {{ t('common.close') }}
        </button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="editor.visible"
      :title="t('workspace.editor.dialogTitle')"
      width="720px"
      class="workspace-dialog"
      append-to-body
    >
      <div class="workspace-preview-title">
        {{ editor.entry?.name || t('workspace.editor.dialogTitle') }}
      </div>
      <div class="workspace-preview-meta">{{ editor.entry?.path || '' }}</div>
      <textarea
        v-model="editor.content"
        class="workspace-editor-text"
        :disabled="editor.loading"
        :placeholder="t('common.loading')"
      />
      <template #footer>
        <button class="workspace-btn secondary" @click="closeEditor">{{ t('common.close') }}</button>
        <button class="workspace-btn" :disabled="editor.loading" @click="saveEditor">
          {{ t('common.save') }}
        </button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue';
import type { WorkspaceThemeIconResolver } from './workspaceIcons';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  batchWunderWorkspaceAction,
  createWunderWorkspaceDir,
  downloadWunderWorkspaceArchive,
  downloadWunderWorkspaceFile,
  fetchWunderWorkspaceContent,
  moveWunderWorkspaceEntry,
  saveWunderWorkspaceFile,
  searchWunderWorkspace,
  uploadWunderWorkspace
} from '@/api/workspace';
import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
import { isDesktopLocalModeEnabled } from '@/config/desktop';
import { emitWorkspaceRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';
import {
  buildWorkspaceTreeCacheKey,
  cloneWorkspaceEntries,
  normalizeWorkspacePath,
  readWorkspaceTreeCache,
  writeWorkspaceTreeCache
} from '@/utils/workspaceTreeCache';
import { chatPerf } from '@/utils/chatPerf';

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  },
  containerId: {
    type: [Number, String],
    default: 1
  },
  title: {
    type: String,
    default: ''
  },
  showContainerId: {
    type: Boolean,
    default: true
  },
  emptyText: {
    type: String,
    default: ''
  }
});

const emit = defineEmits<{
  (event: 'stats', payload: { latestUpdatedAt: number; entryCount: number }): void;
}>();

const { t } = useI18n();
const panelTitle = computed(() => props.title || t('workspace.title'));
const showContainerId = computed(() => props.showContainerId);
const desktopLocalMode = computed(() => isDesktopLocalModeEnabled());
const resourceActionLabel = computed(() =>
  desktopLocalMode.value ? t('workspace.action.exportCopy') : t('common.download')
);

const TEXT_EXTENSIONS = new Set([
  'txt',
  'md',
  'log',
  'json',
  'yaml',
  'yml',
  'toml',
  'ini',
  'xml',
  'csv',
  'tsv',
  'py',
  'js',
  'ts',
  'css',
  'html',
  'htm',
  'sh',
  'bat',
  'ps1',
  'sql'
]);
const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);
const IMAGE_MIME_TYPES = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  bmp: 'image/bmp',
  webp: 'image/webp',
  svg: 'image/svg+xml'
};
const PDF_EXTENSIONS = new Set(['pdf']);
const OFFICE_WORD_EXTENSIONS = new Set(['doc', 'docx']);
const OFFICE_EXCEL_EXTENSIONS = new Set(['xls', 'xlsx']);
const OFFICE_PPT_EXTENSIONS = new Set(['ppt', 'pptx']);
const OFFICE_EXTENSIONS = new Set(['doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx']);
const CODE_EXTENSIONS = new Set(['py', 'js', 'ts', 'css', 'html', 'htm', 'sh', 'bat', 'ps1', 'sql']);
const ARCHIVE_EXTENSIONS = new Set(['zip', 'rar', '7z', 'tar', 'gz', 'bz2']);
const AUDIO_EXTENSIONS = new Set(['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a']);
const VIDEO_EXTENSIONS = new Set(['mp4', 'mov', 'avi', 'mkv', 'webm']);
const WORKSPACE_DOC_ICON_BASE = `${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}doc-icons`;
const WORKSPACE_FOLDER_ICON = `${WORKSPACE_DOC_ICON_BASE}/folder.png`;
const WORKSPACE_DEFAULT_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/other.png`;
const WORKSPACE_TEXT_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/txt.png`;
const WORKSPACE_HTML_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/html.png`;
const WORKSPACE_PDF_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/pdf.png`;
const WORKSPACE_WORD_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/docx.png`;
const WORKSPACE_EXCEL_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/xlsx.png`;
const WORKSPACE_PPT_FILE_ICON = `${WORKSPACE_DOC_ICON_BASE}/pptx.png`;
const WORKSPACE_ICON_IDLE_TIMEOUT = 1200;
const MAX_TEXT_PREVIEW_SIZE = 512 * 1024;
// 沙盒容器上传总大小上限（对齐 Wunder 配置）
const MAX_WORKSPACE_UPLOAD_BYTES = 200 * 1024 * 1024;
const WORKSPACE_DRAG_KEY = 'application/x-wunder-workspace-entry';
const WORKSPACE_SEARCH_DEBOUNCE_MS = 300;
const WORKSPACE_AUTO_REFRESH_DEBOUNCE_MS = 400;
const WORKSPACE_INCREMENTAL_REFRESH_MAX_TARGETS = 6;
const WORKSPACE_INCREMENTAL_REFRESH_MAX_BATCH = 3;

type UploadProgressOptions = {
  percent?: number;
  loaded?: number;
  total?: number;
  indeterminate?: boolean;
};

type PromptInputOptions = {
  title?: string;
  placeholder?: string;
  defaultValue?: string;
};

type WorkspaceUploadOptions = {
  refreshTree?: boolean;
  relativePaths?: string[];
};

type DirectoryReaderLike = {
  readEntries: (
    successCallback: (entries: FileSystemEntryLike[]) => void,
    errorCallback?: (reason: DOMException) => void
  ) => void;
};

type FileSystemEntryLike = {
  isFile?: boolean;
  isDirectory?: boolean;
  name?: string;
  fullPath?: string;
  file?: (successCallback: (file: File) => void, errorCallback?: (reason: DOMException) => void) => void;
  createReader?: () => DirectoryReaderLike;
};

type DataTransferItemLike = DataTransferItem & {
  webkitGetAsEntry?: () => FileSystemEntryLike | null;
};

type WorkspaceDroppedFile = {
  file: File;
  relativePath: string;
};

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const normalizedContainerId = computed(() => {
  const parsed = Number.parseInt(String(props.containerId ?? ''), 10);
  if (!Number.isFinite(parsed)) return 0;
  return Math.min(10, Math.max(0, parsed));
});

const withAgentParams = (params = {}) => {
  const agentId = normalizedAgentId.value;
  const next = { ...params, container_id: normalizedContainerId.value };
  if (!agentId) return next;
  return { ...next, agent_id: agentId };
};

const appendAgentId = (formData) => {
  const agentId = normalizedAgentId.value;
  if (agentId) {
    formData.append('agent_id', agentId);
  }
  formData.append('container_id', String(normalizedContainerId.value));
};

const listRef = ref(null);
const uploadInputRef = ref(null);
const menuRef = ref(null);
const listScrollTop = ref(0);
// 上传进度条状态
const uploadProgress = reactive({
  active: false,
  indeterminate: false,
  percent: 0,
  loaded: 0,
  total: 0
});
let uploadProgressCount = 0;

const state = reactive({
  path: '',
  parent: null,
  entries: [],
  expanded: new Set(),
  selectedPaths: new Set(),
  lastSelectedPath: '',
  selected: null,
  searchKeyword: '',
  searchMode: false,
  sortBy: 'name',
  sortOrder: 'asc',
  renamingPath: '',
  renamingValue: '',
  loading: false,
  visualLoading: false,
  draggingOver: false,
  preview: {
    visible: false,
    entry: null,
    content: '',
    hint: '',
    loading: false,
    embed: false,
    type: '',
    url: ''
  },
  editor: {
    visible: false,
    entry: null,
    content: '',
    loading: false
  },
  contextMenu: {
    visible: false,
    x: 0,
    y: 0,
    primaryPath: '',
    selectionPaths: []
  }
});

const displayPath = computed(() => (state.path ? `/${state.path}` : '/'));
const canGoUp = computed(() => Boolean(state.path));
const selectedEntry = computed(() => state.selected);
const loading = computed(() => state.visualLoading);
const draggingOver = computed(() => state.draggingOver);
const preview = computed(() => state.preview);
const editor = computed(() => state.editor);
const contextMenu = computed(() => state.contextMenu);
const selectedCount = computed(() => state.selectedPaths.size);
const selectionMeta = computed(() =>
  selectedCount.value ? t('workspace.selection', { count: selectedCount.value }) : ''
);
const emptyText = computed(() => {
  if (state.searchMode) return t('workspace.empty.search');
  if (props.emptyText) return props.emptyText;
  return desktopLocalMode.value ? t('workspace.emptyPermanent') : t('workspace.empty');
});
const searchKeyword = computed({
  get: () => state.searchKeyword,
  set: (value) => {
    state.searchKeyword = value;
  }
});
const isTreeView = computed(() => !state.searchMode);
const displayEntries = computed(() => {
  const result = [];
  const walk = (entries, depth) => {
    entries.forEach((entry) => {
      result.push({ entry, depth });
      if (
        !state.searchMode &&
        entry.type === 'dir' &&
        state.expanded.has(entry.path) &&
        Array.isArray(entry.children) &&
        entry.children.length
      ) {
        walk(entry.children, depth + 1);
      }
    });
  };
  if (Array.isArray(state.entries)) {
    walk(state.entries, 0);
  }
  return result;
});
const WORKSPACE_ROW_HEIGHT = 28;
const WORKSPACE_OVERSCAN = 8;
const workspaceVirtual = computed(() => displayEntries.value.length > 120);
const workspaceViewportHeight = computed(() => listRef.value?.clientHeight || 0);
const workspaceVisibleCount = computed(() =>
  Math.max(1, Math.ceil(workspaceViewportHeight.value / WORKSPACE_ROW_HEIGHT))
);
const workspaceStartIndex = computed(() => {
  if (!workspaceVirtual.value) return 0;
  const raw = Math.floor(listScrollTop.value / WORKSPACE_ROW_HEIGHT) - WORKSPACE_OVERSCAN;
  const maxStart = Math.max(0, displayEntries.value.length - workspaceVisibleCount.value);
  return Math.max(0, Math.min(raw, maxStart));
});
const workspaceEndIndex = computed(() => {
  if (!workspaceVirtual.value) return displayEntries.value.length;
  return Math.min(
    displayEntries.value.length,
    workspaceStartIndex.value + workspaceVisibleCount.value + WORKSPACE_OVERSCAN * 2
  );
});
const workspaceEntries = computed(() =>
  (workspaceVirtual.value
    ? displayEntries.value.slice(workspaceStartIndex.value, workspaceEndIndex.value)
    : displayEntries.value
  ).map((item) => ({
    ...item,
    icon: getEntryIcon(item.entry)
  }))
);
const workspacePaddingTop = computed(() =>
  workspaceVirtual.value ? workspaceStartIndex.value * WORKSPACE_ROW_HEIGHT : 0
);
const workspacePaddingBottom = computed(() =>
  workspaceVirtual.value
    ? Math.max(0, (displayEntries.value.length - workspaceEndIndex.value) * WORKSPACE_ROW_HEIGHT)
    : 0
);
// Keep the virtual spacer aligned with the real scroll container after remounts,
// layout/theme changes, and incremental refreshes.
const syncWorkspaceListViewport = async ({ reset = false } = {}) => {
  await nextTick();
  const listElement = listRef.value;
  if (!listElement) {
    listScrollTop.value = 0;
    return;
  }
  if (reset && listElement.scrollTop !== 0) {
    listElement.scrollTop = 0;
  }
  const maxScrollTop = Math.max(0, listElement.scrollHeight - listElement.clientHeight);
  if (listElement.scrollTop > maxScrollTop) {
    listElement.scrollTop = maxScrollTop;
  }
  listScrollTop.value = Math.max(0, listElement.scrollTop || 0);
};
const flatEntries = computed(() => displayEntries.value.map((item) => item.entry));
const singleSelectedEntry = computed(() => {
  if (state.selectedPaths.size !== 1) {
    return null;
  }
  const [path] = Array.from(state.selectedPaths);
  return findWorkspaceEntry(state.entries, path) || state.selected;
});
const contextMenuSelectionPaths = computed(() => {
  if (Array.isArray(state.contextMenu.selectionPaths) && state.contextMenu.selectionPaths.length) {
    return state.contextMenu.selectionPaths.filter((path) => Boolean(path));
  }
  return Array.from(state.selectedPaths);
});
const contextMenuSingleEntry = computed(() => {
  if (contextMenuSelectionPaths.value.length === 1) {
    const [path] = contextMenuSelectionPaths.value;
    return findWorkspaceEntry(state.entries, path) || state.selected;
  }
  if (state.contextMenu.primaryPath) {
    return findWorkspaceEntry(state.entries, state.contextMenu.primaryPath) || state.selected;
  }
  return null;
});
const contextMenuHasSelection = computed(() => contextMenuSelectionPaths.value.length > 0);
const contextMenuCanEdit = computed(
  () => contextMenuSingleEntry.value && isWorkspaceTextEditable(contextMenuSingleEntry.value)
);
const menuStyle = computed(() => ({ left: `${state.contextMenu.x}px`, top: `${state.contextMenu.y}px` }));
const uploadProgressText = computed(() => {
  if (!uploadProgress.active) return '';
  const baseLabel = t('common.upload');
  const hasTotal = Number.isFinite(uploadProgress.total) && uploadProgress.total > 0;
  const hasLoaded = Number.isFinite(uploadProgress.loaded) && uploadProgress.loaded > 0;
  if (hasTotal) {
    const safePercent = Math.max(
      0,
      Math.min(100, Math.round((uploadProgress.loaded / uploadProgress.total) * 100))
    );
    return t('workspace.upload.progress.full', {
      label: baseLabel,
      percent: safePercent,
      loaded: formatBytes(uploadProgress.loaded),
      total: formatBytes(uploadProgress.total)
    });
  }
  if (hasLoaded) {
    return t('workspace.upload.progress.partial', {
      label: baseLabel,
      loaded: formatBytes(uploadProgress.loaded)
    });
  }
  return t('workspace.upload.progress.loading', { label: baseLabel });
});
const uploadProgressBarStyle = computed(() => {
  if (!uploadProgress.active) {
    return { width: '0%' };
  }
  if (uploadProgress.indeterminate) {
    return { width: '30%' };
  }
  const safePercent = Math.max(0, Math.min(100, Number(uploadProgress.percent) || 0));
  return { width: `${safePercent}%` };
});

const previewMeta = computed(() => {
  const entry = state.preview.entry;
  if (!entry) return '';
  const parts = [];
  if (entry.path) parts.push(entry.path);
  if (Number.isFinite(entry.size)) parts.push(formatBytes(entry.size));
  if (entry.updated_time) {
    const updated = new Date(entry.updated_time);
    if (!Number.isNaN(updated.getTime())) {
      parts.push(updated.toLocaleString());
    }
  }
  return parts.join(' · ');
});

let searchTimer = null;
let autoRefreshTimer = null;
let autoRefreshPending = false;
let autoRefreshForceFullReload = false;
let autoRefreshTargetPaths = new Set<string>();
let latestWorkspaceTreeVersion = 0;
let stopWorkspaceRefreshListener = null;
const workspacePanelRefreshSourceId = `workspace-panel-${Math.random().toString(36).slice(2, 10)}`;
const workspaceThemeIconResolver = shallowRef<WorkspaceThemeIconResolver | null>(null);
let workspaceThemeIconResolverPromise: Promise<WorkspaceThemeIconResolver | null> | null = null;
let workspaceThemeIconWarmupHandle: number | null = null;
let workspaceThemeIconWarmupUsesIdleCallback = false;

const joinWorkspacePath = (basePath, name) =>
  normalizeWorkspacePath([basePath, name].filter(Boolean).join('/'));

const getWorkspaceExtension = (entry) => {
  const rawName = String(entry?.name || entry?.path || '');
  const baseName = rawName.split('/').pop().split('\\').pop();
  const dotIndex = baseName.lastIndexOf('.');
  if (dotIndex === -1 || dotIndex === baseName.length - 1) return '';
  return baseName.slice(dotIndex + 1).toLowerCase();
};

const getWorkspaceParentPath = (path) => {
  const normalized = normalizeWorkspacePath(path);
  if (!normalized) return '';
  const parts = normalized.split('/').filter(Boolean);
  parts.pop();
  return parts.join('/');
};

const normalizeWorkspaceEventPathValue = (value) => {
  const text = String(value || '').trim();
  if (!text || text === '/' || text === '.') return '';
  const normalized = normalizeWorkspacePath(text);
  return normalized === '.' ? '' : normalized;
};

const normalizeWorkspaceTreeVersion = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const normalizeWorkspaceEventPaths = (detail) => {
  if (!detail || typeof detail !== 'object') return [];
  const source = detail as Record<string, unknown>;
  const pathKeys = [
    'path',
    'paths',
    'changed_paths',
    'changedPaths',
    'target_path',
    'targetPath',
    'source_path',
    'sourcePath',
    'destination',
    'destination_path',
    'destinationPath',
    'file',
    'files'
  ];
  const result = new Set<string>();

  const appendPathLike = (value: unknown) => {
    if (value === null || value === undefined) return;
    if (Array.isArray(value)) {
      value.forEach((item) => appendPathLike(item));
      return;
    }
    if (typeof value === 'string') {
      const normalized = normalizeWorkspaceEventPathValue(value);
      if (normalized || value.trim() === '/' || value.trim() === '.') {
        result.add(normalized);
      }
      return;
    }
    if (typeof value === 'object') {
      const record = value as Record<string, unknown>;
      appendPathLike(
        record.path ??
        record.relative_path ??
        record.relativePath ??
        record.target_path ??
        record.targetPath ??
        record.source_path ??
        record.sourcePath ??
        record.destination ??
        record.destination_path ??
        record.destinationPath
      );
    }
  };

  pathKeys.forEach((key) => appendPathLike(source[key]));
  if (source.data && typeof source.data === 'object') {
    const nested = source.data as Record<string, unknown>;
    pathKeys.forEach((key) => appendPathLike(nested[key]));
  }

  return Array.from(result);
};

const nowPerf = () =>
  typeof performance !== 'undefined' && typeof performance.now === 'function'
    ? performance.now()
    : Date.now();

const isValidWorkspaceName = (value) => {
  const trimmed = String(value || '').trim();
  if (!trimmed) return false;
  if (trimmed === '.' || trimmed === '..') return false;
  return !(/[\\/]/.test(trimmed));
};

const isValidWorkspacePath = (value) => {
  const normalized = normalizeWorkspacePath(value);
  if (!normalized) return true;
  return normalized.split('/').filter(Boolean).every(isValidWorkspaceName);
};

const isWorkspaceTextEditable = (entry) => {
  if (!entry || entry.type !== 'file') return false;
  const extension = getWorkspaceExtension(entry);
  if (!TEXT_EXTENSIONS.has(extension)) return false;
  const sizeValue = Number.isFinite(entry.size) ? entry.size : 0;
  return sizeValue <= MAX_TEXT_PREVIEW_SIZE;
};

const resolveWorkspaceFallbackFileIcon = (extension) => {
  if (PDF_EXTENSIONS.has(extension)) {
    return WORKSPACE_PDF_FILE_ICON;
  }
  if (OFFICE_WORD_EXTENSIONS.has(extension)) {
    return WORKSPACE_WORD_FILE_ICON;
  }
  if (OFFICE_EXCEL_EXTENSIONS.has(extension)) {
    return WORKSPACE_EXCEL_FILE_ICON;
  }
  if (OFFICE_PPT_EXTENSIONS.has(extension)) {
    return WORKSPACE_PPT_FILE_ICON;
  }
  if (extension === 'html' || extension === 'htm' || extension === 'xhtml') {
    return WORKSPACE_HTML_FILE_ICON;
  }
  if (TEXT_EXTENSIONS.has(extension) || CODE_EXTENSIONS.has(extension)) {
    return WORKSPACE_TEXT_FILE_ICON;
  }
  return WORKSPACE_DEFAULT_FILE_ICON;
};

const loadWorkspaceThemeIconsInBackground = async () => {
  if (workspaceThemeIconResolver.value) {
    return workspaceThemeIconResolver.value;
  }
  workspaceThemeIconResolverPromise ??= import('./workspaceIcons')
    .then((module) => module.loadWorkspaceThemeIconResolver())
    .then((resolver) => {
      workspaceThemeIconResolver.value = resolver;
      return resolver;
    })
    .catch((error) => {
      console.warn('[workspace] failed to warmup theme icons', error);
      return null;
    });
  return workspaceThemeIconResolverPromise;
};

const cancelWorkspaceThemeIconWarmup = () => {
  if (workspaceThemeIconWarmupHandle === null || typeof window === 'undefined') {
    return;
  }
  if (workspaceThemeIconWarmupUsesIdleCallback && typeof window.cancelIdleCallback === 'function') {
    window.cancelIdleCallback(workspaceThemeIconWarmupHandle);
  } else {
    window.clearTimeout(workspaceThemeIconWarmupHandle);
  }
  workspaceThemeIconWarmupHandle = null;
  workspaceThemeIconWarmupUsesIdleCallback = false;
};

const scheduleWorkspaceThemeIconWarmup = () => {
  if (workspaceThemeIconResolver.value || workspaceThemeIconResolverPromise) {
    return;
  }
  cancelWorkspaceThemeIconWarmup();
  const runWarmup = () => {
    workspaceThemeIconWarmupHandle = null;
    workspaceThemeIconWarmupUsesIdleCallback = false;
    void loadWorkspaceThemeIconsInBackground();
  };
  if (typeof window === 'undefined') {
    runWarmup();
    return;
  }
  if (typeof window.requestIdleCallback === 'function') {
    workspaceThemeIconWarmupUsesIdleCallback = true;
    workspaceThemeIconWarmupHandle = window.requestIdleCallback(runWarmup, {
      timeout: WORKSPACE_ICON_IDLE_TIMEOUT
    });
    return;
  }
  workspaceThemeIconWarmupHandle = window.setTimeout(runWarmup, 16);
};

const getEntryIcon = (entry) => {
  if (entry.type === 'dir') {
    return {
      icon: WORKSPACE_FOLDER_ICON,
      className: 'icon-vscode',
      label: t('workspace.icon.folder')
    };
  }
  const ext = getWorkspaceExtension(entry);
  const icon =
    workspaceThemeIconResolver.value?.resolveFileIconPath(String(entry?.name || entry?.path || ''), ext) ||
    resolveWorkspaceFallbackFileIcon(ext);
  if (IMAGE_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.image') };
  }
  if (PDF_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.pdf') };
  }
  if (OFFICE_WORD_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.word') };
  }
  if (OFFICE_EXCEL_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.excel') };
  }
  if (OFFICE_PPT_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.ppt') };
  }
  if (ARCHIVE_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.archive') };
  }
  if (AUDIO_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.audio') };
  }
  if (VIDEO_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.video') };
  }
  if (CODE_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.code') };
  }
  if (TEXT_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.text') };
  }
  if (OFFICE_EXTENSIONS.has(ext)) {
    return { icon, className: 'icon-vscode', label: t('workspace.icon.office') };
  }
  return { icon, className: 'icon-vscode', label: t('workspace.icon.file') };
};

const getEntryMeta = (entry) => {
  const parts = [];
  if (entry.type === 'dir') {
    parts.push(t('workspace.meta.folder'));
  } else {
    parts.push(formatBytes(entry.size || 0));
  }
  if (state.searchMode && entry.path) {
    parts.push(entry.path);
  }
  return parts.join(' · ');
};

const formatBytes = (size) => {
  const value = Number(size) || 0;
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`;
};

const normalizeWorkspaceTimestamp = (value) => {
  if (value === null || value === undefined) return 0;
  const date = new Date(value);
  if (!Number.isNaN(date.getTime())) return date.getTime();
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return 0;
  return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
};

const collectWorkspaceStats = (entries) => {
  let latestUpdatedAt = 0;
  let entryCount = 0;
  const walk = (items) => {
    if (!Array.isArray(items)) return;
    items.forEach((entry) => {
      if (!entry || typeof entry !== 'object') return;
      entryCount += 1;
      const updatedTs = normalizeWorkspaceTimestamp(entry.updated_time || entry.updatedAt || entry.modified_at);
      if (updatedTs > latestUpdatedAt) {
        latestUpdatedAt = updatedTs;
      }
      if (Array.isArray(entry.children) && entry.children.length) {
        walk(entry.children);
      }
    });
  };
  walk(entries);
  return { latestUpdatedAt, entryCount };
};

const emitWorkspaceStats = (entries = state.entries) => {
  emit('stats', collectWorkspaceStats(entries));
};

// 统一管理上传进度条展示，避免并发上传导致状态错乱
const setUploadProgress = (options: UploadProgressOptions = {}) => {
  const { percent = 0, loaded = 0, total = 0, indeterminate = false } = options;
  uploadProgress.active = true;
  uploadProgress.indeterminate = indeterminate;
  uploadProgress.percent = percent;
  uploadProgress.loaded = loaded;
  uploadProgress.total = total;
};

const resetUploadProgress = () => {
  uploadProgress.active = false;
  uploadProgress.indeterminate = false;
  uploadProgress.percent = 0;
  uploadProgress.loaded = 0;
  uploadProgress.total = 0;
};

const beginUploadProgress = () => {
  uploadProgressCount += 1;
  setUploadProgress({ percent: 0, loaded: 0, total: 0, indeterminate: true });
};

const endUploadProgress = () => {
  uploadProgressCount = Math.max(0, uploadProgressCount - 1);
  if (uploadProgressCount === 0) {
    resetUploadProgress();
  }
};

const findWorkspaceEntry = (entries, targetPath) => {
  if (!Array.isArray(entries) || !targetPath) return null;
  for (const entry of entries) {
    if (entry.path === targetPath) return entry;
    if (entry.children?.length) {
      const found = findWorkspaceEntry(entry.children, targetPath);
      if (found) return found;
    }
  }
  return null;
};

const resolveWorkspaceEntryName = (path) => {
  const entry = findWorkspaceEntry(state.entries, path);
  if (entry?.name) return entry.name;
  const fallback = String(path || '').split('/').pop();
  return fallback || t('common.unknown');
};

const attachWorkspaceChildren = (entries, targetPath, children) => {
  const target = findWorkspaceEntry(entries, targetPath);
  if (!target || target.type !== 'dir') return false;
  target.children = Array.isArray(children) ? children : [];
  target.childrenLoaded = true;
  return true;
};

const resetWorkspaceSelection = () => {
  state.selectedPaths = new Set();
  state.selected = null;
  state.lastSelectedPath = '';
};

const setWorkspaceSelection = (paths, primaryPath) => {
  state.selectedPaths = new Set(paths.filter(Boolean));
  state.selected =
    primaryPath && state.selectedPaths.has(primaryPath)
      ? findWorkspaceEntry(state.entries, primaryPath)
      : null;
  if (primaryPath) {
    state.lastSelectedPath = primaryPath;
  }
};

const toggleWorkspaceSelection = (path) => {
  if (!path) return;
  if (state.selectedPaths.has(path)) {
    state.selectedPaths.delete(path);
    if (state.selected?.path === path) {
      state.selected = null;
    }
  } else {
    state.selectedPaths.add(path);
    state.selected = findWorkspaceEntry(state.entries, path);
    state.lastSelectedPath = path;
  }
};

const getWorkspaceSelectionPaths = () => Array.from(state.selectedPaths);

const reconcileWorkspaceSelection = () => {
  if (!state.selectedPaths.size) {
    state.selected = null;
    return;
  }
  const nextSelectedPaths = new Set<string>();
  state.selectedPaths.forEach((path: string) => {
    if (findWorkspaceEntry(state.entries, path)) {
      nextSelectedPaths.add(path);
    }
  });
  state.selectedPaths = nextSelectedPaths;
  const preferredPath =
    state.lastSelectedPath && nextSelectedPaths.has(state.lastSelectedPath)
      ? state.lastSelectedPath
      : Array.from(nextSelectedPaths)[0] || '';
  state.selected = preferredPath ? findWorkspaceEntry(state.entries, preferredPath) : null;
  if (!preferredPath) {
    state.lastSelectedPath = '';
  }
};

const confirmAction = async (message, title = t('common.notice')) => {
  try {
    await ElMessageBox.confirm(message, title, {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    return true;
  } catch (error) {
    return false;
  }
};

const promptInput = async (message: string, options: PromptInputOptions = {}) => {
  const {
    title = t('common.notice'),
    placeholder = '',
    defaultValue = ''
  } = options;
  try {
    const { value } = await ElMessageBox.prompt(message, title, {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      inputValue: defaultValue,
      inputPlaceholder: placeholder
    });
    return value;
  } catch (error) {
    return null;
  }
};

const loadWorkspace = async ({
  path = state.path,
  resetExpanded = false,
  resetSearch = false,
  background = false
} = {}) => {
  const currentPath = normalizeWorkspacePath(path);
  const cacheKey = buildWorkspaceTreeCacheKey(
    normalizedAgentId.value,
    currentPath,
    state.sortBy,
    state.sortOrder
  );
  const cachedTree = readWorkspaceTreeCache(cacheKey);
  const hasCachedEntries = Boolean(cachedTree && Array.isArray(cachedTree.entries) && cachedTree.entries.length > 0);
  if (cachedTree) {
    state.path = normalizeWorkspacePath(cachedTree.path);
    state.parent = cachedTree.parent ? normalizeWorkspacePath(cachedTree.parent) : null;
    state.entries = cloneWorkspaceEntries(cachedTree.entries);
    emitWorkspaceStats(state.entries);
  }
  state.loading = true;
  // Keep background sync fully silent to avoid empty-state skeleton flicker.
  state.visualLoading = !background;
  if (resetSearch) {
    state.searchMode = false;
    state.searchKeyword = '';
  }
  if (resetExpanded) {
    state.expanded = new Set();
  }
  state.renamingPath = '';
  state.renamingValue = '';
  resetWorkspaceSelection();
  try {
    const { data } = await fetchWunderWorkspaceContent(withAgentParams({
      path: currentPath,
      include_content: true,
      depth: 1,
      sort_by: state.sortBy,
      order: state.sortOrder
    }));
    const payload = data || {};
    const normalizedPath = normalizeWorkspacePath(payload.path ?? currentPath);
    state.path = normalizedPath;
    const parentPath = getWorkspaceParentPath(normalizedPath);
    state.parent = parentPath ? parentPath : null;
    state.entries = Array.isArray(payload.entries) ? payload.entries : [];
    emitWorkspaceStats(state.entries);
    writeWorkspaceTreeCache(cacheKey, {
      path: normalizedPath,
      parent: state.parent,
      entries: state.entries
    });
    if (state.expanded.size) {
      const filtered = new Set();
      state.expanded.forEach((value: string) => {
        if (!normalizedPath || value === normalizedPath || value.startsWith(`${normalizedPath}/`)) {
          filtered.add(value);
        }
      });
      state.expanded = filtered;
    }
    await hydrateExpandedEntries();
    await syncWorkspaceListViewport({ reset: true });
    return true;
  } catch (error) {
    showApiError(error, t('workspace.loadFailed'));
    if (!hasCachedEntries) {
      state.entries = [];
      emitWorkspaceStats(state.entries);
    }
    await syncWorkspaceListViewport({ reset: true });
    return false;
  } finally {
    state.loading = false;
    state.visualLoading = false;
    if (autoRefreshPending) {
      scheduleWorkspaceAutoRefresh();
    }
  }
};

const loadWorkspaceSearch = async ({ background = false } = {}) => {
  const keyword = String(state.searchKeyword || '').trim();
  if (!keyword) {
    state.searchMode = false;
    return loadWorkspace({ resetSearch: true, background });
  }
  state.loading = true;
  state.visualLoading = !background;
  state.renamingPath = '';
  state.renamingValue = '';
  resetWorkspaceSelection();
  try {
    const { data } = await searchWunderWorkspace(
      withAgentParams({ keyword, offset: 0, limit: 200 })
    );
    const payload = data || {};
    state.entries = Array.isArray(payload.entries) ? payload.entries : [];
    state.searchMode = true;
    emitWorkspaceStats(state.entries);
    await syncWorkspaceListViewport({ reset: true });
    return true;
  } catch (error) {
    showApiError(error, t('workspace.searchFailed'));
    state.entries = [];
    emitWorkspaceStats(state.entries);
    await syncWorkspaceListViewport({ reset: true });
    return false;
  } finally {
    state.loading = false;
    state.visualLoading = false;
    if (autoRefreshPending) {
      scheduleWorkspaceAutoRefresh();
    }
  }
};

const reloadWorkspaceView = async ({ background = false } = {}) => {
  if (state.searchMode && String(state.searchKeyword || '').trim()) {
    return loadWorkspaceSearch({ background });
  }
  return loadWorkspace({ resetSearch: true, background });
};

const fetchWorkspaceDirectorySnapshot = async (path) => {
  const targetPath = normalizeWorkspacePath(path);
  const { data } = await fetchWunderWorkspaceContent(withAgentParams({
    path: targetPath,
    include_content: true,
    depth: 1,
    sort_by: state.sortBy,
    order: state.sortOrder
  }));
  const payload = data || {};
  const resolvedPath = normalizeWorkspacePath(payload.path ?? targetPath);
  const entryType = String(payload.type ?? payload.entry_type ?? '').trim().toLowerCase();
  if (entryType && entryType !== 'dir') {
    return null;
  }
  return {
    path: resolvedPath,
    entries: Array.isArray(payload.entries) ? payload.entries : []
  };
};

const reloadWorkspaceDirectoryPath = async (path) => {
  const snapshot = await fetchWorkspaceDirectorySnapshot(path);
  if (!snapshot) return false;
  const targetPath = normalizeWorkspacePath(snapshot.path);
  // Patch current view directly when the changed directory is the visible root.
  if (targetPath === state.path) {
    state.path = targetPath;
    state.parent = getWorkspaceParentPath(targetPath) || null;
    state.entries = snapshot.entries;
    emitWorkspaceStats(state.entries);
    writeWorkspaceTreeCache(
      buildWorkspaceTreeCacheKey(
        normalizedAgentId.value,
        targetPath,
        state.sortBy,
        state.sortOrder
      ),
      {
        path: targetPath,
        parent: state.parent,
        entries: state.entries
      }
    );
    reconcileWorkspaceSelection();
    await hydrateExpandedEntries();
    await syncWorkspaceListViewport();
    return true;
  }
  const sourcePath = normalizeWorkspacePath(path);
  const attached =
    attachWorkspaceChildren(state.entries, targetPath, snapshot.entries) ||
    (sourcePath !== targetPath && attachWorkspaceChildren(state.entries, sourcePath, snapshot.entries));
  if (!attached) return false;
  emitWorkspaceStats(state.entries);
  reconcileWorkspaceSelection();
  return true;
};

const refreshWorkspacePathWithFallback = async (path) => {
  try {
    const patched = await reloadWorkspaceDirectoryPath(path);
    if (patched) return true;
  } catch (error) {
    // Fallback to full reload when path patching is not possible.
  }
  return reloadWorkspaceView();
};

// Convert raw workspace event paths to the minimal set of directory targets we can patch.
const collectWorkspaceRefreshTargets = (paths = []) => {
  const currentPath = normalizeWorkspacePath(state.path);
  const targets = new Set<string>();

  paths.forEach((item) => {
    const normalized = normalizeWorkspaceEventPathValue(item);
    if (!normalized) {
      targets.add(currentPath);
      return;
    }
    if (currentPath && (currentPath === normalized || currentPath.startsWith(`${normalized}/`))) {
      targets.add(currentPath);
      return;
    }
    if (
      currentPath &&
      normalized !== currentPath &&
      !normalized.startsWith(`${currentPath}/`)
    ) {
      return;
    }
    const entry = findWorkspaceEntry(state.entries, normalized);
    if (entry?.type === 'dir') {
      targets.add(entry.path);
      return;
    }
    const parentPath = getWorkspaceParentPath(normalized);
    if (currentPath) {
      targets.add(parentPath || currentPath);
    } else {
      targets.add(parentPath);
    }
  });

  const deduped = Array.from(new Set(Array.from(targets).map((value) => normalizeWorkspacePath(value))));
  if (deduped.length > WORKSPACE_INCREMENTAL_REFRESH_MAX_TARGETS) {
    return { targets: [], forceFullReload: true };
  }
  return { targets: deduped, forceFullReload: false };
};

const enqueueWorkspaceAutoRefreshTargets = (targets = []) => {
  if (autoRefreshForceFullReload) return;
  for (const target of targets) {
    autoRefreshTargetPaths.add(normalizeWorkspacePath(target));
    if (autoRefreshTargetPaths.size > WORKSPACE_INCREMENTAL_REFRESH_MAX_TARGETS) {
      autoRefreshForceFullReload = true;
      autoRefreshTargetPaths = new Set<string>();
      return;
    }
  }
};

const scheduleWorkspaceAutoRefresh = () => {
  autoRefreshPending = true;
  if (autoRefreshTimer) return;
  autoRefreshTimer = setTimeout(async () => {
    const refreshStartAt = nowPerf();
    autoRefreshTimer = null;
    if (!autoRefreshPending) return;
    if (state.loading) {
      scheduleWorkspaceAutoRefresh();
      return;
    }
    autoRefreshPending = false;
    const shouldFullReload =
      autoRefreshForceFullReload || state.searchMode || autoRefreshTargetPaths.size === 0;
    const incrementalTargets = shouldFullReload
      ? []
      : Array.from(autoRefreshTargetPaths).slice(0, WORKSPACE_INCREMENTAL_REFRESH_MAX_BATCH);
    autoRefreshForceFullReload = false;
    autoRefreshTargetPaths = new Set<string>();
    if (incrementalTargets.length) {
      let patched = true;
      for (const target of incrementalTargets) {
        try {
          const ok = await reloadWorkspaceDirectoryPath(target);
          if (!ok) {
            patched = false;
            break;
          }
        } catch (error) {
          patched = false;
          break;
        }
      }
      if (patched) {
        chatPerf.count('workspace.panel.refresh', 1, {
          mode: 'incremental',
          targets: incrementalTargets.length
        });
        chatPerf.recordDuration('workspace.panel.refresh.incremental.ms', nowPerf() - refreshStartAt, {
          targets: incrementalTargets.length
        });
        return;
      }
      chatPerf.count('workspace.panel.refresh', 1, {
        mode: 'fallback',
        targets: incrementalTargets.length
      });
    }
    await reloadWorkspaceView({ background: true });
    chatPerf.count('workspace.panel.refresh', 1, {
      mode: shouldFullReload ? 'full' : 'fallback-full',
      targets: incrementalTargets.length
    });
    chatPerf.recordDuration('workspace.panel.refresh.full.ms', nowPerf() - refreshStartAt, {
      mode: shouldFullReload ? 'full' : 'fallback-full',
      targets: incrementalTargets.length
    });
  }, WORKSPACE_AUTO_REFRESH_DEBOUNCE_MS);
};

// Prefer path-based patching for realtime updates, fallback to full reload when signals are weak.
const scheduleWorkspaceAutoRefreshByDetail = (detail: Record<string, unknown> = {}) => {
  const nextTreeVersion = normalizeWorkspaceTreeVersion(
    detail?.treeVersion ?? detail?.tree_version ?? detail?.version
  );
  if (nextTreeVersion !== null) {
    if (nextTreeVersion <= latestWorkspaceTreeVersion) {
      return;
    }
    latestWorkspaceTreeVersion = nextTreeVersion;
  }
  const changedPaths = normalizeWorkspaceEventPaths(detail);
  chatPerf.count('workspace.panel.refresh.event', 1, {
    hasPaths: changedPaths.length > 0,
    pathCount: changedPaths.length
  });
  if (!changedPaths.length) {
    autoRefreshForceFullReload = true;
    autoRefreshTargetPaths = new Set<string>();
    scheduleWorkspaceAutoRefresh();
    return;
  }
  const { targets, forceFullReload } = collectWorkspaceRefreshTargets(changedPaths);
  if (forceFullReload) {
    autoRefreshForceFullReload = true;
    autoRefreshTargetPaths = new Set<string>();
    scheduleWorkspaceAutoRefresh();
    return;
  }
  if (!targets.length) {
    return;
  }
  enqueueWorkspaceAutoRefreshTargets(targets);
  scheduleWorkspaceAutoRefresh();
};

const hydrateExpandedEntries = async () => {
  const expandedPaths = Array.from(state.expanded);
  if (!expandedPaths.length) return;
  for (const path of expandedPaths) {
    const entry = findWorkspaceEntry(state.entries, path);
    if (!entry || entry.type !== 'dir' || entry.childrenLoaded) {
      continue;
    }
    try {
      const { data } = await fetchWunderWorkspaceContent(withAgentParams({
        path,
        include_content: true,
        depth: 1,
        sort_by: state.sortBy,
        order: state.sortOrder
      }));
      attachWorkspaceChildren(state.entries, path, data.entries || []);
      emitWorkspaceStats(state.entries);
    } catch (error) {
      state.expanded.delete(path);
      state.expanded = new Set(state.expanded);
    }
  }
};

const toggleWorkspaceDirectory = async (entry) => {
  if (!entry || entry.type !== 'dir' || state.searchMode) return;
  if (state.expanded.has(entry.path)) {
    state.expanded.delete(entry.path);
    state.expanded = new Set(state.expanded);
    return;
  }
  state.expanded.add(entry.path);
  state.expanded = new Set(state.expanded);
  if (entry.childrenLoaded) return;
  try {
    const { data } = await fetchWunderWorkspaceContent(withAgentParams({
      path: entry.path,
      include_content: true,
      depth: 1,
      sort_by: state.sortBy,
      order: state.sortOrder
    }));
    attachWorkspaceChildren(state.entries, entry.path, data.entries || []);
    emitWorkspaceStats(state.entries);
  } catch (error) {
    state.expanded.delete(entry.path);
    state.expanded = new Set(state.expanded);
    showApiError(error, t('workspace.expandFailed'));
  }
};

const refreshWorkspace = async () => {
  const ok = await reloadWorkspaceView();
  if (ok) {
    ElMessage.success(
      state.searchMode ? t('workspace.refresh.searchSuccess') : t('workspace.refresh.success')
    );
  }
};

const clearWorkspaceCurrent = async () => {
  const display = displayPath.value;
  try {
    await ElMessageBox.confirm(
      t('workspace.clear.confirm', { name: display }),
      t('workspace.clear.title'),
      {
        confirmButtonText: t('workspace.clear.action'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
  } catch (error) {
    return;
  }
  try {
    const { data } = await fetchWunderWorkspaceContent(withAgentParams({
      path: state.path,
      include_content: true,
      depth: 1,
      sort_by: state.sortBy,
      order: state.sortOrder
    }));
    const entries = Array.isArray(data?.entries) ? data.entries : [];
    if (!entries.length) {
      ElMessage.info(t('workspace.clear.empty'));
      return;
    }
    const response = await batchWunderWorkspaceAction(withAgentParams({
      action: 'delete',
      paths: entries.map((entry) => entry.path)
    }));
    notifyBatchResult(response.data, t('workspace.action.clear'));
    await reloadWorkspaceView();
    emitWorkspaceRefresh({
      reason: 'workspace-clear',
      sourceId: workspacePanelRefreshSourceId,
      agentId: normalizedAgentId.value,
      containerId: normalizedContainerId.value,
      path: state.path,
      paths: [state.path]
    });
  } catch (error) {
    showApiError(error, t('workspace.clear.failed'));
  }
};

const handleGoUp = async () => {
  if (!canGoUp.value) return;
  state.path = state.parent || '';
  state.expanded = new Set();
  state.selected = null;
  await loadWorkspace({ resetExpanded: true, resetSearch: true });
};

const handleSearchInput = () => {
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
  searchTimer = setTimeout(() => {
    if (state.searchKeyword.trim()) {
      loadWorkspaceSearch();
    } else {
      loadWorkspace({ resetSearch: true, resetExpanded: true });
    }
  }, WORKSPACE_SEARCH_DEBOUNCE_MS);
};

const handleSearchKeydown = (event) => {
  if (event.key !== 'Escape') return;
  state.searchKeyword = '';
  loadWorkspace({ resetSearch: true, resetExpanded: true });
};

const triggerUpload = () => {
  if (!uploadInputRef.value) return;
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const uploadWorkspaceFiles = async (
  files: File[] | FileList,
  targetPath: string,
  options: WorkspaceUploadOptions = {}
) => {
  if (!files || !files.length) return;
  const { refreshTree = true, relativePaths = [] } = options;
  const fileList = Array.from(files);
  const totalBytes = fileList.reduce((sum: number, file) => sum + (Number(file?.size) || 0), 0);
  // 对齐 Wunder 上传限制：单次上传总大小不超过 200MB
  if (totalBytes > MAX_WORKSPACE_UPLOAD_BYTES) {
    throw new Error(
      t('workspace.upload.tooLarge', { limit: formatBytes(MAX_WORKSPACE_UPLOAD_BYTES) })
    );
  }
  const formData = new FormData();
  formData.append('path', normalizeWorkspacePath(targetPath));
  appendAgentId(formData);
  fileList.forEach((file, index) => {
    formData.append('files', file as Blob);
    if (relativePaths.length) {
      formData.append('relative_paths', relativePaths[index] ?? '');
    }
  });
  beginUploadProgress();
  try {
    await uploadWunderWorkspace(formData, {
      onUploadProgress: (event) => {
        const loaded = Number(event.loaded) || 0;
        const total = Number.isFinite(event.total) ? event.total : 0;
        if (total > 0) {
          const percent = (loaded / total) * 100;
          setUploadProgress({ percent, loaded, total, indeterminate: false });
        } else {
          setUploadProgress({ loaded, total: 0, indeterminate: true });
        }
      }
    });
  } finally {
    endUploadProgress();
  }
  if (refreshTree) {
    await refreshWorkspacePathWithFallback(targetPath);
  }
};

const handleUploadInput = async (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  const files = target?.files ? Array.from(target.files) : [];
  if (!files.length) return;
  try {
    await uploadWorkspaceFiles(files, state.path);
    ElMessage.success(t('workspace.upload.success'));
  } catch (error) {
    showApiError(error, error.message || t('workspace.upload.failed'));
  }
};

const handleWorkspaceItemClick = (event, entry) => {
  if (!entry || state.renamingPath) return;
  const path = entry.path;
  if (!path) return;
  const useRange = event.shiftKey && state.lastSelectedPath;
  const useToggle = event.metaKey || event.ctrlKey;
  if (useRange) {
    const flat = flatEntries.value;
    const startIndex = flat.findIndex((item) => item.path === state.lastSelectedPath);
    const endIndex = flat.findIndex((item) => item.path === path);
    if (startIndex !== -1 && endIndex !== -1) {
      const [from, to] = startIndex < endIndex ? [startIndex, endIndex] : [endIndex, startIndex];
      const rangePaths = flat.slice(from, to + 1).map((item) => item.path);
      if (useToggle) {
        rangePaths.forEach((rangePath) => state.selectedPaths.add(rangePath));
        state.selected = entry;
        state.lastSelectedPath = path;
      } else {
        setWorkspaceSelection(rangePaths, path);
      }
      return;
    }
  }
  if (useToggle) {
    toggleWorkspaceSelection(path);
    return;
  }
  setWorkspaceSelection([path], path);
};

const handleWorkspaceItemDoubleClick = (entry) => {
  if (!entry || state.renamingPath) return;
  if (entry.type === 'dir') {
    // Folder double click only toggles tree expansion to avoid accidental navigation.
    void toggleWorkspaceDirectory(entry);
    return;
  }
  if (isWorkspaceTextEditable(entry)) {
    openEditor(entry);
    return;
  }
  openPreview(entry);
};

const closeContextMenu = () => {
  state.contextMenu.visible = false;
};

const openContextMenu = async (event, entry) => {
  const nextSelectionPaths = entry?.path
    ? state.selectedPaths.has(entry.path)
      ? getWorkspaceSelectionPaths()
      : [entry.path]
    : getWorkspaceSelectionPaths();
  if (entry?.path && !state.selectedPaths.has(entry.path)) {
    setWorkspaceSelection([entry.path], entry.path);
  }
  if (entry?.path) {
    state.selected = entry;
  }
  state.contextMenu.primaryPath = entry?.path || nextSelectionPaths[0] || '';
  state.contextMenu.selectionPaths = nextSelectionPaths;
  state.contextMenu.visible = true;
  state.contextMenu.x = event.clientX;
  state.contextMenu.y = event.clientY;
  await nextTick();
  const menuRect = menuRef.value?.getBoundingClientRect();
  if (!menuRect) return;
  const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
  const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
  state.contextMenu.x = Math.min(Math.max(8, state.contextMenu.x), maxLeft);
  state.contextMenu.y = Math.min(Math.max(8, state.contextMenu.y), maxTop);
};

const handleEdit = () => {
  const targetEntry = contextMenuSingleEntry.value;
  closeContextMenu();
  if (!targetEntry) return;
  openEditor(targetEntry);
};

const handleRename = () => {
  const targetEntry = contextMenuSingleEntry.value;
  closeContextMenu();
  if (!targetEntry) return;
  startWorkspaceRename(targetEntry);
};

const handleNewFile = async () => {
  closeContextMenu();
  await createWorkspaceFile();
};

const handleNewFolder = async () => {
  closeContextMenu();
  await createWorkspaceFolder();
};

const handleContextMenuUpload = () => {
  closeContextMenu();
  triggerUpload();
};

const handleContextMenuRefresh = async () => {
  closeContextMenu();
  await refreshWorkspace();
};

const handleDownload = async () => {
  const targetEntry = contextMenuSingleEntry.value;
  closeContextMenu();
  if (!targetEntry) return;
  await downloadEntry(targetEntry);
};

const handleDelete = async () => {
  closeContextMenu();
  await deleteWorkspaceSelection();
};

const startWorkspaceRename = async (entry) => {
  state.renamingPath = entry.path;
  state.renamingValue = entry.name || '';
  await nextTick();
  const input = listRef.value?.querySelector(`input[data-rename-path="${entry.path}"]`);
  if (input) {
    input.focus();
    input.select();
  }
};

const cancelWorkspaceRename = () => {
  state.renamingPath = '';
  state.renamingValue = '';
};

const finishWorkspaceRename = async (entry, nextName) => {
  if (!entry || state.renamingPath !== entry.path) return;
  state.renamingPath = '';
  const trimmed = String(nextName || '').trim();
    if (!trimmed || !isValidWorkspaceName(trimmed)) {
      ElMessage.warning(t('workspace.name.invalid'));
      state.renamingValue = '';
      return;
    }
  if (trimmed === entry.name) {
    state.renamingValue = '';
    return;
  }
  const parentPath = getWorkspaceParentPath(entry.path);
  const destination = joinWorkspacePath(parentPath, trimmed);
  try {
    await moveWunderWorkspaceEntry(withAgentParams({ source: entry.path, destination }));
      await refreshWorkspacePathWithFallback(parentPath);
      ElMessage.success(t('workspace.rename.success'));
    } catch (error) {
      showApiError(error, t('workspace.rename.failed'));
    } finally {
      state.renamingValue = '';
    }
};

const notifyBatchResult = (payload, actionLabel) => {
  const data = payload?.data || {};
  const failed = Array.isArray(data.failed) ? data.failed : [];
  const succeeded = Array.isArray(data.succeeded) ? data.succeeded : [];
    if (failed.length) {
      ElMessage.warning(
        t('workspace.batch.partial', {
          action: actionLabel,
          success: succeeded.length,
          failed: failed.length
        })
      );
    } else {
      ElMessage.success(t('workspace.batch.success', { action: actionLabel }));
    }
  };

const getActionSelectionPaths = () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (selectedPaths.length) {
    return selectedPaths;
  }
  const menuSelectionPaths = contextMenuSelectionPaths.value;
  if (menuSelectionPaths.length) {
    return [...menuSelectionPaths];
  }
  return state.contextMenu.primaryPath ? [state.contextMenu.primaryPath] : [];
};

const deleteWorkspaceSelection = async () => {
  const selectedPaths = getActionSelectionPaths();
  if (!selectedPaths.length) return;
  const singleName =
    selectedPaths.length === 1 ? resolveWorkspaceEntryName(selectedPaths[0]) : '';
  const confirmed = await confirmAction(
    selectedPaths.length === 1
      ? t('workspace.delete.confirm.single', { name: singleName })
      : t('workspace.delete.confirm.multi', { count: selectedPaths.length })
  );
  if (!confirmed) return;
  try {
    const response = await batchWunderWorkspaceAction(
      withAgentParams({ action: 'delete', paths: selectedPaths })
    );
    notifyBatchResult(response.data, t('common.delete'));
    await reloadWorkspaceView();
    emitWorkspaceRefresh({
      reason: 'workspace-delete',
      sourceId: workspacePanelRefreshSourceId,
      agentId: normalizedAgentId.value,
      containerId: normalizedContainerId.value,
      paths: selectedPaths
    });
  } catch (error) {
    showApiError(error, t('workspace.delete.failed'));
  }
};

  const moveWorkspaceSelectionToDirectory = async () => {
    const selectedPaths = getWorkspaceSelectionPaths();
    if (!selectedPaths.length) {
      ElMessage.info(t('workspace.selection.empty'));
      return;
    }
    const targetDirInput = await promptInput(t('workspace.move.prompt'), {
      placeholder: t('workspace.move.placeholder'),
      defaultValue: ''
    });
    if (targetDirInput === null) return;
    const targetDir = normalizeWorkspacePath(targetDirInput.trim());
    if (!isValidWorkspacePath(targetDir)) {
      ElMessage.warning(t('workspace.path.invalid'));
      return;
    }
    try {
      const response = await batchWunderWorkspaceAction(withAgentParams({
        action: 'move',
        paths: selectedPaths,
        destination: targetDir
      }));
      notifyBatchResult(response.data, t('workspace.action.move'));
      await reloadWorkspaceView();
    } catch (error) {
      showApiError(error, t('workspace.move.failed'));
    }
  };

  const copyWorkspaceSelectionToDirectory = async () => {
    const selectedPaths = getWorkspaceSelectionPaths();
    if (!selectedPaths.length) {
      ElMessage.info(t('workspace.selection.empty'));
      return;
    }
    const targetDirInput = await promptInput(t('workspace.move.prompt'), {
      placeholder: t('workspace.move.placeholder'),
      defaultValue: ''
    });
    if (targetDirInput === null) return;
    const targetDir = normalizeWorkspacePath(targetDirInput.trim());
    if (!isValidWorkspacePath(targetDir)) {
      ElMessage.warning(t('workspace.path.invalid'));
      return;
    }
    try {
      const response = await batchWunderWorkspaceAction(withAgentParams({
        action: 'copy',
        paths: selectedPaths,
        destination: targetDir
      }));
      notifyBatchResult(response.data, t('workspace.action.copy'));
      await reloadWorkspaceView();
    } catch (error) {
      showApiError(error, t('workspace.copy.failed'));
    }
  };

const moveWorkspaceEntryToDirectory = async (entry) => {
  if (!entry) return;
  const targetDirInput = await promptInput(t('workspace.move.prompt'), {
    placeholder: t('workspace.move.placeholder'),
    defaultValue: ''
  });
  if (targetDirInput === null) return;
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    ElMessage.warning(t('workspace.path.invalid'));
    return;
  }
  const sourceName = entry.name || entry.path.split('/').pop();
  if (!sourceName) {
    ElMessage.error(t('workspace.move.sourceMissing'));
    return;
  }
  const destination = joinWorkspacePath(targetDir, sourceName);
  if (destination === entry.path) {
    ElMessage.info(t('workspace.move.sameDir'));
    return;
  }
  try {
    await moveWunderWorkspaceEntry(withAgentParams({ source: entry.path, destination }));
    await reloadWorkspaceView();
    ElMessage.success(t('workspace.move.success', { target: targetDir || '/' }));
  } catch (error) {
    showApiError(error, t('workspace.move.failed'));
  }
};

const resolveWorkspaceCreationDirectoryPath = () => {
  const target = singleSelectedEntry.value;
  if (target?.type === 'dir') {
    return normalizeWorkspacePath(target.path);
  }
  if (target?.type === 'file') {
    return normalizeWorkspacePath(getWorkspaceParentPath(target.path));
  }
  return normalizeWorkspacePath(state.path);
};

const createWorkspaceFile = async () => {
  const fileName = await promptInput(t('workspace.createFile.prompt'), {
    placeholder: t('workspace.createFile.placeholder'),
    defaultValue: 'untitled.txt'
  });
  if (fileName === null) return;
  const trimmed = String(fileName || '').trim();
  if (!isValidWorkspaceName(trimmed)) {
    ElMessage.warning(t('workspace.name.invalid'));
    return;
  }
  const targetDir = resolveWorkspaceCreationDirectoryPath();
  const targetPath = joinWorkspacePath(targetDir, trimmed);
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({ path: targetPath, content: '', create_if_missing: true })
    );
    await refreshWorkspacePathWithFallback(targetDir);
    ElMessage.success(t('workspace.createFile.success', { name: trimmed }));
  } catch (error) {
    showApiError(error, t('workspace.createFile.failed'));
  }
};

const createWorkspaceFolder = async () => {
  const folderName = await promptInput(t('workspace.createFolder.prompt'), {
    placeholder: t('workspace.createFolder.placeholder')
  });
  if (folderName === null) return;
  const trimmed = String(folderName || '').trim();
  if (!isValidWorkspaceName(trimmed)) {
    ElMessage.warning(t('workspace.name.invalid'));
    return;
  }
  const targetDir = resolveWorkspaceCreationDirectoryPath();
  const targetPath = joinWorkspacePath(targetDir, trimmed);
  try {
    await createWunderWorkspaceDir(withAgentParams({ path: targetPath }));
    await refreshWorkspacePathWithFallback(targetDir);
    ElMessage.success(t('workspace.createFolder.success'));
  } catch (error) {
    showApiError(error, t('workspace.createFolder.failed'));
  }
};

const getFilenameFromHeaders = (headers, fallback) => {
  const disposition = headers?.['content-disposition'];
  if (!disposition) return fallback;
  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match) {
    return decodeURIComponent(utf8Match[1]);
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match ? match[1] : fallback;
};

const saveBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || t('workspace.download.defaultName');
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
};

const downloadEntry = async (entry) => {
  try {
    if (entry.type === 'dir') {
      const response = await downloadWunderWorkspaceArchive(withAgentParams({ path: entry.path }));
      const filename = getFilenameFromHeaders(
        response.headers,
        `${entry.name || t('workspace.download.folder')}.zip`
      );
      saveBlob(response.data, filename);
      return;
    }
    const response = await downloadWunderWorkspaceFile(withAgentParams({ path: entry.path }));
    const filename = getFilenameFromHeaders(
      response.headers,
      entry.name || t('workspace.download.defaultName')
    );
    saveBlob(response.data, filename);
  } catch (error) {
    ElMessage.error(resolveWorkspaceTransferFailedText());
  }
};

const downloadArchive = async () => {
  try {
    const response = await downloadWunderWorkspaceArchive(withAgentParams({}));
    const filename = getFilenameFromHeaders(response.headers, 'workspace.zip');
    saveBlob(response.data, filename);
    ElMessage.success(resolveWorkspaceArchiveSuccessText());
  } catch (error) {
    ElMessage.error(resolveWorkspaceArchiveFailedText());
  }
};

const readDirectoryEntries = (reader: DirectoryReaderLike): Promise<FileSystemEntryLike[]> =>
  new Promise((resolve) => {
    const entries: FileSystemEntryLike[] = [];
    const readBatch = () => {
      reader.readEntries(
        (batch: FileSystemEntryLike[]) => {
          if (!batch.length) {
            resolve(entries);
            return;
          }
          entries.push(...batch);
          readBatch();
        },
        () => resolve(entries)
      );
    };
    readBatch();
  });

const walkEntry = async (entry: FileSystemEntryLike, prefix: string): Promise<WorkspaceDroppedFile[]> => {
  if (!entry) return [];
  if (entry.isFile) {
    const file = await new Promise<File | null>((resolve) => {
      entry.file((target) => resolve(target), () => resolve(null));
    });
    if (!file) return [];
    return [
      {
        file,
        relativePath: `${prefix}${file.name}`
      }
    ];
  }
  if (entry.isDirectory) {
    const nextPrefix = `${prefix}${entry.name}/`;
    const reader = entry.createReader();
    const children = await readDirectoryEntries(reader);
    const nested = await Promise.all(children.map((child) => walkEntry(child, nextPrefix)));
    return nested.flat();
  }
  return [];
};

const collectDroppedFiles = async (
  dataTransfer: DataTransfer | null | undefined
): Promise<WorkspaceDroppedFile[]> => {
  const items = Array.from(dataTransfer?.items || []) as DataTransferItemLike[];
  if (items.length) {
    const batches = await Promise.all(
      items.map((item: DataTransferItemLike) => {
        const entry = item.webkitGetAsEntry?.();
        if (entry) {
          return walkEntry(entry, '');
        }
        const file = item.getAsFile();
        return file ? [{ file, relativePath: file.name }] : [];
      })
    );
    return batches.flat();
  }
  const files = Array.from(dataTransfer?.files || []);
  return files.map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name
  }));
};

const uploadWorkspaceGroups = async (items, basePath) => {
  const files = items.map((item) => item.file).filter(Boolean);
  const relativePaths = items.map((item) => normalizeWorkspacePath(item.relativePath || item.file?.name || ''));
  await uploadWorkspaceFiles(files, basePath, { refreshTree: false, relativePaths });
  await refreshWorkspacePathWithFallback(basePath);
};

const hasWorkspaceDrag = (dataTransfer) =>
  Array.from(dataTransfer?.types || []).includes(WORKSPACE_DRAG_KEY);

const hasExternalFileDrag = (dataTransfer) => {
  if (!dataTransfer || hasWorkspaceDrag(dataTransfer)) return false;
  const types = Array.from(dataTransfer.types || []);
  return types.includes('Files') || Boolean(dataTransfer.items?.length) || Boolean(dataTransfer.files?.length);
};

const getWorkspaceDragPaths = (dataTransfer) => {
  const raw = dataTransfer?.getData(WORKSPACE_DRAG_KEY) || '';
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return parsed.filter(Boolean);
    }
  } catch (error) {
    return [raw].filter(Boolean);
  }
  return [raw].filter(Boolean);
};

const resolveExternalDropBasePath = (entry) => {
  if (entry?.type === 'dir') {
    return normalizeWorkspacePath(entry.path);
  }
  return state.path;
};

const handleListDragEnter = (event) => {
  event.preventDefault();
  state.draggingOver = true;
};

const handleListScroll = (event) => {
  listScrollTop.value = event.target.scrollTop || 0;
};

const handleListDragOver = (event) => {
  event.preventDefault();
  state.draggingOver = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = hasWorkspaceDrag(event.dataTransfer) ? 'move' : 'copy';
  }
};

const handleListDragLeave = (event) => {
  if (!event.currentTarget.contains(event.relatedTarget)) {
    state.draggingOver = false;
  }
};

const handleListDrop = async (event) => {
  event.preventDefault();
  state.draggingOver = false;
  if (hasWorkspaceDrag(event.dataTransfer)) return;
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) return;
  try {
    await uploadWorkspaceGroups(dropped, state.path);
    ElMessage.success(t('workspace.dragUpload.success'));
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail || error.message || t('workspace.dragUpload.failed')
    );
  }
};

const handleItemDragStart = (event, entry) => {
  if (!event.dataTransfer || !entry?.path) return;
  if (!state.selectedPaths.has(entry.path)) {
    setWorkspaceSelection([entry.path], entry.path);
  }
  const selectedPaths = state.selectedPaths.has(entry.path)
    ? getWorkspaceSelectionPaths()
    : [entry.path];
  event.dataTransfer.setData(WORKSPACE_DRAG_KEY, JSON.stringify(selectedPaths));
  event.dataTransfer.setData('text/plain', selectedPaths[0] || entry.path);
  event.dataTransfer.effectAllowed = 'move';
  event.currentTarget?.classList?.add('dragging');
};

const handleItemDragEnd = (event) => {
  event.currentTarget?.classList?.remove('dragging');
};

const handleItemDragEnter = (event, entry) => {
  const internalDrag = hasWorkspaceDrag(event.dataTransfer);
  const externalFileDrag = hasExternalFileDrag(event.dataTransfer);
  if (!internalDrag && !externalFileDrag) return;
  if (internalDrag && (!entry || entry.type !== 'dir')) return;
  event.preventDefault();
  state.draggingOver = true;
  if (entry?.type === 'dir') {
    event.currentTarget?.classList?.add('drop-target');
  }
};

const handleItemDragOver = (event, entry) => {
  const internalDrag = hasWorkspaceDrag(event.dataTransfer);
  const externalFileDrag = hasExternalFileDrag(event.dataTransfer);
  if (!internalDrag && !externalFileDrag) return;
  if (internalDrag && (!entry || entry.type !== 'dir')) return;
  event.preventDefault();
  state.draggingOver = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = internalDrag ? 'move' : 'copy';
  }
};

const handleItemDragLeave = (event, entry) => {
  if (entry?.type === 'dir' && !event.currentTarget?.contains(event.relatedTarget)) {
    event.currentTarget?.classList?.remove('drop-target');
  }
  if (!listRef.value?.contains(event.relatedTarget)) {
    state.draggingOver = false;
  }
};

const filterMoveTargets = (paths, targetDir) => {
  const filtered = [];
  let blocked = false;
  paths.forEach((path) => {
    const normalized = normalizeWorkspacePath(path);
    if (!normalized || normalized === targetDir) return;
    const entry = findWorkspaceEntry(state.entries, normalized);
    if (entry?.type === 'dir' && (targetDir === normalized || targetDir.startsWith(`${normalized}/`))) {
      blocked = true;
      return;
    }
    filtered.push(normalized);
  });
  if (blocked) {
    ElMessage.warning(t('workspace.move.blocked'));
  }
  return filtered;
};

const handleItemDrop = async (event, entry) => {
  event.preventDefault();
  event.stopPropagation();
  event.currentTarget?.classList?.remove('drop-target');
  state.draggingOver = false;
  const internalPaths = getWorkspaceDragPaths(event.dataTransfer);
  if (internalPaths.length) {
    if (!entry || entry.type !== 'dir') return;
    const targetDir = normalizeWorkspacePath(entry.path);
    const filtered = filterMoveTargets(internalPaths, targetDir);
    if (!filtered.length) return;
    try {
      const response = await batchWunderWorkspaceAction(
        withAgentParams({
          action: 'move',
          paths: filtered,
          destination: targetDir
        })
      );
      notifyBatchResult(response.data, t('workspace.move.toFolder', { name: entry.name || t('workspace.meta.folder') }));
      await reloadWorkspaceView();
    } catch (error) {
      showApiError(error, t('workspace.move.failed'));
    }
    return;
  }
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) return;
  const uploadBasePath = resolveExternalDropBasePath(entry);
  try {
    // When the list is fully occupied, external drags land on item nodes instead of the list shell.
    await uploadWorkspaceGroups(dropped, uploadBasePath);
    ElMessage.success(t('workspace.dragUpload.success'));
  } catch (error) {
    ElMessage.error(
      error.response?.data?.detail || error.message || t('workspace.dragUpload.failed')
    );
  }
};

const handleUpDragOver = (event) => {
  if (!hasWorkspaceDrag(event.dataTransfer)) return;
  if (!state.path) return;
  event.preventDefault();
  event.currentTarget?.classList?.add('dragover');
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move';
  }
};

const handleUpDragLeave = (event) => {
  if (!event.currentTarget?.contains(event.relatedTarget)) {
    event.currentTarget?.classList?.remove('dragover');
  }
};

const handleUpDrop = async (event) => {
  if (!state.path) return;
  event.preventDefault();
  event.currentTarget?.classList?.remove('dragover');
  const sourcePaths = getWorkspaceDragPaths(event.dataTransfer);
  if (!sourcePaths.length) return;
  const parentPath = getWorkspaceParentPath(state.path);
  try {
    const response = await batchWunderWorkspaceAction(
      withAgentParams({
        action: 'move',
        paths: sourcePaths,
        destination: parentPath
      })
    );
    notifyBatchResult(response.data, t('workspace.action.moveToParent'));
    await reloadWorkspaceView();
  } catch (error) {
    showApiError(error, t('workspace.move.failed'));
  }
};

const clearPreviewUrl = () => {
  if (state.preview.url) {
    URL.revokeObjectURL(state.preview.url);
  }
  state.preview.url = '';
};

const resolvePreviewUnsupportedHint = () =>
  desktopLocalMode.value ? t('workspace.preview.unsupportedHintLocal') : t('workspace.preview.unsupportedHint');

const resolvePreviewTooLargeHint = () =>
  desktopLocalMode.value ? t('workspace.preview.tooLargeHintLocal') : t('workspace.preview.tooLargeHint');

const resolveWorkspaceTransferFailedText = () =>
  desktopLocalMode.value ? t('workspace.download.exportFailed') : t('workspace.download.failed');

const resolveWorkspaceArchiveSuccessText = () =>
  desktopLocalMode.value
    ? t('workspace.download.exportArchiveSuccess')
    : t('workspace.download.archiveSuccess');

const resolveWorkspaceArchiveFailedText = () =>
  desktopLocalMode.value
    ? t('workspace.download.exportArchiveFailed')
    : t('workspace.download.archiveFailed');

const openPreview = async (entry) => {
  if (!entry || entry.type !== 'file') return;
  state.preview.entry = entry;
  state.preview.visible = true;
  state.preview.content = '';
  state.preview.hint = '';
  state.preview.loading = true;
  state.preview.embed = false;
  state.preview.type = '';
  clearPreviewUrl();

  const extension = getWorkspaceExtension(entry);
  const sizeValue = Number.isFinite(entry.size) ? entry.size : 0;
  const canPreviewText = sizeValue <= MAX_TEXT_PREVIEW_SIZE;

  if (OFFICE_EXTENSIONS.has(extension)) {
    state.preview.hint = resolvePreviewUnsupportedHint();
    state.preview.content = t('workspace.preview.empty');
    state.preview.loading = false;
    return;
  }
  const isMediaPreview = IMAGE_EXTENSIONS.has(extension) || PDF_EXTENSIONS.has(extension);
  if (!isMediaPreview && !canPreviewText) {
    state.preview.hint = resolvePreviewTooLargeHint();
    state.preview.content = t('workspace.preview.empty');
    state.preview.loading = false;
    return;
  }

  try {
    if (extension === 'svg') {
      try {
        const response = await fetchWunderWorkspaceContent(
          withAgentParams({
            path: entry.path,
            include_content: true,
            max_bytes: MAX_TEXT_PREVIEW_SIZE
          })
        );
        const payload = response.data || {};
        if (payload.truncated) {
          state.preview.hint = t('workspace.preview.truncatedHint');
          state.preview.content = t('workspace.preview.empty');
          return;
        }
        const text = typeof payload.content === 'string' ? payload.content : '';
        if (text) {
          const blob = new Blob([text], { type: IMAGE_MIME_TYPES.svg });
          state.preview.embed = true;
          state.preview.type = 'svg';
          state.preview.url = URL.createObjectURL(blob);
          return;
        }
      } catch (error) {
        // Fall back to download when content preview fails.
      }
    }
    if (IMAGE_EXTENSIONS.has(extension) || PDF_EXTENSIONS.has(extension)) {
      const response = await downloadWunderWorkspaceFile(withAgentParams({ path: entry.path }));
      let blob = response.data;
      if (IMAGE_EXTENSIONS.has(extension)) {
        const expectedMime = IMAGE_MIME_TYPES[extension] || '';
        if (
          expectedMime &&
          (!blob.type || blob.type === 'application/octet-stream' || blob.type !== expectedMime)
        ) {
          blob = blob.slice(0, blob.size, expectedMime);
        }
        state.preview.embed = true;
        state.preview.type = extension === 'svg' ? 'svg' : 'image';
        state.preview.url = URL.createObjectURL(blob);
      } else {
        state.preview.embed = true;
        state.preview.type = 'pdf';
        state.preview.url = URL.createObjectURL(blob);
      }
      return;
    }
    const response = await fetchWunderWorkspaceContent(
      withAgentParams({
        path: entry.path,
        include_content: true,
        max_bytes: MAX_TEXT_PREVIEW_SIZE
      })
    );
    const payload = response.data || {};
    if (payload.truncated) {
      state.preview.hint = t('workspace.preview.truncatedHint');
    }
    const text = typeof payload.content === 'string' ? payload.content : '';
    state.preview.content = text || t('workspace.preview.emptyContent');
  } catch (error) {
    state.preview.hint = t('workspace.preview.loadFailedHint');
    state.preview.content = t('workspace.preview.empty');
  } finally {
    state.preview.loading = false;
  }
};

const closePreview = () => {
  state.preview.visible = false;
  state.preview.entry = null;
  state.preview.content = '';
  state.preview.hint = '';
  state.preview.embed = false;
  state.preview.type = '';
  clearPreviewUrl();
};

const downloadPreview = async () => {
  if (!state.preview.entry) return;
  await downloadEntry(state.preview.entry);
};

const openEditor = async (entry) => {
  if (!entry || entry.type !== 'file') return;
  if (!isWorkspaceTextEditable(entry)) {
    ElMessage.warning(t('workspace.editor.previewOnly'));
    return;
  }
  state.editor.entry = entry;
  state.editor.visible = true;
  state.editor.content = '';
  state.editor.loading = true;
  try {
    const response = await fetchWunderWorkspaceContent(
      withAgentParams({
        path: entry.path,
        include_content: true,
        max_bytes: MAX_TEXT_PREVIEW_SIZE
      })
    );
    const payload = response.data || {};
    if (payload.truncated) {
      ElMessage.warning(t('workspace.editor.tooLarge'));
      closeEditor();
      return;
    }
    state.editor.content = typeof payload.content === 'string' ? payload.content : '';
  } catch (error) {
    ElMessage.error(t('workspace.editor.loadFailed'));
    closeEditor();
  } finally {
    state.editor.loading = false;
  }
};

const closeEditor = () => {
  state.editor.visible = false;
  state.editor.entry = null;
  state.editor.content = '';
  state.editor.loading = false;
};

const saveEditor = async () => {
  if (!state.editor.entry) return;
  const parentPath = getWorkspaceParentPath(state.editor.entry.path);
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({
        path: state.editor.entry.path,
        content: state.editor.content
      })
    );
    ElMessage.success(t('common.saved'));
    closeEditor();
    await refreshWorkspacePathWithFallback(parentPath);
  } catch (error) {
    showApiError(error, t('workspace.editor.saveFailed'));
  }
};

const handleGlobalClick = (event) => {
  if (!menuRef.value?.contains(event.target)) {
    closeContextMenu();
  }
};

const handleGlobalScroll = () => {
  closeContextMenu();
};

onMounted(async () => {
  scheduleWorkspaceThemeIconWarmup();
  await loadWorkspace();
  stopWorkspaceRefreshListener = onWorkspaceRefresh((event) => {
    const detail =
      event?.detail && typeof event.detail === 'object'
        ? (event.detail as Record<string, unknown>)
        : {};
    if (String(detail.sourceId || '').trim() === workspacePanelRefreshSourceId) return;
    const eventAgentId = String(detail.agentId ?? detail.agent_id ?? '').trim();
    const eventContainerRaw = detail.containerId ?? detail.container_id;
    const eventContainerId = Number.parseInt(String(eventContainerRaw ?? ''), 10);
    const currentAgentId = normalizedAgentId.value;
    if (eventAgentId && eventAgentId !== currentAgentId) return;
    if (Number.isFinite(eventContainerId) && eventContainerId !== normalizedContainerId.value) return;
    scheduleWorkspaceAutoRefreshByDetail(detail);
  });
  document.addEventListener('click', handleGlobalClick);
  document.addEventListener('scroll', handleGlobalScroll, true);
  window.addEventListener('resize', closeContextMenu);
});

watch(
  () => normalizedAgentId.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    state.path = '';
    state.parent = null;
    state.expanded = new Set();
    await loadWorkspace({ path: '', resetExpanded: true, resetSearch: true });
  }
);

watch(
  () => normalizedContainerId.value,
  async (value, oldValue) => {
    if (value === oldValue) return;
    state.path = '';
    state.parent = null;
    state.expanded = new Set();
    await loadWorkspace({ path: '', resetExpanded: true, resetSearch: true });
  }
);

watch(
  listRef,
  () => {
    void syncWorkspaceListViewport();
  },
  { flush: 'post' }
);

watch(
  () => [displayEntries.value.length, workspaceVirtual.value],
  () => {
    void syncWorkspaceListViewport();
  },
  { flush: 'post' }
);

onBeforeUnmount(() => {
  clearPreviewUrl();
  cancelWorkspaceThemeIconWarmup();
  if (searchTimer) {
    clearTimeout(searchTimer);
  }
  if (autoRefreshTimer) {
    clearTimeout(autoRefreshTimer);
    autoRefreshTimer = null;
  }
  if (stopWorkspaceRefreshListener) {
    stopWorkspaceRefreshListener();
    stopWorkspaceRefreshListener = null;
  }
  document.removeEventListener('click', handleGlobalClick);
  document.removeEventListener('scroll', handleGlobalScroll, true);
  window.removeEventListener('resize', closeContextMenu);
});
</script>
