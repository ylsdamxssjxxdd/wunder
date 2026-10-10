<template>
  <div class="interlink-remote-panel" data-testid="interlink-remote-panel">
    <header class="interlink-remote-head">
      <span class="interlink-remote-node">
        <span class="interlink-status-dot" :class="statusDotClass(node?.status || 'offline')" aria-hidden="true"></span>
        <i :class="nodeTypeIcon(node?.node_type || '')" aria-hidden="true"></i>
        <strong :title="nodeLabel">{{ nodeLabel }}</strong>
        <span class="interlink-remote-node-type">{{ t(nodeTypeLabelKey(node?.node_type || '')) }}</span>
        <span class="interlink-remote-node-status">{{ t(statusLabelKey(node?.status || 'offline')) }}</span>
      </span>
      <button class="interlink-remote-btn" type="button" @click="emit('back-to-cloud')">
        <i class="fa-solid fa-cloud" aria-hidden="true"></i>
        {{ t('interlink.workspace.backToCloud') }}
      </button>
    </header>

    <div class="interlink-remote-tabs" role="tablist">
      <button
        v-for="tab in TABS"
        :key="tab.id"
        class="interlink-remote-tab"
        :class="{ 'is-active': activeTab === tab.id }"
        type="button"
        role="tab"
        :aria-selected="activeTab === tab.id"
        @click="activeTab = tab.id"
      >
        {{ t(tab.labelKey) }}
      </button>
    </div>

    <p v-if="notice" class="interlink-remote-notice" :class="{ 'is-warn': noticeWarn }" aria-live="polite">
      {{ notice }}
    </p>

    <template v-if="activeTab === 'files'">
      <div class="interlink-remote-watermark">
        <span class="interlink-remote-watermark-text">
          <i class="fa-regular fa-clock" aria-hidden="true"></i>
          {{ watermarkText }}
          <template v-if="liveDirectoryCount">
            · {{ t('interlink.workspace.liveDirs', { count: liveDirectoryCount }) }}
          </template>
        </span>
        <button
          class="interlink-remote-link"
          type="button"
          :disabled="projectionLoading"
          @click="refreshProjection"
        >
          {{ t('interlink.workspace.refreshProjection') }}
        </button>
      </div>

      <div v-if="shadowTruncated" class="interlink-remote-note">
        {{ t('interlink.workspace.treeTruncated') }}
      </div>

      <WorkspaceFileTree
        :rows="treeRows"
        :active-path="treeActivePath"
        :highlighted-path="treeHighlightedPath"
        :loading="treeLoading"
        :error="treeErrorText"
        :selection-mode="false"
        :selected-paths="EMPTY_PATHS"
        read-only
        @toggle-dir="handleToggleDirectory"
        @load-more="handleLoadMore"
        @activate="handleActivate"
        @open="handleActivate"
        @retry="refreshProjection"
        @command="handleTreeCommand"
      />

      <div v-if="!projectionLoading && shadowIsEmpty" class="interlink-remote-note">
        {{ t('interlink.workspace.emptyShadow') }}
      </div>
    </template>

    <template v-else>
      <div class="interlink-remote-watermark">
        <span class="interlink-remote-watermark-text">
          <i class="fa-regular fa-clock" aria-hidden="true"></i>
          {{ watermarkText }}
        </span>
      </div>
      <div class="interlink-remote-thread-list">
        <div v-if="!threadRows.length" class="interlink-remote-empty">
          {{ t('interlink.workspace.noThreads') }}
        </div>
        <button
          v-for="item in threadRows"
          :key="item.local_thread_id"
          class="interlink-remote-thread-row"
          type="button"
          :disabled="Boolean(threadBlockedReason)"
          :title="threadBlockedReason || t('interlink.workspace.openRemoteThread')"
          @click="openRemoteThread(item)"
        >
          <span class="interlink-remote-thread-title" :title="item.title || item.local_thread_id">
            {{ item.title || item.local_thread_id }}
          </span>
          <span class="interlink-remote-thread-meta">
            {{ item.status || '-' }} · {{ t('interlink.workspace.messageCount', { count: item.message_count }) }} ·
            {{ formatWorkspaceTimestamp(item.updated_at) || '-' }}
          </span>
        </button>
        <div v-if="threadOverflow" class="interlink-remote-note">
          {{ t('interlink.workspace.threadsTruncated', { shown: threadRows.length, total: threadTotal }) }}
        </div>
      </div>
    </template>

    <RemoteSessionView
      v-if="activeThread"
      class="interlink-remote-session-host"
      :node="node"
      :target="target"
      :thread="activeThread"
      @close="activeThread = null"
      @back-to-cloud="handleBackToCloud"
    />

    <WorkspaceFilePreviewDialog
      :visible="preview.visible"
      :loading="preview.loading"
      :title="preview.title"
      :path="preview.path"
      :meta-label="preview.metaLabel"
      :hint="preview.hint"
      :error="preview.error"
      :too-large="preview.tooLarge"
      :editable="false"
      :saving="false"
      :preview-kind="preview.previewKind"
      :preview-url="preview.previewUrl"
      :content="preview.content"
      @close="closePreview"
      @download="handlePreviewDownload"
      @save="noop"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';

import type { InterlinkNode, InterlinkShadow, InterlinkShadowThread } from '@/api/interlink';
import { submitInterlinkCommand } from '@/api/interlink';
import { useI18n } from '@/i18n';
import WorkspaceFilePreviewDialog from '@/views/messenger/workspace/WorkspaceFilePreviewDialog.vue';
import WorkspaceFileTree from '@/views/messenger/workspace/WorkspaceFileTree.vue';
import {
  formatWorkspaceBytes,
  formatWorkspaceTimestamp,
  workspaceBaseName,
  workspaceFileExtension,
  type WorkspaceVisibleRow
} from '@/views/messenger/workspace/workspaceFileModel';
import { useWorkspaceFileTree } from '@/views/messenger/workspace/workspaceFileTree';
import {
  extractWorkspaceResourceExtension,
  normalizeWorkspacePreviewBlob,
  resolveWorkspacePreviewTooLargeHint,
  resolveWorkspacePreviewUnsupportedHint,
  resolveWorkspaceResourcePreviewKind
} from '@/utils/workspaceResourcePreview';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';
import { saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';

import {
  DISABLED_REASON_KEY,
  REMOTE_DIRECTORY_PAGE_SIZE,
  REMOTE_PREVIEW_MAX_BYTES,
  REMOTE_THREAD_MAX_RENDERED,
  WATERMARK_TICK_MS,
  groupShadowTreeByDirectory,
  isShadowEmpty,
  minutesSince,
  nodeTypeIcon,
  nodeTypeLabelKey,
  normalizeRemoteDirectoryEntries,
  normalizeShadowThreads,
  resolveDisabledReason,
  sortEntriesByName,
  sortShadowThreads,
  statusDotClass,
  statusLabelKey,
  supportsInterlinkKind
} from './interlinkNodeModel';
import {
  listRemoteDirectory,
  readRemoteFile
} from './interlinkCommandRunner';
import RemoteSessionView from './RemoteSessionView.vue';
import { useInterlinkShadow } from './useInterlinkShadow';

const props = defineProps<{
  node: InterlinkNode | null;
  deviceId: string;
  target: string;
  /** KeepAlive / 抽屉隐藏时停掉水印滴答，避免后台定时器。 */
  active?: boolean;
}>();

const emit = defineEmits<{
  'back-to-cloud': [];
}>();

const { t } = useI18n();

const TABS = [
  { id: 'files', labelKey: 'interlink.workspace.tabFiles' },
  { id: 'threads', labelKey: 'interlink.workspace.tabThreads' }
] as const;

type RemoteTabId = (typeof TABS)[number]['id'];

const EMPTY_PATHS: string[] = [];

const activeTab = ref<RemoteTabId>('files');
const notice = ref('');
const noticeWarn = ref(false);
const nowTick = ref(Date.now());
const liveDirectories = ref<Set<string>>(new Set());
const activeThread = ref<InterlinkShadowThread | null>(null);

const shadowApi = useInterlinkShadow();
const shadow = computed<InterlinkShadow | null>(() => shadowApi.shadow.value);

let abortHost = createAbortHost();
let tickTimer: ReturnType<typeof setInterval> | null = null;
let shadowSettleTimer: ReturnType<typeof setTimeout> | null = null;

/** 每次切节点/卸载都换一个新 controller，旧命令轮询不会继续回填状态。 */
function createAbortHost(): { controller: AbortController; signal: AbortSignal } {
  const controller = new AbortController();
  return { controller, signal: controller.signal };
}

const tree = useWorkspaceFileTree({
  pageSize: REMOTE_DIRECTORY_PAGE_SIZE,
  loadPage: async (path, pageOptions) => {
    const outcome = await listRemoteDirectory({
      to: props.target,
      path,
      offset: pageOptions.offset,
      limit: pageOptions.limit,
      signal: abortHost.signal
    });
    if (outcome.failure) {
      const thrown = new Error(outcome.failure) as Error & { code?: string };
      // 切换节点导致的主动取消不渲染成错误。
      if (abortHost.signal.aborted) thrown.code = 'ERR_CANCELED';
      throw thrown;
    }
    const entries = normalizeRemoteDirectoryEntries(outcome.entries);
    rememberLiveDirectory(path);
    return { path, entries, total: outcome.total, offset: pageOptions.offset, limit: pageOptions.limit };
  },
  onError: (message) => {
    notice.value = message || t('interlink.workspace.listFailed');
    noticeWarn.value = true;
  }
});

const rememberLiveDirectory = (path: string): void => {
  const next = new Set(liveDirectories.value);
  next.add(path || '');
  liveDirectories.value = next;
};

const treeRows = tree.rows;
const treeActivePath = tree.activePath;
const treeHighlightedPath = tree.highlightedPath;

const projectionLoading = computed(() => shadowApi.loading.value);
const treeLoading = computed(() => shadowApi.loading.value && !tree.rows.value.length);
const treeErrorText = computed(() => shadowApi.error.value || '');
const shadowIsEmpty = computed(() => isShadowEmpty(shadow.value));
const shadowTruncated = computed(() => shadow.value?.tree_truncated === true);
const liveDirectoryCount = computed(() => liveDirectories.value.size);
const nodeLabel = computed(() => props.node?.label || props.deviceId || props.target);

const watermarkText = computed(() => {
  const minutes = minutesSince(shadow.value?.synced_at, nowTick.value);
  if (minutes === null) return t('interlink.workspace.watermarkUnknown');
  return t('interlink.workspace.watermark', { minutes });
});

const readBlockedReason = computed(() => {
  const reason = resolveDisabledReason(props.node, 'workspace.read');
  return reason ? t(DISABLED_REASON_KEY[reason]) : '';
});

const listBlockedReason = computed(() => {
  const reason = resolveDisabledReason(props.node, 'workspace.list');
  return reason ? t(DISABLED_REASON_KEY[reason]) : '';
});

const threadBlockedReason = computed(() => {
  const reason = resolveDisabledReason(props.node, 'threads.get');
  return reason ? t(DISABLED_REASON_KEY[reason]) : '';
});

// ------------------------------------------------------------------ projection

const seedTreeFromShadow = (): void => {
  const keepExpanded = Array.from(tree.expandedPaths);
  tree.reset();
  liveDirectories.value = new Set();
  const entries = shadow.value?.tree || [];
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

const loadProjection = async (): Promise<void> => {
  if (!props.deviceId) return;
  notice.value = '';
  noticeWarn.value = false;
  await shadowApi.load(props.deviceId);
  seedTreeFromShadow();
};

/**
 * 「刷新投影」= 发一条 L0 `shadow.refresh`（不轮询，发完即忘）+ 重新拉一次影子。
 * 本地事件增量最快 2s 合并一次，给一个短 settling 窗口再取数。
 */
const refreshProjection = (): void => {
  shadowApi.refresh(props.deviceId, 0);
  if (shadowSettleTimer) clearTimeout(shadowSettleTimer);
  if (!listBlockedReason.value && supportsInterlinkKind(props.node, 'shadow.refresh')) {
    void submitInterlinkCommand({ to: props.target, kind: 'shadow.refresh', args: {} }).catch(() => undefined);
    shadowSettleTimer = setTimeout(() => {
      shadowSettleTimer = null;
      void loadProjection();
    }, 1500);
  }
};

// ------------------------------------------------------------------ directory

const handleToggleDirectory = async (path: string): Promise<void> => {
  const wasExpanded = tree.isExpanded(path);
  await tree.toggleDirectory(path);
  if (wasExpanded) return;
  // §6.3：展开一个目录＝一条 workspace.list；离线/无能力时保持投影不发包。
  if (listBlockedReason.value) {
    notice.value = listBlockedReason.value;
    noticeWarn.value = true;
    return;
  }
  await tree.refreshDirectory(path);
};

const handleLoadMore = (path: string): void => {
  void tree.loadMore(path);
};

const handleTreeCommand = (command: string, row: WorkspaceVisibleRow | null): void => {
  if (!row) return;
  if (command === 'toggle') {
    void handleToggleDirectory(row.path);
    return;
  }
  if (command === 'preview') {
    void openRemoteFile(row);
  }
};

const activateRow = async (row: WorkspaceVisibleRow): Promise<void> => {
  tree.setActivePath(row.path);
  if (row.kind === 'dir') {
    await handleToggleDirectory(row.path);
    return;
  }
  if (row.kind === 'more') {
    handleLoadMore(row.path);
    return;
  }
  await openRemoteFile(row);
};

const handleActivate = (row: WorkspaceVisibleRow): void => {
  void activateRow(row);
};

// --------------------------------------------------------------------- preview

const preview = reactive({
  visible: false,
  loading: false,
  title: '',
  path: '',
  metaLabel: '',
  hint: '',
  error: '',
  tooLarge: false,
  previewKind: 'unsupported' as WorkspaceResourcePreviewKind,
  previewUrl: '',
  content: ''
});

let previewObjectUrl = '';
let previewAbort: AbortController | null = null;
let previewSerial = 0;

const releasePreviewObjectUrl = (): void => {
  if (!previewObjectUrl) return;
  URL.revokeObjectURL(previewObjectUrl);
  previewObjectUrl = '';
};

const closePreview = (): void => {
  previewAbort?.abort();
  previewAbort = null;
  previewSerial += 1;
  preview.visible = false;
  preview.loading = false;
  releasePreviewObjectUrl();
};

const openRemoteFile = async (row: WorkspaceVisibleRow): Promise<void> => {
  if (row.kind !== 'file') return;
  const blocked = readBlockedReason.value;
  if (blocked) {
    // §6.3：置灰操作必须给出原因，而不是点了没反应。
    notice.value = blocked;
    noticeWarn.value = true;
    return;
  }

  const name = row.name || workspaceBaseName(row.path);
  const extension = extractWorkspaceResourceExtension(name) || workspaceFileExtension(name);
  previewSerial += 1;
  const serial = previewSerial;
  releasePreviewObjectUrl();

  preview.visible = true;
  preview.loading = true;
  preview.title = name;
  preview.path = row.path;
  preview.error = '';
  preview.hint = '';
  preview.tooLarge = false;
  preview.content = '';
  preview.previewUrl = '';
  preview.previewKind = 'unsupported';
  preview.metaLabel = [row.path, formatWorkspaceBytes(row.size)].filter(Boolean).join('  ·  ');

  if (Number(row.size) > REMOTE_PREVIEW_MAX_BYTES) {
    preview.loading = false;
    preview.tooLarge = true;
    preview.hint = resolveWorkspacePreviewTooLargeHint();
    return;
  }

  // 只读投影不接云端编辑器（OnlyOffice/drawio 依赖服务端文件端点），先算出目标渲染类型。
  const declaredKind = resolveWorkspaceResourcePreviewKind(name, row.size);
  const isEditorOnly = declaredKind === 'onlyoffice' || declaredKind === 'drawio';

  const controller = new AbortController();
  previewAbort = controller;
  const outcome = await readRemoteFile({
    to: props.target,
    path: row.path,
    signal: controller.signal
  });

  if (serial !== previewSerial) return;
  previewAbort = null;
  preview.loading = false;

  if (!outcome.payload) {
    preview.error =
      outcome.failure ||
      (outcome.errorCode
        ? t('interlink.workspace.readFailed', { code: outcome.errorCode })
        : t('interlink.workspace.readFailedGeneric'));
    preview.previewKind = 'unsupported';
    return;
  }

  if (outcome.payload.truncated) {
    preview.hint = t('interlink.workspace.truncatedHint', { size: formatWorkspaceBytes(outcome.payload.size) });
  }

  if (isEditorOnly) {
    preview.previewKind = 'unsupported';
    preview.hint = t('interlink.workspace.editorUnsupported');
    return;
  }

  if (declaredKind === 'text' || declaredKind === 'unsupported') {
    if (outcome.payload.text) {
      preview.previewKind = 'text';
      preview.content = outcome.payload.text;
      return;
    }
    preview.previewKind = 'unsupported';
    preview.hint = resolveWorkspacePreviewUnsupportedHint();
    return;
  }

  const bytes = outcome.payload.bytes;
  if (!bytes) {
    preview.error = t('interlink.workspace.readFailedGeneric');
    preview.previewKind = 'unsupported';
    return;
  }

  const blob = normalizeWorkspacePreviewBlob(new Blob([bytes]), declaredKind, extension);
  previewObjectUrl = URL.createObjectURL(blob);
  preview.previewKind = declaredKind;
  preview.previewUrl = previewObjectUrl;
};

const handlePreviewDownload = (): void => {
  if (!previewObjectUrl && !preview.content) {
    notice.value = t('interlink.workspace.downloadUnavailable');
    noticeWarn.value = true;
    return;
  }
  const url = previewObjectUrl || '';
  if (url) {
    saveObjectUrlAsFile(url, preview.title || 'download');
    return;
  }
  const blob = new Blob([preview.content], { type: 'text/plain;charset=utf-8' });
  const textUrl = URL.createObjectURL(blob);
  saveObjectUrlAsFile(textUrl, preview.title || 'download');
  window.setTimeout(() => URL.revokeObjectURL(textUrl), 1000);
};

const noop = (): void => undefined;

// --------------------------------------------------------------------- threads

const sortedThreads = computed(() => sortShadowThreads(normalizeShadowThreads(shadow.value?.threads || [])));
const threadRows = computed(() => sortedThreads.value.slice(0, REMOTE_THREAD_MAX_RENDERED));
const threadTotal = computed(() => sortedThreads.value.length);
const threadOverflow = computed(() => threadTotal.value > threadRows.value.length);

const openRemoteThread = (thread: InterlinkShadowThread): void => {
  if (threadBlockedReason.value) {
    notice.value = threadBlockedReason.value;
    noticeWarn.value = true;
    return;
  }
  activeThread.value = thread;
};

const handleBackToCloud = (): void => {
  activeThread.value = null;
  emit('back-to-cloud');
};

// ---------------------------------------------------------------- lifecycle

const stopTick = (): void => {
  if (tickTimer) {
    clearInterval(tickTimer);
    tickTimer = null;
  }
};

const startTick = (): void => {
  if (tickTimer || props.active === false) return;
  // 水印只精确到分钟，30s 滴答足够，不做每帧重算。
  tickTimer = setInterval(() => {
    nowTick.value = Date.now();
  }, WATERMARK_TICK_MS);
};

watch(
  () => [props.deviceId, props.target] as const,
  () => {
    abortHost.controller.abort();
    abortHost = createAbortHost();
    activeThread.value = null;
    closePreview();
    stopTick();
    startTick();
    void loadProjection();
  }
);

watch(
  () => props.active,
  (active) => {
    if (active === false) {
      stopTick();
      return;
    }
    startTick();
  }
);

onMounted(() => {
  startTick();
  void loadProjection();
});

onBeforeUnmount(() => {
  stopTick();
  if (shadowSettleTimer) clearTimeout(shadowSettleTimer);
  shadowSettleTimer = null;
  previewAbort?.abort();
  previewAbort = null;
  releasePreviewObjectUrl();
  shadowApi.teardown();
  abortHost.controller.abort();
});
</script>
