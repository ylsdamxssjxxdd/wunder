<template>
  <div class="messenger-archived-thread-panel">
    <div class="messenger-archived-thread-panel-header">
      <div class="messenger-archived-thread-panel-title">
        {{ t('chat.history.archivedDialogTitle') }}
      </div>
      <button
        class="messenger-inline-btn"
        type="button"
        :disabled="loading"
        :title="t('common.refresh')"
        :aria-label="t('common.refresh')"
        @click="loadArchivedSessions"
      >
        <i class="fa-solid fa-rotate" aria-hidden="true"></i>
        <span>{{ t('common.refresh') }}</span>
      </button>
    </div>

    <div v-if="loading" class="messenger-list-empty">{{ t('common.loading') }}</div>
    <div v-else-if="!archivedSessions.length" class="messenger-list-empty">
      {{ t('chat.history.archivedEmpty') }}
    </div>
    <div v-else class="messenger-archived-thread-list">
      <article
        v-for="item in archivedSessions"
        :key="item.id"
        class="messenger-archived-thread-item"
        role="button"
        tabindex="0"
        @click="openArchivedSessionDetail(item.id)"
        @keydown.enter.prevent="openArchivedSessionDetail(item.id)"
        @keydown.space.prevent="openArchivedSessionDetail(item.id)"
      >
        <div class="messenger-archived-thread-title-row">
          <div class="messenger-archived-thread-title">{{ item.title }}</div>
          <span class="messenger-archived-thread-time">{{ formatTime(item.lastAt) }}</span>
        </div>
        <div class="messenger-archived-thread-preview">
          {{ item.preview || t('messenger.preview.empty') }}
        </div>
        <div class="messenger-archived-thread-actions">
          <button
            class="messenger-archived-thread-restore-btn"
            type="button"
            :disabled="busySessionIds.has(item.id)"
            :title="t('chat.history.restore')"
            :aria-label="t('chat.history.restore')"
            @click.stop="restoreArchivedSession(item.id)"
          >
            <i class="fa-solid fa-rotate-left" aria-hidden="true"></i>
          </button>
          <button
            class="messenger-archived-thread-delete-btn"
            type="button"
            :disabled="busySessionIds.has(item.id)"
            :title="t('chat.history.delete')"
            :aria-label="t('chat.history.delete')"
            @click.stop="deleteArchivedSession(item.id)"
          >
            <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
          </button>
        </div>
      </article>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { deleteSession as deleteSessionApi } from '@/api/chat';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { showApiError } from '@/utils/apiError';
import { confirmWithFallback } from '@/utils/confirm';

type ArchivedSessionItem = {
  id: string;
  title: string;
  preview: string;
  lastAt: unknown;
};

const props = defineProps<{
  agentId: string;
}>();

const emit = defineEmits<{
  (event: 'open-session-detail', sessionId: string): void;
  (event: 'session-restored', sessionId: string): void;
  (event: 'session-deleted', sessionId: string): void;
}>();

const { t } = useI18n();
const chatStore = useChatStore();

const loading = ref(false);
const archivedSessions = ref<ArchivedSessionItem[]>([]);
const busySessionIds = ref<Set<string>>(new Set());

const normalizedAgentId = computed(() => String(props.agentId || '').trim());

const setSessionBusy = (sessionId: string, busy: boolean) => {
  const next = new Set(busySessionIds.value);
  if (busy) {
    next.add(sessionId);
  } else {
    next.delete(sessionId);
  }
  busySessionIds.value = next;
};

const normalizeTimestamp = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? 0 : value.getTime();
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const date = new Date(text);
  return Number.isNaN(date.getTime()) ? 0 : date.getTime();
};

const formatTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) return '';
  const date = new Date(ts);
  const now = new Date();
  const sameYear = date.getFullYear() === now.getFullYear();
  const sameDay =
    sameYear && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  if (sameDay) {
    return `${hour}:${minute}`;
  }
  if (sameYear) {
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${month}-${day}`;
  }
  return String(date.getFullYear());
};

const resolveSessionPreview = (session: Record<string, unknown>): string =>
  String(
    session?.last_user_message_preview ||
      session?.last_user_message ||
      session?.last_message_preview ||
      session?.last_message ||
      session?.summary ||
      ''
  )
    .replace(/\s+/g, ' ')
    .slice(0, 120);

const loadArchivedSessions = async () => {
  loading.value = true;
  try {
    const sessions = await chatStore.listSessionsByStatus({
      agent_id: normalizedAgentId.value,
      status: 'archived'
    });
    archivedSessions.value = (Array.isArray(sessions) ? sessions : [])
      .map((session: Record<string, unknown>) => ({
        id: String(session?.id || '').trim(),
        title: String(session?.title || t('chat.newSession')).trim() || t('chat.newSession'),
        preview: resolveSessionPreview(session),
        lastAt: session?.updated_at || session?.last_message_at || session?.created_at
      }))
      .filter((item) => item.id)
      .sort((left, right) => normalizeTimestamp(right.lastAt) - normalizeTimestamp(left.lastAt));
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  } finally {
    loading.value = false;
  }
};

const openArchivedSessionDetail = (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  emit('open-session-detail', targetId);
};

const restoreArchivedSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId || busySessionIds.value.has(targetId)) return;
  setSessionBusy(targetId, true);
  try {
    await chatStore.restoreSession(targetId);
    archivedSessions.value = archivedSessions.value.filter((item) => item.id !== targetId);
    ElMessage.success(t('chat.history.restoreSuccess'));
    emit('session-restored', targetId);
  } catch (error) {
    showApiError(error, t('chat.history.restoreFailed'));
  } finally {
    setSessionBusy(targetId, false);
  }
};

const deleteArchivedSession = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId || busySessionIds.value.has(targetId)) return;
  const confirmed = await confirmWithFallback(
    t('chat.history.confirmDelete'),
    t('chat.history.confirmTitle'),
    {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    }
  );
  if (!confirmed) {
    return;
  }
  setSessionBusy(targetId, true);
  try {
    await deleteSessionApi(targetId);
    archivedSessions.value = archivedSessions.value.filter((item) => item.id !== targetId);
    ElMessage.success(t('chat.history.delete'));
    emit('session-deleted', targetId);
  } catch (error) {
    showApiError(error, t('chat.sessions.deleteFailed'));
  } finally {
    setSessionBusy(targetId, false);
  }
};

watch(
  () => normalizedAgentId.value,
  () => {
    void loadArchivedSessions();
  }
);

onMounted(() => {
  void loadArchivedSessions();
});
</script>

<style scoped>
.messenger-archived-thread-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  height: 100%;
}

.messenger-archived-thread-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.messenger-archived-thread-panel-title {
  font-size: 13px;
  font-weight: 600;
  color: #2f2f2f;
}

.messenger-archived-thread-list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.messenger-archived-thread-item {
  position: relative;
  border: 1px solid #e3e4e6;
  border-radius: 10px;
  background: #ffffff;
  padding: 10px 12px;
  cursor: pointer;
  transition: border-color 0.15s ease, background-color 0.15s ease;
}

.messenger-archived-thread-item:hover,
.messenger-archived-thread-item:focus-within {
  border-color: rgba(var(--ui-accent-rgb), 0.4);
  background: #f8fafb;
}

.messenger-archived-thread-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.messenger-archived-thread-title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  color: #2f2f2f;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.messenger-archived-thread-time {
  flex-shrink: 0;
  font-size: 11px;
  color: #8a8a8a;
}

.messenger-archived-thread-preview {
  margin-top: 6px;
  font-size: 12px;
  color: #6f6f6f;
  line-height: 1.4;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.messenger-archived-thread-actions {
  position: absolute;
  top: 10px;
  right: 10px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.15s ease;
}

.messenger-archived-thread-item:hover .messenger-archived-thread-actions,
.messenger-archived-thread-item:focus-within .messenger-archived-thread-actions {
  opacity: 1;
  pointer-events: auto;
}

.messenger-archived-thread-restore-btn,
.messenger-archived-thread-delete-btn {
  width: 24px;
  height: 24px;
  border: 1px solid #d9dde1;
  border-radius: 6px;
  background: #ffffff;
  color: #667085;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.messenger-archived-thread-restore-btn:hover:not(:disabled) {
  border-color: rgba(var(--ui-accent-rgb), 0.42);
  color: var(--ui-accent-deep);
  background: var(--ui-accent-soft-2);
}

.messenger-archived-thread-delete-btn:hover:not(:disabled) {
  border-color: #f0c2cb;
  color: #c14053;
  background: #fbf0f2;
}

.messenger-archived-thread-restore-btn:disabled,
.messenger-archived-thread-delete-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
</style>
