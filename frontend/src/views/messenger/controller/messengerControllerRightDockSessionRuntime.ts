// @ts-nocheck
// File lifecycle text, right dock visibility, right-panel session history, and timeline preview caching.
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

export function installMessengerControllerRightDockSessionRuntime(ctx: MessengerControllerContext): void {
  ctx.fileContainerLifecycleText = computed(() => resolveFileContainerLifecycleText({
      t: ctx.t
  }));

  ctx.activeWorldGroupId = computed(() => {
      if (!ctx.isWorldConversationActive.value)
          return '';
      const conversationId = String(ctx.activeWorldConversationId.value || '').trim();
      if (!conversationId)
          return '';
      const conversation = ctx.userWorldStore.conversations.find((item) => String(item?.conversation_id || '').trim() === conversationId);
      if (String(conversation?.conversation_type || '').trim().toLowerCase() !== 'group') {
          return '';
      }
      const fallbackGroup = ctx.userWorldStore.groups.find((item) => String(item?.conversation_id || '').trim() === conversationId);
      return (String(conversation?.group_id || '').trim() ||
          String(fallbackGroup?.group_id || '').trim() ||
          String(ctx.selectedGroup.value?.group_id || '').trim());
  });

  ctx.shouldHideAgentSettingsRightDock = computed(() => {
      if (ctx.sessionHub.activeSection !== 'agents' || ctx.showAgentGridOverview.value) {
          return false;
      }
      return ctx.navigationPaneCollapsed.value || ctx.isMiddlePaneOverlay.value || ctx.viewportWidth.value <= 1820;
  });

  ctx.showAgentRightDock = computed(() => {
      if (ctx.sessionHub.activeSection === 'agents') {
          return !ctx.showAgentGridOverview.value && !ctx.shouldHideAgentSettingsRightDock.value;
      }
      return ctx.sessionHub.activeSection === 'messages' && ctx.isAgentConversationActive.value;
  });

  ctx.showGroupRightDock = computed(() => ctx.sessionHub.activeSection === 'messages' &&
      ctx.isWorldConversationActive.value &&
      Boolean(ctx.activeWorldGroupId.value));

  ctx.showRightDock = computed(() => ctx.showAgentRightDock.value || ctx.showGroupRightDock.value);

  ctx.showRightAgentPanels = computed(() => ctx.showAgentRightDock.value);

  watch(() => ctx.showAgentRightDock.value, (visible) => {
      if (!visible) {
          return;
      }
      ctx.warmMessengerUserToolsData({
          skills: true,
          summary: true
      });
  }, { immediate: true });

  ctx.RIGHT_DOCK_EDGE_HOVER_THRESHOLD = 84;

  ctx.rightDockEdgeHoverFrame = null;

  ctx.pendingRightDockPointerX = null;

  ctx.cachedMessengerRootRight = 0;

  ctx.cachedMessengerRootWidth = 0;

  ctx.lastMessengerLayoutDebugSignature = '';

  watch(() => ctx.showRightDock.value, (visible) => {
      if (!visible) {
          ctx.pendingRightDockPointerX = null;
          ctx.setRightDockEdgeHover(false);
          return;
      }
      ctx.refreshMessengerRootBounds();
  });

  watch(() => [ctx.viewportWidth.value, ctx.navigationPaneCollapsed.value, ctx.rightDockCollapsed.value, ctx.showMiddlePane.value] as const, () => {
      ctx.refreshMessengerRootBounds();
  });

  ctx.rightPanelAgentId = computed(() => {
      if (!ctx.showRightAgentPanels.value)
          return '';
      return ctx.normalizeAgentId(ctx.settingsAgentId.value || ctx.activeAgentId.value);
  });

  ctx.rightPanelAgentIdForApi = computed(() => {
      const value = ctx.normalizeAgentId(ctx.rightPanelAgentId.value);
      return value === DEFAULT_AGENT_KEY ? '' : value;
  });

  ctx.rightPanelContainerId = computed(() => {
      const value = ctx.normalizeAgentId(ctx.rightPanelAgentId.value);
      const source = ctx.agentMap.value.get(value) || null;
      const parsed = Number.parseInt(String((source as Record<string, unknown> | null)?.sandbox_container_id ?? 1), 10);
      if (!Number.isFinite(parsed))
          return 1;
      return Math.min(10, Math.max(1, parsed));
  });

  ctx.normalizeConversationPreviewText = (value: unknown): string => String(value || '')
      .trim()
      .replace(/\s+/g, ' ')
      .slice(0, 120);

  ctx.extractLatestUserPreview = (messages: unknown[]): string => {
      for (let index = messages.length - 1; index >= 0; index -= 1) {
          const item = (messages[index] || {}) as Record<string, unknown>;
          if (String(item.role || '').trim() !== 'user')
              continue;
          if (item.hiddenInternal === true)
              continue;
          const content = ctx.normalizeConversationPreviewText(item.content);
          if (content) {
              return content;
          }
      }
      return '';
  };

  ctx.extractLatestVisibleMessagePreview = (messages: unknown[]): string => {
      for (let index = messages.length - 1; index >= 0; index -= 1) {
          const item = (messages[index] || {}) as Record<string, unknown>;
          const role = String(item.role || '').trim();
          if (role !== 'user' && role !== 'assistant')
              continue;
          if (item.hiddenInternal === true)
              continue;
          const content = ctx.normalizeConversationPreviewText(item.content);
          if (content) {
              return content;
          }
      }
      return '';
  };

  ctx.extractLatestConversationPreview = (messages: unknown[]): string => ctx.extractLatestUserPreview(messages) || ctx.extractLatestVisibleMessagePreview(messages);

  ctx.resolveLatestConversationMessageTimestamp = (messages: unknown[]): number => {
      for (let index = messages.length - 1; index >= 0; index -= 1) {
          const item = (messages[index] || {}) as Record<string, unknown>;
          const role = String(item.role || '').trim();
          if (role !== 'user' && role !== 'assistant')
              continue;
          if (item.hiddenInternal === true)
              continue;
          const content = ctx.normalizeConversationPreviewText(item.content);
          if (!content)
              continue;
          return ctx.normalizeTimestamp(item.created_at);
      }
      return 0;
  };

  ctx.resolveSessionPreviewFromFields = (session: Record<string, unknown>): string => ctx.normalizeConversationPreviewText(session?.last_user_message_preview ||
      session?.last_user_message ||
      session?.last_message_preview ||
      session?.last_message ||
      session?.summary ||
      '');

  ctx.resolveSessionTimelinePreview = (session: Record<string, unknown>): string => {
      const sessionId = String(session?.id || '').trim();
      const fieldPreview = ctx.resolveSessionPreviewFromFields(session);
      const fieldTimestamp = ctx.normalizeTimestamp(session?.last_message_at || session?.updated_at || session?.created_at);
      if (sessionId) {
          const cachedMessages = ctx.chatStore.getCachedSessionMessages(sessionId);
          if (Array.isArray(cachedMessages) && cachedMessages.length > 0) {
              const preview = ctx.extractLatestConversationPreview(cachedMessages as unknown[]);
              const previewTimestamp = ctx.resolveLatestConversationMessageTimestamp(cachedMessages as unknown[]);
              if (preview && (!fieldPreview || previewTimestamp >= fieldTimestamp)) {
                  return preview;
              }
          }
          const cached = String(ctx.timelinePreviewMap.value.get(sessionId) || '').trim();
          if (cached)
              return cached;
      }
      return fieldPreview;
  };

  ctx.refreshSessionPreviewCache = (sessionId: unknown, session?: Record<string, unknown> | null): string => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return '';
      const cachedMessages = ctx.chatStore.getCachedSessionMessages(targetId);
      const sessionRecord = (session || {}) as Record<string, unknown>;
      const fieldPreview = ctx.resolveSessionPreviewFromFields(sessionRecord);
      const fieldTimestamp = ctx.normalizeTimestamp(sessionRecord?.last_message_at || sessionRecord?.updated_at || sessionRecord?.created_at);
      const cachedPreview = ctx.extractLatestConversationPreview(cachedMessages as unknown[]);
      const cachedTimestamp = ctx.resolveLatestConversationMessageTimestamp(cachedMessages as unknown[]);
      const preview = cachedPreview && (!fieldPreview || cachedTimestamp >= fieldTimestamp) ? cachedPreview : fieldPreview;
      ctx.timelinePreviewMap.value.set(targetId, preview);
      return preview;
  };

  ctx.rightPanelSessionHistory = computed(() => {
      if (!ctx.showAgentRightDock.value)
          return [];
      const targetAgentId = ctx.normalizeAgentId(ctx.rightPanelAgentId.value);
      const seenIds = new Set<string>();
      let mainAssigned = false;
      const result = (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : [])
          .filter((session) => ctx.normalizeAgentId(session?.agent_id) === targetAgentId)
          .map((session) => ({
          id: String(session?.id || '').trim(),
          title: String(session?.title || ctx.t('chat.newSession')),
          preview: ctx.resolveSessionTimelinePreview(session as Record<string, unknown>),
          lastAt: ctx.resolveSessionActivityTimestamp((session || {}) as Record<string, unknown>),
          isMain: Boolean(session?.is_main),
          orchestrationLock: session && typeof session === 'object' && !Array.isArray(session)
              ? ((session as Record<string, unknown>).orchestration_lock as Record<string, unknown> | null | undefined)
              : null
      }))
          .filter((item) => item.id)
          .sort((left, right) => {
          if (left.isMain !== right.isMain) {
              return left.isMain ? -1 : 1;
          }
          return ctx.normalizeTimestamp(right.lastAt) - ctx.normalizeTimestamp(left.lastAt);
      })
          .filter((item) => {
          if (seenIds.has(item.id)) {
              return false;
          }
          seenIds.add(item.id);
          return true;
      })
          .map((item) => {
          if (!item.isMain || mainAssigned) {
              return { ...item, isMain: false };
          }
          mainAssigned = true;
          return item;
      });
      return result;
  });

  ctx.preloadTimelinePreview = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      if (ctx.timelinePreviewMap.value.has(targetId) || ctx.timelinePreviewLoadingSet.value.has(targetId)) {
          return;
      }
      ctx.timelinePreviewLoadingSet.value.add(targetId);
      try {
          const sessionRecord = (Array.isArray(ctx.chatStore.sessions)
              ? ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId)
              : null) || null;
          const sessionPreview = sessionRecord
              ? ctx.resolveSessionTimelinePreview(sessionRecord as Record<string, unknown>)
              : '';
          if (sessionPreview) {
              ctx.timelinePreviewMap.value.set(targetId, sessionPreview);
              return;
          }
          const cachedMessages = ctx.chatStore.getCachedSessionMessages(targetId);
          if (Array.isArray(cachedMessages) && cachedMessages.length > 0) {
              const preview = ctx.extractLatestConversationPreview(cachedMessages as unknown[]);
              ctx.timelinePreviewMap.value.set(targetId, preview);
              return;
          }
          const result = await getChatSessionApi(targetId).catch(() => null);
          const messages = Array.isArray(result?.data?.data?.messages) ? result.data.data.messages : [];
          if (!Array.isArray(messages) || !messages.length) {
              ctx.timelinePreviewMap.value.set(targetId, '');
              return;
          }
          const preview = ctx.extractLatestConversationPreview(messages as unknown[]);
          ctx.timelinePreviewMap.value.set(targetId, preview);
      }
      catch {
      }
      finally {
          ctx.timelinePreviewLoadingSet.value.delete(targetId);
      }
  };
}
