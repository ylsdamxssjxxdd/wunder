import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';
import { listSessions } from '@/api/chat';
import { useChatStore } from '@/stores/chat';
import { sessionCatalogCheckIds, mergeSessionCatalogPage, cacheSessionCatalog } from '@/stores/chatSessionCatalog';

// Fetch summaries only and only on demand. A late page cannot change navigation.
export function useTaskListPages(agentId: Ref<string>) {
  const store = useChatStore();
  const loading = ref(false);
  const error = ref(false);
  const offset = ref(0);
  const total = ref<number | null>(null);
  let generation = 0;
  let disposed = false;
  const hasMore = computed(() => total.value === null || offset.value < total.value);
  const loadMore = async () => {
    if (disposed || loading.value || !hasMore.value) return;
    const request = generation;
    loading.value = true;
    error.value = false;
    try {
      const targetAgentId = String(agentId.value || '').trim().replace(/^(?:default|__default__)$/, '');
      const checkedIds = sessionCatalogCheckIds(store, targetAgentId);
      const { data } = await listSessions({ agent_id: targetAgentId, offset: offset.value, limit: 50, known_session_ids: checkedIds.join(',') });
      if (disposed || request !== generation) return;
      const items = Array.isArray(data?.data?.items) ? data.data.items : [];
      mergeSessionCatalogPage(store, data?.data || {}, checkedIds);
      cacheSessionCatalog(store, targetAgentId);
      offset.value += items.length;
      total.value = items.length ? Number(data?.data?.total ?? offset.value) : offset.value;
    } catch {
      if (!disposed && request === generation) error.value = true;
    } finally {
      if (!disposed && request === generation) loading.value = false;
    }
  };
  watch(agentId, () => {
    generation += 1;
    offset.value = 0;
    total.value = null;
    loading.value = false;
    error.value = false;
    void loadMore();
  });
  onBeforeUnmount(() => { disposed = true; generation += 1; });
  return { loading, error, hasMore, loadMore };
}
