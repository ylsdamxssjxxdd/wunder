<template>
  <aside class="messenger-right-dock" :class="{ 'messenger-right-dock--collapsed': collapsed }">
    <button
      class="messenger-right-dock-toggle"
      type="button"
      :title="collapsed ? t('common.expand') : t('common.collapse')"
      :aria-label="collapsed ? t('common.expand') : t('common.collapse')"
      @click="$emit('toggle-collapse')"
    >
      <i class="fa-solid" :class="collapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
    </button>
    <div v-if="!collapsed" class="messenger-right-content messenger-right-content--stack">
      <div class="messenger-right-panel messenger-right-panel--sandbox">
        <div class="messenger-right-section-title">
          <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
          <span>{{ t('messenger.right.sandbox') }}</span>
        </div>
        <div v-if="showAgentPanels" class="messenger-workspace-scope chat-shell">
          <WorkspacePanel :agent-id="agentIdForApi" :container-id="containerId" />
        </div>
        <div v-else class="messenger-list-empty">{{ t('messenger.settings.agentOnly') }}</div>
      </div>

      <div class="messenger-right-panel messenger-right-panel--timeline">
        <div class="messenger-right-section-title">
          <i class="fa-solid fa-timeline" aria-hidden="true"></i>
          <span>{{ t('messenger.right.timeline') }}</span>
        </div>
        <div v-if="!sessionHistory.length" class="messenger-list-empty">{{ t('messenger.empty.timeline') }}</div>
        <div v-else class="messenger-timeline">
          <div
            v-for="item in sessionHistory"
            :key="item.id"
            class="messenger-timeline-item"
            :class="{ active: activeSessionId === item.id }"
            role="button"
            tabindex="0"
            @click="$emit('restore-session', item.id)"
            @keydown.enter.prevent="$emit('restore-session', item.id)"
            @keydown.space.prevent="$emit('restore-session', item.id)"
          >
            <div class="messenger-timeline-title-row">
              <div class="messenger-timeline-title">{{ item.title }}</div>
              <span v-if="item.isMain" class="messenger-kind-tag">{{ t('chat.history.main') }}</span>
              <div class="messenger-timeline-actions">
                <button
                  class="messenger-timeline-main-btn"
                  :class="{ active: item.isMain }"
                  type="button"
                  :title="item.isMain ? t('chat.history.main') : t('chat.history.setMain')"
                  :aria-label="item.isMain ? t('chat.history.main') : t('chat.history.setMain')"
                  :disabled="item.isMain"
                  @click.stop="$emit('set-main', item.id)"
                >
                  <i class="fa-solid fa-thumbtack" aria-hidden="true"></i>
                </button>
                <button
                  class="messenger-timeline-delete-btn"
                  type="button"
                  :title="t('chat.history.delete')"
                  :aria-label="t('chat.history.delete')"
                  @click.stop="$emit('delete-session', item.id)"
                >
                  <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
                </button>
              </div>
            </div>
            <div class="messenger-timeline-detail-row">
              <div class="messenger-timeline-detail">{{ item.preview || t('messenger.preview.empty') }}</div>
              <span class="messenger-timeline-time">{{ formatTime(item.lastAt) }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </aside>
</template>

<script setup lang="ts">
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { useI18n } from '@/i18n';

type TimelineSessionItem = {
  id: string;
  title: string;
  preview: string;
  lastAt: unknown;
  isMain: boolean;
};

defineProps<{
  collapsed: boolean;
  showAgentPanels: boolean;
  agentIdForApi: string;
  containerId: number;
  activeSessionId: string;
  sessionHistory: TimelineSessionItem[];
}>();

defineEmits<{
  (event: 'toggle-collapse'): void;
  (event: 'restore-session', sessionId: string): void;
  (event: 'set-main', sessionId: string): void;
  (event: 'delete-session', sessionId: string): void;
}>();

const { t } = useI18n();

const normalizeTimestamp = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  const date = new Date(value as string | number);
  if (!Number.isNaN(date.getTime())) return date.getTime();
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) return 0;
  return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
};

const formatTime = (value: unknown): string => {
  const ts = normalizeTimestamp(value);
  if (!ts) return '';
  const date = new Date(ts);
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  return `${month}-${day} ${hour}:${minute}`;
};
</script>
