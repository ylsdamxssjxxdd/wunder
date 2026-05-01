// @ts-nocheck
// Conversation titles, page waiting state, chat footer state, and dismissed conversation persistence.
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

export function installMessengerControllerPanelSummaries(ctx: MessengerControllerContext): void {
  ctx.activeConversationTitle = computed(() => {
      const identity = ctx.activeConversation.value;
      if (!identity)
          return ctx.t('messenger.empty.noConversation');
      if (identity.kind === 'agent') {
          return ctx.activeAgentName.value;
      }
      const conversation = ctx.userWorldStore.conversations.find((item) => String(item?.conversation_id || '') === identity.id);
      return ctx.userWorldStore.resolveConversationTitle(conversation) || ctx.t('messenger.empty.noConversation');
  });

  ctx.activeConversationSubtitle = computed(() => {
      const identity = ctx.activeConversation.value;
      if (!identity)
          return ctx.t('messenger.empty.subtitle');
      if (identity.kind === 'agent') {
          const info = ctx.activeAgent.value as Record<string, unknown> | null;
          return String(info?.description || ctx.t('messenger.agent.subtitle'));
      }
      if (identity.kind === 'group') {
          return ctx.t('messenger.group.subtitle');
      }
      const conversation = ctx.userWorldStore.conversations.find((item) => String(item?.conversation_id || '') === identity.id);
      const peerUserId = String(conversation?.peer_user_id || '').trim();
      if (!peerUserId)
          return ctx.t('messenger.direct.subtitle');
      const contact = (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : []).find((item) => String(item?.user_id || '').trim() === peerUserId);
      return ctx.t('userWorld.chat.userSubtitle', { unit: ctx.resolveUnitLabel(contact?.unit_id) });
  });

  ctx.activeConversationKindLabel = computed(() => {
      const identity = ctx.activeConversation.value;
      if (!identity)
          return '';
      return ctx.t(`messenger.kind.${identity.kind}`);
  });

  ctx.generalSettingsPanelMode = computed<'general' | 'profile'>(() => ctx.settingsPanelMode.value === 'profile' ? 'profile' : 'general');

  ctx.chatPanelTitle = computed(() => {
      if (!ctx.showChatSettingsView.value) {
          return ctx.activeConversationTitle.value;
      }
      if (ctx.showAgentGridOverview.value) {
          return ctx.t('messenger.agent.overviewTitle');
      }
      if (ctx.showAgentSettingsPanel.value) {
          if (ctx.settingsAgentId.value === DEFAULT_AGENT_KEY) {
              return ctx.t('messenger.defaultAgent');
          }
          const target = ctx.agentMap.value.get(ctx.normalizeAgentId(ctx.settingsAgentId.value));
          return String(target?.name || ctx.settingsAgentId.value || ctx.t('messenger.section.agents'));
      }
      if (ctx.showHelperAppsWorkspace.value) {
          return ctx.helperAppsActiveTitle.value || '';
      }
      if (ctx.sessionHub.activeSection === 'users') {
          return String(ctx.selectedContact.value?.username || ctx.selectedContact.value?.user_id || ctx.t('messenger.section.users'));
      }
      if (ctx.sessionHub.activeSection === 'groups') {
          return String(ctx.selectedGroup.value?.group_name || ctx.selectedGroup.value?.group_id || ctx.t('messenger.section.groups'));
      }
      if (ctx.sessionHub.activeSection === 'tools') {
          if (ctx.selectedToolCategory.value)
              return ctx.toolCategoryLabel(ctx.selectedToolCategory.value);
      }
      if (ctx.sessionHub.activeSection === 'more') {
          if (ctx.settingsPanelMode.value === 'profile')
              return ctx.t('user.profile.enter');
          if (ctx.settingsPanelMode.value === 'prompts')
              return ctx.t('messenger.prompt.title');
          if (ctx.settingsPanelMode.value === 'help-manual')
              return ctx.t('messenger.settings.helpManual');
          if (ctx.settingsPanelMode.value === 'desktop-models')
              return ctx.t('desktop.system.llm');
          if (ctx.settingsPanelMode.value === 'desktop-lan')
              return ctx.t('desktop.system.lan.title');
      }
      return ctx.activeSectionTitle.value;
  });

  ctx.chatPanelSubtitle = computed(() => {
      if (!ctx.showChatSettingsView.value) {
          return ctx.activeConversationSubtitle.value;
      }
      if (ctx.showAgentGridOverview.value) {
          return ctx.t('messenger.agent.overviewDesc');
      }
      if (ctx.showAgentSettingsPanel.value) {
          return ctx.t('messenger.agent.subtitle');
      }
      if (ctx.showHelperAppsWorkspace.value) {
          return ctx.helperAppsActiveDescription.value || '';
      }
      if (ctx.sessionHub.activeSection === 'users') {
          return ctx.selectedContact.value
              ? ctx.t('userWorld.chat.userSubtitle', { unit: ctx.resolveUnitLabel(ctx.selectedContact.value.unit_id) })
              : ctx.t('messenger.section.users.desc');
      }
      if (ctx.sessionHub.activeSection === 'groups') {
          return ctx.t('messenger.section.groups.desc');
      }
      if (ctx.sessionHub.activeSection === 'tools') {
          return '';
      }
      if (ctx.sessionHub.activeSection === 'more') {
          if (ctx.settingsPanelMode.value === 'profile')
              return ctx.currentUsername.value;
          if (ctx.settingsPanelMode.value === 'prompts')
              return ctx.t('messenger.prompt.desc');
          if (ctx.settingsPanelMode.value === 'help-manual')
              return ctx.t('messenger.settings.helpManualHint');
          if (ctx.settingsPanelMode.value === 'desktop-models')
              return ctx.t('desktop.system.llmHint');
          if (ctx.settingsPanelMode.value === 'desktop-lan')
              return ctx.t('desktop.system.lan.hint');
      }
      return ctx.activeSectionSubtitle.value;
  });

  ctx.resolveMessengerPageWaitingTarget = (): string => {
      if (ctx.showHelperAppsWorkspace.value && ctx.helperAppsActiveKind.value === 'online') {
          return ctx.helperAppsActiveTitle.value || ctx.t('userWorld.helperApps.title');
      }
      const chatTitle = String(ctx.chatPanelTitle.value || '').trim();
      if (chatTitle) {
          return chatTitle;
      }
      const sectionTitle = String(ctx.activeSectionTitle.value || '').trim();
      if (sectionTitle) {
          return sectionTitle;
      }
      return ctx.t('common.loading');
  };

  ctx.resolveMessengerPageWaitingSummary = (): string => {
      if (ctx.showHelperAppsWorkspace.value && ctx.helperAppsActiveKind.value === 'online') {
          return ctx.t('messenger.waiting.summary.helperApps');
      }
      switch (ctx.sessionHub.activeSection) {
          case 'agents':
              return ctx.t('messenger.waiting.summary.agents');
          case 'tools':
              return ctx.t('messenger.waiting.summary.tools');
          case 'more':
              return ctx.t('messenger.waiting.summary.settings');
          case 'messages':
              return ctx.t('messenger.waiting.summary.messages');
          default:
              return ctx.t('messenger.waiting.summary.general');
      }
  };

  ctx.messengerPageWaitingState = computed<MessengerPageWaitingState | null>(() => {
      if (ctx.workerCardImportOverlayVisible.value ||
          ctx.isMessengerInteractionBlocked.value ||
          ctx.suppressMessengerPageWaitingOverlay.value) {
          return null;
      }
      if (ctx.bootLoading.value) {
          return {
              title: ctx.t('messenger.waiting.title'),
              targetName: ctx.resolveMessengerPageWaitingTarget(),
              phaseLabel: ctx.t('messenger.waiting.phase.preparing'),
              summaryLabel: ctx.resolveMessengerPageWaitingSummary(),
              progress: 22
          };
      }
      if (ctx.showHelperAppsWorkspace.value &&
          ctx.helperAppsActiveKind.value === 'online' &&
          ctx.helperAppsOnlineLoading.value) {
          return {
              title: ctx.t('messenger.waiting.title'),
              targetName: ctx.resolveMessengerPageWaitingTarget(),
              phaseLabel: ctx.t('messenger.waiting.phase.syncing'),
              summaryLabel: ctx.t('messenger.waiting.summary.helperApps'),
              progress: 48
          };
      }
      if (ctx.sessionHub.activeSection === 'tools' && ctx.toolsCatalogLoading.value) {
          return {
              title: ctx.t('messenger.waiting.title'),
              targetName: ctx.resolveMessengerPageWaitingTarget(),
              phaseLabel: ctx.t('messenger.waiting.phase.loading'),
              summaryLabel: ctx.t('messenger.waiting.summary.tools'),
              progress: 56
          };
      }
      return null;
  });

  ctx.chatPanelKindLabel = computed(() => {
      if (!ctx.showChatSettingsView.value)
          return ctx.activeConversationKindLabel.value;
      return '';
  });

  ctx.agentSessionLoading = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return false;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return false;
      return ctx.resolveEffectiveSessionBusy(sessionId, Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : []);
  });

  ctx.buildActiveSessionBusyDebugSnapshot = () => {
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const runtimeStatus = activeSessionId
          ? ctx.resolveSessionRuntimeStatus(activeSessionId)
          : '';
      const loadingBySession = activeSessionId ? ctx.resolveSessionLoadingFlag(activeSessionId) : false;
      const messages = Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [];
      let lastUserIndex = -1;
      for (let index = messages.length - 1; index >= 0; index -= 1) {
          if (String((messages[index] as Record<string, unknown> | null)?.role || '') === 'user') {
              lastUserIndex = index;
              break;
          }
      }
      let tailAssistant: Record<string, unknown> | null = null;
      for (let index = messages.length - 1; index > lastUserIndex; index -= 1) {
          const item = messages[index] as Record<string, unknown> | null;
          if (String(item?.role || '') === 'assistant') {
              tailAssistant = item;
              break;
          }
      }
      return {
          activeSessionId,
          section: ctx.sessionHub.activeSection,
          loadingProp: ctx.agentSessionLoading.value,
          isBusyGetter: activeSessionId ? Boolean(ctx.chatStore.isSessionBusy?.(activeSessionId)) : false,
          isLoadingGetter: activeSessionId ? Boolean(ctx.chatStore.isSessionLoading?.(activeSessionId)) : false,
          loadingBySession,
          runtimeStatus,
          pendingApprovals: Array.isArray(ctx.chatStore.pendingApprovals) ? ctx.chatStore.pendingApprovals.length : 0,
          messageCount: messages.length,
          hasRunningAssistantAfterLatestUser: hasRunningAssistantMessage(messages),
          lastUserIndex,
          hasTailAssistant: Boolean(tailAssistant),
          tailAssistantState: tailAssistant ? String(tailAssistant.state || '') : '',
          tailAssistantStreamIncomplete: Boolean(tailAssistant?.stream_incomplete),
          tailAssistantWorkflowStreaming: Boolean(tailAssistant?.workflowStreaming),
          tailAssistantReasoningStreaming: Boolean(tailAssistant?.reasoningStreaming),
          tailAssistantCompactionRunning: tailAssistant
              ? isCompactionRunningFromWorkflowItems(tailAssistant.workflowItems)
              : false,
          interactionBlocked: ctx.isMessengerInteractionBlocked.value,
          interactionBlockReason: ctx.messengerInteractionBlockReason.value
      };
  };

  watch([
      () => String(ctx.chatStore.activeSessionId || ''),
      () => ctx.agentSessionLoading.value,
      () => ctx.resolveSessionRuntimeStatus(String(ctx.chatStore.activeSessionId || '').trim()),
      () => Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages.length : 0,
      () => Boolean(ctx.isMessengerInteractionBlocked.value)
  ], () => {
      chatDebugLog('messenger.busy', 'snapshot-change', ctx.buildActiveSessionBusyDebugSnapshot());
  }, { immediate: true });

  ctx.canSendWorldMessage = computed(() => ctx.isWorldConversationActive.value &&
      Boolean(ctx.activeConversation.value?.id) &&
      !ctx.userWorldStore.sending &&
      !ctx.worldUploading.value &&
      !ctx.worldVoiceRecording.value &&
      Boolean(ctx.worldDraft.value.trim()));

  ctx.worldContainerPickerPathLabel = computed(() => ctx.worldContainerPickerPath.value ? `/${ctx.worldContainerPickerPath.value}` : '/');

  ctx.worldContainerPickerDisplayEntries = computed(() => {
      const keyword = String(ctx.worldContainerPickerKeyword.value || '').trim().toLowerCase();
      if (!keyword) {
          return ctx.worldContainerPickerEntries.value;
      }
      return ctx.worldContainerPickerEntries.value.filter((entry) => {
          const name = String(entry.name || '').toLowerCase();
          const path = String(entry.path || '').toLowerCase();
          return name.includes(keyword) || path.includes(keyword);
      });
  });

  ctx.normalizeDismissedAgentConversationMap = (value: unknown): Record<string, number> => {
      if (!value || typeof value !== 'object' || Array.isArray(value)) {
          return {};
      }
      return Object.entries(value as Record<string, unknown>).reduce<Record<string, number>>((acc, [key, raw]) => {
          const agentId = ctx.normalizeAgentId(key);
          const timestamp = Number(raw);
          if (!agentId || !Number.isFinite(timestamp) || timestamp <= 0) {
              return acc;
          }
          acc[agentId] = timestamp;
          return acc;
      }, {});
  };

  ctx.resolveDismissedAgentStorageKey = (userId: unknown): string => {
      const cleaned = String(userId || '').trim() || 'anonymous';
      return `${DISMISSED_AGENT_STORAGE_PREFIX}:${cleaned}`;
  };

  ctx.ensureDismissedAgentConversationState = (force = false) => {
      if (typeof window === 'undefined') {
          ctx.dismissedAgentConversationMap.value = {};
          ctx.dismissedAgentStorageKey.value = '';
          return;
      }
      const targetKey = ctx.resolveDismissedAgentStorageKey(ctx.currentUserId.value);
      if (!force && ctx.dismissedAgentStorageKey.value === targetKey) {
          return;
      }
      ctx.dismissedAgentStorageKey.value = targetKey;
      try {
          const raw = window.localStorage.getItem(targetKey);
          ctx.dismissedAgentConversationMap.value = raw ? ctx.normalizeDismissedAgentConversationMap(JSON.parse(raw)) : {};
      }
      catch {
          ctx.dismissedAgentConversationMap.value = {};
      }
  };

  ctx.persistDismissedAgentConversationState = () => {
      if (typeof window === 'undefined')
          return;
      const targetKey = ctx.dismissedAgentStorageKey.value || ctx.resolveDismissedAgentStorageKey(ctx.currentUserId.value);
      ctx.dismissedAgentStorageKey.value = targetKey;
      try {
          window.localStorage.setItem(targetKey, JSON.stringify(ctx.dismissedAgentConversationMap.value));
      }
      catch {
      }
  };

  ctx.markAgentConversationDismissed = (agentId: unknown) => {
      const normalized = ctx.normalizeAgentId(agentId);
      if (!normalized)
          return;
      ctx.dismissedAgentConversationMap.value = {
          ...ctx.dismissedAgentConversationMap.value,
          [normalized]: Date.now()
      };
      ctx.persistDismissedAgentConversationState();
  };

  ctx.clearAgentConversationDismissed = (agentId: unknown) => {
      const normalized = ctx.normalizeAgentId(agentId);
      if (!normalized || !(normalized in ctx.dismissedAgentConversationMap.value))
          return;
      const next = { ...ctx.dismissedAgentConversationMap.value };
      delete next[normalized];
      ctx.dismissedAgentConversationMap.value = next;
      ctx.persistDismissedAgentConversationState();
  };

  ctx.normalizeNumericMap = (value: unknown): Record<string, number> => {
      if (!value || typeof value !== 'object' || Array.isArray(value)) {
          return {};
      }
      return Object.entries(value as Record<string, unknown>).reduce<Record<string, number>>((acc, [key, raw]) => {
          const normalizedKey = ctx.normalizeAgentId(key);
          const numeric = Number(raw);
          if (!normalizedKey || !Number.isFinite(numeric) || numeric <= 0) {
              return acc;
          }
          acc[normalizedKey] = Math.floor(numeric);
          return acc;
      }, {});
  };

  ctx.resolveAgentUnreadStorageKeys = (userId: unknown) => {
      const cleaned = String(userId || '').trim() || 'anonymous';
      return {
          readAt: `${AGENT_MAIN_READ_AT_STORAGE_PREFIX}:${cleaned}`,
          unread: `${AGENT_MAIN_UNREAD_STORAGE_PREFIX}:${cleaned}`
      };
  };

  ctx.persistAgentUnreadState = () => {
      if (typeof window === 'undefined')
          return;
      const { readAt, unread } = ctx.agentUnreadStorageKeys.value;
      if (!readAt || !unread)
          return;
      try {
          window.localStorage.setItem(readAt, JSON.stringify(ctx.agentMainReadAtMap.value));
          window.localStorage.setItem(unread, JSON.stringify(ctx.agentMainUnreadCountMap.value));
      }
      catch {
      }
  };
}
