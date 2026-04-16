<template>
  <el-dialog
    :model-value="visible"
    class="messenger-dialog messenger-timeline-dialog"
    :title="t('chat.history')"
    width="680px"
    top="8vh"
    destroy-on-close
    @update:model-value="(value) => emit('update:visible', value)"
  >
    <div v-if="!sessionHistory.length" class="messenger-list-empty">
      {{ t('messenger.empty.timeline') }}
    </div>
    <div v-else class="messenger-timeline messenger-timeline--dialog">
      <div
        v-for="item in sessionHistory"
        :key="item.id"
        class="messenger-timeline-item"
        :class="{ active: activeSessionId === item.id, 'is-main': item.isMain }"
        role="button"
        tabindex="0"
        @click="emit('activate-session', item.id)"
        @keydown.enter.prevent="emit('activate-session', item.id)"
        @keydown.space.prevent="emit('activate-session', item.id)"
      >
        <div class="messenger-timeline-title-row">
          <div class="messenger-timeline-title">{{ item.title }}</div>
          <div class="messenger-timeline-actions" :class="{ 'messenger-timeline-actions--main': item.isMain }">
            <span
              v-if="item.orchestrationLock?.active"
              class="messenger-timeline-main-badge messenger-timeline-main-badge--orchestration"
              :title="t('orchestration.chat.timelineBadge')"
            >
              <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
              <span>{{ t('orchestration.chat.timelineBadge') }}</span>
            </span>
            <span
              v-if="item.isMain"
              class="messenger-timeline-main-badge"
              :title="t('chat.history.main')"
            >
              <i class="fa-solid fa-thumbtack" aria-hidden="true"></i>
              <span>{{ t('chat.history.main') }}</span>
            </span>
            <button
              v-if="!item.isMain && !item.orchestrationLock?.active"
              class="messenger-timeline-rename-btn"
              type="button"
              :title="t('chat.history.rename')"
              :aria-label="t('chat.history.rename')"
              @click.stop="emit('rename-session', item.id)"
            >
              <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
            </button>
            <button
              class="messenger-timeline-detail-btn"
              type="button"
              :title="t('messenger.timeline.detail.open')"
              :aria-label="t('messenger.timeline.detail.open')"
              @click.stop="emit('open-session-detail', item.id)"
            >
              <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
            </button>
            <button
              v-if="!item.isMain && !item.orchestrationLock?.active"
              class="messenger-timeline-archive-btn"
              type="button"
              :title="t('chat.history.archive')"
              :aria-label="t('chat.history.archive')"
              @click.stop="emit('archive-session', item.id)"
            >
              <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
            </button>
          </div>
        </div>
        <div class="messenger-timeline-detail-row">
          <div class="messenger-timeline-detail">{{ item.preview || t('messenger.preview.empty') }}</div>
          <span class="messenger-timeline-time">{{ formatTime(item.lastAt) }}</span>
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

type TimelineSessionItem = {
  id: string;
  title: string;
  preview: string;
  lastAt: unknown;
  isMain: boolean;
  orchestrationLock?: {
    active?: boolean;
    role?: string;
    run_id?: string;
  } | null;
};

defineProps<{
  visible: boolean;
  activeSessionId: string;
  sessionHistory: TimelineSessionItem[];
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (event: 'activate-session', sessionId: string): void;
  (event: 'open-session-detail', sessionId: string): void;
  (event: 'archive-session', sessionId: string): void;
  (event: 'rename-session', sessionId: string): void;
}>();

const { t } = useI18n();

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
</script>
