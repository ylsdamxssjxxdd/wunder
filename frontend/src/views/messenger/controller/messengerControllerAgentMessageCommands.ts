// @ts-nocheck
// Agent settings save/delete reactions, section selection fallback, local commands, agent send, and stop actions.
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

export function installMessengerControllerAgentMessageCommands(ctx: MessengerControllerContext): void {
  ctx.handleAgentSettingsSaved = async () => {
      const tasks: Promise<unknown>[] = [
          ctx.agentStore.loadAgents(),
          ctx.loadDefaultAgentProfile(),
          ctx.loadRunningAgents({ force: true }),
          ctx.loadAgentUserRounds(),
          ctx.loadChannelBoundAgentIds({ force: true }),
          ctx.loadAgentToolSummary({ force: true })
      ];
      if (!ctx.cronPermissionDenied.value) {
          tasks.push(ctx.loadCronAgentIds({ force: true }));
      }
      await Promise.allSettled(tasks);
      const currentAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      if (currentAgentId && currentAgentId !== DEFAULT_AGENT_KEY) {
          const profile = await ctx.agentStore.getAgent(currentAgentId, { force: true }).catch(() => null);
          if (ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value) === currentAgentId) {
              ctx.activeAgentDetailProfile.value = (profile as Record<string, unknown> | null) || null;
          }
      }
      else {
          ctx.activeAgentDetailProfile.value = null;
      }
  };

  ctx.handleAgentDeleteStart = () => {
      ctx.deletingAgentSelectionSnapshot.value = [...ctx.visibleAgentIdsForSelection.value];
  };

  ctx.handleAgentDeleted = async (deletedAgentId: string) => {
      const normalizedDeletedAgentId = ctx.normalizeAgentId(deletedAgentId);
      const currentIdsWithoutDeleted = ctx.visibleAgentIdsForSelection.value.filter((item) => ctx.normalizeAgentId(item) !== normalizedDeletedAgentId);
      ctx.selectedAgentId.value = resolveAgentSelectionAfterRemoval({
          removedId: normalizedDeletedAgentId,
          previousIds: ctx.deletingAgentSelectionSnapshot.value,
          currentIds: currentIdsWithoutDeleted,
          fallbackId: DEFAULT_AGENT_KEY
      });
      const currentIdentity = ctx.activeConversation.value;
      const activeConversationAgentId = currentIdentity?.kind === 'agent'
          ? ctx.normalizeAgentId(currentIdentity.agentId || String(currentIdentity.id || '').replace(/^draft:/, ''))
          : '';
      if (activeConversationAgentId && activeConversationAgentId === normalizedDeletedAgentId) {
          ctx.sessionHub.clearActiveConversation();
      }
      if (ctx.normalizeAgentId(ctx.chatStore.draftAgentId) === normalizedDeletedAgentId) {
          ctx.chatStore.draftAgentId = '';
          ctx.chatStore.draftToolOverrides = null;
      }
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (activeSessionId) {
          const activeSession = (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).find((item) => String(item?.id || '') === activeSessionId);
          const activeSessionAgentId = ctx.normalizeAgentId(activeSession?.agent_id || (activeSession?.is_default === true ? DEFAULT_AGENT_KEY : ''));
          if (activeSessionAgentId && activeSessionAgentId === normalizedDeletedAgentId) {
              ctx.chatStore.activeSessionId = null;
              ctx.chatStore.messages = [];
          }
      }
      ctx.activeAgentDetailProfile.value = null;
      ctx.deletingAgentSelectionSnapshot.value = [];
      const tasks: Promise<unknown>[] = [
          ctx.refreshAgentMutationState(),
          ctx.chatStore.loadSessions(),
          ctx.loadRunningAgents({ force: true }),
          ctx.loadAgentUserRounds(),
          ctx.loadChannelBoundAgentIds({ force: true }),
          ctx.loadDefaultAgentProfile(),
          ctx.loadAgentToolSummary({ force: true })
      ];
      if (!ctx.cronPermissionDenied.value) {
          tasks.push(ctx.loadCronAgentIds({ force: true }));
      }
      await Promise.allSettled(tasks);
      ctx.ensureSectionSelection();
  };

  ctx.clearMessagePanelWhenConversationEmpty = () => {
      if (ctx.sessionHub.activeSection !== 'messages')
          return;
      if (ctx.hasAnyMixedConversations.value)
          return;
      if (ctx.sessionHub.activeConversation) {
          ctx.sessionHub.clearActiveConversation();
      }
      if (String(ctx.userWorldStore.activeConversationId || '').trim()) {
          ctx.userWorldStore.activeConversationId = '';
      }
      if (String(ctx.chatStore.activeSessionId || '').trim() ||
          String(ctx.chatStore.draftAgentId || '').trim() ||
          (Array.isArray(ctx.chatStore.messages) && ctx.chatStore.messages.length > 0)) {
          ctx.chatStore.activeSessionId = null;
          ctx.chatStore.draftAgentId = '';
          ctx.chatStore.draftToolOverrides = null;
          ctx.chatStore.messages = [];
      }
  };

  ctx.ensureSectionSelection = () => {
      if (ctx.sessionHub.activeSection === 'agents') {
          const visibleAgentIds = ctx.visibleAgentIdsForSelection.value;
          if (!visibleAgentIds.length) {
              ctx.selectedAgentId.value = DEFAULT_AGENT_KEY;
              return;
          }
          if (!visibleAgentIds.includes(ctx.normalizeAgentId(ctx.selectedAgentId.value))) {
              ctx.selectedAgentId.value = visibleAgentIds[0] || DEFAULT_AGENT_KEY;
          }
          return;
      }
      if (ctx.sessionHub.activeSection === 'users') {
          const exists = ctx.filteredContacts.value.some((item) => String(item?.user_id || '') === ctx.selectedContactUserId.value);
          if (!exists) {
              ctx.selectedContactUserId.value = String(ctx.filteredContacts.value[0]?.user_id || '');
          }
          if (!ctx.selectedContactUserId.value && ctx.filteredContacts.value.length > 0) {
              ctx.selectedContactUserId.value = String(ctx.filteredContacts.value[0]?.user_id || '');
          }
          return;
      }
      if (ctx.sessionHub.activeSection === 'groups') {
          if (!ctx.selectedGroupId.value && ctx.filteredGroups.value.length > 0) {
              ctx.selectedGroupId.value = String(ctx.filteredGroups.value[0]?.group_id || '');
          }
          return;
      }
      if (ctx.sessionHub.activeSection === 'swarms' || ctx.sessionHub.activeSection === 'orchestrations') {
          if (ctx.applyInitialBeeroomSectionSelection()) {
              return;
          }
          if (!ctx.beeroomStore.activeGroupId && ctx.filteredBeeroomGroupsOrdered.value.length > 0) {
              const firstGroupId = ctx.resolveFirstVisibleBeeroomGroupId();
              if (firstGroupId) {
                  ctx.beeroomStore.setActiveGroup(firstGroupId);
              }
          }
          return;
      }
      if (ctx.sessionHub.activeSection === 'plaza') {
          ctx.selectedPlazaItemId.value = resolveRetainedSelectedPlazaItemId(ctx.filteredPlazaItems.value, ctx.selectedPlazaItemId.value);
          return;
      }
      if (ctx.sessionHub.activeSection === 'tools') {
          if (!ctx.selectedToolEntryKey.value) {
              ctx.selectedToolCategory.value = 'admin';
          }
          return;
      }
      if (ctx.sessionHub.activeSection === 'files') {
          if (ctx.fileScope.value === 'user') {
              ctx.selectedFileContainerId.value = USER_CONTAINER_ID;
              return;
          }
          const exists = ctx.agentFileContainers.value.some((item) => item.id === ctx.selectedFileContainerId.value);
          if (!exists) {
              const fallbackId = ctx.agentFileContainers.value[0]?.id ?? USER_CONTAINER_ID;
              ctx.selectedFileContainerId.value = fallbackId;
              if (fallbackId === USER_CONTAINER_ID && !ctx.agentFileContainers.value.length) {
                  ctx.fileScope.value = 'user';
              }
          }
          return;
      }
  };

  ctx.syncAgentConversationFallback = () => {
      if (ctx.sessionHub.activeSection !== 'messages')
          return;
      if (!ctx.hasAnyMixedConversations.value) {
          ctx.clearMessagePanelWhenConversationEmpty();
          return;
      }
      if (ctx.sessionHub.activeConversation)
          return;
      const routeConversationId = String(ctx.route.query?.conversation_id || '').trim();
      if (routeConversationId || String(ctx.userWorldStore.activeConversationId || '').trim())
          return;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (sessionId) {
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
          ctx.sessionHub.setActiveConversation({
              kind: 'agent',
              id: sessionId,
              agentId: ctx.normalizeAgentId(session?.agent_id ?? ctx.chatStore.draftAgentId)
          });
          return;
      }
      if (!String(ctx.chatStore.draftAgentId || '').trim() && !ctx.chatStore.messages.length) {
          return;
      }
      const draftAgent = ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.selectedAgentId.value);
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: `draft:${draftAgent}`,
          agentId: draftAgent
      });
  };

  ctx.parseAgentLocalCommand = (value: unknown): AgentLocalCommand | '' => {
      const raw = String(value || '').trim();
      if (!raw.startsWith('/'))
          return '';
      const token = raw.split(/\s+/, 1)[0].replace(/^\/+/, '').toLowerCase();
      if (!token)
          return '';
      if (token === 'new' || token === 'reset')
          return 'new';
      if (token === 'stop' || token === 'cancel')
          return 'stop';
      if (token === 'help' || token === '?')
          return 'help';
      if (token === 'compact')
          return 'compact';
      if (token === 'goal')
          return 'goal';
      return '';
  };

  ctx.resolveCommandErrorMessage = (error: unknown): string => String((error as {
      response?: {
          data?: {
              detail?: string;
          };
      };
      message?: string;
  })?.response?.data?.detail || (error as {
      message?: string;
  })?.message || ctx.t('common.requestFailed')).trim();

  ctx.appendAgentLocalCommandMessages = (commandText: string, replyText: string) => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      ctx.chatStore.appendLocalMessage('user', commandText, { sessionId });
      ctx.chatStore.appendLocalMessage('assistant', replyText, { sessionId });
  };

  ctx.openGoalDialog = async (initialObjective = '') => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId) {
          ElMessage.warning(ctx.t('chat.command.goalMissingSession'));
          return;
      }
      const trimmedInitial = String(initialObjective || '').trim();
      const cachedGoal = typeof ctx.chatStore.sessionGoal === 'function' ? ctx.chatStore.sessionGoal(sessionId) : null;
      ctx.goalDialogSessionId.value = sessionId;
      ctx.goalDialogObjective.value = trimmedInitial || String(cachedGoal?.objective || '');
      ctx.goalDialogVisible.value = true;
      ctx.goalDialogLoading.value = !trimmedInitial && !cachedGoal;
      if (trimmedInitial || cachedGoal) {
          ctx.goalDialogLoading.value = false;
          return;
      }
      try {
          const goal = await ctx.chatStore.refreshSessionGoal(sessionId);
          if (ctx.goalDialogSessionId.value !== sessionId) {
              return;
          }
          if (ctx.goalDialogObjective.value === trimmedInitial) {
              ctx.goalDialogObjective.value = String(goal?.objective || '');
          }
      }
      catch (error) {
          if (ctx.goalDialogSessionId.value === sessionId) {
              ElMessage.warning(ctx.t('chat.command.goalFailed', { message: ctx.resolveCommandErrorMessage(error) }));
          }
      }
      finally {
          if (ctx.goalDialogSessionId.value === sessionId) {
              ctx.goalDialogLoading.value = false;
          }
      }
  };

  ctx.submitGoalDialog = async () => {
      const sessionId = String(ctx.goalDialogSessionId.value || ctx.chatStore.activeSessionId || '').trim();
      const objective = String(ctx.goalDialogObjective.value || '').trim();
      if (!sessionId) {
          ElMessage.warning(ctx.t('chat.command.goalMissingSession'));
          return;
      }
      if (!objective) {
          ElMessage.warning(ctx.t('chat.goal.objectiveRequired'));
          return;
      }
      if (ctx.goalDialogSubmitting.value) {
          return;
      }
      ctx.goalDialogSubmitting.value = true;
      try {
          const originalSessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (originalSessionId !== sessionId) {
              throw new Error(ctx.t('chat.goal.sessionChanged'));
          }
          const result = await ctx.chatStore.setSessionGoal(sessionId, { objective });
          const savedObjective = String(result?.goal?.objective || objective).trim();
          ctx.goalDialogVisible.value = false;
          ctx.goalDialogObjective.value = savedObjective;
          ElMessage.success(ctx.t('chat.command.goalSet', { objective: savedObjective }));
      }
      catch (error) {
          ElMessage.warning(ctx.t('chat.command.goalFailed', { message: ctx.resolveCommandErrorMessage(error) }));
      }
      finally {
          ctx.goalDialogSubmitting.value = false;
      }
  };

  ctx.handleAgentLocalCommand = async (command: AgentLocalCommand, rawText: string) => {
      if (command === 'help') {
          ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.help'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'new') {
          try {
              const outcome = await ctx.runStartNewSession();
              if (outcome !== 'noop') {
                  const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
                  const replyText = outcome === 'already_current' ? ctx.t('chat.newSessionAlreadyCurrent') : ctx.t('chat.command.newSuccess');
                  ctx.chatStore.appendLocalMessage('assistant', replyText, { sessionId });
              }
          }
          catch (error) {
              ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.newFailed', { message: ctx.resolveCommandErrorMessage(error) }));
          }
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'stop') {
          const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (!sessionId) {
              ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.stopNoSession'));
              await ctx.scrollMessagesToBottom();
              return;
          }
          const cancelled = await ctx.chatStore.stopStream();
          ctx.appendAgentLocalCommandMessages(rawText, cancelled ? ctx.t('chat.command.stopRequested') : ctx.t('chat.command.stopNoRunning'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'goal') {
          const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (!sessionId) {
              ElMessage.warning(ctx.t('chat.command.goalMissingSession'));
              return;
          }
          const args = rawText.replace(/^\/+goal\b/i, '').trim();
          const action = args.split(/\s+/, 1)[0].trim().toLowerCase();
          if (action === 'pause' || action === 'resume' || action === 'clear') {
              ElMessage.warning(ctx.t('chat.command.goalExitViaStop'));
              return;
          }
          await ctx.openGoalDialog(args);
          return;
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId) {
          ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.compactMissingSession'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      ctx.chatStore.appendLocalMessage('user', rawText, { sessionId });
      try {
          await ctx.chatStore.compactSession(sessionId);
      }
      catch { }
      await ctx.scrollMessagesToBottom();
  };

  ctx.sendAgentMessage = async (payload: {
      content?: string;
      attachments?: unknown[];
  }) => {
      if (ctx.isMessengerInteractionBlocked.value) {
          chatDebugLog('messenger.send', 'blocked-send-during-interaction-lock', ctx.buildActiveSessionBusyDebugSnapshot());
          return;
      }
      const content = String(payload?.content || '').trim();
      const attachments = Array.isArray(payload?.attachments) ? payload.attachments : [];
      const activeInquiry = ctx.activeAgentInquiryPanel.value;
      const selectedRoutes = ctx.resolveAgentInquirySelectionRoutes(activeInquiry?.panel, ctx.agentInquirySelection.value);
      const hasInquirySelection = selectedRoutes.length > 0;
      if (!content && attachments.length === 0 && !hasInquirySelection)
          return;
      const localCommand = ctx.parseAgentLocalCommand(content);
      if (localCommand && !hasInquirySelection) {
          if (activeInquiry) {
              ctx.chatStore.resolveInquiryPanel(activeInquiry.message, { status: 'dismissed' });
          }
          if (attachments.length > 0) {
              ctx.appendAgentLocalCommandMessages(content, ctx.t('chat.command.attachmentsUnsupported'));
              ctx.agentInquirySelection.value = [];
              await ctx.scrollMessagesToBottom();
              return;
          }
          if (ctx.activeSessionOrchestrationLocked.value && localCommand !== 'stop') {
              ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
              ctx.agentInquirySelection.value = [];
              return;
          }
          if (ctx.activeSessionGoalLocked.value && localCommand !== 'stop') {
              ElMessage.warning(ctx.t('chat.goal.lockedInMessenger'));
              ctx.agentInquirySelection.value = [];
              return;
          }
          await ctx.handleAgentLocalCommand(localCommand, content);
          ctx.agentInquirySelection.value = [];
          return;
      }
      if (ctx.activeSessionOrchestrationLocked.value) {
          ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
          return;
      }
      if (ctx.activeSessionGoalLocked.value) {
          ElMessage.warning(ctx.t('chat.goal.lockedInMessenger'));
          return;
      }
      let finalContent = content;
      if (activeInquiry) {
          if (hasInquirySelection) {
              ctx.chatStore.resolveInquiryPanel(activeInquiry.message, {
                  status: 'answered',
                  selected: selectedRoutes.map((route) => route.label)
              });
              const selectionText = ctx.buildAgentInquiryReply(activeInquiry.panel, selectedRoutes);
              if (content) {
                  finalContent = `${selectionText}\n\n${ctx.t('chat.askPanelUserAppend', { content })}`;
              }
              else {
                  finalContent = selectionText;
              }
          }
          else {
              ctx.chatStore.resolveInquiryPanel(activeInquiry.message, { status: 'dismissed' });
          }
      }
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      ctx.autoStickToBottom.value = true;
      ctx.setRuntimeStateOverride(targetAgentId, 'running', 30000);
      ctx.pendingAssistantCenter = true;
      ctx.pendingAssistantCenterCount = ctx.chatStore.messages.length;
      try {
          await ctx.chatStore.sendMessage(finalContent, {
              attachments,
              suppressQueuedNotice: hasInquirySelection,
              approvalMode: normalizeAgentApprovalMode(ctx.composerApprovalMode.value || ctx.activeAgentApprovalMode.value)
          });
          ctx.setRuntimeStateOverride(targetAgentId, 'done', 8000);
          if (ctx.chatStore.activeSessionId) {
              ctx.sessionHub.setActiveConversation({
                  kind: 'agent',
                  id: String(ctx.chatStore.activeSessionId),
                  agentId: ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.activeAgentId.value)
              });
          }
          await ctx.scrollMessagesToBottom();
      }
      catch (error) {
          ctx.pendingAssistantCenter = false;
          ctx.pendingAssistantCenterCount = 0;
          ctx.setRuntimeStateOverride(targetAgentId, 'error', 8000);
          showApiError(error, ctx.t('chat.error.requestFailed'));
      }
      finally {
          ctx.agentInquirySelection.value = [];
      }
  };

  ctx.stopAgentMessage = async () => {
      if (ctx.isMessengerInteractionBlocked.value) {
          chatDebugLog('messenger.send', 'blocked-stop-during-interaction-lock', ctx.buildActiveSessionBusyDebugSnapshot());
          return;
      }
      chatDebugLog('messenger.send', 'manual-stop-click', ctx.buildActiveSessionBusyDebugSnapshot());
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      ctx.setRuntimeStateOverride(targetAgentId, 'done', 20000);
      ctx.pendingAssistantCenter = false;
      ctx.pendingAssistantCenterCount = 0;
      try {
          await ctx.chatStore.stopStream();
      }
      catch {
      }
  };
}
