<template>
  <section class="messenger-right-panel messenger-right-panel--tasks">
    <div class="messenger-right-section-title">
      <span class="messenger-right-section-title-main"><i class="fa-solid fa-list-check" aria-hidden="true"></i>{{ t('messenger.tasks.title') }}</span>
      <span class="messenger-right-section-count">{{ items.length }}</span>
    </div>
    <div v-if="!items.length" class="messenger-list-empty">
      <span>{{ t('messenger.tasks.empty') }}</span>
      <button v-if="pageError" class="messenger-task-load-more" type="button" :disabled="loading" @click="loadMore">{{ t('messenger.tasks.retry') }}</button>
    </div>
    <div v-else ref="viewport" class="messenger-task-list" @scroll.passive="syncViewport">
      <div :style="{ height: `${range.start * ROW_HEIGHT}px`, flexShrink: 0 }" aria-hidden="true"></div>
      <div v-for="(item, index) in visibleItems" :key="item.id" class="messenger-task-item" :class="{ active: activeSessionId === item.id, 'is-running': isRunning(item.id), 'is-dragging': dragState.key === item.id, 'is-drop-before': isDropBefore(index), 'is-drop-after': isDropAfter(index) }"
        draggable="true" @dragstart="handleDragStart($event, item.id)" @dragover="handleDragOver($event, index)" @drop="handleDrop($event)" @dragend="resetDrag">
        <button class="messenger-task-select" type="button" :aria-current="activeSessionId === item.id ? 'true' : undefined" :title="item.title" @click="emit('activate', item.id)">
          <i class="fa-solid messenger-task-item-icon" :class="isRunning(item.id) ? 'fa-circle-play is-running' : 'fa-message'" :aria-label="isRunning(item.id) ? t('chat.session.running') : undefined" aria-hidden="true"></i>
          <span class="messenger-task-item-main"><span class="messenger-task-item-title">{{ item.title }}</span><span class="messenger-task-item-meta"><span :title="t('messenger.tasks.tokens')"><i class="fa-solid fa-bolt" aria-hidden="true"></i>{{ formatCompactCount(item.consumedTokens) }}</span><span :title="t('messenger.tasks.tools')"><i class="fa-solid fa-screwdriver-wrench" aria-hidden="true"></i>{{ formatCompactCount(item.toolCalls) }}</span></span></span>
        </button>
        <el-dropdown trigger="click" :teleported="true" placement="bottom-end" popper-class="messenger-task-dropdown" @command="(action) => handleAction(action, item.id)">
          <button class="messenger-task-menu" type="button" :title="t('common.more')" :aria-label="t('common.more')"><i class="fa-solid fa-ellipsis" aria-hidden="true"></i></button>
          <template #dropdown><el-dropdown-menu>
            <el-dropdown-item command="detail">{{ t('messenger.timeline.detail.open') }}</el-dropdown-item>
            <el-dropdown-item command="rename">{{ t('chat.history.rename') }}</el-dropdown-item>
            <el-dropdown-item command="archive" :disabled="isRunning(item.id) || item.locked">{{ t('chat.history.archive') }}</el-dropdown-item>
          </el-dropdown-menu></template>
        </el-dropdown>
      </div>
      <div :style="{ height: `${(items.length - range.end) * ROW_HEIGHT}px`, flexShrink: 0 }" aria-hidden="true"></div>
      <button v-if="hasMore" class="messenger-task-load-more" type="button" :disabled="loading" @click="loadMore">{{ loading ? t('common.loading') : pageError ? t('messenger.tasks.retry') : t('messenger.tasks.loadMore') }}</button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { useAuthStore } from '@/stores/auth';
import { selectSessionRuntimeStatus } from '@/realtime/chat/chatRuntimeSelectors';
import { isThreadRuntimeBusy } from '@/utils/chatSessionRuntime';
import { taskWindow, type TaskListItem } from '@/views/messenger/taskList';
import { useTaskListPages } from '@/views/messenger/useTaskListPages';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import { formatCompactCount } from '@/utils/compactNumber';

const props = defineProps<{ items: TaskListItem[]; activeSessionId: string; agentId: string }>();
const emit = defineEmits<{ activate: [id: string]; detail: [id: string]; rename: [id: string]; archive: [id: string] }>();
const { t } = useI18n();
const store = useChatStore();
const authStore = useAuthStore();
const { loading, error: pageError, hasMore, loadMore } = useTaskListPages(toRef(props, 'agentId'));
const ROW_HEIGHT = 54;
const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const height = ref(400);
const ordered = usePersistentStableListOrder(toRef(props, 'items'), {
  getKey: (item) => item.id,
  storageKey: computed(() => `messenger:threads:${String((authStore.user as Record<string, unknown> | null)?.id || (authStore.user as Record<string, unknown> | null)?.user_id || 'guest')}:${props.agentId || 'default'}`)
});
const range = computed(() => taskWindow(ordered.orderedItems.value.length, scrollTop.value, height.value, ROW_HEIGHT));
const visibleItems = computed(() => ordered.orderedItems.value.slice(range.value.start, range.value.end));
const dragState = ref({ key: '', index: -1 });
// Subscribe only inside this small component. Streaming must not invalidate the page shell or sort all tasks.
const isRunning = (id: string) => {
  void store.runtimeProjectionVersionBySession[id];
  return isThreadRuntimeBusy(selectSessionRuntimeStatus(store.runtimeProjection, id)) || Boolean(store.loadingBySession[id]);
};
const handleAction = (action: string, id: string) => {
  if (action === 'detail') emit('detail', id);
  else if (action === 'rename') emit('rename', id);
  else if (action === 'archive') emit('archive', id);
};
const resolveInsertIndex = (event: DragEvent, index: number) => {
  const target = event.currentTarget as HTMLElement | null;
  if (!target) return index;
  const rect = target.getBoundingClientRect();
  return event.clientY > rect.top + rect.height / 2 ? index + 1 : index;
};
const handleDragStart = (event: DragEvent, id: string) => {
  dragState.value = { key: id, index: -1 };
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', id);
  }
};
const handleDragOver = (event: DragEvent, index: number) => {
  if (!dragState.value.key) return;
  event.preventDefault();
  dragState.value.index = resolveInsertIndex(event, index);
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
};
const handleDrop = (event: DragEvent) => {
  const draggedKey = dragState.value.key;
  if (!draggedKey) return;
  event.preventDefault();
  const keys = visibleItems.value.map((item) => item.id);
  const rawIndex = Math.max(0, Math.min(dragState.value.index < 0 ? keys.length : dragState.value.index, keys.length));
  const sourceIndex = keys.indexOf(draggedKey);
  const withoutDragged = keys.filter((key) => key !== draggedKey);
  const insertionIndex = sourceIndex >= 0 && rawIndex > sourceIndex ? rawIndex - 1 : rawIndex;
  const previous = withoutDragged[insertionIndex - 1];
  const next = withoutDragged[insertionIndex];
  // Use the full persisted order when committing. The dragged row may leave
  // the virtual window while the pointer is over another visible row.
  if (next) ordered.moveItem(draggedKey, next, 'before');
  else if (previous) ordered.moveItem(draggedKey, previous, 'after');
  resetDrag();
};
const resetDrag = () => { dragState.value = { key: '', index: -1 }; };
const isDropBefore = (index: number) => dragState.value.index === index;
const isDropAfter = (index: number) => index === visibleItems.value.length - 1 && dragState.value.index === visibleItems.value.length;
const syncViewport = () => { scrollTop.value = viewport.value?.scrollTop || 0; height.value = viewport.value?.clientHeight || 400; };
let observer: ResizeObserver | undefined;
onMounted(() => {
  void loadMore();
  syncViewport();
  if (typeof ResizeObserver !== 'undefined' && viewport.value) { observer = new ResizeObserver(syncViewport); observer.observe(viewport.value); }
});
watch(() => props.agentId, () => { if (viewport.value) viewport.value.scrollTop = 0; syncViewport(); });
watch(() => props.items.length, () => void nextTick(syncViewport));
onBeforeUnmount(() => observer?.disconnect());
</script>

<style scoped>
.messenger-right-panel--tasks { padding: 10px; }
.messenger-task-list { display: block; overflow-x: hidden; }
.messenger-task-load-more { width: 100%; padding: 8px; color: inherit; background: none; border: 0; cursor: pointer; }
.messenger-right-section-title { margin-bottom: 6px; padding-bottom: 6px; gap: 6px; }
.messenger-task-item { position: relative; height: 46px; min-height: 46px; margin-bottom: 8px; padding: 0; border-radius: 7px; }
.messenger-task-select { display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0; height: 100%; background: none; border: 0; color: inherit; text-align: left; cursor: pointer; padding: 4px 6px; }
.messenger-task-menu { background: none; border: 0; color: inherit; cursor: pointer; padding: 4px 6px; }
.messenger-task-item-icon { width: 20px; flex-basis: 20px; }
.messenger-task-item-main { gap: 1px; }
.messenger-task-item-meta { display: flex; align-items: center; gap: 9px; color: var(--messenger-polish-muted, #7d8591); font-size: 10px; line-height: 1; }
.messenger-task-item-meta span { display: inline-flex; align-items: center; gap: 3px; }
.messenger-task-item-meta i { color: var(--ui-accent); font-size: 9px; }
.messenger-task-item.is-dragging { opacity: .55; }
.messenger-task-item.is-drop-before::before, .messenger-task-item.is-drop-after::after { content: ''; position: absolute; left: 4px; right: 4px; height: 2px; border-radius: 999px; background: var(--ui-accent); pointer-events: none; z-index: 2; }
.messenger-task-item.is-drop-before::before { top: -4px; }
.messenger-task-item.is-drop-after::after { bottom: -4px; }
:global(.messenger-task-dropdown.el-popper) { box-sizing: border-box; max-width: min(180px, calc(100vw - 24px)); overflow: hidden; }
:global(.messenger-task-dropdown .el-dropdown-menu) { min-width: 118px; max-width: 180px; overflow-x: hidden; }
:global(.messenger-task-dropdown .el-dropdown-menu__item) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.messenger-task-select:focus-visible, .messenger-task-menu:focus-visible { outline: 2px solid var(--ui-accent); outline-offset: -2px; }
@keyframes messenger-task-running-pulse {
  0%, 100% { opacity: .62; transform: scale(.9); }
  50% { opacity: 1; transform: scale(1); }
}
.messenger-task-item-icon.is-running { animation: messenger-task-running-pulse 1.15s ease-in-out infinite; }
@media (prefers-reduced-motion: reduce) {
  .messenger-task-item-icon.is-running { animation: none; opacity: 1; transform: none; }
}
</style>
