import { computed, createApp, h } from 'vue';
import { createPinia } from 'pinia';
import { ElDropdown, ElDropdownItem, ElDropdownMenu } from 'element-plus';
import 'element-plus/dist/index.css';
import '@/styles/base.css';
import '@/styles/messenger.css';
import '@/vendor/fontawesome/css/fontawesome.min.css';
import '@/vendor/fontawesome/css/solid.min.css';
import MessengerTaskList from '@/components/messenger/MessengerTaskList.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import { useChatStore } from '@/stores/chat';
import { buildTaskList } from '@/views/messenger/taskList';
import { createChatRuntimeSessionProjection } from '@/realtime/chat/chatRuntimeReducer';
import { mergeSessionCatalogPage } from '@/stores/chatSessionCatalog';
import { applyCanonicalStreamRuntimeEvent } from '@/stores/chatRuntimeState';

const pinia = createPinia();
const store = useChatStore(pinia);
const app = createApp({
  setup() {
    const items = computed(() => buildTaskList(store.sessions, '', 'Thread'));
    return () => h('main', { style: 'width:360px;padding:16px' }, [
      h('div', { 'data-testid': 'reference-icons', style: 'display:flex;gap:20px;margin-bottom:20px' },
        (['idle', 'running', 'done', 'pending', 'error'] as const).map(state => h(AgentAvatar, { state }))),
      h(MessengerTaskList, {
        style: 'height:360px', items: items.value, activeSessionId: '', agentId: '',
        onArchive: (id: string) => store.archiveSession(id)
      })
    ]);
  }
});
app.use(pinia);
app.component('ElDropdown', ElDropdown);
app.component('ElDropdownItem', ElDropdownItem);
app.component('ElDropdownMenu', ElDropdownMenu);
app.mount('#fixture');
Object.assign(window, {
  taskListFixture: {
    setStatus(id: string, runtimeStatus: string, loading = false) {
      const projection = createChatRuntimeSessionProjection(id);
      projection.runtimeStatus = runtimeStatus as typeof projection.runtimeStatus;
      store.runtimeProjection.sessions[id] = projection;
      store.loadingBySession[id] = loading;
      store.runtimeProjectionVersionBySession[id] = (store.runtimeProjectionVersionBySession[id] || 0) + 1;
    },
    replayPage(items: Record<string, unknown>[]) { mergeSessionCatalogPage(store, { items }); },
    quota(total: number, eventId: number) {
      applyCanonicalStreamRuntimeEvent(store, 'thread-0', 'quota_usage', {
        session_quota_used: total, consumed: 1, quota_used_total: 999
      }, String(eventId));
    },
    unmount() { app.unmount(); }
  }
});
