<template>
  <div class="mz-thread-tree" :class="{ 'is-selecting': selectionMode }" role="tree" :aria-label="t('messenger.tasks.title')">
    <div ref="viewport" class="mz-thread-scroll" @scroll.passive="syncViewport">
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
          :title="item.title"
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
        <el-dropdown
          v-if="!selectionMode && shouldRenderMenu(item.id)"
          trigger="click"
          :teleported="true"
          placement="bottom-end"
          popper-class="mz-thread-dropdown"
          @command="(action: string) => handleAction(action, item.id)"
          @visible-change="(visible: boolean) => handleMenuVisible(item.id, visible)"
        >
          <button class="mz-thread-menu" type="button" :title="t('common.more')" :aria-label="t('common.more')">
            <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
          </button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="rename">{{ t('messenger.tasks.rename') }}</el-dropdown-item>
              <el-dropdown-item command="trajectory">{{ t('messenger.trajectory.open') }}</el-dropdown-item>
              <el-dropdown-item command="multiSelect">{{ t('messenger.tasks.multiSelect') }}</el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
        <button
          v-if="!selectionMode && shouldRenderMenu(item.id)"
          class="mz-thread-archive"
          type="button"
          :title="t('messenger.tasks.archive')"
          :aria-label="t('messenger.tasks.archive')"
          :disabled="!canArchive(item)"
          @click="emit('archive', item.id)"
        >
          <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
        </button>
      </div>
      <div :style="{ height: `${(displayItems.length - range.end) * ROW_HEIGHT}px` }" aria-hidden="true"></div>
      <button v-if="hasMore" class="mz-thread-load-more" type="button" :disabled="loading" @click="loadMore">
        {{ loading ? t('common.loading') : pageError ? t('messenger.tasks.retry') : t('messenger.tasks.loadMore') }}
      </button>
    </div>
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
import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { useAuthStore } from '@/stores/auth';
import { selectSessionRuntimeStatus } from '@/realtime/chat/chatRuntimeSelectors';
import { getChatThreadStatus } from '@/realtime/chat/chatThreadRuntime';
import { resolveVisibleTaskRows, taskWindow, type TaskListItem } from '@/views/messenger/taskList';
import { hasCompletedTaskTurn, resolveTaskIndicatorState, resolveTaskRuntimeState, type TaskIndicatorState } from '@/views/messenger/taskRuntimeState';
import { getRuntime, getSessionMessages, hasRuntimeControllers } from '@/stores/chatRuntimeState';
import { useTaskListPages } from '@/views/messenger/useTaskListPages';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import type { AgentRuntimeState } from '@/views/messenger/model';

// §5.2 thread rows are 28px tall so the row height is also the virtual window step.
const ROW_HEIGHT = 28;

const props = defineProps<{ items: TaskListItem[]; activeSessionId: string; agentId: string }>();
const emit = defineEmits<{
  open: [id: string];
  trajectory: [id: string];
  rename: [id: string];
  archive: [id: string];
  'archive-threads': [ids: string[]];
}>();

const { t } = useI18n();
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

// Row menus mount on demand: render the dropdown only for the hovered row or
// the row whose menu is currently open, instead of one el-dropdown per row.
const hoverId = ref('');
const openMenuId = ref('');
const shouldRenderMenu = (id: string): boolean =>
  openMenuId.value ? openMenuId.value === id : hoverId.value === id;
const handleRowHover = (id: string) => {
  hoverId.value = id;
  // 注意：菜单打开期间（openMenuId 非空）绝不在别的行 mouseenter 时清它——
  // 弹层是 teleport 到 body 的，指针移向弹层途中扫过其他线程行会触发这里；
  // 一旦清除，el-dropdown 会连同弹层一起被卸载，菜单在鼠标到达前就消失。
  // 关闭只由 visible-change(false)（点外部/选中项）驱动。
};
const handleMenuVisible = (id: string, visible: boolean) => {
  if (visible) openMenuId.value = id;
  else if (openMenuId.value === id) openMenuId.value = '';
};

const stateIcons: Record<TaskIndicatorState, string> = {
  idle: 'fa-pause', running: 'fa-circle-notch fa-spin', pending: 'fa-clock', done: 'fa-check',
  error: 'fa-triangle-exclamation', inquiry: 'fa-circle-question', approval: 'fa-shield-halved'
};
const stateLabel = (state: TaskIndicatorState): string => t(`messenger.thread.state.${state}`);

const syncViewport = () => {
  scrollTop.value = viewport.value?.scrollTop || 0;
  height.value = viewport.value?.clientHeight || 320;
};

const handleAction = (action: string, id: string) => {
  if (action === 'trajectory') emit('trajectory', id);
  else if (action === 'rename') emit('rename', id);
  else if (action === 'multiSelect') enterSelectionMode();
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
});
</script>

<style scoped>
:global(.mz-thread-dropdown .el-dropdown-menu__item) {
  font-size: 13px;
}

/* 拖拽排序的可视反馈：源行半透明，落点行上下缘高亮。 */
.mz-thread-row.is-dragging {
  opacity: 0.45;
}

/* 菜单打开期间保持该行的悬停底色与三个点可见：指针已移入 teleport 的弹层，
   行本身不再处于 :hover，不补这条会视觉上「行和菜单断了关联」。 */
.mz-thread-row.is-menu-open {
  background: var(--mz-hover);
}

.mz-thread-row.is-menu-open .mz-thread-menu {
  opacity: 1;
}

.mz-thread-row.is-drop-before {
  box-shadow: inset 0 2px 0 var(--mz-primary);
}

.mz-thread-row.is-drop-after {
  box-shadow: inset 0 -2px 0 var(--mz-primary);
}
</style>
