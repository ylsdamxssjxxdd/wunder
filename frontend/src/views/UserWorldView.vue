<template>
  <div class="portal-shell user-world-shell">
    <UserTopbar :title="t('userWorld.title')" :subtitle="t('userWorld.subtitle')" />
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
          <button
            v-for="contact in filteredContacts"
            :key="contact.user_id"
            class="user-world-contact-item"
            :class="{ active: activePeerUserId === contact.user_id }"
            type="button"
            @click="handleOpenContact(contact)"
          >
            <div class="user-world-contact-avatar">{{ resolveAvatarLabel(contact.username) }}</div>
            <div class="user-world-contact-main">
              <div class="user-world-contact-row">
                <span class="user-world-contact-name">{{ contact.username || contact.user_id }}</span>
                <span class="user-world-contact-time">{{ formatTime(contact.last_message_at) }}</span>
              </div>
              <div class="user-world-contact-row">
                <span class="user-world-contact-preview">
                  {{ contact.last_message_preview || t('userWorld.contact.emptyPreview') }}
                </span>
                <span v-if="resolveUnread(contact) > 0" class="user-world-contact-unread">
                  {{ resolveUnread(contact) }}
                </span>
              </div>
            </div>
          </button>
          <div v-if="!filteredContacts.length" class="user-world-empty">
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

import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useUserWorldStore } from '@/stores/userWorld';

type ContactItem = {
  user_id: string;
  username: string;
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

const { t } = useI18n();
const authStore = useAuthStore();
const userWorldStore = useUserWorldStore();

const keyword = ref('');
const draft = ref('');
const messageContainerRef = ref<HTMLElement | null>(null);

const filteredContacts = computed(() => {
  const query = keyword.value.trim().toLowerCase();
  if (!query) return userWorldStore.contacts;
  return userWorldStore.contacts.filter((item) => {
    const username = String(item.username || '').toLowerCase();
    const userId = String(item.user_id || '').toLowerCase();
    return username.includes(query) || userId.includes(query);
  });
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
  // 搜索由本地过滤处理，保留入口便于后续接后台检索
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
  try {
    await userWorldStore.sendToActiveConversation(message);
    draft.value = '';
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

onMounted(async () => {
  try {
    await userWorldStore.bootstrap();
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
