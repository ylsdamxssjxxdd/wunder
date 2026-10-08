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
  >
    <MessengerSiteHeader
      v-if="!isEmbeddedChatRoute"
      :controller="controller"
      @open-help="openHelpPanel"
      @open-settings="openSettingsPage"
      @open-profile="openProfilePage"
    />

    <div class="messenger-shell">
      <MessengerSidebar
        v-show="!isEmbeddedChatRoute"
        :controller="controller"
        @new-task="startNewSession"
        @select-workspace="selectWorkspace"
        @open-thread="openThread"
        @thread-detail="openTimelineSessionDetail"
        @rename-thread="renameTimelineSession"
        @archive-thread="archiveTimelineSession"
        @delete-thread="deleteThread"
        @open-settings="openSettingsPage"
      />
      <div
        v-if="sidebarDrawerOpen"
        class="messenger-sidebar-backdrop"
        aria-hidden="true"
        @click="sidebarDrawerOpen = false"
      ></div>

      <section class="messenger-main">
        <MessengerChatHeader
          v-if="showMessengerChatHeader"
          :controller="controller"
          @toggle-sidebar="sidebarDrawerOpen = !sidebarDrawerOpen"
          @new-thread="startNewSession"
          @thread-rename="renameTimelineSession"
          @thread-detail="openTimelineSessionDetail"
          @thread-archive="archiveTimelineSession"
          @thread-delete="deleteThread"
          @jump-top="jumpToMessageTop"
        />

        <div
          ref="messageListRef"
          class="messenger-chat-body"
          data-testid="messenger-message-list"
          :class="{ 'is-messages': hasActiveThread, 'is-welcome': !hasActiveThread }"
          @scroll.passive="handleMessageListScroll"
          @click="handleMessageContentClick"
        >
          <MessengerWelcomePane v-if="!hasActiveThread" :controller="controller" />
          <MessengerMessagePanel v-else :controller="controller" />
        </div>

        <!-- 用户轮次刻度：滚动容器之外（绝对定位），因此不会跟着内容滚走。 -->
        <MessengerTurnRuler v-if="hasActiveThread" :controller="controller" />

        <footer
          v-if="showChatComposerFooter"
          ref="chatFooterRef"
          class="messenger-chat-footer"
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
          <div v-if="resolvedMessageConversationKind === 'agent'" class="messenger-agent-composer messenger-composer-scope chat-shell">
            <ComposerStatusDock
              :loading="agentSessionLoading"
              :messages="agentRenderableMessages"
              :plan="activeAgentPlan"
              @remove="dismissActiveAgentPlan"
            />
            <MessageGoalBar :session-id="String(chatStore.activeSessionId || '')" />
            <InquiryPanel
              v-if="activeAgentInquiryPanel"
              :panel="activeAgentInquiryPanel.panel"
              @update:selected="handleAgentInquirySelection"
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

        <MessengerStatusBar v-if="!isEmbeddedChatRoute" :controller="controller" />
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
      v-model:timeline-detail-dialog-visible="timelineDetailDialogVisible"
      :timeline-detail-session-id="timelineDetailSessionId"
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
import { onUpdated as trackShellUpdate } from 'vue';
import { chatPerf } from '@/utils/chatPerf';
import { computed as vueComputed, ref as vueRef } from 'vue';
import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';
import { ElMessage, ElMessageBox } from 'element-plus';
import { showApiError } from '@/utils/apiError';
import MessengerMessagePanel from '@/views/messenger/sections/MessengerMessagePanel.vue';
import MessengerSidebar from '@/views/messenger/sections/MessengerSidebar.vue';
import MessengerSiteHeader from '@/views/messenger/sections/MessengerSiteHeader.vue';
import MessengerChatHeader from '@/views/messenger/sections/MessengerChatHeader.vue';
import MessengerWelcomePane from '@/views/messenger/sections/MessengerWelcomePane.vue';
import MessengerTurnRuler from '@/views/messenger/sections/MessengerTurnRuler.vue';
import MessengerStatusBar from '@/views/messenger/sections/MessengerStatusBar.vue';
import MessengerSettingsOverlay from '@/views/messenger/sections/MessengerSettingsOverlay.vue';
import MessageGoalBar from '@/components/chat/MessageGoalBar.vue';
import ComposerStatusDock from '@/components/chat/ComposerStatusDock.vue';

const controller = useMessengerViewController();
trackShellUpdate(() => chatPerf.count('chat_shell_render'));
const CompanionFloatingLayer = defineRecoverableAsyncComponent(
  () => import('@/components/companions/CompanionFloatingLayer.vue')
);
const companionLayerReady = vueRef(false);
const showCompanionLayer = vueComputed(() => !isEmbeddedChatRoute.value && companionLayerReady.value);
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
const isSidebarDrawer = vueComputed(() => Number(controller.viewportWidth?.value || 0) <= 1023);
const sidebarDrawerOpen = vueRef(false);
const closeDrawerOnNarrow = () => {
  if (isSidebarDrawer.value) sidebarDrawerOpen.value = false;
};

const openThread = (sessionId: string) => {
  closeDrawerOnNarrow();
  void controller.handleTimelineDialogActivateSession?.(sessionId);
};

const selectWorkspace = () => {
  closeDrawerOnNarrow();
  controller.openAgentDraftSession?.(controller.activeAgentId?.value || '');
};

const openSettingsPage = () => {
  closeDrawerOnNarrow();
  controller.openSettingsPage?.();
};

const openHelpPanel = () => {
  closeDrawerOnNarrow();
  controller.activateSettingsPanel?.('help-manual');
};

const closeSettingsPage = () => {
  controller.switchSection?.('messages');
};

const deleteThread = async (sessionId: string) => {
  const targetId = String(sessionId || '').trim();
  if (!targetId) return;
  try {
    await ElMessageBox.confirm(t('messenger.thread.deleteConfirm'), t('messenger.thread.delete'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    return;
  }
  try {
    await chatStore.deleteSession(targetId);
    ElMessage.success(t('messenger.thread.deleteSuccess'));
  } catch (error) {
    showApiError(error, t('messenger.thread.deleteFailed'));
  }
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
const chatFooterRef = controller.chatFooterRef;
const closeResourcePreview = controller.closeResourcePreview;
const composerApprovalMode = controller.composerApprovalMode;
const composerApprovalModeSyncing = controller.composerApprovalModeSyncing;
const currentContainerId = controller.currentContainerId;
const dismissActiveAgentPlan = controller.dismissActiveAgentPlan;
const drawioAgentId = controller.drawioAgentId;
const drawioContainerId = controller.drawioContainerId;
const drawioPath = controller.drawioPath;
const drawioUserId = controller.drawioUserId;
const drawioVisible = controller.drawioVisible;
const enterSelectedAgentConversation = controller.enterSelectedAgentConversation;
const handleAgentInquirySelection = controller.handleAgentInquirySelection;
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
const openProfilePage = controller.openProfilePage;
const openTimelineSessionDetail = controller.openTimelineSessionDetail;
const renameTimelineSession = controller.renameTimelineSession;
const resolveAgentRuntimeState = controller.resolveAgentRuntimeState;
const resolvedMessageConversationKind = controller.resolvedMessageConversationKind;
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
const showMessengerChatHeader = controller.showMessengerChatHeader;
const showScrollBottomButton = controller.showScrollBottomButton;
const startNewSession = controller.startNewSession;
const stopAgentMessage = controller.stopAgentMessage;
const timelineDetailDialogVisible = controller.timelineDetailDialogVisible;
const timelineDetailSessionId = controller.timelineDetailSessionId;
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
