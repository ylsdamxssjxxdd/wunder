<template>
  <div class="workspace-panel">
    <div class="workspace-header">
    <div class="workspace-title">沙盒容器</div>
      <div class="workspace-header-actions">
        <button
          class="workspace-icon-btn"
          :disabled="!canGoUp"
          title="上级"
          aria-label="上级"
          @click="handleGoUp"
          @dragover="handleUpDragOver"
          @dragleave="handleUpDragLeave"
          @drop="handleUpDrop"
        >
          <i class="fa-solid fa-arrow-up workspace-icon" aria-hidden="true"></i>
        </button>
        <button
          class="workspace-icon-btn"
          title="刷新"
          aria-label="刷新"
          @click="refreshWorkspace"
        >
          <i class="fa-solid fa-rotate workspace-icon" aria-hidden="true"></i>
        </button>
        <button class="workspace-icon-btn" title="清空" aria-label="清空" @click="clearWorkspaceCurrent">
          <i class="fa-solid fa-trash-can workspace-icon" aria-hidden="true"></i>
        </button>
        <button class="workspace-icon-btn" title="上传" aria-label="上传" @click="triggerUpload">
          <i class="fa-solid fa-upload workspace-icon" aria-hidden="true"></i>
        </button>
        <button class="workspace-icon-btn" title="全下" aria-label="全下" @click="downloadArchive">
          <i class="fa-solid fa-download workspace-icon" aria-hidden="true"></i>
        </button>
      </div>
    </div>

    <div class="workspace-path">{{ displayPath }}</div>

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
      :class="['workspace-list', { dragover: draggingOver, virtual: workspaceVirtual }]"
      @scroll="handleListScroll"
      @dragenter="handleListDragEnter"
      @dragover="handleListDragOver"
      @dragleave="handleListDragLeave"
      @drop="handleListDrop"
      @contextmenu.prevent="openContextMenu($event, null)"
    >
      <div v-if="loading" class="workspace-empty">加载中...</div>
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
              aria-label="展开目录"
              @click.stop="toggleWorkspaceDirectory(item.entry)"
            >
              <i class="fa-solid fa-chevron-right workspace-caret-icon" aria-hidden="true"></i>
            </button>
            <span
              :class="['workspace-item-icon', getEntryIcon(item.entry).className]"
              :title="getEntryIcon(item.entry).label"
            >
              {{ getEntryIcon(item.entry).text }}
            </span>
            <div class="workspace-item-name">
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
          placeholder="搜索工作区（名称）"
          @input="handleSearchInput"
          @keydown="handleSearchKeydown"
        />
      </div>
      <div class="workspace-selection-meta muted">{{ selectionMeta }}</div>
    </div>

    <input ref="uploadInputRef" type="file" multiple style="display: none" @change="handleUploadInput" />

    <div
      v-show="contextMenu.visible"
      ref="menuRef"
      class="workspace-context-menu"
      :style="menuStyle"
      @contextmenu.prevent
    >
      <button class="workspace-menu-btn" @click="handleNewFile">新建文件</button>
      <button class="workspace-menu-btn" :disabled="!canEdit" @click="handleEdit">编辑</button>
      <button class="workspace-menu-btn" :disabled="!singleSelectedEntry" @click="handleRename">
        重命名
      </button>
      <button class="workspace-menu-btn" :disabled="!hasSelection" @click="handleMove">
        移动
      </button>
      <button class="workspace-menu-btn" :disabled="!hasSelection" @click="handleCopy">
        复制
      </button>
      <button class="workspace-menu-btn" @click="handleNewFolder">新建文件夹</button>
      <button class="workspace-menu-btn" :disabled="!singleSelectedEntry" @click="handleDownload">
        下载
      </button>
      <button class="workspace-menu-btn danger" :disabled="!hasSelection" @click="handleDelete">
        删除
      </button>
    </div>

    <el-dialog v-model="preview.visible" title="文件预览" width="720px" class="workspace-dialog" append-to-body>
      <div class="workspace-preview-title">{{ preview.entry?.name || '文件预览' }}</div>
      <div class="workspace-preview-meta">{{ previewMeta }}</div>
      <div v-if="preview.hint" class="workspace-preview-hint">{{ preview.hint }}</div>
      <div class="workspace-preview" :class="{ embed: preview.embed, 'is-svg': preview.type === 'svg' }">
        <div v-if="preview.loading" class="workspace-empty">预览加载中...</div>
        <template v-else>
          <img v-if="preview.embed && preview.type === 'image'" :src="preview.url" />
          <iframe v-else-if="preview.embed && (preview.type === 'pdf' || preview.type === 'svg')" :src="preview.url" />
          <pre v-else class="workspace-preview-text">{{ preview.content }}</pre>
        </template>
      </div>
      <template #footer>
        <button class="workspace-btn secondary" @click="downloadPreview">下载</button>
        <button class="workspace-btn secondary" @click="closePreview">关闭</button>
      </template>
    </el-dialog>

    <el-dialog v-model="editor.visible" title="编辑文件" width="720px" class="workspace-dialog" append-to-body>
      <div class="workspace-preview-title">{{ editor.entry?.name || '编辑文件' }}</div>
      <div class="workspace-preview-meta">{{ editor.entry?.path || '' }}</div>
      <textarea
        v-model="editor.content"
        class="workspace-editor-text"
        :disabled="editor.loading"
        placeholder="加载中..."
      />
      <template #footer>
        <button class="workspace-btn secondary" @click="closeEditor">关闭</button>
        <button class="workspace-btn" :disabled="editor.loading" @click="saveEditor">保存</button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
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
import { onWorkspaceRefresh } from '@/utils/workspaceEvents';

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  }
});

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
const MAX_TEXT_PREVIEW_SIZE = 512 * 1024;
// 沙盒容器上传总大小上限（对齐 Wunder 配置）
const MAX_WORKSPACE_UPLOAD_BYTES = 200 * 1024 * 1024;
const WORKSPACE_DRAG_KEY = 'application/x-wunder-workspace-entry';
const WORKSPACE_SEARCH_DEBOUNCE_MS = 300;
const WORKSPACE_AUTO_REFRESH_DEBOUNCE_MS = 400;

const normalizedAgentId = computed(() => String(props.agentId || '').trim());

const withAgentParams = (params = {}) => {
  const agentId = normalizedAgentId.value;
  if (!agentId) return params;
  return { ...params, agent_id: agentId };
};

const appendAgentId = (formData) => {
  const agentId = normalizedAgentId.value;
  if (agentId) {
    formData.append('agent_id', agentId);
  }
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
    y: 0
  }
});

const displayPath = computed(() => (state.path ? `/${state.path}` : '/'));
const canGoUp = computed(() => Boolean(state.path));
const selectedEntry = computed(() => state.selected);
const loading = computed(() => state.loading);
const draggingOver = computed(() => state.draggingOver);
const preview = computed(() => state.preview);
const editor = computed(() => state.editor);
const contextMenu = computed(() => state.contextMenu);
const selectedCount = computed(() => state.selectedPaths.size);
const selectionMeta = computed(() => (selectedCount.value ? `已选择 ${selectedCount.value} 项` : ''));
const emptyText = computed(() => (state.searchMode ? '未找到匹配文件。' : '暂无文件'));
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
const WORKSPACE_ROW_HEIGHT = 36;
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
  workspaceVirtual.value
    ? displayEntries.value.slice(workspaceStartIndex.value, workspaceEndIndex.value)
    : displayEntries.value
);
const workspacePaddingTop = computed(() =>
  workspaceVirtual.value ? workspaceStartIndex.value * WORKSPACE_ROW_HEIGHT : 0
);
const workspacePaddingBottom = computed(() =>
  workspaceVirtual.value
    ? Math.max(0, (displayEntries.value.length - workspaceEndIndex.value) * WORKSPACE_ROW_HEIGHT)
    : 0
);
const flatEntries = computed(() => displayEntries.value.map((item) => item.entry));
const singleSelectedEntry = computed(() => {
  if (state.selectedPaths.size !== 1) {
    return null;
  }
  const [path] = Array.from(state.selectedPaths);
  return findWorkspaceEntry(state.entries, path) || state.selected;
});
const hasSelection = computed(() => selectedCount.value > 0);
const canEdit = computed(() => singleSelectedEntry.value && isWorkspaceTextEditable(singleSelectedEntry.value));
const menuStyle = computed(() => ({ left: `${state.contextMenu.x}px`, top: `${state.contextMenu.y}px` }));
const uploadProgressText = computed(() => {
  if (!uploadProgress.active) return '';
  const baseLabel = '上传';
  const hasTotal = Number.isFinite(uploadProgress.total) && uploadProgress.total > 0;
  const hasLoaded = Number.isFinite(uploadProgress.loaded) && uploadProgress.loaded > 0;
  if (hasTotal) {
    const safePercent = Math.max(
      0,
      Math.min(100, Math.round((uploadProgress.loaded / uploadProgress.total) * 100))
    );
    return `${baseLabel} ${safePercent}% · ${formatBytes(uploadProgress.loaded)} / ${formatBytes(
      uploadProgress.total
    )}`;
  }
  if (hasLoaded) {
    return `${baseLabel} · ${formatBytes(uploadProgress.loaded)}`;
  }
  return `${baseLabel}...`;
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
let stopWorkspaceRefreshListener = null;

const normalizeWorkspacePath = (path) => {
  if (!path) return '';
  return String(path).replace(/\\/g, '/').replace(/^\/+/, '');
};

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

const getEntryIcon = (entry) => {
  if (entry.type === 'dir') return { text: 'D', className: 'icon-folder', label: '目录' };
  const ext = getWorkspaceExtension(entry);
  if (IMAGE_EXTENSIONS.has(ext)) return { text: 'I', className: 'icon-image', label: '图片' };
  if (PDF_EXTENSIONS.has(ext)) return { text: 'P', className: 'icon-pdf', label: 'PDF' };
  if (OFFICE_WORD_EXTENSIONS.has(ext)) return { text: 'W', className: 'icon-word', label: 'Word' };
  if (OFFICE_EXCEL_EXTENSIONS.has(ext)) return { text: 'X', className: 'icon-excel', label: 'Excel' };
  if (OFFICE_PPT_EXTENSIONS.has(ext)) return { text: 'P', className: 'icon-ppt', label: 'PPT' };
  if (ARCHIVE_EXTENSIONS.has(ext)) return { text: 'Z', className: 'icon-archive', label: '压缩包' };
  if (AUDIO_EXTENSIONS.has(ext)) return { text: 'A', className: 'icon-audio', label: '音频' };
  if (VIDEO_EXTENSIONS.has(ext)) return { text: 'V', className: 'icon-video', label: '视频' };
  if (CODE_EXTENSIONS.has(ext)) return { text: 'C', className: 'icon-code', label: '代码' };
  if (TEXT_EXTENSIONS.has(ext)) return { text: 'T', className: 'icon-text', label: '文本' };
  if (OFFICE_EXTENSIONS.has(ext)) return { text: 'O', className: 'icon-office', label: '办公文档' };
  return { text: 'F', className: 'icon-file', label: '文件' };
};

const getEntryMeta = (entry) => {
  const parts = [];
  if (entry.type === 'dir') {
    parts.push('目录');
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

// 统一管理上传进度条展示，避免并发上传导致状态错乱
const setUploadProgress = (options = {}) => {
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

const confirmAction = async (message, title = '提示') => {
  try {
    await ElMessageBox.confirm(message, title, {
      confirmButtonText: '确定',
      cancelButtonText: '取消',
      type: 'warning'
    });
    return true;
  } catch (error) {
    return false;
  }
};

const promptInput = async (message, options = {}) => {
  const {
    title = '提示',
    placeholder = '',
    defaultValue = ''
  } = options;
  try {
    const { value } = await ElMessageBox.prompt(message, title, {
      confirmButtonText: '确定',
      cancelButtonText: '取消',
      inputValue: defaultValue,
      inputPlaceholder: placeholder
    });
    return value;
  } catch (error) {
    return null;
  }
};

const loadWorkspace = async ({ path = state.path, resetExpanded = false, resetSearch = false } = {}) => {
  state.loading = true;
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
  const currentPath = normalizeWorkspacePath(path);
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
    if (state.expanded.size) {
      const filtered = new Set();
      state.expanded.forEach((value) => {
        if (!normalizedPath || value === normalizedPath || value.startsWith(`${normalizedPath}/`)) {
          filtered.add(value);
        }
      });
      state.expanded = filtered;
    }
    await hydrateExpandedEntries();
    return true;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工作区加载失败');
    state.entries = [];
    return false;
  } finally {
    state.loading = false;
    if (autoRefreshPending) {
      scheduleWorkspaceAutoRefresh();
    }
  }
};

const loadWorkspaceSearch = async () => {
  const keyword = String(state.searchKeyword || '').trim();
  if (!keyword) {
    state.searchMode = false;
    return loadWorkspace({ resetSearch: true });
  }
  state.loading = true;
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
    return true;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '搜索失败');
    state.entries = [];
    return false;
  } finally {
    state.loading = false;
    if (autoRefreshPending) {
      scheduleWorkspaceAutoRefresh();
    }
  }
};

const reloadWorkspaceView = async () => {
  if (state.searchMode && String(state.searchKeyword || '').trim()) {
    return loadWorkspaceSearch();
  }
  return loadWorkspace({ resetSearch: true });
};

const scheduleWorkspaceAutoRefresh = () => {
  autoRefreshPending = true;
  if (autoRefreshTimer) return;
  autoRefreshTimer = setTimeout(async () => {
    autoRefreshTimer = null;
    if (!autoRefreshPending) return;
    if (state.loading) {
      scheduleWorkspaceAutoRefresh();
      return;
    }
    autoRefreshPending = false;
    await reloadWorkspaceView();
  }, WORKSPACE_AUTO_REFRESH_DEBOUNCE_MS);
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
  } catch (error) {
    state.expanded.delete(entry.path);
    state.expanded = new Set(state.expanded);
    ElMessage.error(error.response?.data?.detail || '目录展开失败');
  }
};

const refreshWorkspace = async () => {
  const ok = await reloadWorkspaceView();
  if (ok) {
    ElMessage.success(state.searchMode ? '搜索结果已刷新' : '工作区已刷新');
  }
};

const clearWorkspaceCurrent = async () => {
  const display = displayPath.value;
  try {
    await ElMessageBox.confirm(`确认清空 ${display} 下所有内容吗？`, '清空沙盒容器', {
      confirmButtonText: '清空',
      cancelButtonText: '取消',
      type: 'warning'
    });
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
      ElMessage.info('当前目录为空，无需清空');
      return;
    }
    const response = await batchWunderWorkspaceAction(withAgentParams({
      action: 'delete',
      paths: entries.map((entry) => entry.path)
    }));
    notifyBatchResult(response.data, '清空');
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '清空失败');
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

const uploadWorkspaceFiles = async (files, targetPath, options = {}) => {
  if (!files || !files.length) return;
  const { refreshTree = true, relativePaths = [] } = options;
  const fileList = Array.from(files);
  const totalBytes = fileList.reduce((sum, file) => sum + (Number(file?.size) || 0), 0);
  // 对齐 Wunder 上传限制：单次上传总大小不超过 200MB
  if (totalBytes > MAX_WORKSPACE_UPLOAD_BYTES) {
    throw new Error(`上传文件总大小超过限制（上限 ${formatBytes(MAX_WORKSPACE_UPLOAD_BYTES)}）`);
  }
  const formData = new FormData();
  formData.append('path', normalizeWorkspacePath(targetPath));
  appendAgentId(formData);
  fileList.forEach((file, index) => {
    formData.append('files', file);
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
    await reloadWorkspaceView();
  }
};

const handleUploadInput = async (event) => {
  const files = Array.from(event.target.files || []);
  if (!files.length) return;
  try {
    await uploadWorkspaceFiles(files, state.path);
    ElMessage.success('上传完成');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || error.message || '上传失败');
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
    state.path = entry.path || '';
    state.expanded = new Set();
    loadWorkspace({ resetExpanded: true, resetSearch: true });
    return;
  }
  openPreview(entry);
};

const closeContextMenu = () => {
  state.contextMenu.visible = false;
};

const openContextMenu = async (event, entry) => {
  if (entry?.path && !state.selectedPaths.has(entry.path)) {
    setWorkspaceSelection([entry.path], entry.path);
  }
  if (entry?.path) {
    state.selected = entry;
  }
  state.contextMenu.visible = true;
  state.contextMenu.x = event.clientX;
  state.contextMenu.y = event.clientY;
  await nextTick();
  const menuRect = menuRef.value?.getBoundingClientRect();
  if (!menuRect) return;
  const maxLeft = window.innerWidth - menuRect.width - 8;
  const maxTop = window.innerHeight - menuRect.height - 8;
  state.contextMenu.x = Math.min(state.contextMenu.x, maxLeft);
  state.contextMenu.y = Math.min(state.contextMenu.y, maxTop);
};

const handleEdit = () => {
  closeContextMenu();
  if (!singleSelectedEntry.value) return;
  openEditor(singleSelectedEntry.value);
};

const handleRename = () => {
  closeContextMenu();
  if (!singleSelectedEntry.value) return;
  startWorkspaceRename(singleSelectedEntry.value);
};

const handleMove = async () => {
  closeContextMenu();
  if (!hasSelection.value) return;
  if (selectedCount.value > 1) {
    await moveWorkspaceSelectionToDirectory();
    return;
  }
  await moveWorkspaceEntryToDirectory(singleSelectedEntry.value);
};

const handleCopy = async () => {
  closeContextMenu();
  if (!hasSelection.value) return;
  await copyWorkspaceSelectionToDirectory();
};

const handleNewFile = async () => {
  closeContextMenu();
  await createWorkspaceFile();
};

const handleNewFolder = async () => {
  closeContextMenu();
  await createWorkspaceFolder();
};

const handleDownload = async () => {
  closeContextMenu();
  if (!singleSelectedEntry.value) return;
  await downloadEntry(singleSelectedEntry.value);
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
    ElMessage.warning('名称不能为空且不能包含斜杠');
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
    await reloadWorkspaceView();
    ElMessage.success('已重命名');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '重命名失败');
  } finally {
    state.renamingValue = '';
  }
};

const notifyBatchResult = (payload, actionLabel) => {
  const data = payload?.data || {};
  const failed = Array.isArray(data.failed) ? data.failed : [];
  const succeeded = Array.isArray(data.succeeded) ? data.succeeded : [];
  if (failed.length) {
    ElMessage.warning(`${actionLabel}完成：成功 ${succeeded.length} 项，失败 ${failed.length} 项`);
  } else {
    ElMessage.success(`${actionLabel}完成`);
  }
};

const deleteWorkspaceSelection = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) return;
  const confirmed = await confirmAction(
    selectedPaths.length === 1 ? '确认删除所选条目吗？' : `确认删除所选 ${selectedPaths.length} 项吗？`
  );
  if (!confirmed) return;
  try {
    const response = await batchWunderWorkspaceAction(
      withAgentParams({ action: 'delete', paths: selectedPaths })
    );
    notifyBatchResult(response.data, '删除');
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '删除失败');
  }
};

const moveWorkspaceSelectionToDirectory = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) {
    ElMessage.info('未选择任何条目');
    return;
  }
  const targetDirInput = await promptInput('请输入目标目录（相对路径，留空为根目录）', {
    placeholder: '例如：project/docs',
    defaultValue: ''
  });
  if (targetDirInput === null) return;
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    ElMessage.warning('目录格式不正确，不能包含非法路径段');
    return;
  }
  try {
    const response = await batchWunderWorkspaceAction(withAgentParams({
      action: 'move',
      paths: selectedPaths,
      destination: targetDir
    }));
    notifyBatchResult(response.data, '移动');
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '移动失败');
  }
};

const copyWorkspaceSelectionToDirectory = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
  if (!selectedPaths.length) {
    ElMessage.info('未选择任何条目');
    return;
  }
  const targetDirInput = await promptInput('请输入目标目录（相对路径，留空为根目录）', {
    placeholder: '例如：project/docs',
    defaultValue: ''
  });
  if (targetDirInput === null) return;
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    ElMessage.warning('目录格式不正确，不能包含非法路径段');
    return;
  }
  try {
    const response = await batchWunderWorkspaceAction(withAgentParams({
      action: 'copy',
      paths: selectedPaths,
      destination: targetDir
    }));
    notifyBatchResult(response.data, '复制');
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '复制失败');
  }
};

const moveWorkspaceEntryToDirectory = async (entry) => {
  if (!entry) return;
  const targetDirInput = await promptInput('请输入目标目录（相对路径，留空为根目录）', {
    placeholder: '例如：project/docs',
    defaultValue: ''
  });
  if (targetDirInput === null) return;
  const targetDir = normalizeWorkspacePath(targetDirInput.trim());
  if (!isValidWorkspacePath(targetDir)) {
    ElMessage.warning('目录格式不正确，不能包含非法路径段');
    return;
  }
  const sourceName = entry.name || entry.path.split('/').pop();
  if (!sourceName) {
    ElMessage.error('无法解析源文件名称');
    return;
  }
  const destination = joinWorkspacePath(targetDir, sourceName);
  if (destination === entry.path) {
    ElMessage.info('目标目录与当前目录一致');
    return;
  }
  try {
    await moveWunderWorkspaceEntry(withAgentParams({ source: entry.path, destination }));
    await reloadWorkspaceView();
    ElMessage.success(`已移动到 ${targetDir || '/'}`);
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '移动失败');
  }
};

const createWorkspaceFile = async () => {
  const fileName = await promptInput('请输入新文件名称', {
    placeholder: '例如：notes.txt',
    defaultValue: 'untitled.txt'
  });
  if (fileName === null) return;
  const trimmed = String(fileName || '').trim();
  if (!isValidWorkspaceName(trimmed)) {
    ElMessage.warning('名称不能为空且不能包含斜杠');
    return;
  }
  const targetPath = joinWorkspacePath(state.path, trimmed);
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({ path: targetPath, content: '', create_if_missing: true })
    );
    await reloadWorkspaceView();
    ElMessage.success(`已创建文件 ${trimmed}`);
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '创建文件失败');
  }
};

const createWorkspaceFolder = async () => {
  const folderName = await promptInput('请输入新文件夹名称', {
    placeholder: '例如：docs'
  });
  if (folderName === null) return;
  const trimmed = String(folderName || '').trim();
  if (!isValidWorkspaceName(trimmed)) {
    ElMessage.warning('名称不能为空且不能包含斜杠');
    return;
  }
  const targetPath = joinWorkspacePath(state.path, trimmed);
  try {
    await createWunderWorkspaceDir(withAgentParams({ path: targetPath }));
    await reloadWorkspaceView();
    ElMessage.success('文件夹已创建');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '创建失败');
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
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
};

const downloadEntry = async (entry) => {
  try {
    if (entry.type === 'dir') {
      const response = await downloadWunderWorkspaceArchive(withAgentParams({ path: entry.path }));
      const filename = getFilenameFromHeaders(response.headers, `${entry.name || 'folder'}.zip`);
      saveBlob(response.data, filename);
      return;
    }
    const response = await downloadWunderWorkspaceFile(withAgentParams({ path: entry.path }));
    const filename = getFilenameFromHeaders(response.headers, entry.name || 'download');
    saveBlob(response.data, filename);
  } catch (error) {
    ElMessage.error('下载失败');
  }
};

const downloadArchive = async () => {
  try {
    const response = await downloadWunderWorkspaceArchive(withAgentParams({}));
    const filename = getFilenameFromHeaders(response.headers, 'workspace.zip');
    saveBlob(response.data, filename);
    ElMessage.success('压缩包已下载');
  } catch (error) {
    ElMessage.error('压缩包下载失败');
  }
};

const readDirectoryEntries = (reader) =>
  new Promise((resolve) => {
    const entries = [];
    const readBatch = () => {
      reader.readEntries(
        (batch) => {
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

const walkEntry = async (entry, prefix) => {
  if (!entry) return [];
  if (entry.isFile) {
    const file = await new Promise((resolve) => {
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

const collectDroppedFiles = async (dataTransfer) => {
  const items = Array.from(dataTransfer?.items || []);
  if (items.length) {
    const batches = await Promise.all(
      items.map((item) => {
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
  await reloadWorkspaceView();
};

const hasWorkspaceDrag = (dataTransfer) =>
  Array.from(dataTransfer?.types || []).includes(WORKSPACE_DRAG_KEY);

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
    ElMessage.success('拖拽上传完成');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || error.message || '拖拽上传失败');
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
  if (!entry || entry.type !== 'dir') return;
  event.preventDefault();
  event.currentTarget?.classList?.add('drop-target');
};

const handleItemDragOver = (event, entry) => {
  if (!entry || entry.type !== 'dir') return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = hasWorkspaceDrag(event.dataTransfer) ? 'move' : 'copy';
  }
};

const handleItemDragLeave = (event, entry) => {
  if (!entry || entry.type !== 'dir') return;
  if (!event.currentTarget?.contains(event.relatedTarget)) {
    event.currentTarget?.classList?.remove('drop-target');
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
    ElMessage.warning('不能移动到自身或子目录');
  }
  return filtered;
};

const handleItemDrop = async (event, entry) => {
  event.preventDefault();
  event.stopPropagation();
  event.currentTarget?.classList?.remove('drop-target');
  state.draggingOver = false;
  if (!entry || entry.type !== 'dir') return;
  const internalPaths = getWorkspaceDragPaths(event.dataTransfer);
  if (internalPaths.length) {
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
      notifyBatchResult(response.data, `移动到 ${entry.name || '目录'}`);
      await reloadWorkspaceView();
    } catch (error) {
      ElMessage.error(error.response?.data?.detail || '移动失败');
    }
    return;
  }
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) return;
  try {
    await uploadWorkspaceGroups(dropped, entry.path);
    ElMessage.success('拖拽上传完成');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || error.message || '拖拽上传失败');
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
    notifyBatchResult(response.data, '移动到上级目录');
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '移动失败');
  }
};

const clearPreviewUrl = () => {
  if (state.preview.url) {
    URL.revokeObjectURL(state.preview.url);
  }
  state.preview.url = '';
};

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
    state.preview.hint = '浏览器不支持该格式预览，请下载后使用本地程序打开。';
    state.preview.content = '暂无预览';
    state.preview.loading = false;
    return;
  }
  const isMediaPreview = IMAGE_EXTENSIONS.has(extension) || PDF_EXTENSIONS.has(extension);
  if (!isMediaPreview && !canPreviewText) {
    state.preview.hint = '文件过大，无法预览，请下载查看。';
    state.preview.content = '暂无预览';
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
          state.preview.hint = '内容已截断，建议下载查看完整文件。';
          state.preview.content = '暂无预览';
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
      state.preview.hint = '内容已截断，建议下载查看完整文件。';
    }
    const text = typeof payload.content === 'string' ? payload.content : '';
    state.preview.content = text || '暂无内容';
  } catch (error) {
    state.preview.hint = '预览加载失败，请下载查看。';
    state.preview.content = '暂无预览';
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
    ElMessage.warning('仅支持预览范围内的文本文件编辑');
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
      ElMessage.warning('文件过大，无法编辑，请下载查看。');
      closeEditor();
      return;
    }
    state.editor.content = typeof payload.content === 'string' ? payload.content : '';
  } catch (error) {
    ElMessage.error('文件加载失败');
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
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({
        path: state.editor.entry.path,
        content: state.editor.content
      })
    );
    ElMessage.success('已保存');
    closeEditor();
    await reloadWorkspaceView();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '保存失败');
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
  await loadWorkspace();
  stopWorkspaceRefreshListener = onWorkspaceRefresh(() => scheduleWorkspaceAutoRefresh());
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

onBeforeUnmount(() => {
  clearPreviewUrl();
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
