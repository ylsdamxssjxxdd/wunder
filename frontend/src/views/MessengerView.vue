<template>
  <div
    ref="messengerRootRef"
    class="messenger-view"
    data-testid="messenger-view"
    :class="{
      'messenger-view--embedded-chat': isEmbeddedChatRoute,
      'messenger-view--drawer-open': sidebarDrawerOpen,
      'messenger-view--action-blocked': isMessengerInteractionBlocked
    }"
    :style="sidebarWidthStyle"
  >
    <div class="messenger-shell" :inert="showChatSettingsView">
      <MessengerSidebar
        v-show="!isEmbeddedChatRoute && !sidebarCollapsed"
        :controller="controller"
        @new-task="startNewSession"
        @select-workspace="selectWorkspace"
        @open-thread="openThread"
        @open-local-thread="openLocalThread"
        @thread-trajectory="openSessionTrajectory"
        @rename-thread="renameTimelineSession"
        @archive-thread="archiveTimelineSession"
        @archive-threads="archiveTimelineSessions"
        @open-settings="openSettingsPage"
      />
      <!-- 桌面端侧栏拖宽线：悬停/拖拽显色，双击复位；窄视口抽屉模式不显示。 -->
      <div
        v-if="!isEmbeddedChatRoute && !isSidebarDrawer && !sidebarCollapsed"
        class="messenger-sidebar-resizer"
        :class="{ 'is-dragging': sidebarResizing }"
        role="separator"
        aria-orientation="vertical"
        :aria-label="t('messenger.sidebar.resizeWidth')"
        @pointerdown.prevent="startSidebarResize"
        @dblclick.prevent="resetSidebarWidth"
      >
        <button
          class="messenger-sidebar-collapse-btn"
          type="button"
          :title="t('messenger.sidebar.toggle')"
          :aria-label="t('messenger.sidebar.toggle')"
          @pointerdown.stop
          @click="collapseSidebar"
        >
          <i class="fa-solid fa-chevron-left" aria-hidden="true"></i>
        </button>
      </div>
      <div
        v-if="sidebarDrawerOpen"
        class="messenger-sidebar-backdrop"
        aria-hidden="true"
        @click="sidebarDrawerOpen = false"
      ></div>

      <section
        class="messenger-main"
        :class="{ 'is-split-drop-target': singleDropActive, 'has-chat-head': Boolean(chatHeaderTitle) }"
        @dragover="handleSingleSplitDragOver"
        @dragleave="singleDropActive = false"
        @drop="handleSingleSplitDrop"
      >
        <!-- 壳体不再有顶部条：窄视口抽屉入口 / 折叠后的展开入口与「回到顶部」都挂在主区浮层上。 -->
        <button
          v-if="(isSidebarDrawer && !sidebarDrawerOpen) || sidebarCollapsed"
          class="messenger-sidebar-open"
          type="button"
          :title="t('messenger.sidebar.toggle')"
          :aria-label="t('messenger.sidebar.toggle')"
          @click="expandSidebar"
        >
          <i class="fa-solid fa-bars" aria-hidden="true"></i>
        </button>
        <button
          v-if="showScrollTopButton"
          class="messenger-scroll-top-btn"
          type="button"
          :title="t('chat.toTop')"
          :aria-label="t('chat.toTop')"
          @click="jumpToMessageTop"
        >
          <i class="fa-solid fa-angles-up" aria-hidden="true"></i>
        </button>
        <MessengerChatSplitView
          v-if="splitCount !== 1"
          v-model:count="splitCount"
          :controller="controller"
          :active-session-id="String(chatStore.activeSessionId || '')"
          :secondary-session-id="splitSecondaryId"
        />
        <template v-else>

        <!-- 单视图标题行：分屏的每个窗格都有自己的标题，只剩一个视图时聊天区
             却是一段没有名字的记录流。标题取当前线程，运行中点亮状态点。 -->
        <header
          v-if="chatHeaderTitle"
          class="messenger-chat-head"
          :data-session-id="String(chatStore.activeSessionId || '')"
        >
          <span class="messenger-chat-head-state" :class="{ 'is-running': chatHeaderBusy }" aria-hidden="true"></span>
          <span class="messenger-chat-head-title" :title="chatHeaderTitle">{{ chatHeaderTitle }}</span>
        </header>

        <!-- 刻度与滚动视口同排：`.messenger-chat-lane` 给刻度留出独立通道，
             刻度不再悬浮在滚动容器右缘的滚动条上。 -->
        <div class="messenger-chat-lane">
          <div
            ref="messageListRef"
            class="messenger-chat-body"
            data-testid="messenger-message-list"
            :class="{
              'is-messages': hasActiveThread || localThreadActive,
              'is-agent': resolvedMessageConversationKind === 'agent' || localThreadActive
            }"
            @scroll.passive="handleMessageListScroll"
            @click="handleMessageContentClick"
          >
            <RemoteThreadsPanel
              v-if="localThreadActive"
              ref="remoteThreadPanelRef"
              :controller="controller"
              :target="localThreadTarget"
              :workspace-id="localThreadWorkspaceId"
              :open-thread-id="localThreadId"
            />
            <MessengerMessagePanel v-else-if="hasActiveThread" :controller="controller" />
          </div>

          <MessengerTurnRuler v-if="hasActiveThread && !localThreadActive" :controller="controller" />
        </div>

        <footer
          v-if="showChatComposerFooter || localThreadActive"
          ref="chatFooterRef"
          class="messenger-chat-footer"
          :class="{ 'is-threaded': hasActiveThread || localThreadActive }"
        >
          <button
            v-if="showScrollBottomButton"
            class="messenger-scroll-bottom-btn"
            type="button"
            :title="t('chat.toBottom')"
            :aria-label="t('chat.toBottom')"
            @click="jumpToMessageBottom"
          >
            <i class="fa-solid fa-angles-down" aria-hidden="true"></i>
          </button>
          <div v-if="localThreadActive" class="messenger-agent-composer messenger-composer-scope chat-shell">
            <ChatComposer
              :loading="localThreadSending"
              :send-key="messengerSendKey"
              :context-messages="localThreadMessages"
              approval-mode="full_auto"
              :approval-mode-editable="false"
              :voice-supported="false"
              @send="sendLocalThreadMessage"
              @stop="stopLocalThreadMessage"
            />
          </div>
          <div v-else-if="resolvedMessageConversationKind === 'agent'" class="messenger-agent-composer messenger-composer-scope chat-shell">
            <ComposerStatusDock
              :session-id="String(chatStore.activeSessionId || '')"
              :loading="agentSessionLoading"
              :plan="activeAgentPlan"
            />
            <MessageGoalBar :session-id="String(chatStore.activeSessionId || '')" />
            <InquiryPanel
              v-if="activeAgentInquiryPanel"
              :panel="activeAgentInquiryPanel.panel"
              @update:selected="handleAgentInquirySelection"
              @submit="submitAgentInquirySelection"
            />
            <ToolApprovalComposer
              v-if="activeSessionApproval"
              :approval="activeSessionApproval"
              :busy="approvalResponding"
              @decide="handleSessionApprovalDecision"
            />
            <ChatComposer
              v-else
              ref="agentComposerViewRef"
              :class="{ 'messenger-agent-composer-lock': activeSessionGoalLocked }"
              :loading="agentSessionLoading"
              :send-key="messengerSendKey"
              :draft-key="agentComposerDraftKey"
              :inquiry-active="Boolean(activeAgentInquiryPanel)"
              :inquiry-selection="agentInquirySelection"
              :preset-questions="activeAgentPresetQuestions"
              :voice-supported="agentVoiceSupported"
              :voice-recording="agentVoiceRecording"
              :voice-duration-ms="agentVoiceDurationMs"
              :voice-transcribing="agentVoiceTranscribing"
              :approval-mode="composerApprovalMode"
              :approval-mode-editable="showAgentComposerApprovalSelector"
              :approval-mode-syncing="composerApprovalModeSyncing"
              :model-name="agentHeaderModelDisplayName"
              :apply-model="applyComposerModelSelection"
              :reasoning-effort="String(activeSessionRecord?.reasoning_effort || activeSessionRecord?.reasoningEffort || 'default')"
              :context-messages="agentRenderableContextMessages"
              :workspace-agent-id="activeAgentIdForApi"
              :workspace-container-id="currentContainerId"
              @send="sendAgentMessage"
              @stop="stopAgentMessage"
              @new-thread="startNewSession"
              @open-thread="openThread"
              @toggle-voice-record="toggleAgentVoiceRecord"
              @update:approval-mode="updateComposerApprovalMode"
            />
          </div>
        </footer>
        </template>
      </section>
    </div>

    <MessengerSettingsOverlay
      v-if="showChatSettingsView && !isEmbeddedChatRoute"
      :controller="controller"
      @close="closeSettingsPage"
      @open-agent-chat="enterSelectedAgentConversation"
    />

    <div
      v-if="isMessengerInteractionBlocked"
      class="messenger-action-blocker"
      role="status"
      aria-live="polite"
      aria-busy="true"
    >
      <div class="messenger-action-blocker-card">
        <span class="messenger-action-blocker-spinner" aria-hidden="true"></span>
        <div class="messenger-action-blocker-title">{{ messengerInteractionBlockingLabel }}</div>
        <div class="messenger-action-blocker-subtitle">{{ t('common.loading') }}</div>
      </div>
    </div>

    <MessengerDialogsHost
      v-model:agent-prompt-preview-visible="agentPromptPreviewVisible"
      :agent-prompt-preview-loading="agentPromptPreviewLoading"
      :active-agent-prompt-preview-html="activeAgentPromptPreviewHtml"
      :agent-prompt-preview-memory-mode="agentPromptPreviewMemoryMode"
      :agent-prompt-preview-tooling-mode="agentPromptPreviewToolingMode"
      :agent-prompt-preview-tooling-content="agentPromptPreviewToolingContent"
      :agent-prompt-preview-tooling-items="agentPromptPreviewToolingItems"
      :resource-preview-visible="resourcePreviewVisible"
      :resource-preview-loading="resourcePreviewLoading"
      :resource-preview-url="resourcePreviewUrl"
      :resource-preview-title="resourcePreviewTitle"
      :resource-preview-meta="resourcePreviewMeta"
      :resource-preview-hint="resourcePreviewHint"
      :resource-preview-content="resourcePreviewContent"
      :resource-preview-workspace-path="resourcePreviewWorkspacePath"
      :resource-preview-kind="resourcePreviewKind"
      :handle-resource-preview-download="handleResourcePreviewDownload"
      :close-resource-preview="closeResourcePreview"
      v-model:only-office-visible="onlyOfficeVisible"
      :only-office-path="onlyOfficePath"
      :only-office-user-id="onlyOfficeUserId"
      :only-office-sidebar-visible="false"
      :only-office-agent-id="onlyOfficeAgentId"
      :only-office-container-id="onlyOfficeContainerId"
      v-model:drawio-visible="drawioVisible"
      :drawio-path="drawioPath"
      :drawio-user-id="drawioUserId"
      :drawio-sidebar-visible="false"
      :drawio-agent-id="drawioAgentId"
      :drawio-container-id="drawioContainerId"
      :handle-workspace-editor-saved="handleWorkspaceEditorSaved"
      :handle-workspace-editor-fallback="handleWorkspaceEditorFallback"
    />

    <HoneycombWaitingOverlay
      :visible="Boolean(messengerPageWaitingState)"
      :title="messengerPageWaitingState?.title || t('messenger.waiting.title')"
      :target-name="messengerPageWaitingState?.targetName || ''"
      :phase-label="messengerPageWaitingState?.phaseLabel || ''"
      :summary-label="messengerPageWaitingState?.summaryLabel || ''"
      :progress="messengerPageWaitingState?.progress ?? 0"
    />
    <CompanionFloatingLayer
      v-if="showCompanionLayer"
      :acknowledged-done-agent-id="companionAcknowledgedDoneAgentId"
      :acknowledged-done-at="companionAcknowledgedDoneAt"
      :resolve-agent-runtime-state="resolveAgentRuntimeState"
      :open-agent-by-id="openAgentById"
    />
  </div>
</template>

<script setup lang="ts">
defineOptions({
  name: 'MessengerView'
});

import { useMessengerViewController } from '@/views/messenger/useMessengerViewController';
import { useInterlinkNodeTarget } from '@/views/messenger/interlink/interlinkNodeTarget';
import { onUpdated as trackShellUpdate } from 'vue';
import { chatPerf } from '@/utils/chatPerf';
import { computed as vueComputed, ref as vueRef } from 'vue';
import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';
import MessengerMessagePanel from '@/views/messenger/sections/MessengerMessagePanel.vue';
import MessengerChatSplitView from '@/views/messenger/sections/MessengerChatSplitView.vue';
import { CHAT_THREAD_DRAG_TYPE, type ChatSplitCount } from '@/views/messenger/chatSplitLayout';
import MessengerSidebar from '@/views/messenger/sections/MessengerSidebar.vue';
import MessengerTurnRuler from '@/views/messenger/sections/MessengerTurnRuler.vue';
import MessengerSettingsOverlay from '@/views/messenger/sections/MessengerSettingsOverlay.vue';
import MessageGoalBar from '@/components/chat/MessageGoalBar.vue';
import ComposerStatusDock from '@/components/chat/ComposerStatusDock.vue';
import RemoteThreadsPanel from '@/views/messenger/interlink/RemoteThreadsPanel.vue';
import type { RemoteThreadItem } from '@/views/messenger/interlink/remoteThreadModel';

const controller = useMessengerViewController();
trackShellUpdate(() => chatPerf.count('chat_shell_render'));
const CompanionFloatingLayer = defineRecoverableAsyncComponent(
  () => import('@/components/companions/CompanionFloatingLayer.vue')
);
const companionLayerReady = vueRef(false);
// The trajectory route is kept alive while navigating from chat. Its
// Teleport-to-body layer must be disabled there, otherwise the cached chat
// instance and the layout host can both render the same desktop companion.
const showCompanionLayer = vueComputed(() => {
  const path = String(controller.route?.path || '');
  const isTrajectoryRoute = /\/(?:app|demo)\/trajectory(?:\/|$)/.test(path);
  return !isEmbeddedChatRoute.value && !isTrajectoryRoute && companionLayerReady.value;
});
const scheduleCompanionLayerLoad = () => {
  if (companionLayerReady.value || typeof window === 'undefined') return;
  const load = () => {
    companionLayerReady.value = true;
  };
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(load, { timeout: 1800 });
    return;
  }
  window.setTimeout(load, 240);
};
scheduleCompanionLayerLoad();
const companionAcknowledgedDoneAgentId = vueRef('');
const companionAcknowledgedDoneAt = vueRef(0);

const t = controller.t;
const chatStore = controller.chatStore;

// ------------------------------------------------------------- shell state

const hasActiveThread = vueComputed(() => Boolean(String(chatStore.activeSessionId || '')));
const splitCount = vueRef<ChatSplitCount>(1);
const splitSecondaryId = vueRef('');
const singleDropActive = vueRef(false);
const hasThreadDrag = (event: DragEvent): boolean =>
  Array.from(event.dataTransfer?.types || []).includes(CHAT_THREAD_DRAG_TYPE);
const handleSingleSplitDragOver = (event: DragEvent) => {
  if (splitCount.value !== 1 || !hasThreadDrag(event) || localThreadActive.value) return;
  event.preventDefault();
  singleDropActive.value = true;
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
};
const handleSingleSplitDrop = (event: DragEvent) => {
  singleDropActive.value = false;
  if (splitCount.value !== 1) return;
  const id = String(event.dataTransfer?.getData(CHAT_THREAD_DRAG_TYPE) || '').trim();
  if (!id) return;
  event.preventDefault();
  splitSecondaryId.value = id;
  splitCount.value = 2;
};
const localThreadTarget = vueRef('');
const localThreadWorkspaceId = vueRef('');
const localThreadId = vueRef('');
const localThreadActive = vueComputed(() => Boolean(localThreadTarget.value && localThreadId.value));
const remoteThreadPanelRef = vueRef<{
  messages?: RemoteThreadItem[];
  busy?: boolean;
  sendMessage?: (content: string, attachments?: unknown[]) => Promise<boolean>;
  stopMessage?: () => Promise<void>;
} | null>(null);
const localThreadSending = vueComputed(() => Boolean(remoteThreadPanelRef.value?.busy));
const localThreadMessages = vueComputed(() => remoteThreadPanelRef.value?.messages || []);
const sendLocalThreadMessage = async (payload: { content?: string; attachments?: unknown[] }) => {
  const content = String(payload?.content || '').trim();
  if (content || (Array.isArray(payload?.attachments) && payload.attachments.length)) {
    await remoteThreadPanelRef.value?.sendMessage?.(content, payload.attachments);
  }
};
const stopLocalThreadMessage = async () => {
  await remoteThreadPanelRef.value?.stopMessage?.();
};
const isSidebarDrawer = vueComputed(() => Number(controller.viewportWidth?.value || 0) <= 1023);
const sidebarDrawerOpen = vueRef(false);
const closeDrawerOnNarrow = () => {
  if (isSidebarDrawer.value) sidebarDrawerOpen.value = false;
};

// 桌面端侧栏宽度拖拽与折叠：宽度通过覆盖 --mz-sidebar-width 生效，状态持久化在 localStorage。
const SIDEBAR_WIDTH_STORAGE_KEY = 'messenger:sidebar:width';
const SIDEBAR_COLLAPSED_STORAGE_KEY = 'messenger:sidebar:collapsed';
const SIDEBAR_WIDTH_MIN = 200;
const SIDEBAR_WIDTH_MAX = 360;
const SIDEBAR_WIDTH_DEFAULT = 240;

const readStoredSidebarWidth = () => {
  if (typeof window === 'undefined') return SIDEBAR_WIDTH_DEFAULT;
  const raw = Number(window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY));
  if (!Number.isFinite(raw) || raw <= 0) return SIDEBAR_WIDTH_DEFAULT;
  return Math.min(SIDEBAR_WIDTH_MAX, Math.max(SIDEBAR_WIDTH_MIN, raw));
};

const readStoredSidebarCollapsed = () => {
  if (typeof window === 'undefined') return false;
  return window.localStorage.getItem(SIDEBAR_COLLAPSED_STORAGE_KEY) === '1';
};

const sidebarWidth = vueRef(readStoredSidebarWidth());
const sidebarCollapsed = vueRef(readStoredSidebarCollapsed());
const sidebarResizing = vueRef(false);

const sidebarWidthStyle = vueComputed(() => ({
  '--mz-sidebar-width': `${sidebarWidth.value}px`
}));

let sidebarResizePointerId = -1;
const stopSidebarResize = (event: PointerEvent) => {
  if (event.pointerId !== sidebarResizePointerId) return;
  sidebarResizePointerId = -1;
  sidebarResizing.value = false;
  window.removeEventListener('pointermove', onSidebarResizeMove);
  window.removeEventListener('pointerup', stopSidebarResize);
  window.removeEventListener('pointercancel', stopSidebarResize);
};

const onSidebarResizeMove = (event: PointerEvent) => {
  if (event.pointerId !== sidebarResizePointerId) return;
  const rootEl = messengerRootRef.value;
  if (!rootEl) return;
  const left = rootEl.getBoundingClientRect().left;
  sidebarWidth.value = Math.min(
    SIDEBAR_WIDTH_MAX,
    Math.max(SIDEBAR_WIDTH_MIN, Math.round(event.clientX - left))
  );
};

const startSidebarResize = (event: PointerEvent) => {
  if (isSidebarDrawer.value || sidebarCollapsed.value) return;
  sidebarResizePointerId = event.pointerId;
  sidebarResizing.value = true;
  window.addEventListener('pointermove', onSidebarResizeMove);
  window.addEventListener('pointerup', stopSidebarResize);
  window.addEventListener('pointercancel', stopSidebarResize);
  onSidebarResizeMove(event);
};

const persistSidebarWidth = () => {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(sidebarWidth.value));
};

const resetSidebarWidth = () => {
  sidebarWidth.value = SIDEBAR_WIDTH_DEFAULT;
  persistSidebarWidth();
};

const collapseSidebar = () => {
  sidebarCollapsed.value = true;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, '1');
  }
};

const expandSidebar = () => {
  sidebarCollapsed.value = false;
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_STORAGE_KEY, '0');
  }
  // 窄视口下同一颗按钮承担「打开抽屉」：折叠态与抽屉态共用展开入口。
  if (isSidebarDrawer.value) {
    sidebarDrawerOpen.value = true;
  }
};

// 线程驱动的目录切换：打开云端线程时把 `?node=` 归位云端（§6.3）。
const { backToCloud: backToCloudWorkspace } = useInterlinkNodeTarget();

const openThread = (sessionId: string) => {
  closeDrawerOnNarrow();
  localThreadTarget.value = '';
  localThreadWorkspaceId.value = '';
  localThreadId.value = '';
  // 侧栏线程都是云端线程：打开线程＝工作目录跟随切回云端（§6.3 线程驱动切换）。
  backToCloudWorkspace();
  void controller.openSessionById?.(sessionId);
};

const openLocalThread = (payload: { target: string; workspaceId: string; threadId: string }) => {
  closeDrawerOnNarrow();
  localThreadTarget.value = String(payload.target || '');
  localThreadWorkspaceId.value = String(payload.workspaceId || '');
  localThreadId.value = String(payload.threadId || '');
};

const selectWorkspace = () => {
  closeDrawerOnNarrow();
  localThreadTarget.value = '';
  localThreadWorkspaceId.value = '';
  localThreadId.value = '';
  controller.openAgentDraftSession?.(controller.activeAgentId?.value || '');
};

const openSettingsPage = () => {
  closeDrawerOnNarrow();
  controller.openSettingsPage?.();
};

const closeSettingsPage = () => {
  controller.switchSection?.('messages');
};

// -------------------------------------------------------------- controller

const activeAgentIdForApi = controller.activeAgentIdForApi;
const activeAgentInquiryPanel = controller.activeAgentInquiryPanel;
const activeAgentPlan = controller.activeAgentPlan;
const activeAgentPresetQuestions = controller.activeAgentPresetQuestions;
const activeAgentPromptPreviewHtml = controller.activeAgentPromptPreviewHtml;
const activeSessionApproval = controller.activeSessionApproval;
const activeSessionGoalLocked = controller.activeSessionGoalLocked;
const activeSessionRecord = controller.activeSessionRecord;
const agentComposerDraftKey = controller.agentComposerDraftKey;
const agentComposerViewRef = controller.agentComposerViewRef;
const agentHeaderModelDisplayName = controller.agentHeaderModelDisplayName;
const agentInquirySelection = controller.agentInquirySelection;
const agentPromptPreviewLoading = controller.agentPromptPreviewLoading;
const agentPromptPreviewMemoryMode = controller.agentPromptPreviewMemoryMode;
const agentPromptPreviewToolingContent = controller.agentPromptPreviewToolingContent;
const agentPromptPreviewToolingItems = controller.agentPromptPreviewToolingItems;
const agentPromptPreviewToolingMode = controller.agentPromptPreviewToolingMode;
const agentPromptPreviewVisible = controller.agentPromptPreviewVisible;
const agentRenderableContextMessages = controller.agentRenderableContextMessages;
const agentRenderableMessages = controller.agentRenderableMessages;
const agentSessionLoading = controller.agentSessionLoading;
const approvalResponding = controller.approvalResponding;
const applyComposerModelSelection = controller.applyComposerModelSelection;
const archiveTimelineSession = controller.archiveTimelineSession;
const archiveTimelineSessions = controller.archiveTimelineSessions;
const chatFooterRef = controller.chatFooterRef;
const closeResourcePreview = controller.closeResourcePreview;
const composerApprovalMode = controller.composerApprovalMode;
const composerApprovalModeSyncing = controller.composerApprovalModeSyncing;
const currentContainerId = controller.currentContainerId;
const drawioAgentId = controller.drawioAgentId;
const drawioContainerId = controller.drawioContainerId;
const drawioPath = controller.drawioPath;
const drawioUserId = controller.drawioUserId;
const drawioVisible = controller.drawioVisible;
const enterSelectedAgentConversation = controller.enterSelectedAgentConversation;
const handleAgentInquirySelection = controller.handleAgentInquirySelection;
const submitAgentInquirySelection = controller.submitAgentInquirySelection;
const handleMessageContentClick = controller.handleMessageContentClick;
const handleMessageListScroll = controller.handleMessageListScroll;
const handleResourcePreviewDownload = controller.handleResourcePreviewDownload;
const handleSessionApprovalDecision = controller.handleSessionApprovalDecision;
const handleWorkspaceEditorFallback = controller.handleWorkspaceEditorFallback;
const handleWorkspaceEditorSaved = controller.handleWorkspaceEditorSaved;
const isEmbeddedChatRoute = controller.isEmbeddedChatRoute;
const isMessengerInteractionBlocked = controller.isMessengerInteractionBlocked;
const jumpToMessageBottom = controller.jumpToMessageBottom;
const jumpToMessageTop = controller.jumpToMessageTop;
const messageListRef = controller.messageListRef;
const messengerInteractionBlockingLabel = controller.messengerInteractionBlockingLabel;
const messengerPageWaitingState = controller.messengerPageWaitingState;
const messengerRootRef = controller.messengerRootRef;
const messengerSendKey = controller.messengerSendKey;
const onlyOfficeAgentId = controller.onlyOfficeAgentId;
const onlyOfficeContainerId = controller.onlyOfficeContainerId;
const onlyOfficePath = controller.onlyOfficePath;
const onlyOfficeUserId = controller.onlyOfficeUserId;
const onlyOfficeVisible = controller.onlyOfficeVisible;
const openAgentById = controller.openAgentById;
const openSessionTrajectory = controller.openSessionTrajectory;
const renameTimelineSession = controller.renameTimelineSession;
const resolveAgentRuntimeState = controller.resolveAgentRuntimeState;
const resolvedMessageConversationKind = controller.resolvedMessageConversationKind;

// 单视图标题行的数据：标题取当前会话，状态点与分屏窗格用同一套运行判定。
const activeChatSession = vueComputed<Record<string, unknown> | null>(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) {
    return null;
  }
  const sessions = Array.isArray(chatStore.sessions) ? chatStore.sessions : [];
  return (sessions.find((item: any) => String(item?.id || '').trim() === sessionId) || null) as
    | Record<string, unknown>
    | null;
});
const chatHeaderTitle = vueComputed(() => {
  if (localThreadActive.value || resolvedMessageConversationKind.value !== 'agent') {
    return '';
  }
  if (!hasActiveThread.value) {
    return '';
  }
  const title = String(activeChatSession.value?.title || '').trim();
  return title || t('chat.newSession');
});
const chatHeaderBusy = vueComputed(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) {
    return false;
  }
  const status = String(activeChatSession.value?.runtime_status || '');
  return Boolean(chatStore.loadingBySession?.[sessionId])
    || ['accepted', 'running', 'queued', 'waiting_input', 'waiting_approval'].includes(status);
});
const resourcePreviewContent = controller.resourcePreviewContent;
const resourcePreviewHint = controller.resourcePreviewHint;
const resourcePreviewKind = controller.resourcePreviewKind;
const resourcePreviewLoading = controller.resourcePreviewLoading;
const resourcePreviewMeta = controller.resourcePreviewMeta;
const resourcePreviewTitle = controller.resourcePreviewTitle;
const resourcePreviewUrl = controller.resourcePreviewUrl;
const resourcePreviewVisible = controller.resourcePreviewVisible;
const resourcePreviewWorkspacePath = controller.resourcePreviewWorkspacePath;
const sendAgentMessage = controller.sendAgentMessage;
const showAgentComposerApprovalSelector = controller.showAgentComposerApprovalSelector;
const showChatComposerFooter = controller.showChatComposerFooter;
const showChatSettingsView = controller.showChatSettingsView;
const showScrollBottomButton = controller.showScrollBottomButton;
const showScrollTopButton = controller.showScrollTopButton;
const startNewSession = controller.startNewSession;
const stopAgentMessage = controller.stopAgentMessage;
const toggleAgentVoiceRecord = controller.toggleAgentVoiceRecord;
const updateComposerApprovalMode = controller.updateComposerApprovalMode;
const agentVoiceDurationMs = controller.agentVoiceDurationMs;
const agentVoiceRecording = controller.agentVoiceRecording;
const agentVoiceSupported = controller.agentVoiceSupported;
const agentVoiceTranscribing = controller.agentVoiceTranscribing;
const ChatComposer = controller.ChatComposer;
const InquiryPanel = controller.InquiryPanel;
const MessengerDialogsHost = controller.MessengerDialogsHost;
const HoneycombWaitingOverlay = controller.HoneycombWaitingOverlay;
const ToolApprovalComposer = controller.ToolApprovalComposer;
</script>
