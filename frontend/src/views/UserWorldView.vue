<template>
  <div class="portal-shell user-world-shell">
    <UserTopbar :title="t('userWorld.title')" :subtitle="t('userWorld.subtitle')" :hide-chat="true" />
    <main class="user-world-main">
      <aside class="user-world-sidebar">
        <div class="user-world-search">
          <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
          <input
            v-model.trim="keyword"
            type="text"
            :placeholder="t('userWorld.search.placeholder')"
            @input="handleSearch"
          />
        </div>
        <div class="user-world-contact-list">
          <template v-if="displayRows.length">
            <template v-for="row in displayRows" :key="row.key">
              <button
                v-if="isUnitRow(row)"
                class="user-world-unit-item"
                type="button"
                :style="resolveUnitIndentStyle(row.depth)"
                @click="toggleUnit(row.unitId)"
              >
                <i
                  class="fa-solid fa-caret-right user-world-unit-arrow"
                  :class="{ open: isUnitExpanded(row.unitId) }"
                  aria-hidden="true"
                ></i>
                <span class="user-world-unit-name" :title="row.pathName">{{ row.name }}</span>
                <span class="user-world-unit-count">{{ row.total }}</span>
              </button>
              <button
                v-else
                class="user-world-contact-item"
                :class="{ active: activePeerUserId === row.contact.user_id }"
                type="button"
                :style="resolveContactIndentStyle(row.depth)"
                @click="handleOpenContact(row.contact)"
              >
                <div class="user-world-contact-avatar">{{ resolveAvatarLabel(row.contact.username) }}</div>
                <div class="user-world-contact-main">
                  <div class="user-world-contact-row">
                    <span class="user-world-contact-name" :title="row.contact.username || row.contact.user_id">
                      {{ row.contact.username || row.contact.user_id }}
                    </span>
                    <span class="user-world-contact-time">{{ formatTime(row.contact.last_message_at) }}</span>
                  </div>
                  <div class="user-world-contact-row">
                    <span class="user-world-contact-preview">
                      {{ row.contact.last_message_preview || t('userWorld.contact.emptyPreview') }}
                    </span>
                    <span v-if="resolveUnread(row.contact) > 0" class="user-world-contact-unread">
                      {{ resolveUnread(row.contact) }}
                    </span>
                  </div>
                </div>
              </button>
            </template>
          </template>
          <div v-else class="user-world-empty">
            {{ t('userWorld.contact.empty') }}
          </div>
        </div>
      </aside>

      <section class="user-world-chat">
        <header class="user-world-chat-header">
          <div class="user-world-chat-title">
            {{ activeContactName || t('userWorld.chat.placeholderTitle') }}
          </div>
          <div class="user-world-chat-subtitle">
            {{ t('userWorld.chat.subtitle') }}
          </div>
        </header>
        <div ref="messageContainerRef" class="user-world-message-list">
          <div v-if="!activeConversationId" class="user-world-empty">
            {{ t('userWorld.chat.emptyConversation') }}
          </div>
          <template v-else>
            <div
              v-for="message in activeMessages"
              :key="`${message.message_id}`"
              class="user-world-message-item"
              :class="{ mine: isMine(message) }"
            >
              <div class="user-world-message-bubble">
                <div class="user-world-message-content">{{ message.content }}</div>
                <div class="user-world-message-time">{{ formatTime(message.created_at) }}</div>
              </div>
            </div>
          </template>
        </div>
        <footer class="user-world-composer">
          <textarea
            v-model="draft"
            :placeholder="t('userWorld.input.placeholder')"
            @keydown.enter.exact.prevent="handleSend"
          ></textarea>
          <button
            class="user-world-send-btn"
            type="button"
            :disabled="!canSend"
            @click="handleSend"
          >
            {{ t('userWorld.input.send') }}
          </button>
        </footer>
      </section>
    </main>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchOrgUnits } from '@/api/auth';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useUserWorldStore } from '@/stores/userWorld';

const UNIT_UNGROUPED_ID = '__ungrouped__';

type ContactItem = {
  user_id: string;
  username: string;
  unit_id?: string | null;
  conversation_id?: string | null;
  last_message_preview?: string | null;
  last_message_at?: number | null;
  unread_count?: number;
};

type MessageItem = {
  message_id: number;
  sender_user_id: string;
  content: string;
  created_at: number;
};

type UnitNode = {
  unit_id: string;
  name: string;
  path_name: string;
  children: UnitNode[];
};

type UnitDisplayRow = {
  kind: 'unit';
  key: string;
  unitId: string;
  name: string;
  pathName: string;
  depth: number;
  total: number;
};

type ContactDisplayRow = {
  kind: 'contact';
  key: string;
  depth: number;
  contact: ContactItem;
};

type DisplayRow = UnitDisplayRow | ContactDisplayRow;

type UnitBuildResult = {
  rows: DisplayRow[];
  total: number;
};

const { t } = useI18n();
const authStore = useAuthStore();
const userWorldStore = useUserWorldStore();

const keyword = ref('');
const draft = ref('');
const messageContainerRef = ref<HTMLElement | null>(null);
const orgUnitTree = ref<UnitNode[]>([]);
const orgUnitPathMap = ref<Record<string, string>>({});
const collapsedUnitIds = ref<Set<string>>(new Set());

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeUnitNode = (value: unknown): UnitNode | null => {
  const source = asRecord(value);
  const unitId = normalizeText(source.unit_id);
  if (!unitId) return null;
  const name = normalizeText(source.name) || unitId;
  const pathName = normalizeText(source.path_name) || name;
  const rawChildren = Array.isArray(source.children) ? source.children : [];
  const children = rawChildren.map(normalizeUnitNode).filter((item): item is UnitNode => Boolean(item));
  return {
    unit_id: unitId,
    name,
    path_name: pathName,
    children
  };
};

const resolveUnitLabel = (unitId: string): string => {
  const cleaned = normalizeText(unitId);
  if (!cleaned) return t('userWorld.unit.ungrouped');
  return orgUnitPathMap.value[cleaned] || cleaned;
};

const filteredContacts = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  const contacts = userWorldStore.contacts as ContactItem[];
  if (!query) return contacts;
  return contacts.filter((item) => {
    const username = String(item.username || '').toLowerCase();
    const userId = String(item.user_id || '').toLowerCase();
    const unitLabel = resolveUnitLabel(String(item.unit_id || '')).toLowerCase();
    return username.includes(query) || userId.includes(query) || unitLabel.includes(query);
  });
});

const hasSearchKeyword = computed(() => Boolean(keyword.value.trim()));

const isUnitExpanded = (unitId: string): boolean =>
  hasSearchKeyword.value || !collapsedUnitIds.value.has(unitId);

const toggleUnit = (unitId: string) => {
  const cleaned = normalizeText(unitId);
  if (!cleaned || hasSearchKeyword.value) return;
  const next = new Set(collapsedUnitIds.value);
  if (next.has(cleaned)) {
    next.delete(cleaned);
  } else {
    next.add(cleaned);
  }
  collapsedUnitIds.value = next;
};

const resolveContactIndentStyle = (depth: number): Record<string, string> => ({
  '--uw-contact-indent': `${Math.max(0, depth) * 14}px`
});

const resolveUnitIndentStyle = (depth: number): Record<string, string> => ({
  '--uw-unit-indent': `${Math.max(0, depth) * 14}px`
});

const isUnitRow = (row: DisplayRow): row is UnitDisplayRow => row.kind === 'unit';

const bucketContactsByUnit = (contacts: ContactItem[]): Map<string, ContactItem[]> => {
  const buckets = new Map<string, ContactItem[]>();
  contacts.forEach((contact) => {
    const unitId = normalizeText(contact.unit_id);
    const key = unitId || UNIT_UNGROUPED_ID;
    if (!buckets.has(key)) {
      buckets.set(key, []);
    }
    buckets.get(key)?.push(contact);
  });
  return buckets;
};

const sortContactsByRecent = (contacts: ContactItem[]): ContactItem[] =>
  [...contacts].sort((left, right) => {
    const leftTs = Number(left.last_message_at || 0);
    const rightTs = Number(right.last_message_at || 0);
    if (leftTs !== rightTs) {
      return rightTs - leftTs;
    }
    return String(left.username || left.user_id || '').localeCompare(String(right.username || right.user_id || ''));
  });

const buildUnitRows = (
  units: UnitNode[],
  depth: number,
  buckets: Map<string, ContactItem[]>,
  keywordQuery: string
): UnitBuildResult => {
  let rows: DisplayRow[] = [];
  let total = 0;
  units.forEach((unit) => {
    const selfContacts = sortContactsByRecent(buckets.get(unit.unit_id) || []);
    buckets.delete(unit.unit_id);
    const childResult = buildUnitRows(unit.children, depth + 1, buckets, keywordQuery);
    const subtreeTotal = selfContacts.length + childResult.total;
    const unitText = `${unit.name} ${unit.path_name}`.toLowerCase();
    const unitMatched = keywordQuery ? unitText.includes(keywordQuery) : false;
    if (keywordQuery && subtreeTotal <= 0 && !unitMatched) {
      return;
    }
    rows.push({
      kind: 'unit',
      key: `unit:${unit.unit_id}`,
      unitId: unit.unit_id,
      name: unit.name,
      pathName: unit.path_name,
      depth,
      total: subtreeTotal
    });
    total += subtreeTotal;
    if (!isUnitExpanded(unit.unit_id)) {
      return;
    }
    selfContacts.forEach((contact, index) => {
      rows.push({
        kind: 'contact',
        key: `contact:${unit.unit_id}:${contact.user_id}:${index}`,
        depth: depth + 1,
        contact
      });
    });
    rows = rows.concat(childResult.rows);
  });
  return { rows, total };
};

const displayRows = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  const buckets = bucketContactsByUnit(filteredContacts.value);
  const rootResult = buildUnitRows(orgUnitTree.value, 0, buckets, query);
  let rows = [...rootResult.rows];

  const extraEntries = [...buckets.entries()].filter(([, contacts]) => contacts.length > 0);
  extraEntries.sort((left, right) => {
    const leftLabel = left[0] === UNIT_UNGROUPED_ID ? t('userWorld.unit.ungrouped') : resolveUnitLabel(left[0]);
    const rightLabel = right[0] === UNIT_UNGROUPED_ID ? t('userWorld.unit.ungrouped') : resolveUnitLabel(right[0]);
    return leftLabel.localeCompare(rightLabel, 'zh-CN');
  });
  extraEntries.forEach(([unitId, contacts]) => {
    const label = unitId === UNIT_UNGROUPED_ID ? t('userWorld.unit.ungrouped') : resolveUnitLabel(unitId);
    rows.push({
      kind: 'unit',
      key: `unit-extra:${unitId}`,
      unitId,
      name: label,
      pathName: label,
      depth: 0,
      total: contacts.length
    });
    if (!isUnitExpanded(unitId)) {
      return;
    }
    sortContactsByRecent(contacts).forEach((contact, index) => {
      rows.push({
        kind: 'contact',
        key: `contact-extra:${unitId}:${contact.user_id}:${index}`,
        depth: 1,
        contact
      });
    });
  });
  return rows;
});

const activeConversationId = computed(() => userWorldStore.activeConversationId);

const activeConversation = computed(() => userWorldStore.activeConversation);

const activePeerUserId = computed(() => String(activeConversation.value?.peer_user_id || '').trim());

const activeContact = computed(() => {
  const peerId = activePeerUserId.value;
  if (!peerId) return null;
  return userWorldStore.contacts.find((item) => item.user_id === peerId) || null;
});

const activeContactName = computed(() => {
  if (activeContact.value?.username) {
    return activeContact.value.username;
  }
  if (activeConversation.value?.peer_user_id) {
    return activeConversation.value.peer_user_id;
  }
  return '';
});

const activeMessages = computed(() => userWorldStore.activeMessages as MessageItem[]);

const canSend = computed(
  () => Boolean(activeConversationId.value) && Boolean(draft.value.trim()) && !userWorldStore.sending
);

const resolveErrorMessage = (error: unknown): string => {
  const message = String((error as { message?: unknown })?.message || '').trim();
  return message || userWorldStore.error || t('common.requestFailed');
};

const scrollToBottom = async () => {
  await nextTick();
  const el = messageContainerRef.value;
  if (!el) return;
  el.scrollTop = el.scrollHeight;
};

const handleSearch = () => {
  // 搜索在本地联系人与单位树中联合过滤
};

const handleOpenContact = async (contact: ContactItem) => {
  try {
    if (contact.conversation_id) {
      await userWorldStore.setActiveConversation(contact.conversation_id);
    } else {
      await userWorldStore.openConversationByPeer(contact.user_id);
    }
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
};

const handleSend = async () => {
  if (!canSend.value) return;
  const message = draft.value.trim();
  if (!message) return;
  draft.value = '';
  try {
    await userWorldStore.sendToActiveConversation(message);
    await scrollToBottom();
  } catch (error) {
    draft.value = message;
    ElMessage.error(resolveErrorMessage(error));
  }
};

const resolveAvatarLabel = (name: string): string => {
  const value = String(name || '').trim();
  if (!value) return 'U';
  return value.slice(0, 1).toUpperCase();
};

const resolveUnread = (contact: ContactItem): number => {
  if (contact.conversation_id) {
    const value = userWorldStore.unreadByConversation[contact.conversation_id];
    if (Number.isFinite(value)) {
      return Number(value);
    }
  }
  const fallback = Number(contact.unread_count || 0);
  return Number.isFinite(fallback) ? fallback : 0;
};

const formatTime = (value: unknown): string => {
  const timestamp = Number(value || 0);
  if (!Number.isFinite(timestamp) || timestamp <= 0) {
    return '';
  }
  const date = new Date(timestamp * 1000);
  const now = new Date();
  const sameDay = now.toDateString() === date.toDateString();
  return sameDay
    ? date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    : date.toLocaleDateString();
};

const isMine = (message: MessageItem): boolean => {
  const currentUserId = String(authStore.user?.id || '').trim();
  return Boolean(currentUserId && message.sender_user_id === currentUserId);
};

const loadOrgUnits = async () => {
  try {
    const { data } = await fetchOrgUnits();
    const payload = asRecord(data?.data);
    const items = Array.isArray(payload.items) ? payload.items : [];
    const unitPathMap: Record<string, string> = {};
    items.forEach((item) => {
      const source = asRecord(item);
      const unitId = normalizeText(source.unit_id);
      if (!unitId) return;
      unitPathMap[unitId] = normalizeText(source.path_name) || normalizeText(source.name) || unitId;
    });
    const rawTree = Array.isArray(payload.tree) ? payload.tree : [];
    const tree = rawTree.map(normalizeUnitNode).filter((item): item is UnitNode => Boolean(item));
    orgUnitPathMap.value = unitPathMap;
    orgUnitTree.value = tree;
  } catch {
    orgUnitPathMap.value = {};
    orgUnitTree.value = [];
  }
};

onMounted(async () => {
  try {
    await Promise.all([loadOrgUnits(), userWorldStore.bootstrap()]);
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
});

onBeforeUnmount(() => {
  userWorldStore.stopAllWatchers();
});

watch(
  () => activeConversationId.value,
  async () => {
    await scrollToBottom();
  }
);

watch(
  () => activeMessages.value.length,
  async () => {
    await scrollToBottom();
  }
);
</script>
