<template>
  <header class="messenger-chat-header">
    <button
      class="messenger-site-action messenger-sidebar-toggle"
      type="button"
      :title="t('messenger.sidebar.toggle')"
      :aria-label="t('messenger.sidebar.toggle')"
      @click="emit('toggle-sidebar')"
    >
      <i class="fa-solid fa-bars" aria-hidden="true"></i>
    </button>
    <div class="messenger-chat-heading">
      <span class="messenger-chat-workspace">
        <i class="fa-solid fa-folder-tree" aria-hidden="true"></i>
        {{ workspaceName }}
      </span>
      <span class="messenger-chat-heading-divider" aria-hidden="true">/</span>
      <h1 class="messenger-chat-title">{{ title }}</h1>
    </div>
    <div class="messenger-chat-header-actions">
      <button
        class="messenger-header-btn"
        type="button"
        :disabled="Boolean(creatingAgentSession?.value) || Boolean(interactionBlocked?.value)"
        :title="t('chat.newSession')"
        :aria-label="t('chat.newSession')"
        @click="emit('new-thread')"
      >
        <i class="fa-solid fa-plus" aria-hidden="true"></i>
        <span>{{ t('chat.newSession') }}</span>
      </button>
      <el-dropdown
        v-if="hasThread"
        trigger="click"
        :teleported="true"
        placement="bottom-end"
        popper-class="mz-thread-dropdown"
        @command="handleCommand"
      >
        <button
          class="messenger-header-btn"
          type="button"
          :title="t('common.more')"
          :aria-label="t('common.more')"
        >
          <i class="fa-solid fa-ellipsis" aria-hidden="true"></i>
        </button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item command="rename">{{ t('messenger.tasks.rename') }}</el-dropdown-item>
            <el-dropdown-item command="detail">{{ t('messenger.timeline.detail.open') }}</el-dropdown-item>
            <el-dropdown-item command="archive">{{ t('messenger.tasks.archive') }}</el-dropdown-item>
            <el-dropdown-item command="delete" divided>{{ t('messenger.thread.delete') }}</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
      <button
        v-if="showScrollTop"
        class="messenger-header-btn"
        type="button"
        :title="t('chat.toTop')"
        :aria-label="t('chat.toTop')"
        @click="emit('jump-top')"
      >
        <i class="fa-solid fa-angles-up" aria-hidden="true"></i>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { workspaceDisplayNameOverride } from '@/views/messenger/workspaceDisplayName';

const props = defineProps<{ controller: MessengerControllerContext }>();
const emit = defineEmits<{
  'toggle-sidebar': [];
  'new-thread': [];
  'thread-rename': [id: string];
  'thread-detail': [id: string];
  'thread-archive': [id: string];
  'thread-delete': [id: string];
  'jump-top': [];
}>();

const t = props.controller.t;
const title = props.controller.chatPanelTitle;
const creatingAgentSession = props.controller.creatingAgentSession;
const interactionBlocked = props.controller.isMessengerInteractionBlocked;
const showScrollTop = props.controller.showScrollTopButton;
const activeSessionId = computed(() => String(props.controller.chatStore?.activeSessionId || ''));
const hasThread = computed(() => Boolean(activeSessionId.value));
const workspaceName = computed(
  () => workspaceDisplayNameOverride.value || t('messenger.workspace.defaultName')
);

const handleCommand = (command: string) => {
  const id = activeSessionId.value;
  if (!id) return;
  if (command === 'rename') emit('thread-rename', id);
  else if (command === 'detail') emit('thread-detail', id);
  else if (command === 'archive') emit('thread-archive', id);
  else if (command === 'delete') emit('thread-delete', id);
};
</script>
