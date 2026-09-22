<template>
  <section class="messenger-right-panel messenger-right-panel--tasks">
    <div class="messenger-right-section-title">
      <span class="messenger-right-section-title-main"><i class="fa-solid fa-list-check" aria-hidden="true"></i>{{ t('messenger.tasks.title') }}</span>
      <span class="messenger-right-section-count">{{ items.length }}</span>
      <button class="messenger-header-btn" type="button" :disabled="creating" :title="t('messenger.tasks.create')" :aria-label="t('messenger.tasks.create')" @click="emit('create')"><i class="fa-solid fa-plus" aria-hidden="true"></i></button>
    </div>
    <div v-if="!items.length" class="messenger-list-empty">
      <span>{{ t('messenger.tasks.empty') }}</span>
      <button v-if="pageError" class="messenger-task-load-more" type="button" :disabled="loading" @click="loadMore">{{ t('messenger.tasks.retry') }}</button>
    </div>
    <div v-else ref="viewport" class="messenger-task-list" @scroll.passive="syncViewport">
      <div :style="{ height: `${range.start * ROW_HEIGHT}px`, flexShrink: 0 }" aria-hidden="true"></div>
      <div v-for="item in visibleItems" :key="item.id" class="messenger-task-item" :class="{ active: activeSessionId === item.id, 'is-running': isRunning(item.id) }">
        <button class="messenger-task-select" type="button" :aria-current="activeSessionId === item.id ? 'true' : undefined" :title="item.title" @click="emit('activate', item.id)">
          <i class="fa-solid messenger-task-item-icon" :class="isRunning(item.id) ? 'fa-circle-play is-running' : 'fa-message'" :aria-label="isRunning(item.id) ? t('chat.session.running') : undefined" aria-hidden="true"></i>
          <span class="messenger-task-item-main"><span class="messenger-task-item-title">{{ item.title }}</span><span class="messenger-task-item-preview">{{ isRunning(item.id) ? t('chat.session.running') : item.preview || t('messenger.preview.empty') }}</span></span>
        </button>
        <el-dropdown trigger="click" @command="(action) => handleAction(action, item.id)">
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
import { selectSessionRuntimeStatus } from '@/realtime/chat/chatRuntimeSelectors';
import { isThreadRuntimeBusy } from '@/utils/chatSessionRuntime';
import { taskWindow, type TaskListItem } from '@/views/messenger/taskList';
import { useTaskListPages } from '@/views/messenger/useTaskListPages';

const props = defineProps<{ items: TaskListItem[]; activeSessionId: string; agentId: string; creating: boolean }>();
const emit = defineEmits<{ create: []; activate: [id: string]; detail: [id: string]; rename: [id: string]; archive: [id: string] }>();
const { t } = useI18n();
const store = useChatStore();
const { loading, error: pageError, hasMore, loadMore } = useTaskListPages(toRef(props, 'agentId'));
const ROW_HEIGHT = 58;
const viewport = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const height = ref(400);
const range = computed(() => taskWindow(props.items.length, scrollTop.value, height.value, ROW_HEIGHT));
const visibleItems = computed(() => props.items.slice(range.value.start, range.value.end));
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
.messenger-task-list { display: block; }
.messenger-task-load-more { width: 100%; padding: 8px; color: inherit; background: none; border: 0; cursor: pointer; }
.messenger-task-item { height: 58px; min-height: 58px; padding: 0; border-radius: 8px; }
.messenger-task-select { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; height: 100%; background: none; border: 0; color: inherit; text-align: left; cursor: pointer; padding: 8px; }
.messenger-task-menu { background: none; border: 0; color: inherit; cursor: pointer; padding: 8px; }
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
