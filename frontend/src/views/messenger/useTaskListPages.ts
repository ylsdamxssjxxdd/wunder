import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';
import { listSessions } from '@/api/chat';
import { useChatStore } from '@/stores/chat';
import { mergeSessionRuntimeFields } from '@/stores/chatSessionMerge';

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
      const { data } = await listSessions({ agent_id: agentId.value, offset: offset.value, limit: 50 });
      if (disposed || request !== generation) return;
      const items = Array.isArray(data?.data?.items) ? data.data.items : [];
      const byId = new Map(store.sessions.map(item => [String(item.id), item]));
      for (const item of items) {
        if (item?.id) byId.set(String(item.id), mergeSessionRuntimeFields(byId.get(String(item.id)), item));
      }
      store.sessions = Array.from(byId.values());
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
