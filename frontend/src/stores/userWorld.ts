import { defineStore } from 'pinia';

import {
  createUserWorldGroup,
  createOrGetUserWorldConversation,
  listUserWorldContacts,
  listUserWorldConversations,
  listUserWorldGroups,
  listUserWorldMessages,
  markUserWorldRead,
  openUserWorldSocket,
  sendUserWorldMessage,
  streamUserWorldEvents
} from '@/api/userWorld';
import { useAuthStore } from '@/stores/auth';
import { consumeSseStream } from '@/utils/sse';
import { createWsMultiplexer } from '@/utils/ws';

type UserWorldMessage = {
  message_id: number;
  conversation_id: string;
  sender_user_id: string;
  content: string;
  content_type?: string;
  client_msg_id?: string | null;
  created_at: number;
};

type UserWorldConversation = {
  conversation_id: string;
  conversation_type: string;
  peer_user_id: string;
  group_id?: string | null;
  group_name?: string | null;
  member_count?: number | null;
  last_read_message_id?: number | null;
  unread_count_cache: number;
  pinned?: boolean;
  muted?: boolean;
  updated_at?: number;
  last_message_at?: number;
  last_message_id?: number | null;
  last_message_preview?: string | null;
};

type UserWorldGroup = {
  group_id: string;
  conversation_id: string;
  group_name: string;
  owner_user_id: string;
  member_count: number;
  unread_count_cache: number;
  updated_at: number;
  last_message_at: number;
  last_message_id?: number | null;
  last_message_preview?: string | null;
};

type UserWorldContact = {
  user_id: string;
  username: string;
  status?: string;
  unit_id?: string | null;
  conversation_id?: string | null;
  last_message_preview?: string | null;
  last_message_at?: number | null;
  unread_count?: number;
};

type WsError = Error & {
  phase?: string;
};

type OpenConversationByPeerOptions = {
  waitForLoad?: boolean;
  activate?: boolean;
};

const DEFAULT_TRANSPORT: 'ws' | 'sse' = 'ws';
const WATCH_RETRY_DELAY_MS = 1000;
const DISMISSED_STORAGE_PREFIX = 'user_world_dismissed_conversations';

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const parseEventPayload = (dataText: string): Record<string, unknown> => {
  try {
    return asRecord(JSON.parse(dataText));
  } catch {
    return {};
  }
};

const toNumber = (value: unknown): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
};

const resolveHttpStatus = (error: unknown): number => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

const isAuthDeniedError = (error: unknown): boolean => {
  const status = resolveHttpStatus(error);
  return status === 401 || status === 403;
};

const isTimeoutError = (value: unknown): boolean => {
  const code = String((value as { code?: unknown })?.code || '').trim().toUpperCase();
  return code === 'ECONNABORTED';
};

const createTimeoutSignal = (timeoutMs: number): { signal: AbortSignal; cleanup: () => void } => {
  const controller = new AbortController();
  const timer = window.setTimeout(() => {
    controller.abort();
  }, Math.max(1000, timeoutMs));
  return {
    signal: controller.signal,
    cleanup: () => window.clearTimeout(timer)
  };
};

const nowId = (): string => `msg_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;

const resolveDismissedStorageKey = (userId: unknown): string => {
  const normalized = String(userId || '').trim() || 'anonymous';
  return `${DISMISSED_STORAGE_PREFIX}:${normalized}`;
};

const normalizeConversationId = (value: unknown): string => String(value || '').trim();

const parseDismissedConversationIds = (raw: unknown): string[] => {
  if (typeof raw !== 'string' || !raw.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const result = new Set<string>();
    parsed.forEach((item) => {
      const cleaned = normalizeConversationId(item);
      if (cleaned) result.add(cleaned);
    });
    return Array.from(result);
  } catch {
    return [];
  }
};

const wsClient = createWsMultiplexer(() => openUserWorldSocket({ allowQueryToken: true }), {
  idleTimeoutMs: 30000,
  connectTimeoutMs: 10000
});

let wsUnavailableUntil = 0;
let wsRequestSeq = 0;

const watchRuntime = new Map<
  string,
  {
    controller: AbortController;
    requestId: string;
    transport: 'ws' | 'sse';
  }
>();

const buildRequestId = () => {
  wsRequestSeq = (wsRequestSeq + 1) % 1_000_000;
  return `uw_req_${Date.now().toString(36)}_${wsRequestSeq}`;
};

const markWsUnavailable = (ttlMs = 60000) => {
  wsUnavailableUntil = Date.now() + Math.max(5000, ttlMs);
};

const resolveTransport = (): 'ws' | 'sse' => {
  if (typeof WebSocket === 'undefined') {
    return 'sse';
  }
  if (wsUnavailableUntil && Date.now() < wsUnavailableUntil) {
    return 'sse';
  }
  const stored = localStorage.getItem('user_world_stream_transport');
  if (stored === 'ws') {
    return stored;
  }
  return DEFAULT_TRANSPORT;
};

export const useUserWorldStore = defineStore('user-world', {
  state: () => ({
    contacts: [] as UserWorldContact[],
    groups: [] as UserWorldGroup[],
    conversations: [] as UserWorldConversation[],
    activeConversationId: '' as string,
    messagesByConversation: {} as Record<string, UserWorldMessage[]>,
    unreadByConversation: {} as Record<string, number>,
    loading: false,
    sending: false,
    initialized: false,
    permissionDenied: false,
    permissionDeniedKey: '',
    error: '' as string,
    streamTransport: DEFAULT_TRANSPORT as 'ws' | 'sse',
    dismissedConversationIds: [] as string[],
    dismissedStorageKey: '' as string
  }),
  getters: {
    activeConversation(state): UserWorldConversation | null {
      return (
        state.conversations.find((item) => item.conversation_id === state.activeConversationId) || null
      );
    },
    activeMessages(state): UserWorldMessage[] {
      return state.messagesByConversation[state.activeConversationId] || [];
    }
  },
  actions: {
    ensureDismissedConversationState(force = false) {
      const authStore = useAuthStore();
      const storageKey = resolveDismissedStorageKey(authStore.user?.id);
      if (!force && this.dismissedStorageKey === storageKey) {
        return;
      }
      this.dismissedStorageKey = storageKey;
      if (typeof localStorage === 'undefined') {
        this.dismissedConversationIds = [];
        return;
      }
      const raw = localStorage.getItem(storageKey);
      this.dismissedConversationIds = parseDismissedConversationIds(raw);
    },

    persistDismissedConversationState() {
      if (typeof localStorage === 'undefined') return;
      const authStore = useAuthStore();
      const storageKey = this.dismissedStorageKey || resolveDismissedStorageKey(authStore.user?.id);
      this.dismissedStorageKey = storageKey;
      try {
        const payload = JSON.stringify(Array.from(new Set(this.dismissedConversationIds)));
        localStorage.setItem(storageKey, payload);
      } catch {
        // Ignore quota/private-mode storage errors.
      }
    },

    isConversationDismissed(conversationId: string): boolean {
      const cleaned = normalizeConversationId(conversationId);
      if (!cleaned) return false;
      return this.dismissedConversationIds.includes(cleaned);
    },

    markConversationDismissed(conversationId: string) {
      const cleaned = normalizeConversationId(conversationId);
      if (!cleaned || this.dismissedConversationIds.includes(cleaned)) {
        return;
      }
      this.dismissedConversationIds = [...this.dismissedConversationIds, cleaned];
      this.persistDismissedConversationState();
    },

    clearConversationDismissed(conversationId: string) {
      const cleaned = normalizeConversationId(conversationId);
      if (!cleaned || !this.dismissedConversationIds.includes(cleaned)) {
        return;
      }
      this.dismissedConversationIds = this.dismissedConversationIds.filter((item) => item !== cleaned);
      this.persistDismissedConversationState();
    },

    async bootstrap(force = false) {
      this.ensureDismissedConversationState(force);
      if (
        this.permissionDenied &&
        this.permissionDeniedKey &&
        this.permissionDeniedKey !== this.dismissedStorageKey
      ) {
        this.permissionDenied = false;
        this.permissionDeniedKey = '';
      }
      if (this.initialized && !force && !this.permissionDenied) {
        return;
      }
      if (this.permissionDenied && !force) {
        return;
      }
      this.loading = true;
      this.error = '';
      try {
        this.permissionDenied = false;
        this.permissionDeniedKey = '';
        await this.refreshContacts();
        if (this.permissionDenied) {
          this.initialized = true;
          return;
        }
        await Promise.all([this.refreshConversations(), this.refreshGroups()]);
        if (this.permissionDenied) {
          this.initialized = true;
          return;
        }
        if (!this.activeConversationId && this.conversations.length) {
          this.activeConversationId = this.conversations[0].conversation_id;
        }
        if (this.activeConversationId) {
          await this.loadMessages(this.activeConversationId);
          await this.markConversationRead(this.activeConversationId);
        }
        await this.syncConversationWatchers();
        this.initialized = true;
      } catch (error) {
        if (isAuthDeniedError(error)) {
          this.permissionDenied = true;
          this.permissionDeniedKey = this.dismissedStorageKey;
          this.contacts = [];
          this.groups = [];
          this.conversations = [];
          this.unreadByConversation = {};
          this.activeConversationId = '';
          this.stopAllWatchers();
          this.initialized = true;
          return;
        }
        const source = error as { message?: string };
        this.error = source?.message || 'user world bootstrap failed';
        throw error;
      } finally {
        this.loading = false;
      }
    },

    async refreshContacts(keyword = '') {
      if (this.permissionDenied) {
        this.contacts = [];
        return;
      }
      this.ensureDismissedConversationState();
      const params: Record<string, string | number | boolean | null | undefined> = {
        offset: 0,
        limit: 500
      };
      if (keyword.trim()) {
        params.keyword = keyword.trim();
      }
      try {
        const response = await listUserWorldContacts(params);
        const data = asRecord(response.data?.data);
        const items = Array.isArray(data.items) ? (data.items as UserWorldContact[]) : [];
        this.contacts = items.map((item) => {
          const conversationId = normalizeConversationId(item.conversation_id);
          if (conversationId && this.isConversationDismissed(conversationId)) {
            return {
              ...item,
              conversation_id: null,
              unread_count: 0
            };
          }
          return {
            ...item,
            unread_count: toNumber(item.unread_count)
          };
        });
        this.permissionDenied = false;
        this.permissionDeniedKey = '';
      } catch (error) {
        if (isAuthDeniedError(error)) {
          this.permissionDenied = true;
          this.permissionDeniedKey = this.dismissedStorageKey;
          this.contacts = [];
          this.groups = [];
          this.conversations = [];
          this.unreadByConversation = {};
          this.activeConversationId = '';
          this.stopAllWatchers();
          return;
        }
        throw error;
      }
    },

    async refreshGroups() {
      if (this.permissionDenied) {
        this.groups = [];
        return;
      }
      this.ensureDismissedConversationState();
      try {
        const response = await listUserWorldGroups({ offset: 0, limit: 500 });
        const data = asRecord(response.data?.data);
        const items = Array.isArray(data.items) ? (data.items as UserWorldGroup[]) : [];
        this.groups = items
          .filter((item) => !this.isConversationDismissed(normalizeConversationId(item.conversation_id)))
          .map((item) => ({
            ...item,
            member_count: toNumber(item.member_count),
            unread_count_cache: toNumber(item.unread_count_cache),
            updated_at: toNumber(item.updated_at),
            last_message_at: toNumber(item.last_message_at),
            last_message_id: item.last_message_id ? toNumber(item.last_message_id) : item.last_message_id
          }))
          .sort((left, right) => toNumber(right.last_message_at) - toNumber(left.last_message_at));
        this.permissionDenied = false;
        this.permissionDeniedKey = '';
      } catch (error) {
        if (isAuthDeniedError(error)) {
          this.permissionDenied = true;
          this.permissionDeniedKey = this.dismissedStorageKey;
          this.groups = [];
          this.conversations = [];
          this.unreadByConversation = {};
          this.activeConversationId = '';
          this.stopAllWatchers();
          return;
        }
        throw error;
      }
    },

    async refreshConversations() {
      if (this.permissionDenied) {
        this.conversations = [];
        this.unreadByConversation = {};
        return;
      }
      this.ensureDismissedConversationState();
      try {
        const response = await listUserWorldConversations({ offset: 0, limit: 500 });
        const data = asRecord(response.data?.data);
        const items = Array.isArray(data.items) ? (data.items as UserWorldConversation[]) : [];
        this.conversations = items
          .filter((item) => !this.isConversationDismissed(normalizeConversationId(item.conversation_id)))
          .map((item) => ({
            ...item,
            unread_count_cache: toNumber(item.unread_count_cache),
            member_count:
              item.member_count === null || item.member_count === undefined
                ? item.member_count
                : toNumber(item.member_count)
          }));
        this.unreadByConversation = this.conversations.reduce((acc: Record<string, number>, item) => {
          acc[item.conversation_id] = toNumber(item.unread_count_cache);
          return acc;
        }, {} as Record<string, number>);
        if (
          this.activeConversationId &&
          this.isConversationDismissed(normalizeConversationId(this.activeConversationId))
        ) {
          this.activeConversationId = '';
        }
        await this.syncConversationWatchers();
        this.permissionDenied = false;
        this.permissionDeniedKey = '';
      } catch (error) {
        if (isAuthDeniedError(error)) {
          this.permissionDenied = true;
          this.permissionDeniedKey = this.dismissedStorageKey;
          this.conversations = [];
          this.unreadByConversation = {};
          this.activeConversationId = '';
          this.stopAllWatchers();
          return;
        }
        throw error;
      }
    },

    async openConversationByPeer(peerUserId: string, options: OpenConversationByPeerOptions = {}) {
      if (this.permissionDenied) return null;
      const peer = peerUserId.trim();
      if (!peer) return null;
      const shouldActivate = options.activate !== false;
      const contactConversationId = normalizeConversationId(
        this.contacts.find((item) => String(item?.user_id || '').trim() === peer)?.conversation_id
      );
      const existingConversation =
        this.conversations.find((item) => {
          const kind = String(item?.conversation_type || '').trim().toLowerCase();
          if (kind === 'group') return false;
          const itemPeer = String(item?.peer_user_id || '').trim();
          if (itemPeer && itemPeer === peer) return true;
          return Boolean(contactConversationId) && String(item?.conversation_id || '').trim() === contactConversationId;
        }) || null;
      if (existingConversation?.conversation_id) {
        this.clearConversationDismissed(existingConversation.conversation_id);
        const contact = this.contacts.find((item) => String(item?.user_id || '').trim() === peer);
        if (contact) {
          contact.conversation_id = existingConversation.conversation_id;
        }
        if (shouldActivate) {
          await this.setActiveConversation(existingConversation.conversation_id, {
            waitForLoad: options.waitForLoad
          });
          await this.syncConversationWatchers();
        }
        return existingConversation;
      }
      const response = await createOrGetUserWorldConversation({ peer_user_id: peer });
      const incoming = asRecord(response.data?.data) as unknown as UserWorldConversation;
      if (!incoming?.conversation_id) return null;
      const conversation: UserWorldConversation = {
        ...incoming,
        peer_user_id: String(incoming.peer_user_id || peer).trim()
      };
      this.clearConversationDismissed(conversation.conversation_id);
      this.upsertConversation(conversation);
      const contact = this.contacts.find((item) => String(item?.user_id || '').trim() === peer);
      if (contact) {
        contact.conversation_id = conversation.conversation_id;
      }
      if (shouldActivate) {
        await this.setActiveConversation(conversation.conversation_id, {
          waitForLoad: options.waitForLoad
        });
        await this.syncConversationWatchers();
      }
      return conversation;
    },

    async createGroupConversation(groupName: string, memberUserIds: string[]) {
      if (this.permissionDenied) return null;
      const name = groupName.trim();
      const members = memberUserIds
        .map((item) => String(item || '').trim())
        .filter((item) => Boolean(item));
      if (!name || !members.length) {
        return null;
      }
      const response = await createUserWorldGroup({
        group_name: name,
        member_user_ids: members
      });
      const conversation = asRecord(response.data?.data) as unknown as UserWorldConversation;
      if (!conversation?.conversation_id) {
        return null;
      }
      this.clearConversationDismissed(conversation.conversation_id);
      this.upsertConversation(conversation);
      await Promise.all([this.refreshConversations(), this.refreshGroups()]);
      await this.setActiveConversation(conversation.conversation_id);
      await this.syncConversationWatchers();
      return conversation;
    },

    async dismissConversation(conversationId: string) {
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return;
      this.markConversationDismissed(cleaned);

      const wasActive = this.activeConversationId === cleaned;
      this.stopConversationWatch(cleaned);

      this.conversations = this.conversations.filter((item) => item.conversation_id !== cleaned);
      this.groups = this.groups.filter((item) => item.conversation_id !== cleaned);
      this.contacts = this.contacts.map((item) => {
        if (item.conversation_id !== cleaned) {
          return item;
        }
        return {
          ...item,
          conversation_id: null,
          unread_count: 0
        };
      });

      if (cleaned in this.messagesByConversation) {
        delete this.messagesByConversation[cleaned];
      }
      if (cleaned in this.unreadByConversation) {
        delete this.unreadByConversation[cleaned];
      }

      if (wasActive) {
        this.activeConversationId = '';
        const nextId = String(this.conversations[0]?.conversation_id || '').trim();
        if (nextId) {
          await this.setActiveConversation(nextId);
        }
      }

      await this.syncConversationWatchers();
    },

    async setActiveConversation(
      conversationId: string,
      options: { forceReload?: boolean; waitForLoad?: boolean } = {}
    ) {
      if (this.permissionDenied) return;
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return;
      this.clearConversationDismissed(cleaned);
      const switched = this.activeConversationId !== cleaned;
      this.activeConversationId = cleaned;
      const hasCachedMessages =
        Array.isArray(this.messagesByConversation[cleaned]) &&
        this.messagesByConversation[cleaned].length > 0;
      const shouldReload = Boolean(options.forceReload) || switched || !hasCachedMessages;
      const waitForLoad = options.waitForLoad !== false;
      if (shouldReload) {
        const loadTask = this.loadMessages(cleaned);
        if (waitForLoad) {
          await loadTask;
        } else {
          void loadTask.catch(() => undefined);
        }
      } else if (!waitForLoad) {
        void this.loadMessages(cleaned).catch(() => undefined);
      }
      void this.markConversationRead(cleaned);
    },

    async loadMessages(conversationId: string, options: { beforeMessageId?: number } = {}) {
      if (this.permissionDenied) return;
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return;
      const params: Record<string, string | number | boolean | null | undefined> = { limit: 100 };
      if (Number.isFinite(options.beforeMessageId) && Number(options.beforeMessageId) > 0) {
        params.before_message_id = Number(options.beforeMessageId);
      }
      const response = await listUserWorldMessages(cleaned, params);
      const data = asRecord(response.data?.data);
      const items = Array.isArray(data.items) ? (data.items as UserWorldMessage[]) : [];
      const normalized = items
        .map((item) => ({
          ...item,
          message_id: toNumber(item.message_id),
          created_at: toNumber(item.created_at)
        }))
        .sort((left, right) => left.message_id - right.message_id);
      this.messagesByConversation[cleaned] = normalized;
    },

    async sendToActiveConversation(content: string) {
      if (this.permissionDenied) return;
      const text = content.trim();
      if (!text || !this.activeConversationId) return;
      const conversationId = this.activeConversationId;
      this.sending = true;
      this.error = '';
      const clientMsgId = nowId();
      const authStore = useAuthStore();
      const currentUserId = String(authStore.user?.id || '').trim();
      const optimisticMessage: UserWorldMessage = {
        message_id: -Math.floor(Date.now()),
        conversation_id: conversationId,
        sender_user_id: currentUserId,
        content: text,
        content_type: 'text',
        client_msg_id: clientMsgId,
        created_at: Date.now() / 1000
      };
      this.upsertMessage(conversationId, optimisticMessage);
      try {
        const transport = resolveTransport();
        this.streamTransport = transport;
        if (transport === 'ws') {
          localStorage.setItem('user_world_stream_transport', 'ws');
        } else {
          localStorage.removeItem('user_world_stream_transport');
        }
        let sentByWs = false;
        if (transport === 'ws') {
          const requestId = buildRequestId();
          let ackMessage: UserWorldMessage | null = null;
          const timeout = createTimeoutSignal(3500);
          try {
            await wsClient.request({
              requestId,
              sessionId: conversationId,
              signal: timeout.signal,
              message: {
                type: 'send',
                request_id: requestId,
                payload: {
                  conversation_id: conversationId,
                  content: text,
                  content_type: 'text',
                  client_msg_id: clientMsgId
                }
              },
              onEvent: (eventType, dataText) => {
                if (eventType === 'ack') {
                  const payload = parseEventPayload(dataText);
                  const data = asRecord(payload.data || payload);
                  const message = asRecord(data.message);
                  if (message.message_id) {
                    ackMessage = message as unknown as UserWorldMessage;
                  }
                } else if (eventType.startsWith('uw.')) {
                  this.applyRealtimeEvent(conversationId, eventType, dataText);
                }
              }
            });
          } catch (error) {
            const wsError = error as WsError;
            if (wsError?.phase === 'connect') {
              markWsUnavailable();
            }
          } finally {
            timeout.cleanup();
          }
          if (ackMessage) {
            this.upsertMessage(conversationId, ackMessage);
            sentByWs = true;
          }
        }
        if (sentByWs) return;
        const response = await sendUserWorldMessage(conversationId, {
          content: text,
          content_type: 'text',
          client_msg_id: clientMsgId
        });
        const data = asRecord(response.data?.data);
        const message = asRecord(data.message) as unknown as UserWorldMessage;
        if (message?.message_id) {
          this.upsertMessage(conversationId, message);
        }
      } catch (error) {
        if (isTimeoutError(error)) {
          try {
            await this.loadMessages(conversationId);
            const list = this.messagesByConversation[conversationId] || [];
            const delivered = list.some((item) => item.client_msg_id && item.client_msg_id === clientMsgId);
            if (delivered) {
              return;
            }
          } catch {
            // ignore secondary refresh failure
          }
        }
        this.removeMessageByClientMsgId(conversationId, clientMsgId);
        const source = error as { message?: string };
        this.error = source?.message || 'send user world message failed';
        throw error;
      } finally {
        this.sending = false;
      }
    },

    async markConversationRead(conversationId: string) {
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return;
      try {
        const latestMessageId = this.resolveLatestMessageId(cleaned);
        const payload =
          latestMessageId > 0 ? { last_read_message_id: latestMessageId } : { last_read_message_id: null };
        const transport = resolveTransport();
        if (transport === 'ws') {
          const requestId = buildRequestId();
          const timeout = createTimeoutSignal(8000);
          try {
            await wsClient.request({
              requestId,
              signal: timeout.signal,
              message: {
                type: 'read',
                request_id: requestId,
                payload: {
                  conversation_id: cleaned,
                  last_read_message_id: payload.last_read_message_id
                }
              },
              onEvent: (eventType, dataText) => {
                if (eventType.startsWith('uw.')) {
                  this.applyRealtimeEvent(cleaned, eventType, dataText);
                }
              }
            });
          } finally {
            timeout.cleanup();
          }
        } else {
          await markUserWorldRead(cleaned, payload);
        }
      } catch {
        // ignore read failures to avoid blocking main flow
      }
      this.unreadByConversation[cleaned] = 0;
      const conversation = this.conversations.find((item) => item.conversation_id === cleaned);
      if (conversation) {
        conversation.unread_count_cache = 0;
      }
      const contact = this.contacts.find((item) => item.conversation_id === cleaned);
      if (contact) {
        contact.unread_count = 0;
      }
      const group = this.groups.find((item) => item.conversation_id === cleaned);
      if (group) {
        group.unread_count_cache = 0;
      }
    },

    async syncConversationWatchers() {
      const targetIds = new Set<string>();
      const activeId = String(this.activeConversationId || '').trim();
      if (activeId) {
        targetIds.add(activeId);
      } else {
        const firstId = String(this.conversations[0]?.conversation_id || '').trim();
        if (firstId) {
          targetIds.add(firstId);
        }
      }
      Array.from(watchRuntime.keys()).forEach((conversationId) => {
        if (!targetIds.has(conversationId)) {
          this.stopConversationWatch(conversationId);
        }
      });
      for (const conversationId of targetIds) {
        if (!watchRuntime.has(conversationId)) {
          this.startConversationWatch(conversationId);
        }
      }
    },

    stopConversationWatch(conversationId: string) {
      const runtime = watchRuntime.get(conversationId);
      if (!runtime) return;
      runtime.controller.abort();
      watchRuntime.delete(conversationId);
      if (runtime.transport === 'ws') {
        wsClient.sendCancel(runtime.requestId, conversationId);
      }
    },

    stopAllWatchers() {
      Array.from(watchRuntime.keys()).forEach((conversationId) => {
        this.stopConversationWatch(conversationId);
      });
      wsClient.close(1000, 'user-world-stop');
    },

    startConversationWatch(conversationId: string) {
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return;
      const controller = new AbortController();
      const transport = resolveTransport();
      if (transport === 'ws') {
        this.startWsWatch(cleaned, controller);
        return;
      }
      this.startSseWatch(cleaned, controller);
    },

    startWsWatch(conversationId: string, controller: AbortController) {
      const requestId = buildRequestId();
      watchRuntime.set(conversationId, {
        controller,
        requestId,
        transport: 'ws'
      });
      const afterEventId = this.resolveLastEventId(conversationId);
      wsClient
        .request({
          requestId,
          sessionId: conversationId,
          message: {
            type: 'watch',
            request_id: requestId,
            payload: {
              conversation_id: conversationId,
              after_event_id: afterEventId
            }
          },
          closeOnFinal: false,
          signal: controller.signal,
          onEvent: (eventType, dataText, eventId) => {
            this.updateLastEventId(conversationId, eventId);
            if (eventType.startsWith('uw.')) {
              this.applyRealtimeEvent(conversationId, eventType, dataText);
            }
          }
        })
        .catch((error: WsError) => {
          if (controller.signal.aborted) return;
          watchRuntime.delete(conversationId);
          markWsUnavailable(error?.phase === 'connect' ? 180000 : 120000);
          this.startSseWatch(conversationId, controller);
          return;
        });
    },

    startSseWatch(conversationId: string, controller: AbortController) {
      const requestId = buildRequestId();
      watchRuntime.set(conversationId, {
        controller,
        requestId,
        transport: 'sse'
      });
      const run = async () => {
        while (!controller.signal.aborted) {
          try {
            const response = await streamUserWorldEvents(conversationId, {
              signal: controller.signal,
              afterEventId: this.resolveLastEventId(conversationId),
              limit: 200
            });
            if (!response.ok) {
              throw new Error(`sse status ${response.status}`);
            }
            await consumeSseStream(response, (eventType, dataText, eventId) => {
              this.updateLastEventId(conversationId, eventId);
              if (eventType.startsWith('uw.')) {
                this.applyRealtimeEvent(conversationId, eventType, dataText);
              }
            });
          } catch {
            if (controller.signal.aborted) {
              return;
            }
            await new Promise((resolve) => window.setTimeout(resolve, WATCH_RETRY_DELAY_MS));
          }
        }
      };
      run().finally(() => {
        if (watchRuntime.get(conversationId)?.requestId === requestId) {
          watchRuntime.delete(conversationId);
        }
      });
    },

    applyRealtimeEvent(conversationId: string, eventType: string, dataText: string) {
      const payload = parseEventPayload(dataText);
      if (eventType === 'uw.message') {
        const message = asRecord(payload.message) as unknown as UserWorldMessage;
        if (message?.message_id) {
          this.upsertMessage(conversationId, message);
        }
        return;
      }
      if (eventType === 'uw.read') {
        const targetConversationId = String(payload.conversation_id || conversationId).trim();
        const userId = String(payload.user_id || '').trim();
        const unread = toNumber(payload.unread_count);
        const authStore = useAuthStore();
        if (targetConversationId) {
          if (userId && authStore.user?.id && userId === authStore.user.id) {
            this.unreadByConversation[targetConversationId] = unread;
            const conversation = this.conversations.find(
              (item) => item.conversation_id === targetConversationId
            );
            if (conversation) {
              conversation.unread_count_cache = unread;
            }
            const group = this.groups.find((item) => item.conversation_id === targetConversationId);
            if (group) {
              group.unread_count_cache = unread;
            }
          }
        }
      }
    },

    upsertConversation(conversation: UserWorldConversation) {
      const item: UserWorldConversation = {
        ...conversation,
        unread_count_cache: toNumber(conversation.unread_count_cache),
        member_count:
          conversation.member_count === null || conversation.member_count === undefined
            ? conversation.member_count
            : toNumber(conversation.member_count)
      };
      if (this.isConversationDismissed(normalizeConversationId(item.conversation_id))) {
        return;
      }
      const index = this.conversations.findIndex(
        (entry) => entry.conversation_id === item.conversation_id
      );
      if (index >= 0) {
        this.conversations[index] = { ...this.conversations[index], ...item };
      } else {
        this.conversations.unshift(item);
      }
      this.unreadByConversation[item.conversation_id] = toNumber(item.unread_count_cache);
      if (item.conversation_type === 'group') {
        const groupId = String(item.group_id || '').trim();
        const groupIndex = this.groups.findIndex((entry) => entry.conversation_id === item.conversation_id);
        if (groupIndex >= 0) {
          this.groups[groupIndex] = {
            ...this.groups[groupIndex],
            group_id: groupId || this.groups[groupIndex].group_id,
            conversation_id: item.conversation_id,
            group_name: String(item.group_name || this.groups[groupIndex].group_name || '').trim(),
            unread_count_cache: toNumber(item.unread_count_cache),
            member_count: toNumber(item.member_count || this.groups[groupIndex].member_count),
            updated_at: toNumber(item.updated_at || this.groups[groupIndex].updated_at),
            last_message_at: toNumber(item.last_message_at || this.groups[groupIndex].last_message_at),
            last_message_id: item.last_message_id ?? this.groups[groupIndex].last_message_id,
            last_message_preview: item.last_message_preview ?? this.groups[groupIndex].last_message_preview
          };
        } else {
          this.groups.unshift({
            group_id: groupId || `group:${item.conversation_id}`,
            conversation_id: item.conversation_id,
            group_name: String(item.group_name || '').trim() || item.conversation_id,
            owner_user_id: '',
            member_count: toNumber(item.member_count),
            unread_count_cache: toNumber(item.unread_count_cache),
            updated_at: toNumber(item.updated_at),
            last_message_at: toNumber(item.last_message_at),
            last_message_id: item.last_message_id,
            last_message_preview: item.last_message_preview || null
          });
        }
      }
      this.sortConversations();
      this.sortGroups();
    },

    upsertMessage(conversationId: string, message: UserWorldMessage) {
      const cleaned = String(conversationId || message.conversation_id || '').trim();
      if (!cleaned) return;
      const normalized: UserWorldMessage = {
        ...message,
        conversation_id: cleaned,
        message_id: toNumber(message.message_id),
        created_at: toNumber(message.created_at)
      };
      const list = this.messagesByConversation[cleaned] || [];
      const byClientMsgId =
        normalized.client_msg_id && String(normalized.client_msg_id).trim()
          ? list.findIndex((item) => item.client_msg_id === normalized.client_msg_id)
          : -1;
      const index =
        byClientMsgId >= 0 ? byClientMsgId : list.findIndex((item) => item.message_id === normalized.message_id);
      if (index >= 0) {
        list[index] = { ...list[index], ...normalized };
      } else {
        list.push(normalized);
      }
      list.sort((left, right) => {
        const leftId = toNumber(left.message_id);
        const rightId = toNumber(right.message_id);
        if (leftId > 0 && rightId > 0 && leftId !== rightId) {
          return leftId - rightId;
        }
        return toNumber(left.created_at) - toNumber(right.created_at);
      });
      this.messagesByConversation[cleaned] = list;

      const authStore = useAuthStore();
      const currentUserId = String(authStore.user?.id || '').trim();
      const isIncoming = normalized.sender_user_id && normalized.sender_user_id !== currentUserId;
      const isActive = this.activeConversationId === cleaned;
      if (isIncoming && !isActive) {
        this.unreadByConversation[cleaned] = toNumber(this.unreadByConversation[cleaned]) + 1;
      } else if (isActive) {
        this.unreadByConversation[cleaned] = 0;
      }

      const conversation = this.conversations.find((item) => item.conversation_id === cleaned);
      if (conversation) {
        conversation.last_message_at = normalized.created_at;
        conversation.last_message_id = normalized.message_id;
        conversation.last_message_preview = normalized.content;
        conversation.updated_at = normalized.created_at;
        conversation.unread_count_cache = toNumber(this.unreadByConversation[cleaned]);
      }

      const peerId = conversation?.peer_user_id || '';
      const contact = this.contacts.find((item) => item.user_id === peerId || item.conversation_id === cleaned);
      if (contact) {
        contact.conversation_id = cleaned;
        contact.last_message_at = normalized.created_at;
        contact.last_message_preview = normalized.content;
        contact.unread_count = toNumber(this.unreadByConversation[cleaned]);
      }
      const group = this.groups.find((item) => item.conversation_id === cleaned);
      if (group) {
        group.last_message_at = normalized.created_at;
        group.last_message_id = normalized.message_id;
        group.last_message_preview = normalized.content;
        group.updated_at = normalized.created_at;
        group.unread_count_cache = toNumber(this.unreadByConversation[cleaned]);
      }
      this.sortConversations();
      this.sortContacts();
      this.sortGroups();
    },

    removeMessageByClientMsgId(conversationId: string, clientMsgId: string) {
      const cleanedConversation = String(conversationId || '').trim();
      const cleanedClientMsgId = String(clientMsgId || '').trim();
      if (!cleanedConversation || !cleanedClientMsgId) return;
      const list = this.messagesByConversation[cleanedConversation] || [];
      const nextList = list.filter((item) => item.client_msg_id !== cleanedClientMsgId);
      if (nextList.length === list.length) return;
      this.messagesByConversation[cleanedConversation] = nextList;
    },

    sortConversations() {
      this.conversations.sort((left, right) => {
        const leftPinned = left.pinned ? 1 : 0;
        const rightPinned = right.pinned ? 1 : 0;
        if (leftPinned !== rightPinned) {
          return rightPinned - leftPinned;
        }
        return toNumber(right.last_message_at) - toNumber(left.last_message_at);
      });
    },

    sortContacts() {
      this.contacts.sort((left, right) => {
        const leftTs = toNumber(left.last_message_at);
        const rightTs = toNumber(right.last_message_at);
        if (leftTs !== rightTs) {
          return rightTs - leftTs;
        }
        return String(left.username || '').localeCompare(String(right.username || ''));
      });
    },

    sortGroups() {
      this.groups.sort((left, right) => {
        const leftTs = toNumber(left.last_message_at);
        const rightTs = toNumber(right.last_message_at);
        if (leftTs !== rightTs) {
          return rightTs - leftTs;
        }
        return String(left.group_name || '').localeCompare(String(right.group_name || ''));
      });
    },

    resolveConversationTitle(conversation: UserWorldConversation | null | undefined): string {
      if (!conversation) return '';
      if (conversation.conversation_type === 'group') {
        const groupName = String(conversation.group_name || '').trim();
        if (groupName) return groupName;
        const group = this.groups.find((item) => item.conversation_id === conversation.conversation_id);
        if (group?.group_name) {
          return group.group_name;
        }
        return conversation.conversation_id;
      }
      const peerId = String(conversation.peer_user_id || '').trim();
      if (!peerId) return conversation.conversation_id;
      const contact = this.contacts.find((item) => item.user_id === peerId);
      if (contact?.username) {
        return contact.username;
      }
      return peerId;
    },

    resolveConversationUnread(conversationId: string): number {
      const cleaned = String(conversationId || '').trim();
      if (!cleaned) return 0;
      return toNumber(this.unreadByConversation[cleaned]);
    },

    resolveLatestMessageId(conversationId: string): number {
      const list = this.messagesByConversation[conversationId] || [];
      if (!list.length) return 0;
      return toNumber(list[list.length - 1].message_id);
    },

    resolveLastEventId(conversationId: string): number {
      const key = `user_world_event_id:${conversationId}`;
      const value = Number(localStorage.getItem(key) || 0);
      return Number.isFinite(value) ? Math.max(0, value) : 0;
    },

    updateLastEventId(conversationId: string, eventId: string) {
      const next = Number(eventId || 0);
      if (!Number.isFinite(next) || next <= 0) return;
      const key = `user_world_event_id:${conversationId}`;
      const current = this.resolveLastEventId(conversationId);
      if (next > current) {
        localStorage.setItem(key, String(next));
      }
    }
  }
});
