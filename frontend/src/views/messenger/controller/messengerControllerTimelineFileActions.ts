// @ts-nocheck
// Timeline session operations and file container context-menu actions.
import type { MessengerControllerContext } from './messengerControllerContext';
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus';
import { createAgent as createAgentApi, deleteAgent as deleteAgentApi, listAgentUserRounds, listRunningAgents } from '@/api/agents';
import { fetchOrgUnits, updateProfile } from '@/api/auth';
import { listChannelBindings } from '@/api/channels';
import {
  getSession as getChatSessionApi,
  fetchSessionSystemPrompt,
  fetchRealtimeSystemPrompt
} from '@/api/chat';
import { fetchCronJobs } from '@/api/cron';
import { fetchDesktopSettings } from '@/api/desktop';
import { fetchExternalLinks } from '@/api/externalLinks';
import { downloadUserWorldFile } from '@/api/userWorld';
import {
  fetchUserSkillContent,
  uploadUserSkillZip
} from '@/api/userTools';
import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent, uploadWunderWorkspace } from '@/api/workspace';
import BeeroomWorkbench from '@/components/beeroom/BeeroomWorkbench.vue';
import OrchestrationWorkbench from '@/components/orchestration/OrchestrationWorkbench.vue';
import AbilityTooltipListItem from '@/components/common/AbilityTooltipListItem.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import AgentQuickCreateDialog from '@/components/messenger/AgentQuickCreateDialog.vue';
import {
  scheduleMessengerBootstrapBackgroundTasks,
  settleMessengerBootstrapTasks,
  splitMessengerBootstrapTasks
} from '@/views/messenger/bootstrap';
import { resolveAgentSelectionAfterRemoval } from '@/views/messenger/agentSelection';
import { createBeeroomRealtimeSync } from '@/views/messenger/beeroomRealtimeSync';
import { createMessageViewportRuntime, type MessageViewportRuntime } from '@/views/messenger/messageViewportRuntime';
import { useStableMixedConversationOrder } from '@/views/messenger/mixedConversationOrder';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import { createMessengerRealtimePulse } from '@/views/messenger/realtimePulse';
import { useMessengerHostWidth } from '@/views/messenger/hostWidth';
import { useMessengerInteractionBlocker } from '@/views/messenger/interactionBlocker';
import { useMessengerRightDockResize } from '@/views/messenger/rightDockResize';
import {
  settleAgentSessionBusyAfterRefresh,
  type SessionBusyRecoveryStatus
} from '@/views/messenger/chatRefreshRecovery';
import { resolveAgentConfiguredAbilityNames, resolveAgentOverviewAbilityCounts } from '@/views/messenger/agentOverviewAbilities';
import MessengerHivePlazaPanel from '@/components/messenger/MessengerHivePlazaPanel.vue';
import {
  filterPlazaItemsByKindAndKeyword,
  normalizePlazaBrowseKind,
  resolveRetainedSelectedPlazaItemId,
  type PlazaBrowseKind
} from '@/components/messenger/hivePlazaPanelState';
import MessengerMiddlePane from '@/views/messenger/sections/MessengerMiddlePane.vue';
import MessengerDialogsHost from '@/views/messenger/sections/MessengerDialogsHost.vue';
import MessengerToolsSection from '@/views/messenger/sections/MessengerToolsSection.vue';
import { useMiddlePaneOverlayPreview } from '@/views/messenger/middlePaneOverlayPreview';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';
import {
  InquiryPanel,
  MessageCompactionDivider,
  MessageFeedbackActions,
  MessageKnowledgeCitation,
  MessageSubagentPanel,
  MessageThinking,
  PlanPanel,
  ToolApprovalComposer,
  WorkspacePanel
} from '@/views/messenger/lazyMessageBlocks';
import {
  MessengerFileContainerMenu,
  MessengerGroupDock,
  MessengerRightDock,
  MessengerTimelineDialog
} from '@/views/messenger/lazyShell';
import {
  AgentCronPanel,
  AgentMemoryPanel,
  AgentRuntimeRecordsPanel,
  AgentSettingsPanel,
  ArchivedThreadManager,
  DesktopContainerManagerPanel,
  DesktopSystemSettingsPanel,
  GlobeAppPanel,
  MessengerHelpManualPanel,
  MessengerLocalFileSearchPanel,
  MessengerSettingsPanel,
  MessengerWorldComposer,
  preloadAgentSettingsPanels,
  preloadMessengerSettingsPanels,
  UserChannelSettingsPanel,
  UserPromptSettingsPanel
} from '@/views/messenger/lazyPanels';
import {
  resolveFileContainerLifecycleText,
  resolveFileWorkspaceEmptyText
} from '@/views/messenger/fileWorkspacePresentation';
import { isDesktopModeEnabled } from '@/config/desktop';
import { getRuntimeConfig } from '@/config/runtime';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useBeeroomStore, type BeeroomGroup } from '@/stores/beeroom';
import { useChatStore } from '@/stores/chat';
import { usePlazaStore } from '@/stores/plaza';
import { useThemeStore } from '@/stores/theme';
import {
  useSessionHubStore,
  resolveSectionFromRoute,
  type MessengerSection
} from '@/stores/sessionHub';
import { useUserWorldStore } from '@/stores/userWorld';
import { hydrateExternalMarkdownImages, renderMarkdown } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { showApiError } from '@/utils/apiError';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { buildDeclaredDependencyPayload, resolveAgentDependencyStatus } from '@/utils/agentDependencyStatus';
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import WorkerCardImportWaitingOverlay from '@/components/agent/WorkerCardImportWaitingOverlay.vue';
import { downloadWorkerCardBundle, parseWorkerCardText, workerCardToAgentPayload } from '@/utils/workerCard';
import { redirectToLoginAfterLogout } from '@/utils/authNavigation';
import { copyText } from '@/utils/clipboard';
import { confirmWithFallback } from '@/utils/confirm';
import {
  buildAssistantDisplayContent,
  resolveAssistantFailureNotice
} from '@/utils/assistantFailureNotice';
import {
  hasAssistantWaitingForCurrentOutput,
  normalizeAssistantMessageRuntimeState,
  resolveAssistantMessageRuntimeState
} from '@/utils/assistantMessageRuntime';
import {
  hasActiveSubagentsAfterLatestUser,
  hasRunningAssistantMessage,
  hasStreamingAssistantMessage
} from '@/utils/chatSessionRuntime';
import { hasActiveSubagentItems } from '@/utils/subagentRuntime';
import { buildAssistantMessageStatsEntries } from '@/utils/messageStats';
import {
  isCompactionOnlyWorkflowItems,
  isCompactionRunningFromWorkflowItems,
  resolveLatestCompactionSnapshot
} from '@/utils/chatCompactionWorkflow';
import {
  isAudioRecordingSupported,
  startAudioRecording,
  type AudioRecordingResult,
  type AudioRecordingSession
} from '@/utils/audioRecorder';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import {
  extractPromptToolingPreview,
  type PromptToolingPreviewItem
} from '@/utils/promptToolingPreview';
import { collectAbilityDetails, collectAbilityGroupDetails, collectAbilityNames } from '@/utils/toolSummary';
import {
  buildWorkspacePublicPath,
  normalizeWorkspaceOwnerId,
  resolveMarkdownWorkspacePath
} from '@/utils/messageWorkspacePath';
import {
  isImagePath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import {
  clearWorkspaceLoadingLabelTimer,
  getFilenameFromHeaders,
  normalizeWorkspaceImageBlob,
  resetWorkspaceImageCardState,
  saveObjectUrlAsFile,
  scheduleWorkspaceLoadingLabel
} from '@/utils/workspaceResourceCards';
import {
  extractWorkspaceRefreshPaths,
  isWorkspacePathAffected
} from '@/utils/workspaceRefresh';
import { emitWorkspaceRefresh, onAgentRuntimeRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { emitUserToolsUpdated, onUserToolsUpdated } from '@/utils/userToolsEvents';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import {
  invalidateAllUserToolsCaches,
  invalidateUserSkillsCache,
  invalidateUserToolsCatalogCache,
  invalidateUserToolsSummaryCache,
  loadUserSkillsCache,
  loadUserToolsCatalogCache,
  loadUserToolsSummaryCache
} from '@/utils/userToolsCache';
import {
  normalizeAvatarColor,
  normalizeAvatarIcon,
  normalizeThemePalette,
  type ThemePalette,
  type UserAppearancePreferences
} from '@/utils/userPreferences';
import {
  PROFILE_AVATAR_COLORS,
  PROFILE_AVATAR_IMAGE_KEYS,
  PROFILE_AVATAR_IMAGE_MAP,
  PROFILE_AVATAR_OPTION_KEYS
} from '@/utils/avatarCatalog';
import {
  classifyWorldHistoryMessage,
  normalizeWorldHistoryText,
  resolveWorldHistoryIcon
} from '@/views/messenger/worldHistory';
import { loadUserAppearance, saveUserAppearance } from '@/views/messenger/userAppearanceSync';
import {
  defaultMessengerOrderPreferences,
  loadMessengerOrderPreferences,
  saveMessengerOrderPreferences,
  type MessengerOrderPreferences
} from '@/views/messenger/messengerOrderSync';
import { clearBeeroomMissionCanvasState } from '@/components/beeroom/beeroomMissionCanvasStateCache';
import { clearBeeroomMissionChatState } from '@/components/beeroom/beeroomMissionChatStateCache';
import { clearCachedDispatchPreview } from '@/components/beeroom/useBeeroomDispatchSessionPreview';
import {
  buildWorldVoicePayloadContent,
  formatWorldVoiceDuration,
  isWorldVoiceContentType,
  parseWorldVoicePayload
} from '@/views/messenger/worldVoice';
import {
  buildAgentApprovalOptions,
  normalizeAgentApprovalMode,
  useComposerApprovalMode,
  type AgentApprovalMode
} from '@/views/messenger/composerApprovalMode';
import {
  buildUnitTreeFromFlat,
  buildUnitTreeRows,
  collectUnitNodeIds,
  flattenUnitNodes,
  normalizeUnitNode,
  normalizeUnitShortLabel,
  normalizeUnitText,
  resolveUnitIdKey,
  resolveUnitTreeRowStyle
} from '@/views/messenger/orgUnits';
import {
  AGENT_CONTAINER_IDS,
  AGENT_MAIN_READ_AT_STORAGE_PREFIX,
  AGENT_MAIN_UNREAD_STORAGE_PREFIX,
  AGENT_TOOL_OVERRIDE_NONE,
  DEFAULT_AGENT_KEY,
  DISMISSED_AGENT_STORAGE_PREFIX,
  MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY,
  MESSENGER_SEND_KEY_STORAGE_KEY,
  MESSENGER_UI_FONT_SIZE_STORAGE_KEY,
  USER_CONTAINER_ID,
  USER_WORLD_UPLOAD_BASE,
  UNIT_UNGROUPED_ID,
  WORLD_COMPOSER_HEIGHT_STORAGE_KEY,
  WORLD_EMOJI_CATALOG,
  WORLD_QUICK_EMOJI_STORAGE_KEY,
  WORLD_UPLOAD_SIZE_LIMIT,
  sectionRouteMap,
  type AgentFileContainer,
  type AgentLocalCommand,
  type AgentOverviewCard,
  type AgentRuntimeState,
  type DesktopBridge,
  type DesktopInstallResult,
  type DesktopScreenshotResult,
  type DesktopUpdateState,
  type FileContainerMenuTarget,
  type MessengerPerfTrace,
  type MessengerSendKeyMode,
  type MixedConversation,
  type ToolEntry,
  type UnitTreeNode,
  type UnitTreeRow,
  type WorldComposerViewRef,
  type WorldHistoryCategory,
  type WorldHistoryRecord
} from '@/views/messenger/model';

type HelperAppOfflineItem = {
  key: string;
  title: string;
  description: string;
  icon: string;
};

type HelperAppExternalItem = {
  linkId: string;
  title: string;
  description: string;
  url: string;
  icon: string;
  sortOrder: number;
};

type WorldContainerPickerEntry = {
  path: string;
  name: string;
  type: 'dir' | 'file';
};

type TooltipLike = { updatePopper?: () => void; popperRef?: { update?: () => void } };

type AgentSettingMode = 'agent' | 'cron' | 'channel' | 'runtime' | 'memory' | 'archived';

type SettingsPanelMode =
  | 'general'
  | 'profile'
  | 'prompts'
  | 'help-manual'
  | 'desktop-models'
  | 'desktop-lan';

type RightDockSkillItem = {
  name: string;
  description: string;
  enabled: boolean;
};

type RightDockSkillCatalogItem = {
  name: string;
  description: string;
  path: string;
  source: string;
  builtin: boolean;
  readonly: boolean;
};

type WorldVoiceRecordingRuntime = {
  session: AudioRecordingSession;
  startedAt: number;
  timerId: number | null;
  conversationId: string;
};

type AgentVoiceRecordingRuntime = {
  session: AudioRecordingSession;
  startedAt: number;
  timerId: number | null;
  draftIdentity: string;
};

type WorldVoicePlaybackRuntime = {
  audio: HTMLAudioElement;
  objectUrlCache: Map<string, string>;
  currentMessageKey: string;
  currentResourceKey: string;
};

type WorkspaceResourceCachePayload = { objectUrl: string; filename: string };

type WorkspaceResourceCacheEntry = {
  objectUrl?: string;
  filename?: string;
  promise?: Promise<WorkspaceResourceCachePayload>;
};

type AttachmentResourceState = {
  objectUrl?: string;
  filename?: string;
  error?: boolean;
  loading?: boolean;
};

type MessengerPageWaitingState = {
  title: string;
  targetName: string;
  phaseLabel: string;
  summaryLabel: string;
  progress: number;
};

type AgentMainSessionEntry = {
  agentId: string;
  sessionId: string;
  lastAt: number;
};

type AgentRenderableMessage = {
  key: string;
  sourceIndex: number;
  message: Record<string, unknown>;
};

type WorldRenderableMessage = {
  key: string;
  sourceIndex: number;
  domId: string;
  message: Record<string, unknown>;
};

type AgentInquiryPanelRoute = { label: string; description?: string };

type AgentInquiryPanelData = { question?: string; routes?: AgentInquiryPanelRoute[]; status?: string };

type ActiveAgentInquiryPanel = { message: Record<string, unknown>; panel: AgentInquiryPanelData };

type WorkspaceResolvedResource = ReturnType<typeof parseWorkspaceResourceUrl> & {
  requestUserId: string | null;
  requestAgentId: string | null;
  requestContainerId: number | null;
  allowed: boolean;
};

type WorldScreenshotCaptureOption = {
  hideWindow?: boolean;
  region?: boolean;
};

type StartNewSessionOutcome = 'noop' | 'already_current' | 'opened';

export function installMessengerControllerTimelineFileActions(ctx: MessengerControllerContext): void {
  ctx.restoreTimelineSession = async (sessionId: string) => {
      if (!sessionId)
          return;
      await ctx.openAgentSession(sessionId);
  };

  ctx.openTimelineSessionDetail = (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      ctx.timelineDetailSessionId.value = targetId;
      ctx.timelineDetailDialogVisible.value = true;
  };

  watch(() => ctx.timelineDetailDialogVisible.value, (visible) => {
      if (!visible) {
          ctx.timelineDetailSessionId.value = '';
      }
  });

  ctx.setTimelineSessionMain = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return false;
      const targetSession = ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
      const targetLock = targetSession && typeof targetSession === 'object' && !Array.isArray(targetSession)
          ? (targetSession.orchestration_lock as Record<string, unknown> | null | undefined)
          : null;
      if (targetLock?.active === true) {
          ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
          return false;
      }
      if (targetSession?.is_main) {
          return true;
      }
      try {
          await ctx.chatStore.setMainSession(targetId);
          return true;
      }
      catch (error) {
          showApiError(error, ctx.t('chat.history.setMainFailed'));
          return false;
      }
  };

  ctx.handleTimelineDialogActivateSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const targetSession = ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
      const targetLock = targetSession && typeof targetSession === 'object' && !Array.isArray(targetSession)
          ? (targetSession.orchestration_lock as Record<string, unknown> | null | undefined)
          : null;
      if (targetLock?.active === true) {
          ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
          return;
      }
      ctx.timelineDialogVisible.value = false;
      await ctx.setTimelineSessionMain(targetId);
      await ctx.restoreTimelineSession(targetId);
  };

  ctx.renameTimelineSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const session = ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
      const currentTitle = String(session?.title || ctx.t('chat.newSession')).trim() || ctx.t('chat.newSession');
      try {
          const { value } = await ElMessageBox.prompt(ctx.t('chat.history.renamePrompt'), ctx.t('chat.history.rename'), {
              confirmButtonText: ctx.t('common.confirm'),
              cancelButtonText: ctx.t('common.cancel'),
              inputValue: currentTitle,
              inputPlaceholder: ctx.t('chat.history.renamePlaceholder'),
              inputValidator: (inputValue: string) => String(inputValue || '').trim() ? true : ctx.t('chat.history.renameRequired')
          });
          const nextTitle = String(value || '').trim();
          if (!nextTitle || nextTitle === currentTitle) {
              return;
          }
          await ctx.chatStore.renameSession(targetId, nextTitle);
          ElMessage.success(ctx.t('chat.history.renameSuccess'));
      }
      catch (error) {
          if (error === 'cancel' || error === 'close') {
              return;
          }
          showApiError(error, ctx.t('chat.history.renameFailed'));
      }
  };

  ctx.archiveTimelineSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const confirmed = await confirmWithFallback(ctx.t('chat.history.confirmArchive'), ctx.t('chat.history.confirmTitle'), {
          type: 'warning',
          confirmButtonText: ctx.t('common.confirm'),
          cancelButtonText: ctx.t('common.cancel')
      });
      if (!confirmed) {
          return;
      }
      try {
          await ctx.chatStore.archiveSession(targetId);
          ctx.timelinePreviewMap.value.delete(targetId);
          ctx.triggerRealtimePulseRefresh?.('archive-session');
          ElMessage.success(ctx.t('chat.history.archiveSuccess'));
      }
      catch (error) {
          showApiError(error, ctx.t('chat.history.archiveFailed'));
      }
  };

  ctx.handleArchivedSessionRemoved = (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      ctx.timelinePreviewMap.value.delete(targetId);
      if (ctx.timelineDetailSessionId.value === targetId) {
          ctx.timelineDetailDialogVisible.value = false;
      }
      ctx.triggerRealtimePulseRefresh?.('archived-session-removed');
  };

  ctx.closeFileContainerMenu = () => {
      ctx.fileContainerContextMenu.value.visible = false;
  };

  ctx.openDesktopContainerSettings = async (containerId?: number) => {
      if (ctx.desktopMode.value) {
          if (ctx.sessionHub.activeSection !== 'files') {
              ctx.switchSection('files');
              await nextTick();
          }
          const fallbackContainerId = ctx.fileScope.value === 'user' ? USER_CONTAINER_ID : ctx.selectedFileContainerId.value;
          const normalized = Math.min(10, Math.max(0, Number.parseInt(String(containerId ?? fallbackContainerId), 10) || 0));
          ctx.desktopContainerManagerPanelRef.value?.openManager(normalized);
          return;
      }
      ctx.settingsPanelMode.value = 'general';
      ctx.sessionHub.setSection('more');
      ctx.sessionHub.setKeyword('');
      const nextQuery = {
          ...ctx.route.query,
          section: 'more'
      } as Record<string, any>;
      delete nextQuery.session_id;
      delete nextQuery.agent_id;
      delete nextQuery.entry;
      delete nextQuery.conversation_id;
      delete nextQuery.panel;
      ctx.router.push({ path: `${ctx.basePrefix.value}/settings`, query: nextQuery }).catch(() => undefined);
  };

  ctx.openFileContainerMenu = async (event: MouseEvent, scope: 'user' | 'agent', containerId: number) => {
      const currentTarget = event.currentTarget as HTMLElement | null;
      const targetElement = (event.target as HTMLElement | null) || currentTarget;
      const fallbackRect = (currentTarget || targetElement)?.getBoundingClientRect();
      const baseX = Number.isFinite(event.clientX) && event.clientX > 0
          ? event.clientX
          : Math.round((fallbackRect?.left || 0) + (fallbackRect?.width || 0) / 2);
      const baseY = Number.isFinite(event.clientY) && event.clientY > 0
          ? event.clientY
          : Math.round((fallbackRect?.top || 0) + (fallbackRect?.height || 0) / 2);
      const normalizedId = scope === 'user'
          ? USER_CONTAINER_ID
          : Math.min(10, Math.max(1, Number.parseInt(String(containerId || 1), 10) || 1));
      if (scope === 'agent' && !ctx.agentFileContainers.value.some((item) => item.id === normalizedId)) {
          ElMessage.warning(ctx.t('messenger.files.agentContainerEmpty'));
          return;
      }
      ctx.selectContainer(scope === 'user' ? 'user' : normalizedId);
      ctx.fileContainerContextMenu.value.target = { scope, id: normalizedId };
      ctx.fileContainerContextMenu.value.visible = true;
      ctx.fileContainerContextMenu.value.x = Math.max(8, Math.round(baseX + 2));
      ctx.fileContainerContextMenu.value.y = Math.max(8, Math.round(baseY + 2));
      await nextTick();
      const menuRect = ctx.fileContainerMenuViewRef.value?.getMenuElement()?.getBoundingClientRect();
      if (!menuRect)
          return;
      const maxLeft = Math.max(8, window.innerWidth - menuRect.width - 8);
      const maxTop = Math.max(8, window.innerHeight - menuRect.height - 8);
      ctx.fileContainerContextMenu.value.x = Math.min(Math.max(8, ctx.fileContainerContextMenu.value.x), maxLeft);
      ctx.fileContainerContextMenu.value.y = Math.min(Math.max(8, ctx.fileContainerContextMenu.value.y), maxTop);
  };

  ctx.handleFileContainerMenuOpen = () => {
      const target = ctx.fileContainerContextMenu.value.target;
      ctx.closeFileContainerMenu();
      if (!target)
          return;
      ctx.selectContainer(target.scope === 'user' ? 'user' : target.id);
  };
}
