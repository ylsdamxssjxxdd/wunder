<template>
  <div class="portal-shell user-world-shell">
    <UserTopbar :title="t('userWorld.title')" :subtitle="t('userWorld.subtitle')" :hide-chat="true" />
    <main class="user-world-main">
      <aside class="user-world-sidebar">
        <div class="user-world-tabbar" role="tablist" :aria-label="t('userWorld.tab.ariaLabel')">
          <button
            v-for="tab in sidebarTabs"
            :key="tab.value"
            class="user-world-tab-btn"
            :class="{ active: activeTab === tab.value }"
            type="button"
            role="tab"
            :title="tab.label"
            :aria-label="tab.label"
            :aria-selected="activeTab === tab.value"
            @click="activeTab = tab.value"
          >
            <i :class="tab.icon" aria-hidden="true"></i>
          </button>
        </div>

        <div v-if="activeTab === 'groups'" class="user-world-group-toolbar">
          <button class="user-world-group-create" type="button" @click="openCreateGroupDialog">
            <i class="fa-solid fa-user-group" aria-hidden="true"></i>
            <span>{{ t('userWorld.group.create') }}</span>
          </button>
        </div>

        <div class="user-world-contact-list">
          <template v-if="activeTab === 'chat'">
            <template v-if="chatRows.length">
              <div
                v-for="row in chatRows"
                :key="row.conversation_id"
                class="user-world-contact-entry"
              >
                <button
                  class="user-world-contact-item"
                  :class="{ active: activeConversationId === row.conversation_id }"
                  type="button"
                  @click="handleOpenConversation(row.conversation_id)"
                >
                  <div class="user-world-contact-avatar" :class="{ group: row.is_group }">
                    {{ resolveAvatarLabel(row.title) }}
                  </div>
                  <div class="user-world-contact-main">
                    <div class="user-world-contact-row">
                      <span class="user-world-contact-name" :title="row.title">{{ row.title }}</span>
                      <span class="user-world-contact-time">{{ formatTime(row.last_message_at) }}</span>
                    </div>
                    <div class="user-world-contact-row">
                      <span class="user-world-contact-preview">
                        {{ row.preview || t('userWorld.contact.emptyPreview') }}
                      </span>
                      <span v-if="row.unread > 0" class="user-world-contact-unread">{{ row.unread }}</span>
                    </div>
                  </div>
                </button>
                <button
                  class="user-world-contact-delete"
                  type="button"
                  :title="t('userWorld.chat.deleteConversation')"
                  :aria-label="t('userWorld.chat.deleteConversation')"
                  @click.stop="handleDeleteConversation(row.conversation_id)"
                >
                  <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
                </button>
              </div>
            </template>
            <div v-else class="user-world-empty">{{ t('userWorld.chat.emptyList') }}</div>
          </template>

          <template v-else-if="activeTab === 'users'">
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
            <div v-else class="user-world-empty">{{ t('userWorld.contact.empty') }}</div>
          </template>

          <template v-else-if="activeTab === 'container'">
            <div class="user-world-container-panel">
              <div class="user-world-workspace-shell chat-shell">
                <div class="glass-card info-panel">
                  <WorkspacePanel :title="t('userWorld.container.title')" :show-container-id="false" />
                </div>
              </div>
            </div>
          </template>

          <template v-else>
            <template v-if="groupRows.length">
              <div
                v-for="group in groupRows"
                :key="group.group_id"
                class="user-world-contact-entry"
              >
                <button
                  class="user-world-contact-item"
                  :class="{ active: activeConversationId === group.conversation_id }"
                  type="button"
                  @click="handleOpenConversation(group.conversation_id)"
                >
                  <div class="user-world-contact-avatar group">{{ resolveAvatarLabel(group.group_name) }}</div>
                  <div class="user-world-contact-main">
                    <div class="user-world-contact-row">
                      <span class="user-world-contact-name" :title="group.group_name">{{ group.group_name }}</span>
                      <span class="user-world-contact-time">{{ formatTime(group.last_message_at) }}</span>
                    </div>
                    <div class="user-world-contact-row">
                      <span class="user-world-contact-preview">
                        {{ group.last_message_preview || t('userWorld.group.emptyPreview') }}
                      </span>
                      <span v-if="group.unread_count_cache > 0" class="user-world-contact-unread">
                        {{ group.unread_count_cache }}
                      </span>
                    </div>
                  </div>
                </button>
                <button
                  class="user-world-contact-delete"
                  type="button"
                  :title="t('userWorld.chat.deleteConversation')"
                  :aria-label="t('userWorld.chat.deleteConversation')"
                  @click.stop="handleDeleteConversation(group.conversation_id)"
                >
                  <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
                </button>
              </div>
            </template>
            <div v-else class="user-world-empty">{{ t('userWorld.group.empty') }}</div>
          </template>
        </div>

        <div v-if="activeTab !== 'container'" class="user-world-search">
          <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
          <input v-model.trim="keyword" type="text" :placeholder="searchPlaceholder" />
        </div>
      </aside>

      <section class="user-world-chat">
        <header class="user-world-chat-header">
          <div class="user-world-chat-title">{{ activeConversationTitle || t('userWorld.chat.placeholderTitle') }}</div>
          <div class="user-world-chat-subtitle">{{ activeConversationSubtitle }}</div>
        </header>
        <div ref="messageContainerRef" class="user-world-message-list" @click="handleMessageClick">
          <div v-if="!activeConversationId" class="user-world-empty">
            {{ t('userWorld.chat.emptyConversation') }}
          </div>
          <template v-else>
            <div
              v-for="message in activeMessages"
              :key="`${message.message_id}`"
              class="user-world-message-item"
              :class="{ mine: isMine(message) }"
              :data-sender-id="message.sender_user_id"
            >
              <div class="user-world-message-bubble">
                <div class="user-world-message-content">
                  <div class="user-world-markdown chat-shell">
                    <div class="markdown-body" v-html="renderUserWorldMessage(message)"></div>
                  </div>
                </div>
                <div class="user-world-message-time">{{ formatTime(message.created_at) }}</div>
              </div>
            </div>
          </template>
        </div>
        <footer class="user-world-composer">
          <div class="user-world-input-box">
            <div v-if="mentionMenuVisible" class="user-world-mention-menu" role="listbox">
              <button
                v-for="(item, index) in mentionSuggestions"
                :key="item.fullPath"
                class="user-world-mention-item"
                :class="{ active: index === mentionMenuIndex }"
                type="button"
                role="option"
                :aria-selected="index === mentionMenuIndex"
                @mousedown.prevent="applyMentionSuggestion(index)"
                @mouseenter="setMentionMenuIndex(index)"
              >
                <i
                  :class="item.type === 'dir' ? 'fa-solid fa-folder user-world-mention-icon' : 'fa-solid fa-file user-world-mention-icon'"
                  aria-hidden="true"
                ></i>
                <span class="user-world-mention-name" :title="item.label">{{ item.label }}</span>
              </button>
            </div>
            <textarea
              ref="draftInputRef"
              v-model="draft"
              rows="1"
              :placeholder="t('userWorld.input.placeholder')"
              @input="handleDraftInput"
              @click="syncDraftCaret"
              @keyup="syncDraftCaret"
              @keydown="handleDraftKeydown"
              @keydown.enter.exact.prevent="handleSend"
            ></textarea>
            <button
              class="user-world-icon-btn upload-btn"
              type="button"
              :title="t('userWorld.attachments.upload')"
              :aria-label="t('userWorld.attachments.upload')"
              :disabled="uploading"
              @click="triggerUpload"
            >
              <i class="fa-solid fa-paperclip user-world-icon" aria-hidden="true"></i>
            </button>
            <button
              class="user-world-icon-btn send-btn"
              type="button"
              :disabled="!canSend"
              :title="t('userWorld.input.send')"
              :aria-label="t('userWorld.input.send')"
              @click="handleSend"
            >
              <i class="fa-solid fa-paper-plane user-world-icon user-world-icon-fill" aria-hidden="true"></i>
            </button>
          </div>
          <input
            ref="uploadInputRef"
            type="file"
            multiple
            style="display: none"
            @change="handleUploadInput"
          />
        </footer>
      </section>
    </main>

    <el-dialog
      v-model="createGroupDialogVisible"
      :title="t('userWorld.group.createTitle')"
      width="520px"
      :close-on-click-modal="false"
      destroy-on-close
    >
      <div class="user-world-group-dialog">
        <label class="user-world-group-label" for="uw-group-name">{{ t('userWorld.group.nameLabel') }}</label>
        <input
          id="uw-group-name"
          v-model.trim="groupName"
          class="user-world-group-input"
          type="text"
          :placeholder="t('userWorld.group.namePlaceholder')"
          maxlength="64"
        />

        <label class="user-world-group-label" for="uw-group-search">{{ t('userWorld.group.memberLabel') }}</label>
        <input
          id="uw-group-search"
          v-model.trim="groupMemberKeyword"
          class="user-world-group-input"
          type="text"
          :placeholder="t('userWorld.group.memberPlaceholder')"
        />

        <div class="user-world-group-members">
          <label
            v-for="contact in selectableGroupContacts"
            :key="contact.user_id"
            class="user-world-group-member-item"
          >
            <input
              v-model="selectedGroupMembers"
              type="checkbox"
              :value="contact.user_id"
              :disabled="isCurrentUser(contact.user_id)"
            />
            <span class="user-world-group-member-name" :title="contact.username || contact.user_id">
              {{ contact.username || contact.user_id }}
            </span>
            <span class="user-world-group-member-id">{{ contact.user_id }}</span>
          </label>
          <div v-if="!selectableGroupContacts.length" class="user-world-empty user-world-group-empty">
            {{ t('userWorld.group.memberEmpty') }}
          </div>
        </div>
      </div>
      <template #footer>
        <span class="dialog-footer">
          <button class="user-world-dialog-btn muted" type="button" @click="closeCreateGroupDialog">
            {{ t('common.cancel') }}
          </button>
          <button
            class="user-world-dialog-btn primary"
            type="button"
            :disabled="!canCreateGroup"
            @click="handleCreateGroup"
          >
            {{ creatingGroup ? t('common.loading') : t('userWorld.group.createSubmit') }}
          </button>
        </span>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchOrgUnits } from '@/api/auth';
import { downloadUserWorldFile } from '@/api/userWorld';
import { fetchWunderWorkspaceContent, searchWunderWorkspace, uploadWunderWorkspace } from '@/api/workspace';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useUserWorldStore } from '@/stores/userWorld';
import { renderMarkdown } from '@/utils/markdown';
import { isImagePath, parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
import { emitWorkspaceRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';

const UNIT_UNGROUPED_ID = '__ungrouped__';
const USER_WORLD_UPLOAD_BASE = 'user-world';

type SidebarTab = 'chat' | 'users' | 'groups' | 'container';

type ContactItem = {
  user_id: string;
  username: string;
  unit_id?: string | null;
  conversation_id?: string | null;
  last_message_preview?: string | null;
  last_message_at?: number | null;
  unread_count?: number;
};

type ConversationItem = {
  conversation_id: string;
  conversation_type: string;
  peer_user_id: string;
  group_id?: string | null;
  group_name?: string | null;
  member_count?: number | null;
  last_message_preview?: string | null;
  last_message_at?: number;
};

type GroupItem = {
  group_id: string;
  conversation_id: string;
  group_name: string;
  member_count: number;
  unread_count_cache: number;
  last_message_preview?: string | null;
  last_message_at?: number;
};

type MessageItem = {
  message_id: number;
  sender_user_id: string;
  content: string;
  created_at: number;
};

type MentionSuggestion = {
  label: string;
  path: string;
  fullPath: string;
  type: 'file' | 'dir';
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

const activeTab = ref<SidebarTab>('chat');
const keyword = ref('');
const draft = ref('');
const messageContainerRef = ref<HTMLElement | null>(null);
const draftInputRef = ref<HTMLTextAreaElement | null>(null);
const uploadInputRef = ref<HTMLInputElement | null>(null);
const uploading = ref(false);
const orgUnitTree = ref<UnitNode[]>([]);
const orgUnitPathMap = ref<Record<string, string>>({});
const collapsedUnitIds = ref<Set<string>>(new Set());
const createGroupDialogVisible = ref(false);
const creatingGroup = ref(false);
const groupName = ref('');
const groupMemberKeyword = ref('');
const selectedGroupMembers = ref<string[]>([]);
const mentionSuggestions = ref<MentionSuggestion[]>([]);
const mentionMenuIndex = ref(0);
const mentionMenuDismissed = ref(false);
const draftCaretPosition = ref(0);

const sidebarTabs = computed(() => [
  { value: 'chat' as SidebarTab, label: t('userWorld.tab.chat'), icon: 'fa-solid fa-comment-dots' },
  { value: 'users' as SidebarTab, label: t('userWorld.tab.users'), icon: 'fa-solid fa-users' },
  { value: 'groups' as SidebarTab, label: t('userWorld.tab.groups'), icon: 'fa-solid fa-comments' },
  { value: 'container' as SidebarTab, label: t('userWorld.tab.container'), icon: 'fa-solid fa-folder-open' }
]);

const DRAFT_INPUT_MAX_HEIGHT = 180;
const MENTION_DEBOUNCE_MS = 160;
const USER_WORLD_PREFIX = `${USER_WORLD_UPLOAD_BASE}/`;

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

const conversations = computed(() => userWorldStore.conversations as ConversationItem[]);
const groups = computed(() => userWorldStore.groups as GroupItem[]);
const activeConversationId = computed(() => userWorldStore.activeConversationId);
const activeConversation = computed(() => (userWorldStore.activeConversation || null) as ConversationItem | null);
const activeMessages = computed(() => userWorldStore.activeMessages as MessageItem[]);

const searchPlaceholder = computed(() => {
  if (activeTab.value === 'chat') return t('userWorld.search.chatPlaceholder');
  if (activeTab.value === 'groups') return t('userWorld.search.groupPlaceholder');
  return t('userWorld.search.placeholder');
});

const chatRows = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  const items = [...conversations.value]
    .sort((left, right) => Number(right.last_message_at || 0) - Number(left.last_message_at || 0))
    .map((conversation) => {
      const title = userWorldStore.resolveConversationTitle(conversation);
      const preview = String(conversation.last_message_preview || '').trim();
      const unread = userWorldStore.resolveConversationUnread(conversation.conversation_id);
      const lastMessageAt = Number(conversation.last_message_at || 0);
      return {
        conversation_id: conversation.conversation_id,
        title,
        preview,
        unread,
        last_message_at: lastMessageAt,
        is_group: conversation.conversation_type === 'group'
      };
    });
  if (!query) {
    return items;
  }
  return items.filter((item) => {
    const title = item.title.toLowerCase();
    const preview = item.preview.toLowerCase();
    return title.includes(query) || preview.includes(query);
  });
});

const groupRows = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  const items = [...groups.value].sort(
    (left, right) => Number(right.last_message_at || 0) - Number(left.last_message_at || 0)
  );
  if (!query) {
    return items;
  }
  return items.filter((item) => {
    const groupName = String(item.group_name || '').toLowerCase();
    const preview = String(item.last_message_preview || '').toLowerCase();
    return groupName.includes(query) || preview.includes(query);
  });
});

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

const activePeerUserId = computed(() => {
  const conversation = activeConversation.value;
  if (!conversation || conversation.conversation_type !== 'direct') {
    return '';
  }
  return String(conversation.peer_user_id || '').trim();
});

const activeConversationTitle = computed(() =>
  userWorldStore.resolveConversationTitle(activeConversation.value || undefined)
);

const activeConversationSubtitle = computed(() => {
  const conversation = activeConversation.value;
  if (!conversation) {
    return t('userWorld.chat.subtitle');
  }
  if (conversation.conversation_type === 'group') {
    const fallback = groups.value.find((item) => item.conversation_id === conversation.conversation_id);
    const count = Number(conversation.member_count || fallback?.member_count || 0);
    return t('userWorld.chat.groupSubtitle', { count: count > 0 ? count : '-' });
  }
  const peerUserId = normalizeText(conversation.peer_user_id);
  if (!peerUserId) {
    return t('userWorld.chat.subtitle');
  }
  const contact = (userWorldStore.contacts as ContactItem[]).find(
    (item) => normalizeText(item.user_id) === peerUserId
  );
  if (!contact) {
    return t('userWorld.chat.subtitle');
  }
  const unitLabel = resolveUnitLabel(normalizeText(contact.unit_id));
  return t('userWorld.chat.userSubtitle', { unit: unitLabel });
});

const selectableGroupContacts = computed(() => {
  const query = groupMemberKeyword.value.trim().toLowerCase();
  const contacts = userWorldStore.contacts as ContactItem[];
  const currentUserId = String(authStore.user?.id || '').trim();
  return contacts
    .filter((item) => String(item.user_id || '').trim() !== currentUserId)
    .filter((item) => {
      if (!query) return true;
      const username = String(item.username || '').toLowerCase();
      const userId = String(item.user_id || '').toLowerCase();
      return username.includes(query) || userId.includes(query);
    })
    .sort((left, right) => String(left.username || '').localeCompare(String(right.username || ''), 'zh-CN'));
});

const canSend = computed(
  () =>
    Boolean(activeConversationId.value) &&
    Boolean(draft.value.trim()) &&
    !userWorldStore.sending &&
    !uploading.value
);

const canCreateGroup = computed(
  () => Boolean(groupName.value.trim()) && selectedGroupMembers.value.length > 0 && !creatingGroup.value
);
const mentionMenuVisible = computed(
  () => !mentionMenuDismissed.value && mentionSuggestions.value.length > 0
);

const resolveErrorMessage = (error: unknown): string => {
  const message = String((error as { message?: unknown })?.message || '').trim();
  return message || userWorldStore.error || t('common.requestFailed');
};

const resizeDraftInput = () => {
  const el = draftInputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  const nextHeight = Math.min(el.scrollHeight, DRAFT_INPUT_MAX_HEIGHT);
  el.style.height = `${nextHeight}px`;
  el.style.overflowY = el.scrollHeight > DRAFT_INPUT_MAX_HEIGHT ? 'auto' : 'hidden';
};

const resetDraftInputHeight = () => {
  const el = draftInputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  el.style.overflowY = 'hidden';
};

const handleDraftInput = () => {
  resizeDraftInput();
  syncDraftCaret();
  scheduleMentionSearch();
};

const syncDraftCaret = () => {
  const el = draftInputRef.value;
  const fallback = String(draft.value || '').length;
  const selectionStart = Number(el?.selectionStart);
  draftCaretPosition.value = Number.isFinite(selectionStart) ? selectionStart : fallback;
  scheduleMentionSearch();
};

const setMentionMenuIndex = (index: number) => {
  const total = mentionSuggestions.value.length;
  if (total <= 0) {
    mentionMenuIndex.value = 0;
    return;
  }
  mentionMenuIndex.value = Math.max(0, Math.min(index, total - 1));
};

const moveMentionMenuIndex = (delta: number) => {
  const total = mentionSuggestions.value.length;
  if (total <= 0) {
    mentionMenuIndex.value = 0;
    return;
  }
  const next = (mentionMenuIndex.value + delta + total) % total;
  mentionMenuIndex.value = next;
};

const parseMentionContext = () => {
  const text = String(draft.value || '');
  const caret = Math.max(0, Math.min(draftCaretPosition.value, text.length));
  const before = text.slice(0, caret);
  let atIndex = -1;
  for (let i = before.length - 1; i >= 0; i -= 1) {
    const ch = before[i];
    if (ch === '@') {
      const prev = i > 0 ? before[i - 1] : '';
      if (i === 0 || /\s/.test(prev)) {
        atIndex = i;
      }
      break;
    }
    if (/\s/.test(ch)) {
      break;
    }
  }
  if (atIndex < 0) return null;
  const query = before.slice(atIndex + 1);
  return {
    query,
    start: atIndex,
    end: caret
  };
};

const normalizeUserWorldSuggestionPath = (value: string) => {
  const normalized = normalizeWorkspacePath(value);
  if (!normalized) return '';
  if (normalized === USER_WORLD_UPLOAD_BASE) return '';
  if (normalized.startsWith(USER_WORLD_PREFIX)) {
    return normalized.slice(USER_WORLD_PREFIX.length);
  }
  return '';
};

const buildMentionSuggestion = (entry: { path?: string; type?: string; name?: string }) => {
  const normalizedPath = normalizeWorkspacePath(entry?.path || entry?.name || '');
  const relative = normalizeUserWorldSuggestionPath(normalizedPath);
  if (!relative) return null;
  return {
    label: relative,
    path: relative,
    fullPath: `${USER_WORLD_UPLOAD_BASE}/${relative}`,
    type: entry?.type === 'dir' ? 'dir' : 'file'
  } as MentionSuggestion;
};

let mentionSearchTimer: number | null = null;
let mentionSearchToken = 0;

const clearMentionSuggestions = () => {
  mentionSuggestions.value = [];
  mentionMenuIndex.value = 0;
};

const loadMentionRoot = async (token: number) => {
  const { data } = await fetchWunderWorkspaceContent({
    path: USER_WORLD_UPLOAD_BASE,
    include_content: true,
    depth: 1,
    sort_by: 'name',
    order: 'asc'
  });
  if (token !== mentionSearchToken) return;
  const entries = Array.isArray(data?.entries) ? data.entries : [];
  const next = entries
    .map((entry) => buildMentionSuggestion(entry))
    .filter((item): item is MentionSuggestion => Boolean(item));
  mentionSuggestions.value = next.slice(0, 24);
  mentionMenuIndex.value = 0;
};

const loadMentionSearch = async (query: string, token: number) => {
  const { data } = await searchWunderWorkspace({
    keyword: query,
    offset: 0,
    limit: 50
  });
  if (token !== mentionSearchToken) return;
  const entries = Array.isArray(data?.entries) ? data.entries : [];
  const next = entries
    .map((entry) => buildMentionSuggestion(entry))
    .filter((item): item is MentionSuggestion => Boolean(item));
  mentionSuggestions.value = next.slice(0, 24);
  mentionMenuIndex.value = 0;
};

const scheduleMentionSearch = () => {
  if (mentionSearchTimer) {
    window.clearTimeout(mentionSearchTimer);
  }
  mentionSearchTimer = window.setTimeout(async () => {
    mentionSearchTimer = null;
    const context = parseMentionContext();
    if (!context) {
      clearMentionSuggestions();
      return;
    }
    mentionMenuDismissed.value = false;
    const token = ++mentionSearchToken;
    const query = context.query.trim();
    try {
      if (!query) {
        await loadMentionRoot(token);
      } else {
        await loadMentionSearch(query, token);
      }
    } catch {
      if (token === mentionSearchToken) {
        clearMentionSuggestions();
      }
    }
  }, MENTION_DEBOUNCE_MS);
};

const applyMentionSuggestion = (index = mentionMenuIndex.value) => {
  const item = mentionSuggestions.value[index];
  if (!item) return false;
  const context = parseMentionContext();
  if (!context) return false;
  const token = buildAttachmentToken(item.fullPath);
  if (!token) return false;
  const before = draft.value.slice(0, context.start);
  const after = draft.value.slice(context.end);
  draft.value = `${before}${token}${after}`;
  mentionMenuDismissed.value = true;
  nextTick(() => {
    const el = draftInputRef.value;
    if (!el) return;
    const cursor = before.length + token.length;
    if (typeof el.focus === 'function') {
      el.focus();
    }
    if (typeof el.setSelectionRange === 'function') {
      el.setSelectionRange(cursor, cursor);
    }
    draftCaretPosition.value = cursor;
    resizeDraftInput();
  });
  return true;
};

const handleDraftKeydown = (event: KeyboardEvent) => {
  if (!mentionMenuVisible.value) return;
  if (event.key === 'ArrowDown') {
    event.preventDefault();
    moveMentionMenuIndex(1);
    return;
  }
  if (event.key === 'ArrowUp') {
    event.preventDefault();
    moveMentionMenuIndex(-1);
    return;
  }
  if (event.key === 'Tab' || event.key === 'Enter') {
    event.preventDefault();
    applyMentionSuggestion();
    return;
  }
  if (event.key === 'Escape') {
    event.preventDefault();
    mentionMenuDismissed.value = true;
  }
};

const scrollToBottom = async () => {
  await nextTick();
  const el = messageContainerRef.value;
  if (!el) return;
  el.scrollTop = el.scrollHeight;
};

const handleOpenConversation = async (conversationId: string) => {
  try {
    await userWorldStore.setActiveConversation(conversationId);
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
};

const handleDeleteConversation = async (conversationId: string) => {
  const cleaned = normalizeText(conversationId);
  if (!cleaned) return;
  try {
    await userWorldStore.dismissConversation(cleaned);
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
};

const handleOpenContact = async (contact: ContactItem) => {
  try {
    if (contact.conversation_id) {
      await userWorldStore.setActiveConversation(contact.conversation_id);
    } else {
      await userWorldStore.openConversationByPeer(contact.user_id);
    }
    activeTab.value = 'chat';
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
};

const buildAttachmentToken = (rawPath: string): string => {
  const normalized = normalizeWorkspacePath(rawPath);
  if (!normalized) return '';
  if (/\s/.test(normalized)) {
    if (!normalized.includes('"')) {
      return `@"${normalized}"`;
    }
    if (!normalized.includes("'")) {
      return `@'${normalized}'`;
    }
    return `@${encodeURIComponent(normalized)}`;
  }
  return `@${normalized}`;
};

const appendAttachmentTokens = (paths: string[]) => {
  const tokens = paths.map(buildAttachmentToken).filter(Boolean);
  if (!tokens.length) return;
  const prefix = draft.value.trim() ? '\n' : '';
  draft.value = `${draft.value}${prefix}${tokens.join(' ')}`;
  nextTick(() => {
    resizeDraftInput();
  });
};

const triggerUpload = () => {
  if (!uploadInputRef.value || uploading.value) return;
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const handleUploadInput = async (event: Event) => {
  const target = event.target as HTMLInputElement | null;
  const files = target?.files ? Array.from(target.files) : [];
  if (!files.length) return;
  uploading.value = true;
  try {
    const formData = new FormData();
    formData.append('path', USER_WORLD_UPLOAD_BASE);
    files.forEach((file) => {
      formData.append('files', file as Blob);
    });
    const { data } = await uploadWunderWorkspace(formData);
    const uploaded = Array.isArray(data?.files) ? data.files : [];
    if (uploaded.length) {
      appendAttachmentTokens(uploaded);
      emitWorkspaceRefresh({ reason: 'user-world-upload' });
    }
    ElMessage.success(
      t('userWorld.attachments.uploadSuccess', { count: uploaded.length || files.length })
    );
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  } finally {
    uploading.value = false;
    if (target) {
      target.value = '';
    }
  }
};

const handleSend = async () => {
  if (!canSend.value) return;
  if (mentionMenuVisible.value && applyMentionSuggestion()) {
    return;
  }
  const message = draft.value.trim();
  if (!message) return;
  draft.value = '';
  mentionMenuDismissed.value = true;
  nextTick(() => {
    resetDraftInputHeight();
  });
  try {
    await userWorldStore.sendToActiveConversation(message);
    await scrollToBottom();
  } catch (error) {
    draft.value = message;
    nextTick(() => {
      resizeDraftInput();
    });
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

const markdownCache = new WeakMap();

const normalizeWorkspaceOwnerId = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/[^a-zA-Z0-9_-]/g, '_');

const encodeWorkspacePath = (value: string): string =>
  String(value || '')
    .split('/')
    .map((part) => encodeURIComponent(part))
    .join('/');

const buildWorkspacePublicPath = (ownerId: string, relativePath: string): string => {
  const safeOwner = normalizeWorkspaceOwnerId(ownerId);
  const normalized = normalizeWorkspacePath(relativePath);
  if (!safeOwner || !normalized) return '';
  return `/workspaces/${safeOwner}/${encodeWorkspacePath(normalized)}`;
};

const AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;
const AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?，。；：！？》】]+)?$/;

const decodeAtPathToken = (value: string): string => {
  if (!/%[0-9a-fA-F]{2}/.test(value)) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const replaceAtPathTokens = (content: string, senderUserId: string): string => {
  if (!content) return '';
  const ownerId = normalizeWorkspaceOwnerId(senderUserId);
  if (!ownerId) return content;
  return content.replace(AT_PATH_RE, (match, prefix, token, doubleQuoted, singleQuoted) => {
    const raw = doubleQuoted ?? singleQuoted ?? token ?? '';
    if (!raw) return match;
    let value = raw;
    let suffix = '';
    if (!doubleQuoted && !singleQuoted) {
      const split = AT_PATH_SUFFIX_RE.exec(value);
      if (split) {
        value = split[1] ?? value;
        suffix = split[2] ?? '';
      }
    }
    const decoded = decodeAtPathToken(String(value || '').trim());
    const normalized = normalizeWorkspacePath(decoded);
    if (!normalized) return match;
    const pathLike =
      decoded.startsWith('/') ||
      decoded.startsWith('./') ||
      decoded.startsWith('../') ||
      normalized.includes('/') ||
      normalized.includes('.');
    if (!pathLike) return match;
    const publicPath = buildWorkspacePublicPath(ownerId, normalized);
    if (!publicPath) return match;
    const label = decoded;
    const replacement = isImagePath(normalized)
      ? `![${label}](${publicPath})`
      : `[${label}](${publicPath})`;
    return `${prefix}${replacement}${suffix}`;
  });
};

const renderUserWorldMessage = (message: MessageItem): string => {
  const content = String(message?.content || '');
  if (!content) return '';
  const patched = replaceAtPathTokens(content, message.sender_user_id);
  const cached = markdownCache.get(message);
  if (cached && cached.source === patched) {
    return cached.html;
  }
  const html = renderMarkdown(patched);
  markdownCache.set(message, { source: patched, html });
  return html;
};

type UserWorldResourceCacheEntry = {
  objectUrl?: string;
  filename?: string;
  exists?: boolean;
  promise?: Promise<UserWorldResourceCacheEntry>;
};

const userWorldResourceCache = new Map<string, UserWorldResourceCacheEntry>();
let userWorldResourceHydrationFrame: number | null = null;
let stopUserWorldWorkspaceRefresh: (() => void) | null = null;

const getFilenameFromHeaders = (headers: Record<string, string>, fallback: string) => {
  const disposition = headers?.['content-disposition'] || headers?.['Content-Disposition'];
  if (!disposition) return fallback;
  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match) {
    return decodeURIComponent(utf8Match[1]);
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match ? match[1] : fallback;
};

const getFileExtension = (filename: string) => {
  const base = String(filename || '').split('?')[0].split('#')[0];
  const parts = base.split('.');
  if (parts.length < 2) return '';
  return parts.pop()?.toLowerCase() || '';
};

const normalizeWorkspaceImageBlob = (blob: Blob, filename: string, contentType: string) => {
  if (!(blob instanceof Blob)) return blob;
  const extension = getFileExtension(filename);
  if (extension !== 'svg') return blob;
  const expectedType = 'image/svg+xml';
  if (blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.includes('image/svg')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob.slice(0, blob.size, expectedType);
};

const saveBlobUrl = (url: string, filename: string) => {
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

const ensureFileCardStatus = (card: HTMLElement | null) => {
  if (!card) return null;
  const body = card.querySelector('.ai-resource-body');
  if (!body) return null;
  let status = body.querySelector('.ai-resource-status') as HTMLElement | null;
  if (!status) {
    status = document.createElement('div');
    status.className = 'ai-resource-status';
    body.appendChild(status);
  }
  return status;
};

const setFileCardStatus = (card: HTMLElement | null, message: string) => {
  const status = ensureFileCardStatus(card);
  if (status) {
    status.textContent = message;
  }
  card?.classList.add('is-error');
};

const clearFileCardStatus = (card: HTMLElement | null) => {
  if (!card) return;
  const body = card.querySelector('.ai-resource-body');
  const status = body?.querySelector('.ai-resource-status') as HTMLElement | null;
  if (status) {
    status.remove();
  }
  card.classList.remove('is-error');
};

const resolveCardOwnerId = (card: Element | null): string => {
  const wrapper = card?.closest('.user-world-message-item') as HTMLElement | null;
  return String(wrapper?.dataset?.senderId || '').trim();
};

const resolveUserWorldResource = (publicPath: string, ownerId: string) => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const relativePath = String(parsed.relativePath || '').trim();
  const resolvedOwner = String(ownerId || parsed.ownerId || parsed.workspaceId || parsed.userId || '').trim();
  if (!relativePath || !resolvedOwner) return null;
  return { ...parsed, ownerId: resolvedOwner, relativePath };
};

const buildUserWorldCacheKey = (publicPath: string, suffix = '') => {
  const conversationId = String(activeConversationId.value || '').trim();
  if (!conversationId) return '';
  return `${conversationId}:${publicPath}${suffix}`;
};

const fetchUserWorldResource = async (resource: {
  publicPath: string;
  ownerId: string;
  relativePath: string;
  filename?: string;
}) => {
  const conversationId = String(activeConversationId.value || '').trim();
  if (!conversationId) {
    throw new Error('conversation_id is missing');
  }
  const cacheKey = buildUserWorldCacheKey(resource.publicPath);
  if (!cacheKey) {
    throw new Error('conversation_id is missing');
  }
  const cached = userWorldResourceCache.get(cacheKey);
  if (cached?.objectUrl) return cached;
  if (cached?.promise) return cached.promise;
  const promise = downloadUserWorldFile({
    conversation_id: conversationId,
    owner_user_id: resource.ownerId,
    path: String(resource.relativePath || '')
  })
    .then((response) => {
      const filename = getFilenameFromHeaders(response.headers, resource.filename || 'download');
      const contentType = response.headers?.['content-type'] || response.headers?.['Content-Type'];
      const blob = normalizeWorkspaceImageBlob(response.data, filename, contentType);
      const objectUrl = URL.createObjectURL(blob);
      const entry: UserWorldResourceCacheEntry = { objectUrl, filename };
      userWorldResourceCache.set(cacheKey, entry);
      return entry;
    })
    .catch((error) => {
      userWorldResourceCache.delete(cacheKey);
      throw error;
    });
  userWorldResourceCache.set(cacheKey, { promise });
  return promise;
};

const checkUserWorldResource = async (resource: {
  publicPath: string;
  ownerId: string;
  relativePath: string;
}) => {
  const conversationId = String(activeConversationId.value || '').trim();
  if (!conversationId) {
    throw new Error('conversation_id is missing');
  }
  const cacheKey = buildUserWorldCacheKey(resource.publicPath, ':check');
  if (!cacheKey) {
    throw new Error('conversation_id is missing');
  }
  const cached = userWorldResourceCache.get(cacheKey);
  if (cached?.exists) return cached;
  if (cached?.promise) return cached.promise;
  const promise = downloadUserWorldFile({
    conversation_id: conversationId,
    owner_user_id: resource.ownerId,
    path: String(resource.relativePath || ''),
    check: true
  })
    .then(() => {
      const entry: UserWorldResourceCacheEntry = { exists: true };
      userWorldResourceCache.set(cacheKey, entry);
      return entry;
    })
    .catch((error) => {
      userWorldResourceCache.delete(cacheKey);
      throw error;
    });
  userWorldResourceCache.set(cacheKey, { promise });
  return promise;
};

const isUserWorldResourceMissing = (error: unknown) => {
  const status = (error as { response?: { status?: number } })?.response?.status;
  if (status === 404 || status === 410) return true;
  const raw =
    (error as { response?: { data?: { detail?: string; message?: string } } })?.response?.data?.detail ??
    (error as { response?: { data?: { message?: string } } })?.response?.data?.message ??
    (error as { message?: string })?.message ??
    '';
  const message = typeof raw === 'string' ? raw : String(raw || '');
  return /not found|no such|不存在|找不到|已删除|已移除|removed/i.test(message);
};

const hydrateUserWorldResourceCard = async (card: HTMLElement) => {
  if (!card || card.dataset.workspaceState) return;
  const kind = card.dataset.workspaceKind || 'image';
  const publicPath = card.dataset.workspacePath || '';
  if (!publicPath) return;
  if (kind !== 'image') {
    const ownerId = resolveCardOwnerId(card);
    const resource = resolveUserWorldResource(publicPath, ownerId);
    if (!resource) {
      setFileCardStatus(card, t('chat.resourceUnavailable'));
      card.dataset.workspaceState = 'error';
      return;
    }
    card.dataset.workspaceState = 'loading';
    try {
      await checkUserWorldResource(resource);
      card.dataset.workspaceState = 'ready';
      clearFileCardStatus(card);
    } catch (error) {
      setFileCardStatus(
        card,
        isUserWorldResourceMissing(error) ? t('chat.resourceMissing') : t('chat.resourceDownloadFailed')
      );
      card.dataset.workspaceState = 'error';
    }
    return;
  }
  const status = card.querySelector('.ai-resource-status');
  const preview = card.querySelector('.ai-resource-preview') as HTMLImageElement | null;
  if (!preview) return;
  const ownerId = resolveCardOwnerId(card);
  const resource = resolveUserWorldResource(publicPath, ownerId);
  if (!resource) {
    if (status) status.textContent = t('chat.resourceUnavailable');
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
    return;
  }
  card.dataset.workspaceState = 'loading';
  try {
    const entry = await fetchUserWorldResource(resource);
    preview.src = entry.objectUrl;
    card.dataset.workspaceState = 'ready';
    card.classList.add('is-ready');
    if (status) status.textContent = '';
  } catch (error) {
    if (status) {
      status.textContent = isUserWorldResourceMissing(error)
        ? t('chat.resourceMissing')
        : t('chat.resourceImageFailed');
    }
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
  }
};

const hydrateUserWorldResources = () => {
  const container = messageContainerRef.value;
  if (!container) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    hydrateUserWorldResourceCard(card as HTMLElement);
  });
};

const resetUserWorldResourceCards = () => {
  const container = messageContainerRef.value;
  if (!container) return;
  const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
  cards.forEach((card) => {
    const kind = card.getAttribute('data-workspace-kind') || 'image';
    card.setAttribute('data-workspace-state', '');
    card.classList.remove('is-error');
    card.classList.remove('is-ready');
    if (kind === 'image') {
      const status = card.querySelector('.ai-resource-status');
      if (status) {
        status.textContent = t('chat.resourceImageLoading');
      }
    } else {
      clearFileCardStatus(card as HTMLElement);
    }
  });
};

const handleWorkspaceRefresh = () => {
  clearUserWorldResourceCache();
  resetUserWorldResourceCards();
  scheduleUserWorldResourceHydration();
};

const scheduleUserWorldResourceHydration = () => {
  if (userWorldResourceHydrationFrame) return;
  userWorldResourceHydrationFrame = requestAnimationFrame(() => {
    userWorldResourceHydrationFrame = null;
    hydrateUserWorldResources();
  });
};

const clearUserWorldResourceCache = () => {
  if (userWorldResourceHydrationFrame) {
    cancelAnimationFrame(userWorldResourceHydrationFrame);
    userWorldResourceHydrationFrame = null;
  }
  userWorldResourceCache.forEach((entry) => {
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
  });
  userWorldResourceCache.clear();
};

const downloadUserWorldResource = async (
  publicPath: string,
  ownerId: string,
  card?: HTMLElement | null
) => {
  const resource = resolveUserWorldResource(publicPath, ownerId);
  if (!resource) return;
  try {
    const entry = await fetchUserWorldResource(resource);
    saveBlobUrl(entry.objectUrl, entry.filename || resource.filename || 'download');
  } catch (error) {
    const missing = isUserWorldResourceMissing(error);
    if (card && (card.dataset?.workspaceKind || 'file') !== 'image') {
      setFileCardStatus(card, missing ? t('chat.resourceMissing') : t('chat.resourceDownloadFailed'));
    }
    ElMessage.error(
      missing ? t('chat.resourceMissing') : t('chat.resourceDownloadFailed')
    );
  }
};

const handleMessageClick = (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  const resourceButton = target.closest('[data-workspace-action]') as HTMLElement | null;
  if (resourceButton) {
    const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
    const publicPath = container?.dataset?.workspacePath || '';
    if (!publicPath) return;
    const ownerId = resolveCardOwnerId(container);
    downloadUserWorldResource(publicPath, ownerId, container);
    return;
  }
  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
  if (resourceLink) {
    const publicPath = resourceLink.dataset?.workspacePath || '';
    if (!publicPath) return;
    const ownerId = resolveCardOwnerId(resourceLink);
    downloadUserWorldResource(publicPath, ownerId, resourceLink.closest('[data-workspace-path]') as HTMLElement | null);
  }
};

const isMine = (message: MessageItem): boolean => {
  const currentUserId = String(authStore.user?.id || '').trim();
  return Boolean(currentUserId && message.sender_user_id === currentUserId);
};

const isCurrentUser = (userId: string): boolean => {
  const currentUserId = String(authStore.user?.id || '').trim();
  return Boolean(currentUserId && currentUserId === String(userId || '').trim());
};

const resetCreateGroupDialog = () => {
  groupName.value = '';
  groupMemberKeyword.value = '';
  selectedGroupMembers.value = [];
  creatingGroup.value = false;
};

const openCreateGroupDialog = () => {
  resetCreateGroupDialog();
  createGroupDialogVisible.value = true;
};

const closeCreateGroupDialog = () => {
  createGroupDialogVisible.value = false;
};

const handleCreateGroup = async () => {
  if (!canCreateGroup.value) return;
  creatingGroup.value = true;
  try {
    const conversation = await userWorldStore.createGroupConversation(
      groupName.value,
      selectedGroupMembers.value
    );
    if (!conversation?.conversation_id) {
      throw new Error(t('userWorld.group.createFailed'));
    }
    createGroupDialogVisible.value = false;
    activeTab.value = 'chat';
    ElMessage.success(t('userWorld.group.createSuccess'));
    await scrollToBottom();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  } finally {
    creatingGroup.value = false;
  }
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
    scheduleUserWorldResourceHydration();
    stopUserWorldWorkspaceRefresh = onWorkspaceRefresh(handleWorkspaceRefresh);
    nextTick(() => {
      resizeDraftInput();
    });
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  }
});

onBeforeUnmount(() => {
  userWorldStore.stopAllWatchers();
  clearUserWorldResourceCache();
  if (stopUserWorldWorkspaceRefresh) {
    stopUserWorldWorkspaceRefresh();
    stopUserWorldWorkspaceRefresh = null;
  }
  if (mentionSearchTimer) {
    window.clearTimeout(mentionSearchTimer);
    mentionSearchTimer = null;
  }
});

watch(
  () => activeConversationId.value,
  async () => {
    clearUserWorldResourceCache();
    resetUserWorldResourceCards();
    await scrollToBottom();
    scheduleUserWorldResourceHydration();
  }
);

watch(
  () => activeMessages.value.length,
  async () => {
    await scrollToBottom();
    scheduleUserWorldResourceHydration();
  }
);

watch(
  () => activeMessages.value[activeMessages.value.length - 1]?.content,
  () => {
    scheduleUserWorldResourceHydration();
  }
);

watch(
  () => draft.value,
  () => {
    nextTick(() => {
      resizeDraftInput();
    });
  }
);

watch(
  () => parseMentionContext()?.query ?? '',
  () => {
    mentionMenuDismissed.value = false;
    mentionMenuIndex.value = 0;
  }
);

watch(
  () => createGroupDialogVisible.value,
  (visible) => {
    if (!visible) {
      resetCreateGroupDialog();
    }
  }
);
</script>
