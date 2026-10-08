<template>
  <div
    ref="scrollerRef"
    class="workspace-files-scroll"
    role="tree"
    tabindex="0"
    :aria-label="t('messenger.filesArea.group')"
    @scroll.passive="handleScroll"
    @keydown.esc.prevent="emit('exit-selection')"
    @click="closeMenu"
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
      <span class="workspace-files-state-text">{{ t('messenger.filesArea.empty') }}</span>
      <span class="workspace-files-state-hint">{{ t('messenger.filesArea.emptyHint') }}</span>
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
            'is-dir': row.kind === 'dir'
          }"
          :style="{ paddingLeft: `${6 + row.depth * 12}px` }"
          :title="resolveRowTitle(row)"
          role="treeitem"
          :aria-expanded="row.kind === 'dir' ? row.expanded : undefined"
          @click.stop="emit('activate', row)"
          @dblclick.stop="emit('open', row)"
          @contextmenu.prevent.stop="openMenu(row, $event)"
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
            v-if="row.kind === 'dir'"
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
          <i :class="['workspace-file-icon', rowIconClass(row)]" aria-hidden="true"></i>
          <span class="workspace-file-name">{{ row.name }}</span>
          <span v-if="row.loading" class="workspace-files-spinner" aria-hidden="true"></span>
          <button
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
import {
  WORKSPACE_ROW_HEIGHT,
  WORKSPACE_ROW_OVERSCAN,
  formatWorkspaceBytes,
  formatWorkspaceTimestamp,
  workspaceFileExtension,
  type WorkspaceVisibleRow
} from './workspaceFileModel';

const props = defineProps<{
  rows: WorkspaceVisibleRow[];
  activePath: string;
  highlightedPath: string;
  loading: boolean;
  error: string;
  selectionMode: boolean;
  selectedPaths: string[];
}>();

const emit = defineEmits<{
  'toggle-dir': [path: string];
  'load-more': [path: string];
  activate: [row: WorkspaceVisibleRow];
  open: [row: WorkspaceVisibleRow];
  'toggle-select': [path: string];
  'exit-selection': [];
  retry: [];
  command: [command: string, row: WorkspaceVisibleRow];
}>();

const { t } = useI18n();

const scrollerRef = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportHeight = ref(0);

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

let scrollFrame = 0;
const handleScroll = () => {
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
  window.removeEventListener('click', closeMenu);
  window.removeEventListener('keydown', handleMenuKeydown);
});

// --------------------------------------------------------------- row menu

const menu = ref({ visible: false, x: 0, y: 0, row: null as WorkspaceVisibleRow | null });

const menuStyle = computed(() => ({
  left: `${menu.value.x}px`,
  top: `${menu.value.y}px`
}));

const closeMenu = () => {
  menu.value.visible = false;
};

const handleMenuKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') closeMenu();
};

const openMenu = (row: WorkspaceVisibleRow, event: MouseEvent | Event) => {
  if (row.kind === 'more') return;
  const source = event as MouseEvent;
  const x = Number.isFinite(source.clientX) && source.clientX > 0 ? source.clientX : 24;
  const y = Number.isFinite(source.clientY) && source.clientY > 0 ? source.clientY : 24;
  const maxX = Math.max(8, window.innerWidth - 180);
  const maxY = Math.max(8, window.innerHeight - 260);
  menu.value = { visible: true, x: Math.min(x, maxX), y: Math.min(y, maxY), row };
};

type MenuItem = { command: string; label: string; icon: string; danger?: boolean };

const menuItems = computed<MenuItem[]>(() => {
  const row = menu.value.row;
  if (!row) return [];
  const items: MenuItem[] = [];
  if (row.kind === 'dir') {
    items.push({ command: 'toggle', label: t('messenger.filesArea.menu.open'), icon: 'fa-folder-open' });
    items.push({ command: 'archive', label: t('messenger.filesArea.menu.archive'), icon: 'fa-file-zipper' });
  } else {
    items.push({ command: 'preview', label: t('messenger.filesArea.menu.preview'), icon: 'fa-eye' });
    items.push({ command: 'download', label: t('common.download'), icon: 'fa-download' });
  }
  items.push({ command: 'rename', label: t('messenger.filesArea.menu.rename'), icon: 'fa-i-cursor' });
  items.push({ command: 'move', label: t('messenger.filesArea.menu.move'), icon: 'fa-arrow-right-arrow-left' });
  items.push({ command: 'copy', label: t('messenger.filesArea.menu.copy'), icon: 'fa-copy' });
  items.push({ command: 'quote', label: t('messenger.filesArea.menu.quote'), icon: 'fa-comment-dots' });
  items.push({ command: 'delete', label: t('common.delete'), icon: 'fa-trash-can', danger: true });
  return items;
});

const chooseMenuItem = (command: string) => {
  const row = menu.value.row;
  closeMenu();
  if (!row) return;
  emit('command', command, row);
};

onMounted(() => {
  window.addEventListener('click', closeMenu);
  window.addEventListener('keydown', handleMenuKeydown);
});

// ------------------------------------------------------------------ helpers

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
