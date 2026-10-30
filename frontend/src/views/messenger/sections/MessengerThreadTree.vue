<template>
  <div class="mz-thread-tree" :class="{ 'is-selecting': selectionMode }" role="tree" :aria-label="t('messenger.tasks.title')">
    <!-- 空白区右键：只有列表级动作（多选）。行上右键会被行自己的 .stop 拦下。 -->
    <div ref="viewport" class="mz-thread-scroll" @scroll.passive="syncViewport" @contextmenu.prevent="openMenu(null, $event)">
      <div v-if="!displayItems.length" class="mz-thread-empty" role="status">
        {{ emptyLabel }}
      </div>
      <div :style="{ height: `${range.start * ROW_HEIGHT}px` }" aria-hidden="true"></div>
      <div
        v-for="item in visibleItems"
        :key="item.id"
        class="mz-thread-row"
        :class="{
          'is-active': activeSessionId === item.id,
          'is-menu-open': openMenuId === item.id,
          'is-selecting': selectionMode,
          'is-dragging': dragId === item.id,
          'is-drop-before': dropTarget?.id === item.id && dropTarget.position === 'before',
          'is-drop-after': dropTarget?.id === item.id && dropTarget.position === 'after'
        }"
        :draggable="selectionMode ? false : 'true'"
        role="treeitem"
        :aria-selected="activeSessionId === item.id ? 'true' : 'false'"
        @mouseenter="handleRowHover(item.id)"
        @mouseleave="hoverId = ''"
        @contextmenu.prevent.stop="openMenu(item, $event)"
        @dragstart="handleDragStart($event, item.id)"
        @dragover="handleDragOver($event, item.id)"
        @drop="handleDrop($event, item.id)"
        @dragend="resetDrag"
      >
        <span
          v-if="selectionMode"
          class="mz-thread-check"
          :class="{ 'is-checked': selectedIds.has(item.id), 'is-disabled': !canArchive(item) }"
          role="checkbox"
          :aria-checked="selectedIds.has(item.id) ? 'true' : 'false'"
          :aria-label="item.title"
          @click.stop.prevent="toggleSelected(item)"
        >
          <i v-if="selectedIds.has(item.id)" class="fa-solid fa-check" aria-hidden="true"></i>
        </span>
        <button
          class="mz-thread-select"
          type="button"
          :title="threadTooltip(item)"
          :data-thread-title="item.title"
          :aria-current="activeSessionId === item.id ? 'true' : undefined"
          @click="handleRowClick(item)"
        >
          <span
            class="mz-thread-status"
            :class="`is-${item.indicator}`"
            :title="stateLabel(item.indicator)"
            role="img"
            :aria-label="stateLabel(item.indicator)"
          ><i :class="['fa-solid', stateIcons[item.indicator]]" aria-hidden="true"></i></span>
          <span class="mz-thread-title">{{ item.title }}</span>
        </button>
        <!-- 行内动作直接摊平：重命名 / 轨迹视图 / 归档各占一个按钮，
             不再折叠进「更多」下拉；多选改由右键菜单承载。 -->
        <template v-if="!selectionMode && shouldRenderActions(item.id)">
          <button
            v-for="action in rowActions(item)"
            :key="action.key"
            class="mz-thread-action"
            type="button"
            :title="action.label"
            :aria-label="action.label"
            :disabled="action.disabled"
            @click.stop="runRowAction(action.key, item.id)"
          >
            <i :class="['fa-solid', action.icon]" aria-hidden="true"></i>
          </button>
        </template>
      </div>
      <div :style="{ height: `${(displayItems.length - range.end) * ROW_HEIGHT}px` }" aria-hidden="true"></div>
      <button v-if="hasMore" class="mz-thread-load-more" type="button" :disabled="loading" @click="loadMore">
        {{ loading ? t('common.loading') : pageError ? t('messenger.tasks.retry') : t('messenger.tasks.loadMore') }}
      </button>
    </div>

    <!-- 右键菜单：行上右键给该行的三个动作＋多选，空白区右键只给多选。
         菜单 teleport 到 body，避免被侧栏的 overflow 裁掉。 -->
    <Teleport to="body">
      <div
        v-if="menu.visible"
        ref="menuRef"
        class="mz-thread-context-menu"
        :style="menuStyle"
        role="menu"
        :aria-label="t('messenger.tasks.title')"
        @click.stop
        @contextmenu.prevent.stop
      >
        <template v-for="entry in menuItems" :key="entry.command">
          <span v-if="entry.divider" class="mz-thread-context-divider" aria-hidden="true"></span>
          <button
            v-else
            class="mz-thread-context-item"
            type="button"
            role="menuitem"
            :disabled="entry.disabled"
            @click="chooseMenuItem(entry.command)"
          >
            <i :class="['fa-solid', entry.icon]" aria-hidden="true"></i>
            <span>{{ entry.label }}</span>
          </button>
        </template>
      </div>
    </Teleport>

    <!-- 多选模式工具条：覆盖在树底部，参考端同款「全选 | 已选 N 项 + 关闭」＋ 归档动作。 -->
    <div v-if="selectionMode" class="mz-thread-selection-bar">
      <div class="mz-thread-selection-summary">
        <span
          class="mz-thread-check"
          :class="{ 'is-checked': allSelected, 'is-partial': partialSelected }"
          role="checkbox"
          :aria-checked="allSelected ? 'true' : partialSelected ? 'mixed' : 'false'"
          :aria-label="t('common.selectAll')"
          @click.stop.prevent="toggleSelectAll"
        >
          <i v-if="allSelected" class="fa-solid fa-check" aria-hidden="true"></i>
          <i v-else-if="partialSelected" class="fa-solid fa-minus" aria-hidden="true"></i>
        </span>
        <button class="mz-thread-selection-label" type="button" @click="toggleSelectAll">
          {{ t('common.selectAll') }}
        </button>
        <span class="mz-thread-selection-divider" aria-hidden="true"></span>
        <span class="mz-thread-selection-count">{{ t('messenger.tasks.selectedCount', { count: selectedCount }) }}</span>
        <span class="mz-thread-selection-spacer" aria-hidden="true"></span>
        <button
          class="mz-thread-selection-close"
          type="button"
          :title="t('common.close')"
          :aria-label="t('common.close')"
          @click="exitSelectionMode"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
      <button class="mz-thread-selection-archive" type="button" :disabled="!selectedCount" @click="archiveSelected">
        <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
        {{ t('messenger.tasks.archive') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { useAuthStore } from '@/stores/auth';
import { selectSessionRuntimeStatus } from '@/realtime/chat/chatRuntimeSelectors';
import { getChatThreadStatus } from '@/realtime/chat/chatThreadRuntime';
import { resolveVisibleTaskRows, taskWindow, type TaskListItem } from '@/views/messenger/taskList';
import { hasCompletedTaskTurn, resolveTaskIndicatorState, resolveTaskRuntimeState, type TaskIndicatorState } from '@/views/messenger/taskRuntimeState';
import { getRuntime, getSessionMessages, hasRuntimeControllers } from '@/stores/chatRuntimeState';
import { useTaskListPages } from '@/views/messenger/useTaskListPages';
import { formatDateTime } from '@/utils/formatDateTime';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import type { AgentRuntimeState } from '@/views/messenger/model';

// §5.2 thread rows are 28px tall so the row height is also the virtual window step.
const ROW_HEIGHT = 32;

const props = defineProps<{ items: TaskListItem[]; activeSessionId: string; agentId: string }>();
const emit = defineEmits<{
  open: [id: string];
  trajectory: [id: string];
  rename: [id: string];
  archive: [id: string];
  'archive-threads': [ids: string[]];
}>();

const { t } = useI18n();

/**
 * Row tooltip: the full title (it may be elided) plus when the thread last
 * finished a turn, so a long-lost thread can be identified by date instead of
 * by scrolling its transcript.
 */
const threadTooltip = (item: TaskListItem): string => {
  const at = formatDateTime(item.lastActiveAt);
  return at ? `${item.title} · ${t('messenger.tasks.lastFinishedAt', { time: at })}` : item.title;
};

const store = useChatStore();
const authStore = useAuthStore();
const { loading, error: pageError, hasMore, loadMore } = useTaskListPages(toRef(props, 'agentId'));

const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const height = ref(320);

const ordered = usePersistentStableListOrder(toRef(props, 'items'), {
  getKey: (item) => item.id,
  getTimestamp: (item) => item.createdAt,
  storageKey: computed(
    () =>
      `messenger:threads:${String(
        (authStore.user as Record<string, unknown> | null)?.id ||
          (authStore.user as Record<string, unknown> | null)?.user_id ||
          'guest'
      )}:${props.agentId || 'default'}`
  )
});

const resolveItemState = (item: TaskListItem): AgentRuntimeState => {
  void store.runtimeProjectionVersionBySession[item.id];
  const runtime = getRuntime(item.id);
  const durableStatus = getChatThreadStatus(item.id);
  const state = resolveTaskRuntimeState(
    selectSessionRuntimeStatus(store.runtimeProjection, item.id),
    item.runtimeStatus,
    Boolean(store.loadingBySession[item.id]),
    durableStatus || runtime?.threadStatus,
    hasRuntimeControllers(runtime)
  );
  if (
    state === 'idle' &&
    hasCompletedTaskTurn(
      String(store.activeSessionId || '').trim() === item.id ? store.messages : getSessionMessages(item.id)
    )
  ) {
    return 'done';
  }
  return state;
};

const displayItems = computed(() => ordered.orderedItems.value);
const range = computed(() => taskWindow(displayItems.value.length, scrollTop.value, height.value, ROW_HEIGHT));
// Resolve runtime state only for rows inside the virtual window. Subscribing to
// a session's projection/loading clock for every row turns one session bump
// into a full-list recompute (dep magnet); keeping it windowed bounds the work
// to the visible rows even when the backing list is large.
const visibleItems = computed(() => resolveVisibleTaskRows(displayItems.value, range.value, resolveItemState).map(item => ({
  ...item,
  indicator: resolveTaskIndicatorState(item.state, selectSessionRuntimeStatus(store.runtimeProjection, item.id),
    item.runtimeStatus, getChatThreadStatus(item.id) || getRuntime(item.id)?.threadStatus)
})));
const emptyLabel = computed(() => t('messenger.tasks.empty'));

// 行内动作按钮按需挂载：只为悬停行或右键菜单所属行渲染，避免每行都挂三个按钮。
const hoverId = ref('');
const openMenuId = ref('');
const shouldRenderActions = (id: string): boolean =>
  openMenuId.value ? openMenuId.value === id : hoverId.value === id;
const handleRowHover = (id: string) => {
  hoverId.value = id;
  // 注意：菜单打开期间（openMenuId 非空）绝不在别的行 mouseenter 时清它——
  // 菜单 teleport 到 body，指针移向菜单途中会扫过其他线程行；清掉会让菜单
  // 在鼠标到达前消失。关闭只由 closeMenu 驱动。
};

type RowAction = { key: 'rename' | 'trajectory' | 'archive'; label: string; icon: string; disabled: boolean };

const rowActions = (item: TaskListItem): RowAction[] => [
  { key: 'rename', label: t('messenger.tasks.rename'), icon: 'fa-pen-to-square', disabled: false },
  { key: 'trajectory', label: t('messenger.tasks.trajectory'), icon: 'fa-route', disabled: false },
  { key: 'archive', label: t('messenger.tasks.archive'), icon: 'fa-box-archive', disabled: !canArchive(item) }
];

const runRowAction = (key: RowAction['key'], id: string) => {
  if (key === 'rename') emit('rename', id);
  else if (key === 'trajectory') emit('trajectory', id);
  else emit('archive', id);
};

// -------------------------------------------------- 右键菜单（多选的唯一入口）

type ThreadMenuEntry = { command: string; label: string; icon: string; disabled?: boolean; divider?: boolean };

const menu = ref({ visible: false, x: 0, y: 0, id: '', archivable: false });
const menuRef = ref<HTMLElement | null>(null);
const menuStyle = computed(() => ({ left: `${menu.value.x}px`, top: `${menu.value.y}px` }));

const menuItems = computed<ThreadMenuEntry[]>(() => {
  if (!menu.value.visible) return [];
  const entries: ThreadMenuEntry[] = [];
  if (menu.value.id) {
    entries.push({ command: 'rename', label: t('messenger.tasks.rename'), icon: 'fa-pen-to-square' });
    entries.push({ command: 'trajectory', label: t('messenger.tasks.trajectory'), icon: 'fa-route' });
    entries.push({
      command: 'archive',
      label: t('messenger.tasks.archive'),
      icon: 'fa-box-archive',
      disabled: !menu.value.archivable
    });
    entries.push({ command: 'divider', label: '', icon: '', divider: true });
  }
  entries.push({
    command: 'multiSelect',
    label: t('messenger.tasks.multiSelect'),
    icon: 'fa-list-check',
    disabled: displayItems.value.length === 0
  });
  return entries;
});

const closeMenu = () => {
  if (!menu.value.visible) return;
  menu.value.visible = false;
  menu.value.id = '';
  if (openMenuId.value) openMenuId.value = '';
};

/** 菜单按实际尺寸回夹进视口：贴着右/下边缘右键时不会顶出屏幕。 */
const clampMenuPosition = (x: number, y: number) => {
  const element = menuRef.value;
  if (!element) return;
  const maxX = Math.max(8, window.innerWidth - (element.offsetWidth || 160) - 8);
  const maxY = Math.max(8, window.innerHeight - (element.offsetHeight || 120) - 8);
  menu.value.x = Math.min(x, maxX);
  menu.value.y = Math.min(y, maxY);
};

const openMenu = (item: TaskListItem | null, event: MouseEvent) => {
  // 多选态下动作由底部工具条承载，不再叠一层菜单。
  if (selectionMode.value) return;
  const x = Number.isFinite(event.clientX) && event.clientX > 0 ? event.clientX : 24;
  const y = Number.isFinite(event.clientY) && event.clientY > 0 ? event.clientY : 24;
  menu.value = { visible: true, x, y, id: item?.id || '', archivable: item ? canArchive(item) : false };
  openMenuId.value = item?.id || '';
  hoverId.value = item?.id || '';
  void nextTick(() => clampMenuPosition(x, y));
};

const chooseMenuItem = (command: string) => {
  const { id, archivable } = menu.value;
  closeMenu();
  if (command === 'multiSelect') {
    enterSelectionMode();
    return;
  }
  if (!id) return;
  if (command === 'rename') emit('rename', id);
  else if (command === 'trajectory') emit('trajectory', id);
  else if (command === 'archive' && archivable) emit('archive', id);
};

const handleOutsideMenu = (event: Event) => {
  if (!menu.value.visible) return;
  if (menuRef.value && event.composedPath().includes(menuRef.value)) return;
  closeMenu();
};

const handleMenuKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') closeMenu();
};

const stateIcons: Record<TaskIndicatorState, string> = {
  idle: 'fa-pause', running: 'fa-circle-notch fa-spin', pending: 'fa-clock', done: 'fa-check',
  error: 'fa-triangle-exclamation', inquiry: 'fa-circle-question', approval: 'fa-shield-halved'
};
const stateLabel = (state: TaskIndicatorState): string => t(`messenger.thread.state.${state}`);

const syncViewport = () => {
  // 列表滚动时右键菜单会脱离它指向的行，直接收起。
  closeMenu();
  scrollTop.value = viewport.value?.scrollTop || 0;
  height.value = viewport.value?.clientHeight || 320;
};

// -------------------------------------------------- 多选归档

// 归档门槛与单条归档一致：运行中/待执行/被编排锁定的线程不可归档。
const canArchive = (item: { state?: string; locked?: boolean }): boolean =>
  item.state !== 'running' && item.state !== 'pending' && !item.locked;

const selectionMode = ref(false);
const selectedIds = ref(new Set<string>());
const selectedCount = computed(() => selectedIds.value.size);

// 可归档条目只在多选模式下解析运行态（state 解析会订阅运行投影，平时不碰）；
// 非多选期返回空数组，避免把整个列表变成投影变化的依赖磁铁。
const archivableItems = computed(() => {
  if (!selectionMode.value) return [];
  return displayItems.value.filter(
    (item) => resolveItemState(item) !== 'running' && resolveItemState(item) !== 'pending' && !item.locked
  );
});
const allSelected = computed(
  () => archivableItems.value.length > 0 && archivableItems.value.every((item) => selectedIds.value.has(item.id))
);
const partialSelected = computed(() => !allSelected.value && archivableItems.value.some((item) => selectedIds.value.has(item.id)));

const enterSelectionMode = () => {
  selectedIds.value = new Set();
  // 菜单打开时点「多选」会让 el-dropdown 直接卸载，visible-change(false) 不会
  // 触发，openMenuId 一旦残留，退出多选后该行 is-menu-open 常亮、其他行再也
  // 呼不出菜单——进出多选都统一清掉悬停/菜单状态。
  openMenuId.value = '';
  hoverId.value = '';
  selectionMode.value = true;
};

const exitSelectionMode = () => {
  selectionMode.value = false;
  selectedIds.value = new Set();
  openMenuId.value = '';
  hoverId.value = '';
};

const toggleSelected = (item: { id: string; state?: string; locked?: boolean }) => {
  if (!selectionMode.value || !canArchive(item)) return;
  const next = new Set(selectedIds.value);
  if (next.has(item.id)) next.delete(item.id);
  else next.add(item.id);
  selectedIds.value = next;
};

const toggleSelectAll = () => {
  const next = new Set(selectedIds.value);
  if (allSelected.value) {
    for (const item of archivableItems.value) next.delete(item.id);
  } else {
    for (const item of archivableItems.value) next.add(item.id);
  }
  selectedIds.value = next;
};

const handleRowClick = (item: { id: string; state?: string; locked?: boolean }) => {
  if (selectionMode.value) {
    toggleSelected(item);
    return;
  }
  emit('open', item.id);
};

const archiveSelected = () => {
  if (!selectedCount.value) return;
  // 只提交仍可归档的条目，避免行状态在勾选后变化带来脏数据。
  const ids = archivableItems.value
    .filter((item) => selectedIds.value.has(item.id))
    .map((item) => item.id);
  if (!ids.length) return;
  exitSelectionMode();
  emit('archive-threads', ids);
};

// -------------------------------------------------- 拖拽排序（本地持久化）
// 行序由 usePersistentStableListOrder 保存；拖拽只调整已加载条目之间的次序。

const dragId = ref('');
const dropTarget = ref<{ id: string; position: 'before' | 'after' } | null>(null);

const resetDrag = () => {
  dragId.value = '';
  dropTarget.value = null;
};

const handleDragStart = (event: DragEvent, id: string) => {
  dragId.value = id;
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', id);
  }
};

const handleDragOver = (event: DragEvent, id: string) => {
  if (!dragId.value || dragId.value === id) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  const position = event.clientY - rect.top < rect.height / 2 ? 'before' : 'after';
  dropTarget.value = { id, position };
};

const handleDrop = (event: DragEvent, id: string) => {
  event.preventDefault();
  const target = dropTarget.value;
  if (dragId.value && target && target.id === id) {
    ordered.moveItem(
      dragId.value,
      target.id,
      target.position,
      displayItems.value.map((item) => item.id)
    );
  }
  resetDrag();
};

let observer: ResizeObserver | undefined;
onMounted(() => {
  void loadMore();
  syncViewport();
  // 捕获阶段拦截：行和按钮都会 stop 冒泡，capture 才能在它们之前收到。
  document.addEventListener('pointerdown', handleOutsideMenu, true);
  document.addEventListener('click', handleOutsideMenu, true);
  document.addEventListener('keydown', handleMenuKeydown, true);
  window.addEventListener('resize', closeMenu);
  window.addEventListener('blur', closeMenu);
});
watch(viewport, (element) => {
  observer?.disconnect();
  if (typeof ResizeObserver !== 'undefined' && element) {
    observer = new ResizeObserver(syncViewport);
    observer.observe(element);
  }
  syncViewport();
});
watch(
  () => props.agentId,
  () => {
    if (viewport.value) viewport.value.scrollTop = 0;
    syncViewport();
  }
);
watch(displayItems, () => {
  void syncViewport();
});
onBeforeUnmount(() => {
  observer?.disconnect();
  document.removeEventListener('pointerdown', handleOutsideMenu, true);
  document.removeEventListener('click', handleOutsideMenu, true);
  document.removeEventListener('keydown', handleMenuKeydown, true);
  window.removeEventListener('resize', closeMenu);
  window.removeEventListener('blur', closeMenu);
});
</script>

<style scoped>
/* 拖拽排序的可视反馈：源行半透明，落点行上下缘高亮。 */
.mz-thread-row.is-dragging {
  opacity: 0.45;
}

/* 右键菜单打开期间保持该行的悬停底色与动作按钮可见：指针已移入 teleport 的菜单，
   行本身不再处于 :hover，不补这条会视觉上「行和菜单断了关联」。 */
.mz-thread-row.is-menu-open {
  background: var(--mz-hover);
}

.mz-thread-row.is-menu-open .mz-thread-action {
  opacity: 1;
}

.mz-thread-row.is-drop-before {
  box-shadow: inset 0 2px 0 var(--mz-primary);
}

.mz-thread-row.is-drop-after {
  box-shadow: inset 0 -2px 0 var(--mz-primary);
}
</style>
