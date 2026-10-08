import { shallowRef, watch, type Ref, type ShallowRef } from 'vue';

import type { MixedConversation } from '@/views/messenger/model';

function normalizeConversationKey(value: MixedConversation | null | undefined): string {
  return String(value?.key || '').trim();
}

function normalizeConversationLastAt(value: MixedConversation | null | undefined): number {
  const normalized = Number(value?.lastAt || 0);
  return Number.isFinite(normalized) ? normalized : 0;
}

export function stabilizeMixedConversationOrder(
  previous: MixedConversation[],
  incoming: MixedConversation[]
): MixedConversation[] {
  const next = Array.isArray(incoming) ? incoming.filter((item) => normalizeConversationKey(item)) : [];
  if (!next.length) {
    return [];
  }
  if (!Array.isArray(previous) || !previous.length) {
    return next;
  }

  const previousIndexByKey = new Map<string, number>();
  previous.forEach((item, index) => {
    const key = normalizeConversationKey(item);
    if (!key || previousIndexByKey.has(key)) return;
    previousIndexByKey.set(key, index);
  });

  const additions: MixedConversation[] = [];
  const existing: MixedConversation[] = [];
  next.forEach((item) => {
    const key = normalizeConversationKey(item);
    if (!key) return;
    if (previousIndexByKey.has(key)) {
      existing.push(item);
      return;
    }
    additions.push(item);
  });

  additions.sort((left, right) => normalizeConversationLastAt(right) - normalizeConversationLastAt(left));
  existing.sort((left, right) => {
    const leftIndex = previousIndexByKey.get(normalizeConversationKey(left)) ?? Number.MAX_SAFE_INTEGER;
    const rightIndex = previousIndexByKey.get(normalizeConversationKey(right)) ?? Number.MAX_SAFE_INTEGER;
    return leftIndex - rightIndex;
  });
  return [...additions, ...existing];
}

export function useStableMixedConversationOrder(
  source: Ref<MixedConversation[]>
): ShallowRef<MixedConversation[]> {
  const stable = shallowRef<MixedConversation[]>([]);
  watch(
    source,
    (incoming) => {
      stable.value = stabilizeMixedConversationOrder(stable.value, incoming);
    },
    {
      immediate: true,
      deep: false
    }
  );
  return stable;
}
