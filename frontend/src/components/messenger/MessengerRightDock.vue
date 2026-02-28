<template>
  <aside
    class="messenger-right-dock"
    :class="{ 'messenger-right-dock--collapsed': collapsed }"
    @pointerdown.right.stop="swallowRightDockRightPointer"
  >
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
        <div
          class="messenger-right-section-title"
          @contextmenu.prevent.stop="openSandboxContainerMenu($event)"
          @mousedown.right.prevent.stop="openSandboxContainerMenu($event)"
        >
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
            :class="{ active: activeSessionId === item.id, 'is-main': item.isMain }"
            role="button"
            tabindex="0"
            @click="$emit('restore-session', item.id)"
            @keydown.enter.prevent="$emit('restore-session', item.id)"
            @keydown.space.prevent="$emit('restore-session', item.id)"
          >
            <div class="messenger-timeline-title-row">
              <div class="messenger-timeline-title">{{ item.title }}</div>
              <div class="messenger-timeline-actions" :class="{ 'messenger-timeline-actions--main': item.isMain }">
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

  <Teleport to="body">
    <div
      v-if="sandboxContextMenu.visible"
      ref="sandboxMenuRef"
      class="messenger-files-context-menu"
      :style="sandboxContextMenuStyle"
      @contextmenu.prevent
    >
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuOpenContainer">
        {{ t('messenger.files.menu.open') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuCopyId">
        {{ t('messenger.files.menu.copyId') }}
      </button>
      <button class="messenger-files-menu-btn" type="button" @click="handleSandboxMenuOpenSettings">
        {{ t('messenger.files.menu.settings') }}
      </button>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import { useI18n } from '@/i18n';
import { copyText } from '@/utils/clipboard';

type TimelineSessionItem = {
  id: string;
  title: string;
  preview: string;
  lastAt: unknown;
  isMain: boolean;
};

const props = defineProps<{
  collapsed: boolean;
  showAgentPanels: boolean;
  agentIdForApi: string;
  containerId: number;
  activeSessionId: string;
  sessionHistory: TimelineSessionItem[];
}>();

const emit = defineEmits<{
  (event: 'toggle-collapse'): void;
  (event: 'restore-session', sessionId: string): void;
  (event: 'set-main', sessionId: string): void;
  (event: 'delete-session', sessionId: string): void;
  (event: 'open-container', containerId: number): void;
  (event: 'open-container-settings', containerId: number): void;
}>();

const { t } = useI18n();
const sandboxMenuRef = ref<HTMLElement | null>(null);
const sandboxContextMenu = ref({
  visible: false,
  x: 0,
  y: 0
});
const sandboxContextMenuStyle = computed(() => ({
  left: `${sandboxContextMenu.value.x}px`,
  top: `${sandboxContextMenu.value.y}px`
}));

const closeSandboxContextMenu = () => {
  sandboxContextMenu.value.visible = false;
};

const swallowRightDockRightPointer = () => {
  // Stop bubbling right-click pointer events so overlay auto-hide logic does not collapse the dock.
};

const openSandboxContainerMenu = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('.workspace-context-menu, .workspace-dialog')) {
    return;
  }
  const targetRect = target?.getBoundingClientRect();
  const initialX =
    Number.isFinite(event.clientX) && event.clientX > 0
      ? event.clientX
      : Math.round((targetRect?.left || 0) + (targetRect?.width || 0) / 2);
  const initialY =
    Number.isFinite(event.clientY) && event.clientY > 0
      ? event.clientY
      : Math.round((targetRect?.top || 0) + (targetRect?.height || 0) / 2);
  sandboxContextMenu.value.visible = true;
  sandboxContextMenu.value.x = initialX;
  sandboxContextMenu.value.y = initialY;
  await nextTick();
  const menuRect = sandboxMenuRef.value?.getBoundingClientRect();
  if (!menuRect) return;
  const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
  const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
  sandboxContextMenu.value.x = Math.min(Math.max(8, sandboxContextMenu.value.x), maxLeft);
  sandboxContextMenu.value.y = Math.min(Math.max(8, sandboxContextMenu.value.y), maxTop);
};

const normalizeContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? 1), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const handleSandboxMenuOpenContainer = () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  emit('open-container', targetId);
};

const handleSandboxMenuCopyId = async () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  const copied = await copyText(String(targetId));
  if (copied) {
    ElMessage.success(t('messenger.files.copyIdSuccess', { id: targetId }));
  } else {
    ElMessage.warning(t('messenger.files.copyIdFailed'));
  }
};

const handleSandboxMenuOpenSettings = () => {
  const targetId = normalizeContainerId(props.containerId);
  closeSandboxContextMenu();
  emit('open-container-settings', targetId);
};

const closeSandboxMenuWhenOutside = (event: Event) => {
  if (!sandboxContextMenu.value.visible) {
    return;
  }
  const target = event.target as Node | null;
  if (!target || !sandboxMenuRef.value?.contains(target)) {
    closeSandboxContextMenu();
  }
};

onMounted(() => {
  window.addEventListener('pointerdown', closeSandboxMenuWhenOutside);
  window.addEventListener('resize', closeSandboxContextMenu);
  document.addEventListener('scroll', closeSandboxContextMenu, true);
});

onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', closeSandboxMenuWhenOutside);
  window.removeEventListener('resize', closeSandboxContextMenu);
  document.removeEventListener('scroll', closeSandboxContextMenu, true);
});

watch(
  () => props.collapsed,
  () => {
    closeSandboxContextMenu();
  }
);

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
