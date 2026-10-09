<template>
  <div class="mz-thread-tree" role="tree" :aria-label="t('messenger.tasks.title')">
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
          'is-dragging': dragId === item.id,
          'is-drop-before': dropTarget?.id === item.id && dropTarget.position === 'before',
          'is-drop-after': dropTarget?.id === item.id && dropTarget.position === 'after'
        }"
        draggable="true"
        role="treeitem"
        :aria-selected="activeSessionId === item.id ? 'true' : 'false'"
        @dragstart="handleDragStart($event, item.id)"
        @dragover="handleDragOver($event, item.id)"
        @drop="handleDrop($event, item.id)"
        @dragend="resetDrag"
      >
        <button
          class="mz-thread-select"
          type="button"
          :title="item.title"
          :aria-current="activeSessionId === item.id ? 'true' : undefined"
          @click="emit('open', item.id)"
        >
          <span
            class="mz-thread-dot"
            :class="`is-${item.state}`"
            :title="stateLabel(item.state)"
            aria-hidden="true"
          ></span>
          <span class="mz-thread-title">{{ item.title }}</span>
        </button>
        <el-dropdown
          trigger="click"
          :teleported="true"
          placement="bottom-end"
          popper-class="mz-thread-dropdown"
          @command="(action: string) => handleAction(action, item.id)"
        >
          <button class="mz-thread-menu" type="button" :title="t('common.more')" :aria-label="t('common.more')">
            <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
          </button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="rename">{{ t('messenger.tasks.rename') }}</el-dropdown-item>
              <el-dropdown-item command="detail">{{ t('messenger.timeline.detail.open') }}</el-dropdown-item>
              <el-dropdown-item
                command="archive"
                :disabled="item.state === 'running' || item.state === 'pending' || item.locked"
              >
                {{ t('messenger.tasks.archive') }}
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
      <div :style="{ height: `${(displayItems.length - range.end) * ROW_HEIGHT}px` }" aria-hidden="true"></div>
      <button v-if="hasMore" class="mz-thread-load-more" type="button" :disabled="loading" @click="loadMore">
        {{ loading ? t('common.loading') : pageError ? t('messenger.tasks.retry') : t('messenger.tasks.loadMore') }}
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
import { taskWindow, type TaskListItem } from '@/views/messenger/taskList';
import { hasCompletedTaskTurn, resolveTaskRuntimeState } from '@/views/messenger/taskRuntimeState';
import { getRuntime, getSessionMessages, hasRuntimeControllers } from '@/stores/chatRuntimeState';
import { useTaskListPages } from '@/views/messenger/useTaskListPages';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import type { AgentRuntimeState } from '@/views/messenger/model';

// §5.2 thread rows are 28px tall so the row height is also the virtual window step.
const ROW_HEIGHT = 28;

const props = defineProps<{ items: TaskListItem[]; activeSessionId: string; agentId: string }>();
const emit = defineEmits<{
  open: [id: string];
  detail: [id: string];
  rename: [id: string];
  archive: [id: string];
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

const displayItems = computed(() =>
  ordered.orderedItems.value.map((item) => ({ ...item, state: resolveItemState(item) }))
);
const range = computed(() => taskWindow(displayItems.value.length, scrollTop.value, height.value, ROW_HEIGHT));
// Subscribe inside this small component; text streaming must not invalidate the page shell.
const visibleItems = computed(() => displayItems.value.slice(range.value.start, range.value.end));
const emptyLabel = computed(() => t('messenger.tasks.empty'));

const stateLabel = (state: AgentRuntimeState): string => t(`messenger.thread.state.${state}`);

const syncViewport = () => {
  scrollTop.value = viewport.value?.scrollTop || 0;
  height.value = viewport.value?.clientHeight || 320;
};

const handleAction = (action: string, id: string) => {
  if (action === 'detail') emit('detail', id);
  else if (action === 'rename') emit('rename', id);
  else if (action === 'archive') emit('archive', id);
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

.mz-thread-row.is-drop-before {
  box-shadow: inset 0 2px 0 var(--mz-primary);
}

.mz-thread-row.is-drop-after {
  box-shadow: inset 0 -2px 0 var(--mz-primary);
}
</style>
