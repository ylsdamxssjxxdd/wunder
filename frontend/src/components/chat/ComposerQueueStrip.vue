<template>
  <div v-if="items.length" class="composer-queue" data-testid="chat-queue-strip">
    <div class="composer-queue-head">
      <span class="composer-queue-title">{{ t('chat.composer.queue.title') }}</span>
      <span class="composer-queue-count">{{ t('chat.composer.queue.count', { count: items.length }) }}</span>
    </div>
    <div class="composer-queue-list">
      <div
        v-for="(item, index) in items"
        :key="item.queueId"
        class="composer-queue-item"
        :class="{ 'is-dragging': draggingId === item.queueId, 'is-drop-target': dropIndex === index && draggingId && draggingId !== item.queueId }"
        draggable="true"
        :data-queue-index="index"
        @dragstart="handleDragStart(item.queueId, $event)"
        @dragover="handleDragOver(index, $event)"
        @dragend="handleDragEnd"
        @drop="handleDrop($event)"
      >
        <span class="composer-queue-grip" :title="t('chat.composer.queue.dragHint')" aria-hidden="true">
          <i class="fa-solid fa-grip-vertical"></i>
        </span>
        <span class="composer-queue-text" :title="item.content || t('chat.composer.queue.attachmentOnly')">
          {{ item.content || t('chat.composer.queue.attachmentOnly') }}
        </span>
        <div class="composer-queue-actions">
          <button
            class="composer-queue-action composer-queue-action--label"
            type="button"
            data-testid="chat-queue-interject"
            :disabled="busy || item.priority > 0"
            :title="t('chat.composer.queue.interjectHint')"
            @click.stop="handleInterject(item)"
          >
            <i class="fa-solid fa-rotate-left composer-queue-action-icon" aria-hidden="true"></i>
            <span>{{ t('chat.composer.queue.interject') }}</span>
          </button>
          <button
            class="composer-queue-action"
            type="button"
            data-testid="chat-queue-edit"
            :disabled="busy"
            :title="t('chat.composer.queue.editHint')"
            :aria-label="t('chat.composer.queue.edit')"
            @click.stop="handleEdit(item)"
          >
            <i class="fa-solid fa-pen composer-queue-action-icon" aria-hidden="true"></i>
          </button>
          <button
            class="composer-queue-action composer-queue-action--danger"
            type="button"
            data-testid="chat-queue-remove"
            :disabled="busy"
            :title="t('chat.composer.queue.removeHint')"
            :aria-label="t('chat.composer.queue.remove')"
            @click.stop="handleRemove(item)"
          >
            <i class="fa-solid fa-trash-can composer-queue-action-icon" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { ElMessage } from 'element-plus';
import {
  chatQueueState,
  interjectChatQueueTurn,
  refreshChatQueue,
  reorderChatQueueTurns,
  withdrawChatQueueTurn,
  type QueuedTurn
} from '@/stores/chatQueueState';

const props = defineProps<{ sessionId?: string }>();
const emit = defineEmits<{ (event: 'edit', item: QueuedTurn): void }>();
const { t } = useI18n();

const items = computed(() => chatQueueState.items);
const busy = ref(false);
const draggingId = ref('');
const dropIndex = ref(-1);

watch(
  () => String(props.sessionId || '').trim(),
  (sessionId) => {
    if (sessionId) void refreshChatQueue(sessionId, { silent: true });
  },
  { immediate: true }
);

const resolveQueueId = (event: DragEvent): string =>
  String(event?.dataTransfer?.getData('application/x-wunder-queue') || '').trim();

const handleDragStart = (queueId: string, event: DragEvent) => {
  draggingId.value = queueId;
  dropIndex.value = -1;
  event?.dataTransfer?.setData('application/x-wunder-queue', queueId);
  if (event?.dataTransfer) event.dataTransfer.effectAllowed = 'move';
};

const handleDragOver = (index: number, event: DragEvent) => {
  if (!draggingId.value) return;
  event.preventDefault();
  event.stopPropagation();
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  dropIndex.value = index;
};

const handleDragEnd = () => {
  draggingId.value = '';
  dropIndex.value = -1;
};

const handleDrop = async (event: DragEvent) => {
  event.preventDefault();
  event.stopPropagation();
  const sourceId = resolveQueueId(event) || draggingId.value;
  const targetIndex = dropIndex.value;
  handleDragEnd();
  const sessionId = String(props.sessionId || '').trim();
  if (!sessionId || !sourceId || targetIndex < 0) return;
  const ordered = chatQueueState.items.map((item) => item.queueId);
  const from = ordered.indexOf(sourceId);
  if (from < 0 || from === targetIndex) return;
  ordered.splice(from, 1);
  ordered.splice(targetIndex, 0, sourceId);
  busy.value = true;
  try {
    const result = await reorderChatQueueTurns(sessionId, ordered);
    if (!result.ok) {
      ElMessage.warning(result.message || t('chat.composer.queue.reorderFailed'));
      await refreshChatQueue(sessionId, { silent: true });
    }
  } finally {
    busy.value = false;
  }
};

const handleInterject = async (item: QueuedTurn) => {
  const sessionId = String(props.sessionId || '').trim();
  if (!sessionId) return;
  busy.value = true;
  try {
    const result = await interjectChatQueueTurn(sessionId, item.queueId);
    if (!result.ok) {
      ElMessage.warning(result.message || t('chat.composer.queue.actionFailed'));
    }
  } finally {
    busy.value = false;
  }
};

const handleWithdraw = async (item: QueuedTurn): Promise<boolean> => {
  const sessionId = String(props.sessionId || '').trim();
  if (!sessionId) return false;
  busy.value = true;
  try {
    const result = await withdrawChatQueueTurn(sessionId, item.queueId);
    if (!result.ok) {
      ElMessage.warning(result.message || t('chat.composer.queue.actionFailed'));
      return false;
    }
    if (result.item) emit('edit', result.item);
    return true;
  } finally {
    busy.value = false;
  }
};

const handleEdit = (item: QueuedTurn) => {
  void handleWithdraw(item);
};

const handleRemove = (item: QueuedTurn) => {
  void handleWithdraw(item);
};
</script>

<style scoped>
.composer-queue {
  border: 1px solid var(--wunder-border-soft, #ececec);
  border-radius: 12px;
  background: #fff;
  padding: 6px 8px;
  margin-bottom: 8px;
  box-shadow: 0 1px 2px rgba(15, 23, 42, 0.04);
}

.composer-queue-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 4px 4px;
  font-size: 12px;
  color: var(--wunder-text-muted, #8a8f99);
}

.composer-queue-count {
  margin-left: auto;
}

.composer-queue-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 168px;
  overflow-y: auto;
}

.composer-queue-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 4px;
  border-radius: 8px;
  cursor: grab;
}

.composer-queue-item:hover {
  background: var(--wunder-bg-soft, #f7f8fa);
}

.composer-queue-item.is-dragging {
  opacity: 0.45;
}

.composer-queue-item.is-drop-target {
  box-shadow: inset 0 2px 0 var(--wunder-accent, #4f7cff);
}

.composer-queue-grip {
  color: var(--wunder-text-faint, #b6bac2);
  font-size: 12px;
  width: 14px;
  flex: 0 0 auto;
}

.composer-queue-text {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 13px;
  color: var(--wunder-text, #1f2329);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.composer-queue-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: 0 0 auto;
}

.composer-queue-action {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: 0;
  background: transparent;
  color: var(--wunder-text-secondary, #4b5563);
  font-size: 13px;
  line-height: 1;
  padding: 5px 7px;
  border-radius: 7px;
  cursor: pointer;
}

.composer-queue-action:hover:not(:disabled) {
  background: var(--wunder-bg-soft, #f1f2f4);
  color: var(--wunder-text, #1f2329);
}

.composer-queue-action--danger:hover:not(:disabled) {
  color: var(--wunder-danger, #d92d20);
}

.composer-queue-action:disabled {
  opacity: 0.5;
  cursor: default;
}

.composer-queue-action-icon {
  font-size: 12px;
}
</style>
