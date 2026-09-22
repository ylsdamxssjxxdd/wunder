<template>
  <aside
    class="messenger-right-dock"
    :class="{
      'messenger-right-dock--collapsed': collapsed,
      'messenger-right-dock--edge-active': edgeActive
    }"
  >
    <div class="messenger-right-dock-toggle-hitbox" aria-hidden="true"></div>
    <button
      class="messenger-right-dock-toggle"
      type="button"
      :title="collapsed ? t('common.expand') : t('common.collapse')"
      :aria-label="collapsed ? t('common.expand') : t('common.collapse')"
      @click="$emit('toggle-collapse')"
    >
      <i class="fa-solid" :class="collapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
    </button>
    <div class="messenger-right-content messenger-right-content--stack">
      <MessengerTaskList :items="sessionHistory" :active-session-id="activeSessionId" :agent-id="agentIdForApi" :creating="creating"
        @create="emit('create-session')" @activate="(id) => emit('activate-session', id)"
        @detail="(id) => emit('open-session-detail', id)" @rename="(id) => emit('rename-session', id)" @archive="(id) => emit('archive-session', id)" />
      <div class="messenger-right-panel messenger-right-panel--sandbox">
        <div v-if="showAgentPanels" class="messenger-workspace-scope chat-shell">
          <WorkspacePanel
            ref="workspacePanelRef"
            :agent-id="agentIdForApi"
            :container-id="containerId"
            preserve-dock-layout
            :sidebar-visible="!collapsed"
            @quote-path="handleQuotePath"
            @open-workspace-binding="handleOpenWorkspaceBinding"
          />
        </div>
        <div v-else class="messenger-list-empty">{{ t('messenger.settings.agentOnly') }}</div>
      </div>

    </div>
  </aside>

</template>

<script setup lang="ts">
import { ref } from 'vue';

import { WorkspacePanel } from '@/components/messenger/lazyDockPanels';
import { isDesktopSafeModeEnabled } from '@/config/desktop';
import { useI18n } from '@/i18n';
import MessengerTaskList from './MessengerTaskList.vue';
import type { TaskListItem } from '@/views/messenger/taskList';

type WorkspacePanelViewRef = {
  refreshView?: (options?: { background?: boolean }) => Promise<boolean>;
};

defineProps<{
  collapsed: boolean;
  edgeActive: boolean;
  showAgentPanels: boolean;
  agentIdForApi: string;
  containerId: number;
  activeSessionId: string;
  sessionHistory: TaskListItem[];
  creating: boolean;
}>();

const emit = defineEmits<{
  (event: 'toggle-collapse'): void;
  (event: 'request-quote-path', payload: { paths: string[] }): void;
  (event: 'open-workspace-binding', payload: { containerId: number; currentPath: string }): void;
  (event: 'activate-session', sessionId: string): void;
  (event: 'create-session'): void;
  (event: 'open-session-detail' | 'rename-session' | 'archive-session', sessionId: string): void;
}>();

const { t } = useI18n();
const workspacePanelRef = ref<WorkspacePanelViewRef | null>(null);

const handleQuotePath = (payload: { paths?: string[] } = {}) => {
  const paths = Array.isArray(payload.paths)
    ? payload.paths.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  if (!paths.length) return;
  emit('request-quote-path', { paths });
};

const handleOpenWorkspaceBinding = (payload: { containerId: number; currentPath: string }) => {
  emit('open-workspace-binding', payload);
};


const refreshWorkspace = async (options: { background?: boolean } = {}) => {
  if (isDesktopSafeModeEnabled()) {
    return false;
  }
  if (!workspacePanelRef.value?.refreshView) {
    return false;
  }
  return workspacePanelRef.value.refreshView({
    background: options.background !== false
  });
};

defineExpose({
  refreshWorkspace
});
</script>
