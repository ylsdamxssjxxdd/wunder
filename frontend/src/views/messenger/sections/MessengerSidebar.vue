<template>
  <aside
    ref="sidebarEl"
    class="messenger-sidebar"
    data-testid="messenger-sidebar"
    :aria-label="t('messenger.sidebar.workspaceGroup')"
  >
    <div class="messenger-sidebar-top">
      <button
        class="messenger-sidebar-new-task"
        type="button"
        :disabled="newTaskDisabled"
        :title="t('messenger.sidebar.newTask')"
        @click="emit('new-task')"
      >
        <i class="fa-solid fa-arrow-rotate-right" aria-hidden="true"></i>
        <span>{{ t('messenger.sidebar.newTask') }}</span>
      </button>
    </div>

    <div ref="workspaceRegionEl" class="messenger-sidebar-workspace-region" :style="workspaceRegionStyle">
      <div class="messenger-sidebar-group-title">{{ t('messenger.sidebar.workspaceGroup') }}</div>
      <div
        class="messenger-sidebar-workspace-row"
        :class="{ 'is-selected': !activeSessionId }"
        role="button"
        tabindex="0"
        :aria-label="workspaceName"
        @click="emit('select-workspace')"
        @keydown.enter.prevent="emit('select-workspace')"
      >
        <i class="fa-solid fa-folder-tree messenger-sidebar-workspace-icon" aria-hidden="true"></i>
        <span class="messenger-sidebar-workspace-name" :title="workspaceName">{{ workspaceName }}</span>
        <span class="messenger-sidebar-workspace-meta">{{ workspaceMetaLabel }}</span>
        <el-dropdown
          trigger="click"
          :teleported="true"
          placement="bottom-end"
          popper-class="mz-thread-dropdown"
          @command="handleWorkspaceCommand"
        >
          <button
            class="messenger-sidebar-workspace-menu"
            type="button"
            :title="t('common.more')"
            :aria-label="t('common.more')"
            @click.stop
          >
            <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
          </button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="new-thread">{{ t('chat.newSession') }}</el-dropdown-item>
              <el-dropdown-item command="rename-workspace">
                {{ t('messenger.sidebar.renameWorkspace') }}
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>

      <MessengerThreadTree
        :items="threads"
        :active-session-id="activeSessionId"
        :agent-id="agentIdForApi"
        @open="emit('open-thread', $event)"
        @detail="emit('thread-detail', $event)"
        @rename="emit('rename-thread', $event)"
        @archive="emit('archive-thread', $event)"
      />
    </div>

    <div
      class="messenger-sidebar-splitter"
      :class="{ 'is-dragging': isResizing }"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('messenger.sidebar.resize')"
      tabindex="0"
      @pointerdown.prevent="startResize"
      @dblclick.prevent="resetResize"
      @keydown.up.prevent="nudgeResize(-24)"
      @keydown.down.prevent="nudgeResize(24)"
    ></div>

    <!-- Lower region: cloud working-directory interaction area (B2). -->
    <div class="messenger-sidebar-files-region" :style="filesRegionStyle">
      <WorkspaceFilesPanel />
    </div>

    <div class="messenger-sidebar-footer">
      <button
        class="messenger-sidebar-settings"
        :class="{ 'is-active': settingsActive }"
        type="button"
        :title="t('messenger.sidebar.settings')"
        @click="emit('open-settings')"
      >
        <!-- 对齐桌面参考主页面：按钮上直接展示当前用户（头像 + 名称），点击呼出设置窗口。 -->
        <span class="messenger-sidebar-avatar" :style="currentUserAvatarStyle" aria-hidden="true">
          <img
            v-if="currentUserAvatarImageUrl"
            class="messenger-sidebar-avatar-image"
            :src="currentUserAvatarImageUrl"
            alt=""
          />
          <span v-else class="messenger-sidebar-avatar-text">{{ avatarLabel(currentUsername) }}</span>
        </span>
        <span class="messenger-sidebar-settings-name">{{ currentUsername || t('user.guest') }}</span>
      </button>
    </div>
  </aside>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import MessengerThreadTree from './MessengerThreadTree.vue';
import WorkspaceFilesPanel from '@/views/messenger/workspace/WorkspaceFilesPanel.vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { buildTaskList } from '@/views/messenger/taskList';
import { setWorkspaceDisplayNameOverride, workspaceDisplayNameOverride } from '@/views/messenger/workspaceDisplayName';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{
  'new-task': [];
  'select-workspace': [];
  'open-thread': [id: string];
  'thread-detail': [id: string];
  'rename-thread': [id: string];
  'archive-thread': [id: string];
  'open-settings': [];
}>();

const t = props.controller.t;
const chatStore = props.controller.chatStore;
const activeSessionId = computed(() => String(chatStore.activeSessionId || ''));
const agentIdForApi = props.controller.activeAgentIdForApi;
const newTaskDisabled = computed(
  () => Boolean(props.controller.creatingAgentSession?.value) || Boolean(props.controller.isMessengerInteractionBlocked?.value)
);
const settingsActive = computed(() => Boolean(props.controller.showChatSettingsView?.value));

// 设置按钮头像：复用与站点头部同一份用户头像投影，不新造接口。
const avatarLabel = props.controller.avatarLabel;
const currentUsername = props.controller.currentUsername;
const currentUserAvatarImageUrl = props.controller.currentUserAvatarImageUrl;
const currentUserAvatarStyle = props.controller.currentUserAvatarStyle;

// ---------------------------------------------------------------- workspace

const WORKSPACE_SPLIT_STORAGE_KEY = 'messenger:sidebar:split';
const MIN_SPLIT = 0.3;
const MAX_SPLIT = 0.8;
const DEFAULT_SPLIT = 0.6;

const workspaceName = computed(
  () => workspaceDisplayNameOverride.value || t('messenger.workspace.defaultName')
);

// 搜索栏已移除（对齐桌面）：线程树恢复为不过滤。
// 注意：`controller.keywordInput` 仍是 sessionHub 搜索（中间栏 sections 过滤）的唯一输入源，
// 其 store 字段、debounce watcher 与 controller 内过滤逻辑保留未删，等主智能体决定是否重新挂载入口。
const threads = computed(() =>
  buildTaskList(chatStore.sessions || [], String(agentIdForApi?.value || ''), t('chat.newSession'))
);
const workspaceMetaLabel = computed(() => {
  const total = threads.value.length;
  if (!total) return t('messenger.status.idle');
  return t('messenger.sidebar.threadCount', { count: total });
});

const handleWorkspaceCommand = (command: string) => {
  if (command === 'new-thread') {
    emit('new-task');
    return;
  }
  if (command === 'rename-workspace') {
    void renameWorkspace();
  }
};

const renameWorkspace = async () => {
  try {
    const { value } = await ElMessageBox.prompt(
      t('messenger.sidebar.renameWorkspacePrompt'),
      t('messenger.sidebar.renameWorkspace'),
      {
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel'),
        inputValue: workspaceName.value,
        inputPlaceholder: t('messenger.workspace.defaultName'),
        inputValidator: (input: string) => (String(input || '').trim() ? true : t('chat.history.renameRequired'))
      }
    );
    const next = String(value || '').trim();
    if (!next || next === workspaceName.value) return;
    setWorkspaceDisplayNameOverride(next);
    ElMessage.success(t('messenger.sidebar.renameWorkspaceSuccess'));
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
  }
};

// ------------------------------------------------------------------ resizer

const sidebarEl = ref<HTMLElement | null>(null);
const workspaceRegionEl = ref<HTMLElement | null>(null);
const splitRatio = ref(DEFAULT_SPLIT);
const isResizing = ref(false);

try {
  const stored = Number.parseFloat(String(localStorage.getItem(WORKSPACE_SPLIT_STORAGE_KEY) || ''));
  if (Number.isFinite(stored) && stored >= MIN_SPLIT && stored <= MAX_SPLIT) {
    splitRatio.value = stored;
  }
} catch {
  // Keep the default ratio when storage is unavailable.
}

const workspaceRegionStyle = computed(() => ({ flex: `${splitRatio.value} 1 0%` }));
const filesRegionStyle = computed(() => ({ flex: `${1 - splitRatio.value} 1 0%` }));

const persistSplit = () => {
  try {
    localStorage.setItem(WORKSPACE_SPLIT_STORAGE_KEY, splitRatio.value.toFixed(3));
  } catch {
    // Preference only.
  }
};

const handleResizeMove = (event: PointerEvent) => {
  if (!isResizing.value) return;
  const rect = sidebarEl.value?.getBoundingClientRect();
  if (!rect || rect.height <= 0) return;
  const offset = event.clientY - rect.top;
  const next = Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, offset / rect.height));
  splitRatio.value = next;
};

const stopResize = () => {
  if (!isResizing.value) return;
  isResizing.value = false;
  window.removeEventListener('pointermove', handleResizeMove);
  persistSplit();
};

const startResize = (event: PointerEvent) => {
  isResizing.value = true;
  (event.currentTarget as HTMLElement | null)?.focus?.();
  window.addEventListener('pointermove', handleResizeMove);
  window.addEventListener('pointerup', stopResize, { once: true });
};

const resetResize = () => {
  splitRatio.value = DEFAULT_SPLIT;
  persistSplit();
};

const nudgeResize = (delta: number) => {
  const rect = sidebarEl.value?.getBoundingClientRect();
  if (!rect || rect.height <= 0) return;
  splitRatio.value = Math.min(MAX_SPLIT, Math.max(MIN_SPLIT, splitRatio.value + delta / rect.height));
  persistSplit();
};

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', handleResizeMove);
  window.removeEventListener('pointerup', stopResize);
});
</script>
