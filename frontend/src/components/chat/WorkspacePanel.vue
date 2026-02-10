<template>
  <div class="workspace-panel">
    <div class="workspace-header">
      <div class="workspace-title-row">
        <div class="workspace-title">{{ t('workspace.title') }}</div>
        <div class="workspace-container-id">{{ normalizedContainerId }}</div>
      </div>
      <div class="workspace-header-actions">
        <button
          class="workspace-icon-btn"
          :disabled="!canGoUp"
          :title="t('workspace.action.up')"
          :aria-label="t('workspace.action.up')"
          @click="handleGoUp"
          @dragover="handleUpDragOver"
          @dragleave="handleUpDragLeave"
          @drop="handleUpDrop"
        >
          <i class="fa-solid fa-arrow-up workspace-icon" aria-hidden="true"></i>
        </button>
        <button
          class="workspace-icon-btn"
          :title="t('common.refresh')"
          :aria-label="t('common.refresh')"
          @click="refreshWorkspace"
        >
          <i class="fa-solid fa-rotate workspace-icon" aria-hidden="true"></i>
        </button>
        <button
          class="workspace-icon-btn"
          :title="t('workspace.action.clear')"
          :aria-label="t('workspace.action.clear')"
          @click="clearWorkspaceCurrent"
        >
          <i class="fa-solid fa-trash-can workspace-icon" aria-hidden="true"></i>
        </button>
        <button
          class="workspace-icon-btn"
          :title="t('common.upload')"
          :aria-label="t('common.upload')"
          @click="triggerUpload"
        >
          <i class="fa-solid fa-upload workspace-icon" aria-hidden="true"></i>
        </button>
        <button
          class="workspace-icon-btn"
          :title="t('workspace.action.downloadAll')"
          :aria-label="t('workspace.action.downloadAll')"
          @click="downloadArchive"
        >
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
      <div v-if="loading" class="workspace-empty">{{ t('common.loading') }}</div>
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
            <span
              :class="['workspace-item-icon', getEntryIcon(item.entry).className]"
              :title="getEntryIcon(item.entry).label"
            >
              <img
                class="workspace-item-icon-img"
                :src="getEntryIcon(item.entry).icon"
                :alt="getEntryIcon(item.entry).label"
              />
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
          :placeholder="t('workspace.search.placeholder')"
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
      <button class="workspace-menu-btn" @click="handleNewFile">
        {{ t('workspace.menu.newFile') }}
      </button>
      <button class="workspace-menu-btn" :disabled="!canEdit" @click="handleEdit">
        {{ t('common.edit') }}
      </button>
      <button class="workspace-menu-btn" :disabled="!singleSelectedEntry" @click="handleRename">
        {{ t('workspace.menu.rename') }}
      </button>
      <button class="workspace-menu-btn" :disabled="!hasSelection" @click="handleMove">
        {{ t('workspace.menu.move') }}
      </button>
      <button class="workspace-menu-btn" :disabled="!hasSelection" @click="handleCopy">
        {{ t('workspace.menu.copy') }}
      </button>
      <button class="workspace-menu-btn" @click="handleNewFolder">
        {{ t('workspace.menu.newFolder') }}
      </button>
      <button class="workspace-menu-btn" :disabled="!singleSelectedEntry" @click="handleDownload">
        {{ t('common.download') }}
      </button>
      <button class="workspace-menu-btn danger" :disabled="!hasSelection" @click="handleDelete">
        {{ t('common.delete') }}
      </button>
    </div>

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
          <img v-if="preview.embed && preview.type === 'image'" :src="preview.url" />
          <iframe v-else-if="preview.embed && (preview.type === 'pdf' || preview.type === 'svg')" :src="preview.url" />
          <pre v-else class="workspace-preview-text">{{ preview.content }}</pre>
        </template>
      </div>
      <template #footer>
        <button class="workspace-btn secondary" @click="downloadPreview">
          {{ t('common.download') }}
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
import { useI18n } from '@/i18n';
import vscodeIconsTheme from '@/assets/vscode-icons-theme.json';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  },
  containerId: {
    type: [Number, String],
    default: 1
  }
});

const { t } = useI18n();

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
const WORKSPACE_ICON_BASE = `${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}vscode-icons`;
const WORKSPACE_DOC_ICON_BASE = `${(import.meta.env.BASE_URL || '/').replace(/\/+$/, '/')}doc-icons`;
const WORKSPACE_FOLDER_ICON = `${WORKSPACE_DOC_ICON_BASE}/folder.png`;
const WORKSPACE_ICON_PATH_RE = /^(\.\.\/|\.\/)+/;
const ICON_DEFINITIONS = (vscodeIconsTheme?.iconDefinitions || {}) as Record<string, unknown>;
const FILE_EXTENSION_ICON_MAP = new Map(
  Object.entries(vscodeIconsTheme?.fileExtensions || {}).map(([key, value]) => [
    String(key).toLowerCase(),
    value
  ])
);
const FALLBACK_EXTENSION_ICON_ENTRIES: Array<[string, string]> = [
  ['7z', '_f_zip'],
  ['aac', '_f_audio'],
  ['adoc', '_f_asciidoc'],
  ['astro', '_f_astro'],
  ['avi', '_f_video'],
  ['bash', '_f_shell'],
  ['bat', '_f_bat'],
  ['bmp', '_f_image'],
  ['bz2', '_f_zip'],
  ['c', '_f_c'],
  ['cc', '_f_cpp'],
  ['cfg', '_f_config'],
  ['cjs', '_f_js'],
  ['clj', '_f_clojure'],
  ['cljc', '_f_clojure'],
  ['cljs', '_f_clojure'],
  ['cmd', '_f_bat'],
  ['coffee', '_f_coffeescript'],
  ['conf', '_f_config'],
  ['cpp', '_f_cpp'],
  ['cs', '_f_csharp'],
  ['css', '_f_css'],
  ['csv', '_f_text'],
  ['cts', '_f_typescript'],
  ['cxx', '_f_cpp'],
  ['dart', '_f_dartlang'],
  ['db', '_f_db'],
  ['doc', '_f_word'],
  ['docx', '_f_word'],
  ['env', '_f_dotenv'],
  ['erb', '_f_erb'],
  ['erl', '_f_erlang'],
  ['ex', '_f_elixir'],
  ['exs', '_f_elixir'],
  ['fish', '_f_shell'],
  ['flac', '_f_audio'],
  ['fs', '_f_fsharp'],
  ['fsi', '_f_fsharp'],
  ['fsx', '_f_fsharp'],
  ['gif', '_f_image'],
  ['go', '_f_go'],
  ['gql', '_f_graphql'],
  ['gradle', '_f_gradle'],
  ['graphql', '_f_graphql'],
  ['groovy', '_f_groovy'],
  ['gz', '_f_zip'],
  ['h', '_f_c'],
  ['hpp', '_f_cpp'],
  ['hrl', '_f_erlang'],
  ['hs', '_f_haskell'],
  ['htm', '_f_html'],
  ['html', '_f_html'],
  ['hxx', '_f_cpp'],
  ['ico', '_f_image'],
  ['ini', '_f_ini'],
  ['ipynb', '_f_jupyter'],
  ['java', '_f_java'],
  ['jpeg', '_f_image'],
  ['jpg', '_f_image'],
  ['js', '_f_js'],
  ['json', '_f_json'],
  ['json5', '_f_json5'],
  ['jsonc', '_f_json'],
  ['jsonl', '_f_json'],
  ['jsx', '_f_reactjs'],
  ['kt', '_f_kotlin'],
  ['kts', '_f_kotlin'],
  ['less', '_f_less'],
  ['lhs', '_f_haskell'],
  ['log', '_f_log'],
  ['lua', '_f_lua'],
  ['m', '_f_objectivec'],
  ['m4a', '_f_audio'],
  ['markdown', '_f_markdown'],
  ['md', '_f_markdown'],
  ['mdx', '_f_mdx'],
  ['mjs', '_f_js'],
  ['mkv', '_f_video'],
  ['ml', '_f_ocaml'],
  ['mli', '_f_ocaml'],
  ['mm', '_f_objectivec'],
  ['mov', '_f_video'],
  ['mp3', '_f_audio'],
  ['mp4', '_f_video'],
  ['mts', '_f_typescript'],
  ['nim', '_f_nim'],
  ['nimble', '_f_nimble'],
  ['ogg', '_f_audio'],
  ['pdf', '_f_pdf'],
  ['php', '_f_php'],
  ['phtml', '_f_php'],
  ['pl', '_f_perl'],
  ['pm', '_f_perl'],
  ['png', '_f_image'],
  ['postcss', '_f_postcss'],
  ['ppt', '_f_powerpoint'],
  ['pptx', '_f_powerpoint'],
  ['proto', '_f_protobuf'],
  ['ps1', '_f_powershell'],
  ['py', '_f_python'],
  ['pyi', '_f_python'],
  ['pyw', '_f_python'],
  ['r', '_f_r'],
  ['rar', '_f_zip'],
  ['rb', '_f_ruby'],
  ['rmd', '_f_rmd'],
  ['rs', '_f_rust'],
  ['rst', '_f_markdown'],
  ['sass', '_f_sass'],
  ['sc', '_f_scala'],
  ['scala', '_f_scala'],
  ['scss', '_f_scss'],
  ['sh', '_f_shell'],
  ['sql', '_f_sql'],
  ['sqlite', '_f_sqlite'],
  ['styl', '_f_stylus'],
  ['stylus', '_f_stylus'],
  ['svelte', '_f_svelte'],
  ['svg', '_f_svg'],
  ['swift', '_f_swift'],
  ['tar', '_f_zip'],
  ['tex', '_f_tex'],
  ['tgz', '_f_zip'],
  ['toml', '_f_toml'],
  ['ts', '_f_typescript'],
  ['tsv', '_f_text'],
  ['tsx', '_f_reactts'],
  ['txt', '_f_text'],
  ['vb', '_f_vb'],
  ['vue', '_f_vue'],
  ['wav', '_f_audio'],
  ['webm', '_f_video'],
  ['webp', '_f_image'],
  ['xhtml', '_f_html'],
  ['xls', '_f_excel'],
  ['xlsx', '_f_excel'],
  ['xml', '_f_xml'],
  ['xsd', '_f_xml'],
  ['xsl', '_f_xml'],
  ['xslt', '_f_xml'],
  ['xz', '_f_zip'],
  ['yaml', '_f_yaml'],
  ['yml', '_f_yaml'],
  ['zip', '_f_zip'],
  ['zsh', '_f_shell'],
];
const FALLBACK_EXTENSION_ICON_MAP = new Map<string, string>(
  FALLBACK_EXTENSION_ICON_ENTRIES.filter(([, iconId]) => Boolean(ICON_DEFINITIONS[iconId]))
);
const FILE_NAME_ICON_MAP = new Map(
  Object.entries(vscodeIconsTheme?.fileNames || {}).map(([key, value]) => [
    String(key).toLowerCase(),
    value
  ])
);
const EXTRA_ALLOWED_ICON_IDS = [
  '_f_babel',
  '_f_bun',
  '_f_cargo',
  '_f_composer',
  '_f_docker',
  '_f_editorconfig',
  '_f_eslint',
  '_f_git',
  '_f_go_package',
  '_f_jsconfig',
  '_f_maven',
  '_f_npm',
  '_f_pip',
  '_f_pnpm',
  '_f_poetry',
  '_f_prettier',
  '_f_pypi',
  '_f_rollup',
  '_f_stylelint',
  '_f_tsconfig',
  '_f_vite',
  '_f_webpack',
  '_f_yarn',
].filter((iconId) => ICON_DEFINITIONS[iconId]);

const DEFAULT_FILE_ICON_ID = vscodeIconsTheme?.file || '';
const ALLOWED_ICON_IDS = new Set(
  [DEFAULT_FILE_ICON_ID, ...FALLBACK_EXTENSION_ICON_MAP.values(), ...EXTRA_ALLOWED_ICON_IDS].filter(
    Boolean
  )
);
const MAX_TEXT_PREVIEW_SIZE = 512 * 1024;
// 沙盒容器上传总大小上限（对齐 Wunder 配置）
const MAX_WORKSPACE_UPLOAD_BYTES = 200 * 1024 * 1024;
const WORKSPACE_DRAG_KEY = 'application/x-wunder-workspace-entry';
const WORKSPACE_SEARCH_DEBOUNCE_MS = 300;
const WORKSPACE_AUTO_REFRESH_DEBOUNCE_MS = 400;

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
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
});

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
const selectionMeta = computed(() =>
  selectedCount.value ? t('workspace.selection', { count: selectedCount.value }) : ''
);
const emptyText = computed(() =>
  state.searchMode ? t('workspace.empty.search') : t('workspace.empty')
);
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

const normalizeIconKey = (value) => String(value || '').trim().toLowerCase();

const resolveThemeIconPath = (iconId, fallbackId = DEFAULT_FILE_ICON_ID) => {
  const resolvedId = iconId && ALLOWED_ICON_IDS.has(iconId) ? iconId : '';
  const definition = resolvedId
    ? (ICON_DEFINITIONS[resolvedId] as { iconPath?: string } | undefined)
    : null;
  const rawPath = definition?.iconPath || '';
  if (rawPath) {
    const normalized = rawPath.replace(WORKSPACE_ICON_PATH_RE, '');
    return `${WORKSPACE_ICON_BASE}/${normalized}`;
  }
  if (fallbackId && fallbackId !== iconId) {
    return resolveThemeIconPath(fallbackId, '');
  }
  return '';
};

const resolveFileIconId = (entry) => {
  const nameKey = normalizeIconKey(entry?.name);
  if (nameKey && FILE_NAME_ICON_MAP.has(nameKey)) {
    return FILE_NAME_ICON_MAP.get(nameKey);
  }
  const extension = normalizeIconKey(getWorkspaceExtension(entry));
  if (extension) {
    if (FILE_EXTENSION_ICON_MAP.has(extension)) {
      return FILE_EXTENSION_ICON_MAP.get(extension);
    }
    if (FALLBACK_EXTENSION_ICON_MAP.has(extension)) {
      return FALLBACK_EXTENSION_ICON_MAP.get(extension);
    }
  }
  return DEFAULT_FILE_ICON_ID;
};

const getEntryIcon = (entry) => {
  if (entry.type === 'dir') {
    return {
      icon: WORKSPACE_FOLDER_ICON,
      className: 'icon-vscode',
      label: t('workspace.icon.folder')
    };
  }
  const iconId = resolveFileIconId(entry);
  const icon =
    resolveThemeIconPath(iconId, DEFAULT_FILE_ICON_ID) ||
    resolveThemeIconPath(DEFAULT_FILE_ICON_ID, '');
  const ext = getWorkspaceExtension(entry);
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
      state.expanded.forEach((value: string) => {
        if (!normalizedPath || value === normalizedPath || value.startsWith(`${normalizedPath}/`)) {
          filtered.add(value);
        }
      });
      state.expanded = filtered;
    }
    await hydrateExpandedEntries();
    return true;
  } catch (error) {
    showApiError(error, t('workspace.loadFailed'));
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
    showApiError(error, t('workspace.searchFailed'));
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
    await reloadWorkspaceView();
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
    state.path = entry.path || '';
    state.expanded = new Set();
    loadWorkspace({ resetExpanded: true, resetSearch: true });
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
      await reloadWorkspaceView();
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

const deleteWorkspaceSelection = async () => {
  const selectedPaths = getWorkspaceSelectionPaths();
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
  const targetPath = joinWorkspacePath(state.path, trimmed);
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({ path: targetPath, content: '', create_if_missing: true })
    );
    await reloadWorkspaceView();
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
  const targetPath = joinWorkspacePath(state.path, trimmed);
  try {
    await createWunderWorkspaceDir(withAgentParams({ path: targetPath }));
    await reloadWorkspaceView();
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
    ElMessage.error(t('workspace.download.failed'));
  }
};

const downloadArchive = async () => {
  try {
    const response = await downloadWunderWorkspaceArchive(withAgentParams({}));
    const filename = getFilenameFromHeaders(response.headers, 'workspace.zip');
    saveBlob(response.data, filename);
    ElMessage.success(t('workspace.download.archiveSuccess'));
  } catch (error) {
    ElMessage.error(t('workspace.download.archiveFailed'));
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
    ElMessage.warning(t('workspace.move.blocked'));
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
      notifyBatchResult(response.data, t('workspace.move.toFolder', { name: entry.name || t('workspace.meta.folder') }));
      await reloadWorkspaceView();
    } catch (error) {
      showApiError(error, t('workspace.move.failed'));
    }
    return;
  }
  const dropped = await collectDroppedFiles(event.dataTransfer);
  if (!dropped.length) return;
  try {
    await uploadWorkspaceGroups(dropped, entry.path);
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
    state.preview.hint = t('workspace.preview.unsupportedHint');
    state.preview.content = t('workspace.preview.empty');
    state.preview.loading = false;
    return;
  }
  const isMediaPreview = IMAGE_EXTENSIONS.has(extension) || PDF_EXTENSIONS.has(extension);
  if (!isMediaPreview && !canPreviewText) {
    state.preview.hint = t('workspace.preview.tooLargeHint');
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
  try {
    await saveWunderWorkspaceFile(
      withAgentParams({
        path: state.editor.entry.path,
        content: state.editor.content
      })
    );
    ElMessage.success(t('common.saved'));
    closeEditor();
    await reloadWorkspaceView();
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
  await loadWorkspace();
  stopWorkspaceRefreshListener = onWorkspaceRefresh((event) => {
    const detail = event?.detail || {};
    const eventAgentId = String(detail.agentId ?? detail.agent_id ?? '').trim();
    const currentAgentId = normalizedAgentId.value;
    if (eventAgentId && eventAgentId !== currentAgentId) return;
    scheduleWorkspaceAutoRefresh();
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