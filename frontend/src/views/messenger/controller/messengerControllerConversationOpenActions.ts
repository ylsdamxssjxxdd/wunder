// @ts-nocheck
// Search-create routing, middle-pane selections, world conversation openers, agent sessions, and prompt previews.
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

export function installMessengerControllerConversationOpenActions(ctx: MessengerControllerContext): void {
  ctx.handleSearchCreateAction = async (command?: string) => {
      if (ctx.sessionHub.activeSection === 'groups') {
          if (ctx.userWorldPermissionDenied.value) {
              ElMessage.warning(ctx.t('auth.login.noPermission'));
              return;
          }
          ctx.groupCreateName.value = '';
          ctx.groupCreateKeyword.value = '';
          ctx.groupCreateMemberIds.value = [];
          ctx.groupCreateVisible.value = true;
          return;
      }
      if (ctx.sessionHub.activeSection === 'swarms' || ctx.sessionHub.activeSection === 'orchestrations') {
          return;
      }
      if (ctx.sessionHub.activeSection === 'agents') {
          if (command === 'import_worker_card') {
              ctx.openWorkerCardImportPicker();
              return;
          }
          await ctx.createAgentQuickly();
      }
  };

  ctx.openMixedConversation = async (item: MixedConversation) => {
      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneOverlayVisible.value = false;
      if (ctx.sessionHub.activeSection === 'messages' &&
          !ctx.showChatSettingsView.value &&
          ctx.isMixedConversationActive(item)) {
          return;
      }
      if (item.kind === 'agent') {
          const goalLockedSessionId = ctx.resolveAgentGoalLockedSessionId(item.agentId);
          if (goalLockedSessionId) {
              await ctx.openAgentSession(goalLockedSessionId, item.agentId);
              return;
          }
          const targetSessionId = String(item.sourceId || '').trim();
          if (targetSessionId) {
              await ctx.openAgentSession(targetSessionId, item.agentId);
              return;
          }
          await ctx.openAgentById(item.agentId);
          return;
      }
      await ctx.openWorldConversation(item.sourceId, item.kind, 'messages');
  };

  ctx.selectContact = (contact: Record<string, unknown>) => {
      ctx.selectedContactUserId.value = String(contact?.user_id || '').trim();
      ctx.selectedGroupId.value = '';
  };

  ctx.selectGroup = (group: Record<string, unknown>) => {
      ctx.selectedGroupId.value = String(group?.group_id || '').trim();
      ctx.selectedContactUserId.value = '';
  };

  ctx.selectPlazaItem = (itemId: unknown) => {
      ctx.selectedPlazaItemId.value = String(itemId || '').trim();
  };

  ctx.resolveFirstVisibleBeeroomGroupId = (): string => String(ctx.filteredBeeroomGroupsOrdered.value[0]?.group_id ||
      ctx.filteredBeeroomGroupsOrdered.value[0]?.hive_id ||
      '').trim();

  ctx.applyInitialBeeroomSectionSelection = (): boolean => {
      if (!ctx.beeroomFirstEntryAutoSelectionPending.value ||
          !['swarms', 'orchestrations'].includes(ctx.sessionHub.activeSection) ||
          ctx.messengerOrderHydrating.value ||
          !ctx.messengerOrderReady.value) {
          return false;
      }
      const firstGroupId = ctx.resolveFirstVisibleBeeroomGroupId();
      if (!firstGroupId) {
          return false;
      }
      ctx.beeroomFirstEntryAutoSelectionPending.value = false;
      if (String(ctx.beeroomStore.activeGroupId || '').trim() !== firstGroupId) {
          ctx.beeroomStore.setActiveGroup(firstGroupId);
      }
      return true;
  };

  ctx.triggerPlazaPublish = () => {
      void ctx.messengerHivePlazaPanelRef.value?.openPublishDialog();
  };

  ctx.triggerPlazaRefresh = () => {
      void ctx.messengerHivePlazaPanelRef.value?.reload();
  };

  ctx.selectBeeroomGroup = async (group: Record<string, unknown>) => {
      const groupId = String(group?.group_id || group?.hive_id || '').trim();
      if (!groupId)
          return;
      ctx.beeroomFirstEntryAutoSelectionPending.value = false;
      ctx.beeroomStore.setActiveGroup(groupId);
      await ctx.beeroomStore.loadActiveGroup().catch(() => null);
  };

  ctx.handleDeleteBeeroomGroup = async (group: Record<string, unknown>) => {
      const groupId = String(group?.group_id || group?.hive_id || '').trim();
      const mode = String(group?.__delete_mode || '').trim().toLowerCase() === 'purge' ? 'purge' : 'standard';
      if (!groupId) {
          return;
      }
      try {
          ctx.clearBeeroomRuntimeCachesByGroup(groupId);
          await ctx.beeroomStore.deleteGroup(groupId, { mode });
          await Promise.all([
              ctx.agentStore.loadAgents().catch(() => null),
              ctx.loadRunningAgents({ force: true }).catch(() => null)
          ]);
          ElMessage.success(ctx.t('beeroom.message.hiveDeleted'));
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.handleHivePackImportedFromMiddlePane = async (job: unknown) => {
      const record = job && typeof job === 'object' && !Array.isArray(job)
          ? (job as Record<string, unknown>)
          : {};
      const report = record.report && typeof record.report === 'object' && !Array.isArray(record.report)
          ? (record.report as Record<string, unknown>)
          : {};
      const groupId = String(report.hive_id || '').trim();
      if (!groupId) {
          return;
      }
      ctx.beeroomFirstEntryAutoSelectionPending.value = false;
      ctx.clearBeeroomRuntimeCachesByGroup(groupId);
      ctx.orderedBeeroomGroupsState.orderedKeys.value = ctx.normalizeStringListUnique([
          groupId,
          ...ctx.orderedBeeroomGroupsState.orderedKeys.value
      ]);
      await Promise.all([
          ctx.beeroomStore.selectGroup(groupId, { silent: true }).catch(() => null),
          ctx.agentStore.loadAgents().catch(() => null)
      ]);
      await ctx.prioritizeImportedBeeroomAgents(report, groupId);
      if (ctx.sessionHub.activeSection === 'agents') {
          ctx.selectedAgentHiveGroupId.value = groupId;
      }
  };

  ctx.openContactConversationFromList = async (contact: Record<string, unknown>) => {
      ctx.selectContact(contact);
      await ctx.openContactConversation(contact);
  };

  ctx.openWorldConversation = async (conversationId: string, kind: 'direct' | 'group', mode: 'detail' | 'messages' = 'detail') => {
      if (ctx.userWorldPermissionDenied.value)
          return;
      if (!conversationId)
          return;
      const perfTrace = ctx.startMessengerPerfTrace('openWorldConversation', {
          conversationId,
          kind,
          mode
      });
      try {
          if (mode === 'messages') {
              ctx.clearMiddlePaneOverlayHide();
              ctx.middlePaneOverlayVisible.value = false;
          }
          ctx.markMessengerPerfTrace(perfTrace, 'beforeActivate');
          const activateTask = ctx.userWorldStore.setActiveConversation(conversationId, { waitForLoad: false });
          ctx.markMessengerPerfTrace(perfTrace, 'afterActivateScheduled');
          ctx.sessionHub.setActiveConversation({ kind, id: conversationId });
          const section = mode === 'messages' ? 'messages' : kind === 'group' ? 'groups' : 'users';
          const nextQuery = { ...ctx.route.query, section, conversation_id: conversationId } as Record<string, any>;
          delete nextQuery.session_id;
          delete nextQuery.agent_id;
          delete nextQuery.entry;
          ctx.markMessengerPerfTrace(perfTrace, 'beforeRouteReplace');
          ctx.router.replace({
              path: mode === 'messages' ? ctx.resolveChatShellPath() : `${ctx.basePrefix.value}/user-world`,
              query: nextQuery
          }).catch(() => undefined);
          ctx.markMessengerPerfTrace(perfTrace, 'afterRouteReplace');
          await ctx.scrollMessagesToBottom(true);
          ctx.markMessengerPerfTrace(perfTrace, 'afterScrollBottom');
          ctx.finishMessengerPerfTrace(perfTrace, 'pending');
          void activateTask.then(() => {
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { phase: 'activateTask' });
          }, (error) => {
              ctx.finishMessengerPerfTrace(perfTrace, 'fail', {
                  phase: 'activateTask',
                  error: (error as {
                      message?: string;
                  })?.message || String(error)
              });
              showApiError(error, ctx.t('messenger.error.openConversation'));
          });
      }
      catch (error) {
          ctx.finishMessengerPerfTrace(perfTrace, 'fail', {
              phase: 'openWorldConversation',
              error: (error as {
                  message?: string;
              })?.message || String(error)
          });
          showApiError(error, ctx.t('messenger.error.openConversation'));
      }
  };

  ctx.openAgentById = async (agentId: unknown) => {
      const normalized = ctx.normalizeAgentId(agentId);
      ctx.clearAgentConversationDismissed(normalized);
      ctx.selectedAgentId.value = normalized;
      const goalLockedSessionId = ctx.resolveAgentGoalLockedSessionId(normalized);
      if (goalLockedSessionId) {
          await ctx.openAgentSession(goalLockedSessionId, normalized);
          return;
      }
      const preferredSessionId = ctx.resolvePreferredAgentSessionId(normalized);
      if (preferredSessionId) {
          await ctx.openAgentSession(preferredSessionId, normalized);
          return;
      }
      try {
          const freshSessionId = await ctx.openOrReuseFreshAgentSession(normalized);
          if (freshSessionId) {
              await ctx.openAgentSession(freshSessionId, normalized);
              return;
          }
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
      // Keep navigation usable when the backend is temporarily unavailable.
      await ctx.openAgentDraftSessionWithScroll(normalized);
  };

  ctx.openAgentDraftSession = (agentId: unknown) => {
      if (ctx.blockWhenAgentGoalLocked(agentId)) {
          return;
      }
      const normalized = ctx.normalizeAgentId(agentId);
      ctx.chatStore.openDraftSession({ agent_id: normalized === DEFAULT_AGENT_KEY ? '' : normalized });
      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneOverlayVisible.value = false;
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: `draft:${normalized}`,
          agentId: normalized
      });
      ctx.sessionHub.setSection('messages');
      const nextQuery = {
          ...ctx.route.query,
          section: 'messages',
          agent_id: normalized === DEFAULT_AGENT_KEY ? '' : normalized,
          entry: normalized === DEFAULT_AGENT_KEY ? 'default' : undefined
      } as Record<string, any>;
      delete nextQuery.conversation_id;
      delete nextQuery.session_id;
      ctx.router.replace({
          path: ctx.resolveChatShellPath(),
          query: nextQuery
      }).catch(() => undefined);
  };

  ctx.openAgentDraftSessionWithScroll = async (agentId: unknown) => {
      ctx.openAgentDraftSession(agentId);
      await ctx.scrollMessagesToBottom(true);
  };

  ctx.selectAgentForSettings = (agentId: unknown) => {
      ctx.agentOverviewMode.value = 'detail';
      ctx.selectedAgentId.value = ctx.normalizeAgentId(agentId);
  };

  ctx.toggleAgentOverviewMode = () => {
      ctx.agentOverviewMode.value = ctx.agentOverviewMode.value === 'grid' ? 'detail' : 'grid';
  };

  ctx.enterSelectedAgentConversation = async () => {
      const target = ctx.settingsAgentId.value || DEFAULT_AGENT_KEY;
      await ctx.openAgentById(target);
  };

  ctx.triggerAgentSettingsReload = () => {
      void ctx.agentSettingsPanelRef.value?.triggerReload();
  };

  ctx.triggerAgentSettingsDelete = () => {
      if (!ctx.canDeleteSettingsAgent.value)
          return;
      void ctx.agentSettingsPanelRef.value?.triggerDelete();
  };

  ctx.triggerAgentSettingsSave = () => {
      void ctx.agentSettingsPanelRef.value?.triggerSave();
  };

  ctx.triggerAgentSettingsExport = () => {
      void ctx.agentSettingsPanelRef.value?.triggerExportWorkerCard();
  };

  ctx.openActiveAgentSettings = (optionsOrEvent: {
      focusSection?: '' | 'model';
  } | Event = {}) => {
      const options = optionsOrEvent instanceof Event
          ? {}
          : optionsOrEvent;
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      if (options.focusSection === 'model') {
          ctx.requestAgentSettingsFocus('model');
      }
      ctx.agentOverviewMode.value = 'detail';
      ctx.selectedAgentId.value = targetAgentId;
      ctx.switchSection('agents');
      const nextQuery = {
          ...ctx.route.query,
          section: 'agents',
          agent_id: targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId
      } as Record<string, any>;
      delete nextQuery.session_id;
      delete nextQuery.entry;
      delete nextQuery.conversation_id;
      ctx.scheduleSectionRouteSync(ctx.resolveChatShellPath(), nextQuery);
  };

  ctx.updateAgentAbilityTooltip = async () => {
      await nextTick();
      const raw = ctx.agentAbilityTooltipRef.value;
      const tooltipRefs = Array.isArray(raw) ? raw : raw ? [raw] : [];
      tooltipRefs.forEach((tooltip) => {
          if (tooltip?.updatePopper) {
              tooltip.updatePopper();
          }
          else if (tooltip?.popperRef?.update) {
              tooltip.popperRef.update();
          }
      });
      requestAnimationFrame(() => {
          tooltipRefs.forEach((tooltip) => {
              if (tooltip?.updatePopper) {
                  tooltip.updatePopper();
              }
              else if (tooltip?.popperRef?.update) {
                  tooltip.popperRef.update();
              }
          });
      });
  };

  ctx.resolveActiveAgentPromptPreviewKey = (): string => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim() || 'draft';
      const agentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId);
      return `${sessionId}:${agentId}`;
  };

  ctx.fetchActiveAgentPromptPreviewPayload = async (options: {
      force?: boolean;
  } = {}): Promise<Record<string, unknown>> => {
      const force = options.force === true;
      const cacheKey = ctx.resolveActiveAgentPromptPreviewKey();
      const now = Date.now();
      if (!force && ctx.agentPromptPreviewPayloadCache &&
          ctx.agentPromptPreviewPayloadCache.key === cacheKey &&
          now - ctx.agentPromptPreviewPayloadCache.updatedAt <= ctx.AGENT_PROMPT_PREVIEW_CACHE_MS) {
          return ctx.agentPromptPreviewPayloadCache.payload;
      }
      if (ctx.agentPromptPreviewPayloadPromise && ctx.agentPromptPreviewPayloadPromiseKey === cacheKey) {
          return ctx.agentPromptPreviewPayloadPromise;
      }
      ctx.agentPromptPreviewPayloadPromiseKey = cacheKey;
      ctx.agentPromptPreviewPayloadPromise = (async () => {
          const currentAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId);
          const session = ctx.activeAgentSession.value as Record<string, unknown> | null;
          const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
          const sourceAgentId = ctx.normalizeAgentId(session?.agent_id || ctx.chatStore.draftAgentId || ctx.activeAgentId.value);
          const agentId = sourceAgentId === DEFAULT_AGENT_KEY ? '' : sourceAgentId;
          let previewAgentProfile = sourceAgentId === DEFAULT_AGENT_KEY
              ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)
              : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
                  (ctx.activeAgent.value as Record<string, unknown> | null));
          if (!sessionId) {
              if (sourceAgentId === DEFAULT_AGENT_KEY) {
                  if (!previewAgentProfile) {
                      previewAgentProfile =
                          ((await ctx.agentStore.getAgent(DEFAULT_AGENT_KEY).catch(() => null)) as Record<string, unknown> | null) ||
                              null;
                      ctx.defaultAgentProfile.value = previewAgentProfile;
                  }
              }
              else if (sourceAgentId) {
                  const hasConfiguredAbilities = resolveAgentConfiguredAbilityNames(previewAgentProfile).length > 0;
                  if (!hasConfiguredAbilities) {
                      previewAgentProfile =
                          ((await ctx.agentStore.getAgent(sourceAgentId).catch(() => null)) as Record<string, unknown> | null) ||
                              previewAgentProfile;
                      if (previewAgentProfile) {
                          ctx.activeAgentDetailProfile.value = previewAgentProfile;
                      }
                  }
              }
          }
          const previewAgentDefaults = ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(previewAgentProfile));
          const overrides = previewAgentDefaults.length > 0
              ? previewAgentDefaults
              : [AGENT_TOOL_OVERRIDE_NONE];
          const payload = sessionId
              ? {
                  ...(agentId ? { agent_id: agentId } : {})
              }
              : {
                  ...(agentId ? { agent_id: agentId } : {}),
                  ...(overrides ? { tool_overrides: overrides } : {})
              };
          const promptResult = sessionId
              ? await fetchSessionSystemPrompt(sessionId, payload)
              : await fetchRealtimeSystemPrompt(payload);
          const promptPayload = (promptResult?.data?.data || {}) as Record<string, unknown>;
          ctx.agentPromptPreviewPayloadCache = {
              key: cacheKey,
              payload: promptPayload,
              updatedAt: Date.now()
          };
          return promptPayload;
      })();
      try {
          return await ctx.agentPromptPreviewPayloadPromise;
      }
      finally {
          if (ctx.agentPromptPreviewPayloadPromiseKey === cacheKey) {
              ctx.agentPromptPreviewPayloadPromise = null;
              ctx.agentPromptPreviewPayloadPromiseKey = '';
          }
      }
  };

  ctx.syncAgentPromptPreviewSelectedNames = async (options: {
      force?: boolean;
  } = {}) => {
      if (ctx.agentPromptPreviewSelectedNames.value !== null && options.force !== true) {
          return ctx.agentPromptPreviewSelectedNames.value;
      }
      try {
          const promptPayload = await ctx.fetchActiveAgentPromptPreviewPayload(options);
          ctx.agentPromptPreviewSelectedNames.value = ctx.extractPromptPreviewSelectedAbilityNames(promptPayload);
          return ctx.agentPromptPreviewSelectedNames.value;
      }
      catch {
          ctx.agentPromptPreviewSelectedNames.value = null;
          return null;
      }
      finally {
          if (ctx.agentAbilityTooltipVisible.value) {
              await ctx.updateAgentAbilityTooltip();
          }
      }
  };

  ctx.clearRightDockSkillAutoRetry = () => {
      if (typeof window === 'undefined')
          return;
      if (ctx.rightDockSkillAutoRetryTimer !== null) {
          window.clearTimeout(ctx.rightDockSkillAutoRetryTimer);
          ctx.rightDockSkillAutoRetryTimer = null;
      }
  };

  ctx.scheduleRightDockSkillAutoRetry = () => {
      if (typeof window === 'undefined')
          return;
      if (ctx.rightDockSkillAutoRetryTimer !== null)
          return;
      ctx.rightDockSkillAutoRetryTimer = window.setTimeout(() => {
          ctx.rightDockSkillAutoRetryTimer = null;
          if (!ctx.showAgentRightDock.value)
              return;
          if (ctx.rightDockSkillCatalog.value.length > 0)
              return;
          void ctx.loadRightDockSkills({ force: true, silent: true });
      }, ctx.RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS);
  };

  ctx.openRightDockSkillDetail = async (name: unknown) => {
      const normalized = String(name || '').trim();
      if (!normalized)
          return;
      ctx.rightDockSelectedSkillName.value = normalized;
      ctx.rightDockSkillDialogVisible.value = true;
      ctx.rightDockSkillContent.value = '';
      ctx.rightDockSkillContentPath.value = String(ctx.rightDockSkillCatalog.value.find((item) => item.name === normalized)?.path || '').trim();
      const currentVersion = ++ctx.rightDockSkillContentLoadVersion;
      ctx.rightDockSkillContentLoading.value = true;
      try {
          const result = await fetchUserSkillContent(normalized);
          if (currentVersion !== ctx.rightDockSkillContentLoadVersion)
              return;
          const payload = (result?.data?.data || {}) as Record<string, unknown>;
          ctx.rightDockSkillContent.value = String(payload.content || '');
          ctx.rightDockSkillContentPath.value = String(payload.path || ctx.rightDockSkillContentPath.value || '').trim();
      }
      catch (error) {
          if (currentVersion !== ctx.rightDockSkillContentLoadVersion)
              return;
          ctx.rightDockSkillContent.value = '';
          ctx.rightDockSkillContentPath.value = '';
          showApiError(error, ctx.t('userTools.skills.file.readFailed', { message: ctx.t('common.requestFailed') }));
      }
      finally {
          if (currentVersion === ctx.rightDockSkillContentLoadVersion) {
              ctx.rightDockSkillContentLoading.value = false;
          }
      }
  };

  ctx.handleRightDockSkillEnabledToggle = async (value: unknown) => {
      const targetName = String(ctx.rightDockSelectedSkillName.value || '').trim();
      if (!targetName || ctx.rightDockSkillToggleSaving.value)
          return;
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId);
      if (!targetAgentId)
          return;
      const sourceProfile = targetAgentId === DEFAULT_AGENT_KEY
          ? ((ctx.defaultAgentProfile.value as Record<string, unknown> | null) ||
              ((await ctx.agentStore.getAgent(DEFAULT_AGENT_KEY, { force: true }).catch(() => null)) as Record<string, unknown> | null))
          : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
              (ctx.activeAgent.value as Record<string, unknown> | null) ||
              ((await ctx.agentStore.getAgent(targetAgentId, { force: true }).catch(() => null)) as Record<string, unknown> | null));
      if (!sourceProfile) {
          ElMessage.warning(ctx.t('chat.features.agentMissing'));
          return;
      }
      const nextToolNameSet = new Set<string>(ctx.normalizeRightDockSkillNameList(ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(sourceProfile))));
      if (Boolean(value)) {
          nextToolNameSet.add(targetName);
      }
      else {
          nextToolNameSet.delete(targetName);
      }
      const nextToolNames = Array.from(nextToolNameSet).sort((left, right) => left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' }));
      const dependencyPayload = buildDeclaredDependencyPayload(nextToolNames, sourceProfile, (ctx.agentPromptToolSummary.value || {}) as Record<string, unknown>);
      ctx.rightDockSkillToggleSaving.value = true;
      try {
          const updated = (await ctx.agentStore.updateAgent(targetAgentId, {
              tool_names: dependencyPayload.tool_names,
              declared_tool_names: dependencyPayload.declared_tool_names,
              declared_skill_names: dependencyPayload.declared_skill_names
          })) as Record<string, unknown> | null;
          if (targetAgentId === DEFAULT_AGENT_KEY) {
              ctx.defaultAgentProfile.value = updated;
          }
          else if (targetAgentId === ctx.activeAgentId.value) {
              ctx.activeAgentDetailProfile.value = updated;
          }
          await ctx.loadAgentToolSummary({ force: true });
      }
      catch (error) {
          showApiError(error, ctx.t('portal.agent.saveFailed'));
      }
      finally {
          ctx.rightDockSkillToggleSaving.value = false;
      }
  };

  ctx.isUserToolsScopeForAgentSummary = (scope: unknown): boolean => {
      const normalized = String(scope || '').trim().toLowerCase();
      if (!normalized || normalized === 'all')
          return true;
      return normalized === 'skills' || normalized === 'mcp' || normalized === 'knowledge';
  };

  ctx.handleUserToolsUpdatedEvent = (event: CustomEvent<{
      scope?: string;
      action?: string;
  }>) => {
      const scope = event?.detail?.scope;
      if (!ctx.isUserToolsScopeForAgentSummary(scope)) {
          return;
      }
      ctx.agentToolSummaryPromise = null;
      ctx.agentToolSummaryLoading.value = false;
      invalidateUserToolsCatalogCache();
      invalidateUserToolsSummaryCache();
      invalidateUserSkillsCache();
      void ctx.loadAgentToolSummary({ force: true });
      void ctx.loadRightDockSkills({ force: true, silent: true });
      if (ctx.sessionHub.activeSection === 'tools') {
          void ctx.loadToolsCatalog({ silent: true });
      }
  };

  ctx.handleChatPageRefresh = () => {
      if (ctx.isMessengerInteractionBlocked.value) {
          return;
      }
      void ctx.refreshActiveAgentConversation();
  };

  ctx.handleRightDockSkillArchiveUpload = async (file: File) => {
      if (!file || ctx.skillDockUploading.value)
          return;
      const filename = String(file.name || '').trim().toLowerCase();
      if (!ctx.SUPPORTED_SKILL_ARCHIVE_SUFFIXES.some((suffix) => filename.endsWith(suffix))) {
          ElMessage.warning(ctx.t('userTools.skills.upload.zipOnly'));
          return;
      }
      ctx.skillDockUploading.value = true;
      try {
          await uploadUserSkillZip(file);
          ctx.agentToolSummaryPromise = null;
          ctx.agentToolSummaryLoading.value = false;
          invalidateUserSkillsCache();
          invalidateUserToolsSummaryCache();
          invalidateUserToolsCatalogCache();
          await ctx.loadRightDockSkills({ force: true, silent: true });
          void ctx.loadAgentToolSummary({ force: true });
          emitUserToolsUpdated({ scope: 'skills', action: 'upload' });
          ElMessage.success(ctx.t('userTools.skills.upload.success'));
      }
      catch (error) {
          showApiError(error, ctx.t('userTools.skills.upload.failed'));
      }
      finally {
          ctx.skillDockUploading.value = false;
      }
  };

  ctx.handleAgentAbilityTooltipShow = () => {
      ctx.agentAbilityTooltipVisible.value = true;
      void ctx.loadAgentToolSummary();
      void ctx.syncAgentPromptPreviewSelectedNames();
      void ctx.updateAgentAbilityTooltip();
  };

  ctx.handleAgentAbilityTooltipHide = () => {
      ctx.agentAbilityTooltipVisible.value = false;
  };

  ctx.openAgentPromptPreview = async () => {
      ctx.agentPromptPreviewVisible.value = true;
      ctx.agentPromptPreviewLoading.value = true;
      ctx.agentPromptPreviewContent.value = '';
      ctx.agentPromptPreviewMemoryMode.value = 'none';
      ctx.agentPromptPreviewToolingMode.value = '';
      ctx.agentPromptPreviewToolingContent.value = '';
      ctx.agentPromptPreviewToolingItems.value = [];
      const summaryPromise = ctx.loadAgentToolSummary();
      try {
          const promptPayload = await ctx.fetchActiveAgentPromptPreviewPayload();
          ctx.agentPromptPreviewSelectedNames.value = ctx.extractPromptPreviewSelectedAbilityNames(promptPayload);
          ctx.agentPromptPreviewContent.value = String(promptPayload.prompt || '').replace(/<<WUNDER_HISTORY_MEMORY>>/g, '');
          const nextMode = String(promptPayload.memory_preview_mode || 'none').trim().toLowerCase();
          ctx.agentPromptPreviewMemoryMode.value =
              nextMode === 'frozen' || nextMode === 'pending' ? nextMode : 'none';
          const toolingPreview = extractPromptToolingPreview(promptPayload);
          ctx.agentPromptPreviewToolingMode.value = toolingPreview.mode;
          ctx.agentPromptPreviewToolingContent.value = toolingPreview.text;
          ctx.agentPromptPreviewToolingItems.value = toolingPreview.items;
          void summaryPromise.catch(() => null);
      }
      catch (error) {
          showApiError(error, ctx.t('chat.systemPromptFailed'));
          ctx.agentPromptPreviewSelectedNames.value = null;
          ctx.agentPromptPreviewContent.value = '';
          ctx.agentPromptPreviewMemoryMode.value = 'none';
          ctx.agentPromptPreviewToolingMode.value = '';
          ctx.agentPromptPreviewToolingContent.value = '';
          ctx.agentPromptPreviewToolingItems.value = [];
      }
      finally {
          ctx.agentPromptPreviewLoading.value = false;
      }
  };

  ctx.openContactConversation = async (targetContact: Record<string, unknown> | null | undefined) => {
      if (ctx.userWorldPermissionDenied.value) {
          ElMessage.warning(ctx.t('auth.login.noPermission'));
          return;
      }
      if (!targetContact)
          return;
      const perfTrace = ctx.startMessengerPerfTrace('openSelectedContactConversation', {
          selectedContactUserId: String(targetContact?.user_id || '').trim()
      });
      const peerUserId = String(targetContact.user_id || '').trim();
      const listMatchedConversationId = (Array.isArray(ctx.userWorldStore.conversations) ? ctx.userWorldStore.conversations : [])
          .find((item) => {
          const kind = String(item?.conversation_type || '').trim().toLowerCase();
          return kind !== 'group' && String(item?.peer_user_id || '').trim() === peerUserId;
      })
          ?.conversation_id;
      const conversationId = String(targetContact.conversation_id || listMatchedConversationId || '').trim();
      if (conversationId) {
          ctx.markMessengerPerfTrace(perfTrace, 'hitExistingConversation');
          await ctx.openWorldConversation(conversationId, 'direct', 'messages');
          ctx.finishMessengerPerfTrace(perfTrace, 'ok', { reusedConversation: true });
          return;
      }
      if (!peerUserId)
          return;
      try {
          ctx.markMessengerPerfTrace(perfTrace, 'callOpenConversationByPeer');
          const conversation = await ctx.userWorldStore.openConversationByPeer(peerUserId, {
              waitForLoad: false,
              activate: false
          });
          ctx.markMessengerPerfTrace(perfTrace, 'returnedOpenConversationByPeer');
          const targetConversationId = String((conversation as Record<string, unknown> | null)?.conversation_id || ctx.userWorldStore.activeConversationId || '').trim();
          if (targetConversationId) {
              await ctx.openWorldConversation(targetConversationId, 'direct', 'messages');
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { reusedConversation: false });
              return;
          }
          ctx.finishMessengerPerfTrace(perfTrace, 'fail', { phase: 'missingConversationId' });
      }
      catch (error) {
          ctx.finishMessengerPerfTrace(perfTrace, 'fail', {
              phase: 'openConversationByPeer',
              error: (error as {
                  message?: string;
              })?.message || String(error)
          });
          showApiError(error, ctx.t('userWorld.contact.openFailed'));
      }
  };

  ctx.openSelectedContactConversation = async () => {
      await ctx.openContactConversation(ctx.selectedContact.value);
  };

  ctx.openSelectedGroupConversation = async () => {
      if (ctx.userWorldPermissionDenied.value) {
          ElMessage.warning(ctx.t('auth.login.noPermission'));
          return;
      }
      if (!ctx.selectedGroup.value)
          return;
      const conversationId = String(ctx.selectedGroup.value.conversation_id || '').trim();
      if (!conversationId)
          return;
      await ctx.openWorldConversation(conversationId, 'group', 'messages');
  };

  ctx.submitGroupCreate = async () => {
      if (ctx.userWorldPermissionDenied.value) {
          ElMessage.warning(ctx.t('auth.login.noPermission'));
          return;
      }
      const groupName = String(ctx.groupCreateName.value || '').trim();
      const members = ctx.groupCreateMemberIds.value
          .map((item) => String(item || '').trim())
          .filter((item) => Boolean(item));
      if (!groupName) {
          ElMessage.warning(ctx.t('userWorld.group.namePlaceholder'));
          return;
      }
      if (!members.length) {
          ElMessage.warning(ctx.t('userWorld.group.memberEmpty'));
          return;
      }
      ctx.groupCreating.value = true;
      try {
          const created = await ctx.userWorldStore.createGroupConversation(groupName, members);
          ctx.groupCreateVisible.value = false;
          ctx.groupCreateName.value = '';
          ctx.groupCreateKeyword.value = '';
          ctx.groupCreateMemberIds.value = [];
          ElMessage.success(ctx.t('userWorld.group.createSuccess'));
          const conversationId = String(created?.conversation_id || '').trim();
          if (conversationId) {
              await ctx.openWorldConversation(conversationId, 'group', 'messages');
          }
          else {
              await ctx.userWorldStore.refreshGroups();
          }
      }
      catch (error) {
          showApiError(error, ctx.t('userWorld.group.createFailed'));
      }
      finally {
          ctx.groupCreating.value = false;
      }
  };

  ctx.openAgentSession = async (sessionId: string, agentId = '') => {
      if (!sessionId)
          return;
      const normalizedSessionId = String(sessionId || '').trim();
      if (!normalizedSessionId)
          return;
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const knownSession = ctx.chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
      const fallbackAgentId = agentId
          ? ctx.normalizeAgentId(agentId)
          : ctx.resolveSessionAgentId(knownSession, ctx.chatStore.draftAgentId);
      if (activeSessionId && normalizedSessionId !== activeSessionId && ctx.blockWhenAgentGoalLocked(fallbackAgentId || DEFAULT_AGENT_KEY, normalizedSessionId)) {
          return;
      }
      const perfTrace = ctx.startMessengerPerfTrace('openAgentSession', { sessionId: normalizedSessionId, agentId });
      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneOverlayVisible.value = false;
      ctx.clearAgentConversationDismissed(fallbackAgentId);
      ctx.selectedAgentId.value = fallbackAgentId || DEFAULT_AGENT_KEY;
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: normalizedSessionId,
          agentId: fallbackAgentId || DEFAULT_AGENT_KEY
      });
      const nextQuery = {
          ...ctx.route.query,
          section: 'messages',
          session_id: normalizedSessionId,
          agent_id: fallbackAgentId === DEFAULT_AGENT_KEY ? '' : fallbackAgentId
      } as Record<string, any>;
      delete nextQuery.conversation_id;
      ctx.router.replace({
          path: ctx.resolveChatShellPath(),
          query: nextQuery
      }).catch(() => undefined);
      const isForegroundSession = () => String(ctx.chatStore.activeSessionId || '').trim() === normalizedSessionId;
      try {
          ctx.markMessengerPerfTrace(perfTrace, 'beforeLoadSessionDetail');
          let sessionDetail = null;
          let sessionDetailError: unknown = null;
          const sessionDetailTask = ctx.chatStore.loadSessionDetail(normalizedSessionId)
              .then((value) => {
              sessionDetail = value;
          })
              .catch((error) => {
              sessionDetailError = error;
          });
          ctx.markMessengerPerfTrace(perfTrace, 'loadSessionDetailScheduled');
          await ctx.scrollMessagesToBottom(true);
          ctx.markMessengerPerfTrace(perfTrace, 'uiReady');
          await sessionDetailTask;
          if (sessionDetailError) {
              throw sessionDetailError;
          }
          ctx.markMessengerPerfTrace(perfTrace, 'afterLoadSessionDetail');
          if (!isForegroundSession()) {
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { stale: true });
              return;
          }
          if (!sessionDetail) {
              await ctx.openAgentById(fallbackAgentId || DEFAULT_AGENT_KEY);
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { recovered: true });
              return;
          }
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
          const targetAgentId = ctx.normalizeAgentId(session?.agent_id ?? fallbackAgentId);
          ctx.refreshSessionPreviewCache(normalizedSessionId, (session || sessionDetail || null) as Record<string, unknown> | null);
          ctx.selectedAgentId.value = targetAgentId || DEFAULT_AGENT_KEY;
          ctx.sessionHub.setActiveConversation({
              kind: 'agent',
              id: normalizedSessionId,
              agentId: targetAgentId || DEFAULT_AGENT_KEY
          });
          const mainEntry = ctx.collectMainAgentSessionEntries().find((item) => item.agentId === targetAgentId);
          if (mainEntry?.sessionId === normalizedSessionId) {
              ctx.setAgentMainReadAt(targetAgentId, mainEntry.lastAt || Date.now());
              ctx.setAgentMainUnreadCount(targetAgentId, 0);
              ctx.persistAgentUnreadState();
          }
          ctx.finishMessengerPerfTrace(perfTrace, 'ok');
      }
      catch (error) {
          if (!isForegroundSession()) {
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { stale: true });
              return;
          }
          ctx.finishMessengerPerfTrace(perfTrace, 'fail', {
              error: (error as {
                  message?: string;
              })?.message || String(error)
          });
          showApiError(error, ctx.t('messenger.error.openConversation'));
      }
  };
}
