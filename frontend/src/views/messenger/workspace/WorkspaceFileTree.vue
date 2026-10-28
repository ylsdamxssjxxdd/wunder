<template>
  <div
    ref="scrollerRef"
    class="workspace-files-scroll"
    :class="{ 'is-drop-root': dragOverPath === ROOT_DROP_TARGET }"
    role="tree"
    tabindex="0"
    :aria-label="t('messenger.filesArea.group')"
    @scroll.passive="handleScroll"
    @keydown.esc.prevent="emit('exit-selection')"
    @contextmenu.prevent.stop="openMenu(null, $event)"
    @click="handleTreeClick"
    @dragenter="handleTreeDragEnter"
    @dragover="handleTreeDragOver"
    @dragleave="handleTreeDragLeave"
    @drop="handleTreeDrop"
  >
    <div v-if="loading && !rows.length" class="workspace-files-state">
      <span class="workspace-files-spinner" aria-hidden="true"></span>
      <span>{{ t('common.loading') }}</span>
    </div>

    <div v-else-if="error && !rows.length" class="workspace-files-state is-error">
      <span class="workspace-files-state-text">{{ error }}</span>
      <button class="workspace-files-state-action" type="button" @click.stop="emit('retry')">
        {{ t('messenger.filesArea.retry') }}
      </button>
    </div>

    <div v-else-if="!rows.length" class="workspace-files-state is-empty">
      <i class="fa-regular fa-folder-open" aria-hidden="true"></i>
      <span class="workspace-files-state-text">{{ props.emptyText || t('messenger.filesArea.empty') }}</span>
      <span class="workspace-files-state-hint">{{ props.emptyHint || t('messenger.filesArea.emptyHint') }}</span>
    </div>

    <div v-else class="workspace-files-rows" :style="rowsStyle">
      <div :style="spacerStyle(topSpacer)" aria-hidden="true"></div>
      <template v-for="row in visibleRows" :key="row.key">
        <button
          v-if="row.kind === 'more'"
          class="workspace-files-more"
          type="button"
          :style="{ paddingLeft: `${8 + row.depth * 12}px` }"
          :disabled="row.loading"
          @click.stop="emit('load-more', row.path)"
        >
          <span v-if="row.loading" class="workspace-files-spinner" aria-hidden="true"></span>
          <template v-else>
            <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
            {{ t('messenger.filesArea.loadMore', { count: row.remaining || 0 }) }}
          </template>
        </button>
        <div
          v-else
          class="workspace-file-row"
          :class="{
            'is-active': row.path === activePath,
            'is-selected': selectedSet.has(row.path),
            'is-highlighted': row.path === highlightedPath,
            'is-drop-target': row.path === dragOverPath,
            'is-dir': row.kind === 'dir'
          }"
          :style="{ paddingLeft: `${6 + row.depth * 12}px` }"
          :title="resolveRowTitle(row)"
          role="treeitem"
          :data-path="row.path"
          :data-kind="row.kind"
          :aria-expanded="row.kind === 'dir' ? row.expanded : undefined"
          :draggable="canDragWorkspacePath(row)"
          @click.stop="emit('activate', row)"
          @dblclick.stop="emit('open', row)"
          @contextmenu.prevent.stop="openMenu(row, $event)"
          @dragstart="handleRowDragStart($event, row)"
          @dragend="handleRowDragEnd"
        >
          <input
            v-if="selectionMode"
            class="workspace-file-check"
            type="checkbox"
            :checked="selectedSet.has(row.path)"
            :aria-label="row.name"
            @click.stop="emit('toggle-select', row.path)"
          />
          <span
            v-if="row.kind === 'dir' && row.expandable"
            class="workspace-file-twisty"
            role="button"
            tabindex="-1"
            @click.stop="emit('toggle-dir', row.path)"
          >
            <i
              :class="['fa-solid', row.expanded ? 'fa-chevron-down' : 'fa-chevron-right']"
              aria-hidden="true"
            ></i>
          </span>
          <span v-else class="workspace-file-twisty is-placeholder" aria-hidden="true"></span>
          <img
            v-if="row.kind !== 'dir' && rowIconPath(row)"
            class="workspace-file-icon-img"
            :src="rowIconPath(row)"
            alt=""
            aria-hidden="true"
            draggable="false"
          />
          <i v-else :class="['workspace-file-icon', rowIconClass(row)]" aria-hidden="true"></i>
          <span class="workspace-file-name">{{ row.name }}</span>
          <span v-if="row.pathHint" class="workspace-file-path">{{ row.pathHint }}</span>
          <span v-if="row.loading" class="workspace-files-spinner" aria-hidden="true"></span>
          <span
            v-else-if="archivingPath === row.path"
            class="workspace-files-spinner"
            :title="t('messenger.filesArea.archivingNote', { name: row.name })"
            aria-hidden="true"
          ></span>
          <button
            v-if="!readOnly"
            class="workspace-file-menu"
            type="button"
            :title="t('common.more')"
            :aria-label="t('common.more')"
            @click.stop="openMenu(row, $event)"
          >
            <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
          </button>
        </div>
      </template>
      <div :style="spacerStyle(bottomSpacer)" aria-hidden="true"></div>
    </div>

    <Teleport to="body">
      <div
        v-if="menu.visible"
        ref="menuRef"
        class="workspace-files-menu"
        :style="menuStyle"
        role="menu"
        @click.stop
      >
        <button
          v-for="item in menuItems"
          :key="item.command"
          class="workspace-files-menu-item"
          :class="{ 'is-danger': item.danger }"
          type="button"
          role="menuitem"
          @click="chooseMenuItem(item.command)"
        >
          <i :class="['fa-solid', item.icon]" aria-hidden="true"></i>
          <span>{{ item.label }}</span>
        </button>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import { loadWorkspaceThemeIconResolver } from '@/components/chat/workspaceIcons';
import { clearWorkspaceDragPaths, hasWorkspaceDragPaths, readWorkspaceDragPaths, setWorkspaceDragPaths } from '@/components/chat/workspaceDrag';
import {
  WORKSPACE_ROW_HEIGHT,
  WORKSPACE_ROW_OVERSCAN,
  formatWorkspaceBytes,
  formatWorkspaceTimestamp,
  workspaceFileExtension,
  workspaceParentPath,
  type WorkspaceVisibleRow
} from './workspaceFileModel';

const props = withDefaults(
  defineProps<{
    rows: WorkspaceVisibleRow[];
    activePath: string;
    highlightedPath: string;
    loading: boolean;
    error: string;
    selectionMode: boolean;
    selectedPaths: string[];
    /** 只读投影（互通远程节点）：隐藏写操作菜单，只留预览/展开。 */
    readOnly?: boolean;
    remote?: boolean;
    /** 正在打包下载的目录路径：该行显示转圈，直到 zip 下载落地。 */
    archivingPath?: string;
    /** 空态提示覆盖（远程投影与云端拖拽提示语义不同）。 */
    emptyHint?: string;
    /** 空态主文案覆盖（搜索结果等非目录语义）。 */
    emptyText?: string;
  }>(),
  { readOnly: false, remote: false, archivingPath: '', emptyHint: '', emptyText: '' }
);

const emit = defineEmits<{
  'toggle-dir': [path: string];
  'load-more': [path: string];
  activate: [row: WorkspaceVisibleRow];
  open: [row: WorkspaceVisibleRow];
  'toggle-select': [path: string];
  'exit-selection': [];
  retry: [];
  command: [command: string, row: WorkspaceVisibleRow | null];
  /** 内部拖拽落到目录行：把这些路径移动到目标目录。 */
  move: [payload: { paths: string[]; targetDir: string }];
  /** 点击空白区（没有宿主行）：回到根目录，清选中。 */
  'blank-activate': [];
}>();

const { t } = useI18n();

const scrollerRef = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);
/** 当前高亮的拖拽落点（目录行路径），用于 is-drop-target 反馈。 */
const dragOverPath = ref('');

const selectedSet = computed(() => new Set(props.selectedPaths));

// -------------------------------------------------------------- virtual window

const startIndex = computed(() =>
  Math.max(0, Math.floor(scrollTop.value / WORKSPACE_ROW_HEIGHT) - WORKSPACE_ROW_OVERSCAN)
);

const endIndex = computed(() => {
  const height = viewportHeight.value || WORKSPACE_ROW_HEIGHT * 12;
  const visible = Math.ceil(height / WORKSPACE_ROW_HEIGHT) + WORKSPACE_ROW_OVERSCAN * 2;
  return Math.min(props.rows.length, startIndex.value + visible);
});

const visibleRows = computed(() => props.rows.slice(startIndex.value, endIndex.value));
const topSpacer = computed(() => startIndex.value * WORKSPACE_ROW_HEIGHT);
const bottomSpacer = computed(() =>
  Math.max(0, (props.rows.length - endIndex.value) * WORKSPACE_ROW_HEIGHT)
);
const rowsStyle = computed(() => ({ minHeight: `${props.rows.length * WORKSPACE_ROW_HEIGHT}px` }));

const spacerStyle = (height: number) => ({ height: `${height}px` });

const canDragWorkspacePath = (row: WorkspaceVisibleRow): boolean =>
  !props.readOnly && !props.remote && row.kind !== 'more' && Boolean(row.path);

/**
 * 本次拖拽的源路径。`dataTransfer` 在 dragover 阶段处于保护模式（getData 返回空），
 * 模块级暂存又可能被中途的刷新打断，所以组件自己再记一份：判定落点不依赖任何
 * 可能在拖拽中途丢掉的通道。
 */
const draggingPaths = ref<string[]>([]);

const handleRowDragStart = (event: DragEvent, row: WorkspaceVisibleRow) => {
  if (!canDragWorkspacePath(row)) {
    event.preventDefault();
    return;
  }
  // 多选状态下拖动已选中的行，整批一起移动。
  const draggingSelection =
    props.selectionMode && selectedSet.value.has(row.path) && props.selectedPaths.length > 0;
  const paths = setWorkspaceDragPaths(
    event.dataTransfer,
    draggingSelection ? props.selectedPaths : [row.path]
  );
  if (!paths.length) {
    event.preventDefault();
    return;
  }
  draggingPaths.value = paths;
  // 工作区内部是移动语义；拖到输入框引用文件时，接收方只读路径。
  if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
};

const handleRowDragEnd = () => {
  dragOverPath.value = '';
  draggingPaths.value = [];
  clearWorkspaceDragPaths();
};

/**
 * 树里没有「根目录」这一行，空白区就是工作区根。用一个不可能出现在真实路径里的
 * 哨兵值表达它，既能做高亮，又不会和 `''`（真空路径）混淆。
 */
const ROOT_DROP_TARGET = '\u0000workspace-root';

const isWritableTree = computed(() => !props.readOnly && !props.remote);

/** 目录不能移到自己或自己的子孙目录里。 */
const isSameOrNestedPath = (source: string, targetDir: string): boolean =>
  source === targetDir || targetDir.startsWith(`${source}/`);

/**
 * 从事件落点反推目标目录：目录行 → 该目录；文件行 → 它所在的父目录；
 * 空白区 / 分页脚注以外的空白 → 工作区根（`''`）。没有任何落点语义时返回 null。
 */
const resolveDropDestination = (event: DragEvent): { dir: string; highlight: string } | null => {
  const target = event.target as HTMLElement | null;
  const rowElement = target?.closest?.('.workspace-file-row') as HTMLElement | null;
  if (!rowElement) {
    // 「加载更多」是分页脚注，不是目录，落在这里不做移动。
    if (target?.closest?.('.workspace-files-more')) return null;
    return { dir: '', highlight: ROOT_DROP_TARGET };
  }
  const path = String(rowElement.dataset.path || '');
  if (!path) return null;
  if (rowElement.dataset.kind === 'dir') return { dir: path, highlight: path };
  // 文件行本身不是容器：落到它所在的目录（父目录行就在上方，高亮它即可；
  // 父目录是根时不额外高亮，因为列表本身就是根，再套一圈反而吵）。
  const parent = workspaceParentPath(path);
  return { dir: parent, highlight: parent };
};

/**
 * 统一计算一次投放：只有「内部拖拽 + 可写 + 有明确落点 + 至少一条真的会换目录」
 * 才返回可执行载荷。外部文件的拖拽一律返回 null，交给面板做上传。
 */
const resolveDropPayload = (
  event: DragEvent
): { dir: string; highlight: string; paths: string[] } | null => {
  if (!isWritableTree.value) return null;
  const dataTransfer = event.dataTransfer;
  // 源路径优先取组件自己的记录：dragover 阶段 dataTransfer 读不出内容。
  const sources = draggingPaths.value.length
    ? draggingPaths.value
    : dataTransfer && hasWorkspaceDragPaths(dataTransfer)
      ? readWorkspaceDragPaths(dataTransfer)
      : [];
  if (!sources.length) return null;
  const destination = resolveDropDestination(event);
  if (!destination) return null;
  const paths = sources.filter((path) => {
    if (isSameOrNestedPath(path, destination.dir)) return false;
    // 已经在这个目录里：不算可投放的移动，避免松手后只弹一句「目标路径相同」。
    return workspaceParentPath(path) !== destination.dir;
  });
  if (!paths.length) return null;
  return { ...destination, paths };
};

const handleTreeDragEnter = (event: DragEvent) => {
  const payload = resolveDropPayload(event);
  if (!payload) return;
  event.preventDefault();
  dragOverPath.value = payload.highlight;
};

const handleTreeDragOver = (event: DragEvent) => {
  const payload = resolveDropPayload(event);
  if (!payload) {
    if (dragOverPath.value) dragOverPath.value = '';
    return;
  }
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  dragOverPath.value = payload.highlight;
};

const handleTreeDragLeave = (event: DragEvent) => {
  const related = event.relatedTarget as Node | null;
  const current = event.currentTarget as Node | null;
  if (related && current?.contains(related)) return;
  dragOverPath.value = '';
};

const handleTreeDrop = (event: DragEvent) => {
  const payload = resolveDropPayload(event);
  dragOverPath.value = '';
  // 外部文件：冒泡给面板走上传。
  if (!payload) return;
  // 阻止冒泡，否则面板的 @drop 会把它当成一次上传。
  event.preventDefault();
  event.stopPropagation();
  emit('move', { paths: payload.paths, targetDir: payload.dir });
};

/** 点空白区＝回到根：清掉选中行，同时把「新建/上传」的落点收回根目录。 */
const handleTreeClick = (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  if (target.closest('.workspace-file-row') || target.closest('.workspace-files-more')) return;
  if (!props.activePath && !props.selectedPaths.length) return;
  emit('blank-activate');
};

let scrollFrame = 0;
const handleScroll = () => {
  closeMenu();
  if (scrollFrame) return;
  const apply = () => {
    scrollFrame = 0;
    const element = scrollerRef.value;
    if (element) scrollTop.value = element.scrollTop;
  };
  scrollFrame = window.requestAnimationFrame(apply);
};

/** Bring a highlighted row (upload result, timeline reveal) into the viewport. */
watch(
  () => props.highlightedPath,
  (path) => {
    if (!path) return;
    const index = props.rows.findIndex((row) => row.path === path);
    if (index < 0) return;
    const element = scrollerRef.value;
    if (!element) return;
    const height = element.clientHeight || WORKSPACE_ROW_HEIGHT * 12;
    const target = Math.max(0, index * WORKSPACE_ROW_HEIGHT - height / 2);
    element.scrollTo({ top: target });
    scrollTop.value = element.scrollTop;
  }
);

let resizeObserver: ResizeObserver | null = null;
onMounted(() => {
  const element = scrollerRef.value;
  if (!element) return;
  viewportHeight.value = element.clientHeight;
  if (typeof ResizeObserver === 'function') {
    resizeObserver = new ResizeObserver(() => {
      viewportHeight.value = element.clientHeight;
    });
    resizeObserver.observe(element);
  }
});

onBeforeUnmount(() => {
  if (scrollFrame) window.cancelAnimationFrame(scrollFrame);
  resizeObserver?.disconnect();
  resizeObserver = null;
  document.removeEventListener('pointerdown', handleOutsideMenu, true);
  document.removeEventListener('click', handleOutsideMenu, true);
  document.removeEventListener('keydown', handleMenuKeydown, true);
  window.removeEventListener('resize', closeMenu);
  window.removeEventListener('blur', closeMenu);
});

// --------------------------------------------------------------- row menu

const menu = ref({ visible: false, x: 0, y: 0, row: null as WorkspaceVisibleRow | null });
const menuRef = ref<HTMLElement | null>(null);

const menuStyle = computed(() => ({
  left: `${menu.value.x}px`,
  top: `${menu.value.y}px`
}));

const closeMenu = () => {
  menu.value.visible = false;
  menu.value.row = null;
};

const handleOutsideMenu = (event: Event) => {
  if (!menu.value.visible || (menuRef.value && event.composedPath().includes(menuRef.value))) return;
  closeMenu();
};

const handleMenuKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') closeMenu();
};

const openMenu = (row: WorkspaceVisibleRow | null, event: MouseEvent | Event) => {
  if (row?.kind === 'more') return;
  const source = event as MouseEvent;
  const x = Number.isFinite(source.clientX) && source.clientX > 0 ? source.clientX : 24;
  const y = Number.isFinite(source.clientY) && source.clientY > 0 ? source.clientY : 24;
  const maxX = Math.max(8, window.innerWidth - 180);
  const maxY = Math.max(8, window.innerHeight - 260);
  menu.value = { visible: true, x: Math.min(x, maxX), y: Math.min(y, maxY), row };
};

type MenuItem = { command: string; label: string; icon: string; danger?: boolean };

const createItems = (): MenuItem[] => [
  { command: 'new-dir', label: t('messenger.filesArea.newDir'), icon: 'fa-folder-plus' },
  { command: 'new-file', label: t('messenger.filesArea.newFile'), icon: 'fa-file-circle-plus' }
];

const menuItems = computed<MenuItem[]>(() => {
  const row = menu.value.row;
  // 只读投影：行上只给「打开目录 / 预览」，写类动作在远程节点上不存在；
  // 空白区给「刷新」，右键不再无声无息。
  if (props.readOnly) {
    if (!row) return [{ command: 'refresh', label: t('common.refresh'), icon: 'fa-rotate-right' }];
    if (row.kind === 'more') return [];
    return [
      { command: 'refresh', label: t('common.refresh'), icon: 'fa-rotate-right' },
      ...(row.kind === 'dir'
        ? [{ command: 'toggle', label: t('messenger.filesArea.menu.open'), icon: 'fa-folder-open' }]
        : [{ command: 'preview', label: t('messenger.filesArea.menu.preview'), icon: 'fa-eye' }, { command: 'download', label: t('common.download'), icon: 'fa-download' }])
    ];
  }
  // 空白区没有宿主行，菜单落在当前目录上，并承接已下线头部 ⋯ 的三个动作。
  if (!row) {
    const items: MenuItem[] = [
      { command: 'refresh', label: t('common.refresh'), icon: 'fa-rotate-right' },
      ...createItems()
    ];
    if (!props.remote) items.push(
      { command: 'archive-root', label: t('messenger.filesArea.archiveCurrent'), icon: 'fa-file-zipper' },
      { command: 'clear', label: t('messenger.filesArea.clear'), icon: 'fa-broom', danger: true }
    );
    return items;
  }
  const items: MenuItem[] = [
    { command: 'refresh', label: t('common.refresh'), icon: 'fa-rotate-right' }
  ];
  if (row.kind === 'dir') {
    items.push({ command: 'toggle', label: t('messenger.filesArea.menu.open'), icon: 'fa-folder-open' });
    if (!props.remote) items.push({ command: 'archive', label: t('messenger.filesArea.menu.archive'), icon: 'fa-file-zipper' });
    items.push(...createItems());
  } else {
    items.push({ command: 'preview', label: t('messenger.filesArea.menu.preview'), icon: 'fa-eye' });
    items.push({ command: 'download', label: t('common.download'), icon: 'fa-download' });
  }
  // 根层的条目没有上一层，只有不在根层时才给这个动作。
  if (isWritableTree.value && workspaceParentPath(row.path)) {
    items.push({ command: 'move-up', label: t('messenger.filesArea.menu.moveUp'), icon: 'fa-arrow-up' });
  }
  items.push({ command: 'rename', label: t('messenger.filesArea.menu.rename'), icon: 'fa-i-cursor' });
  if (!props.remote) items.push({ command: 'quote', label: t('messenger.filesArea.menu.quote'), icon: 'fa-comment-dots' });
  items.push({ command: 'delete', label: t('common.delete'), icon: 'fa-trash-can', danger: true });
  return items;
});

/** 「移动到上一层目录」的载荷：同层多选时整批一起上移。 */
const resolveMoveUpPayload = (
  row: WorkspaceVisibleRow
): { paths: string[]; targetDir: string } | null => {
  const parent = workspaceParentPath(row.path);
  if (!parent) return null;
  const targetDir = workspaceParentPath(parent);
  const selection =
    props.selectionMode && selectedSet.value.has(row.path) ? props.selectedPaths : [];
  const sameLevel = selection.filter((path) => workspaceParentPath(path) === parent);
  return { paths: sameLevel.length > 1 ? sameLevel : [row.path], targetDir };
};

const chooseMenuItem = (command: string) => {
  const row = menu.value.row;
  closeMenu();
  if (command === 'move-up' && row) {
    const payload = resolveMoveUpPayload(row);
    if (payload) emit('move', payload);
    return;
  }
  emit('command', command, row);
};

onMounted(() => {
  // Capture outside interactions before rows or other controls stop bubbling.
  // Ignore the teleported menu itself so choosing an action still works.
  document.addEventListener('pointerdown', handleOutsideMenu, true);
  document.addEventListener('click', handleOutsideMenu, true);
  document.addEventListener('keydown', handleMenuKeydown, true);
  window.addEventListener('resize', closeMenu);
  window.addEventListener('blur', closeMenu);
});

// ------------------------------------------------------------------ helpers

// 文件行用 VSCode 彩色图标主题（与 chat 工作面板同源），FA 图标仅作主题未就绪时的兜底。
const themeIconResolver = ref<Awaited<ReturnType<typeof loadWorkspaceThemeIconResolver>> | null>(null);
void loadWorkspaceThemeIconResolver().then((resolver) => {
  themeIconResolver.value = resolver;
});

const rowIconPath = (row: WorkspaceVisibleRow): string => {
  if (row.kind === 'dir') return '';
  return themeIconResolver.value?.resolveFileIconPath(row.name, workspaceFileExtension(row.name)) || '';
};

const rowIconClass = (row: WorkspaceVisibleRow): string => {
  if (row.kind === 'dir') {
    return row.expanded ? 'fa-solid fa-folder-open' : 'fa-solid fa-folder';
  }
  const extension = workspaceFileExtension(row.name);
  if (!extension) return 'fa-regular fa-file';
  if (['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg'].includes(extension)) return 'fa-regular fa-image';
  if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(extension)) return 'fa-regular fa-file-zipper';
  if (['md', 'markdown', 'txt', 'log'].includes(extension)) return 'fa-regular fa-file-lines';
  if (['json', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'xml'].includes(extension)) {
    return 'fa-regular fa-file-code';
  }
  if (['doc', 'docx', 'odt', 'rtf'].includes(extension)) return 'fa-regular fa-file-word';
  if (['xls', 'xlsx', 'ods', 'csv'].includes(extension)) return 'fa-regular fa-file-excel';
  if (['ppt', 'pptx', 'odp'].includes(extension)) return 'fa-regular fa-file-powerpoint';
  if (extension === 'pdf') return 'fa-regular fa-file-pdf';
  return 'fa-regular fa-file';
};

const resolveRowTitle = (row: WorkspaceVisibleRow): string => {
  if (row.kind === 'dir') return row.name;
  const parts = [row.name];
  const size = formatWorkspaceBytes(row.size);
  if (size) parts.push(size);
  const time = formatWorkspaceTimestamp(row.updatedTime);
  if (time) parts.push(time);
  return parts.join('  ·  ');
};

defineExpose({
  focus: () => scrollerRef.value?.focus()
});
</script>
