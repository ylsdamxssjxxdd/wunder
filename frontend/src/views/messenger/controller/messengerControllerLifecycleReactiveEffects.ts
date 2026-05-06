// @ts-nocheck
// Cross-domain watchers, mounted listeners, realtime pulse wiring, and unmount cleanup.
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

export function installMessengerControllerLifecycleReactiveEffects(ctx: MessengerControllerContext): void {
  watch(() => String(ctx.chatStore.activeSessionId || '').trim(), (sessionId) => {
      if (!sessionId) {
          ctx.agentGoalComposerRequested.value = false;
          ctx.agentGoalComposerVisible.value = false;
          ctx.agentGoalComposerObjective.value = '';
          ctx.goalDialogObjective.value = '';
          return;
      }
      const goal = typeof ctx.chatStore.sessionGoal === 'function'
          ? ctx.chatStore.sessionGoal(sessionId)
          : null;
      const objective = String(goal?.objective || '').trim();
      if (objective) {
          ctx.goalDialogSessionId.value = sessionId;
          ctx.goalDialogObjective.value = objective;
          if (!ctx.goalDialogSubmitting.value) {
              ctx.agentGoalComposerObjective.value = objective;
          }
          ctx.agentGoalComposerRequested.value = false;
          ctx.agentGoalComposerVisible.value = true;
          return;
      }
      if (!ctx.goalDialogSubmitting.value && !ctx.agentGoalComposerRequested.value) {
          ctx.agentGoalComposerVisible.value = false;
          ctx.agentGoalComposerObjective.value = '';
          ctx.goalDialogObjective.value = '';
      }
  }, { immediate: true });

  watch(() => ctx.activeSessionGoal.value, (goal) => {
      const status = String(goal?.status || '').trim().toLowerCase();
      const objective = String(goal?.objective || '').trim();
      if (objective) {
          ctx.goalDialogObjective.value = objective;
          if (!ctx.goalDialogSubmitting.value) {
              ctx.agentGoalComposerObjective.value = objective;
          }
          ctx.agentGoalComposerRequested.value = false;
          ctx.agentGoalComposerVisible.value = status !== 'complete';
          return;
      }
      if (!ctx.goalDialogSubmitting.value && !ctx.agentGoalComposerRequested.value) {
          ctx.agentGoalComposerVisible.value = false;
          ctx.agentGoalComposerObjective.value = '';
          ctx.goalDialogObjective.value = '';
      }
  }, { immediate: true });

  watch(() => ctx.currentUserId.value, (value, previousValue) => {
      const changed = String(value || '') !== String(previousValue || '');
      const shouldClearConversationState = changed && ctx.currentUserContextInitialized && !ctx.bootLoading.value;
      ctx.currentUserContextInitialized = true;
      if (changed) {
          ctx.chatStore.resetState();
      }
      if (shouldClearConversationState) {
          ctx.sessionHub.clearActiveConversation();
          ctx.userWorldStore.activeConversationId = '';
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.session_id;
          delete nextQuery.conversation_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
      }
      ctx.beeroomStore.resetState();
      ctx.beeroomFirstEntryAutoSelectionPending.value = true;
      ctx.beeroomGroupsLastRefreshAt = 0;
      ctx.selectedAgentHiveGroupId.value = '';
      void ctx.hydrateCurrentUserAppearance();
      void ctx.hydrateMessengerOrderPreferences();
      ctx.cronPermissionDenied.value = false;
      ctx.cronAgentIds.value = new Set<string>();
      ctx.timelineDialogVisible.value = false;
      ctx.skillDockUploading.value = false;
      ctx.agentPromptToolSummary.value = null;
      ctx.agentToolSummaryLoading.value = false;
      ctx.rightDockSkillCatalog.value = [];
      ctx.rightDockSkillDialogVisible.value = false;
      ctx.rightDockSelectedSkillName.value = '';
      ctx.rightDockSkillContent.value = '';
      ctx.rightDockSkillContentPath.value = '';
      ctx.rightDockSkillCatalogLoading.value = false;
      ctx.rightDockSkillContentLoading.value = false;
      ctx.rightDockSkillToggleSaving.value = false;
      ctx.clearRightDockSkillAutoRetry();
      ctx.rightDockSkillCatalogLoadVersion += 1;
      ctx.rightDockSkillContentLoadVersion += 1;
      ctx.plazaStore.$reset();
      ctx.plazaBrowseKind.value = 'hive_pack';
      ctx.selectedPlazaItemId.value = '';
      ctx.agentToolSummaryPromise = null;
      invalidateAllUserToolsCaches();
      ctx.clearWorkspaceResourceCache();
      ctx.ensureDismissedAgentConversationState(true);
      ctx.ensureAgentUnreadState(true);
      ctx.refreshAgentMainUnreadFromSessions();
      ctx.warmMessengerUserToolsData({
          catalog: ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools',
          skills: ctx.showAgentRightDock.value,
          summary: ctx.sessionHub.activeSection === 'agents' || ctx.showAgentRightDock.value
      });
      ctx.scheduleWorkspaceResourceHydration();
  }, { immediate: true });

  watch(() => ctx.userAttachmentWorkspacePaths.value, (paths) => {
      paths.forEach((path) => {
          void ctx.ensureUserAttachmentResource(path);
      });
  }, { immediate: true });

  watch(() => [ctx.themeStore.palette], () => {
      if (ctx.appearanceHydrating.value)
          return;
      void ctx.persistCurrentUserAppearance();
  });

  watch(() => [
      ctx.orderedMixedConversationsState.orderedKeys.value.join('\n'),
      ctx.orderedOwnedAgentsState.orderedKeys.value.join('\n'),
      ctx.orderedSharedAgentsState.orderedKeys.value.join('\n'),
      ctx.orderedBeeroomGroupsState.orderedKeys.value.join('\n')
  ], () => {
      if (ctx.messengerOrderHydrating.value || !ctx.messengerOrderReady.value) {
          chatDebugLog('messenger.order', 'watch-skip', {
              hydrating: ctx.messengerOrderHydrating.value,
              ready: ctx.messengerOrderReady.value
          });
          return;
      }
      const current = ctx.captureMessengerOrderPreferences();
      const snapshot = ctx.messengerOrderSnapshot.value;
      if (current.messages.join('\n') === snapshot.messages.join('\n') &&
          current.agentsOwned.join('\n') === snapshot.agentsOwned.join('\n') &&
          current.agentsShared.join('\n') === snapshot.agentsShared.join('\n') &&
          current.swarms.join('\n') === snapshot.swarms.join('\n')) {
          chatDebugLog('messenger.order', 'watch-no-change', {
              current,
              snapshot
          });
          return;
      }
      chatDebugLog('messenger.order', 'watch-change', {
          current,
          snapshot
      });
      ctx.scheduleMessengerOrderPersist();
  }, { deep: false });

  watch(() => ctx.sessionHub.activeSection, (section) => {
      ctx.closeFileContainerMenu();
      if (!ctx.isSearchableMiddlePaneSection(section) && (ctx.keywordInput.value || ctx.sessionHub.keyword)) {
          ctx.clearKeywordDebounce();
          ctx.keywordInput.value = '';
          ctx.sessionHub.setKeyword('');
      }
      if (section === 'swarms') {
          ctx.stopRealtimePulse?.();
          ctx.beeroomGroupsLastRefreshAt = 0;
          ctx.startBeeroomRealtimeSync?.();
          ctx.triggerBeeroomRealtimeSyncRefresh?.('enter-swarms');
      }
      else {
          ctx.stopBeeroomRealtimeSync?.();
          ctx.startRealtimePulse?.();
          ctx.triggerRealtimePulseRefresh?.(`enter-${section}`);
      }
      if (section === 'tools' &&
          !ctx.builtinTools.value.length &&
          !ctx.mcpTools.value.length &&
          !ctx.skillTools.value.length &&
          !ctx.knowledgeTools.value.length) {
          ctx.loadToolsCatalog();
      }
      if (section === 'agents') {
          ctx.warmMessengerUserToolsData({
              catalog: true,
              summary: true
          });
          void ctx.loadChannelBoundAgentIds();
          if (!ctx.cronPermissionDenied.value) {
              void ctx.loadCronAgentIds();
          }
      }
      if (section === 'tools') {
          void ctx.loadChannelBoundAgentIds();
          if (!ctx.cronPermissionDenied.value) {
              void ctx.loadCronAgentIds();
          }
      }
      if (section === 'plaza' && !ctx.plazaStore.items.length) {
          void ctx.plazaStore.loadItems()
              .then(() => ctx.ensureSectionSelection())
              .catch(() => null);
      }
      if (section === 'more') {
          void preloadMessengerSettingsPanels({ desktopMode: ctx.desktopMode.value });
      }
      if (section === 'users' && !ctx.userWorldPermissionDenied.value) {
          ctx.resetContactVirtualScroll();
          void nextTick(ctx.syncContactVirtualMetrics);
      }
      if (section === 'swarms') {
          if (!ctx.beeroomStore.groups.length) {
              void ctx.beeroomStore.loadGroups()
                  .then(() => ctx.ensureSectionSelection())
                  .catch(() => null);
          }
          if (ctx.beeroomStore.activeGroupId) {
              void ctx.beeroomStore.loadActiveGroup().catch(() => null);
          }
      }
      ctx.ensureSectionSelection();
  }, { immediate: true });

  watch(() => ctx.beeroomStore.activeGroupId, (value) => {
      if (!['swarms', 'orchestrations'].includes(ctx.sessionHub.activeSection) || !String(value || '').trim())
          return;
      void ctx.beeroomStore.loadActiveGroup({ silent: true }).catch(() => null);
  });

  watch(() => [
      String(ctx.beeroomStore.activeGroupId || '').trim(),
      String(ctx.beeroomStore.activeGroup?.latest_mission?.parent_session_id || '').trim(),
      String(ctx.beeroomStore.activeGroup?.latest_mission?.team_run_id || '').trim()
  ] as const, ([groupId, parentSessionId]) => {
      if (!groupId) {
          return;
      }
      ctx.rememberBeeroomDispatchSessionIds(groupId, [parentSessionId]);
  }, { immediate: true });

  watch(ctx.plazaBrowseKind, (nextKind, previousKind) => {
      if (nextKind !== previousKind) {
          ctx.selectedPlazaItemId.value = '';
      }
  });

  watch(() => ctx.sessionHub.activeSection, (section, previousSection) => {
      if (previousSection === 'plaza' && section !== 'plaza') {
          ctx.selectedPlazaItemId.value = '';
      }
  });

  watch(() => ctx.showAgentGridOverview.value, (visible) => {
      if (visible) {
          ctx.loadAgentUserRounds();
      }
  });

  watch(() => ctx.hasHotRuntimeState.value, (hot, previousHot) => {
      if (hot) {
          if (ctx.sessionHub.activeSection === 'swarms' || ctx.sessionHub.activeSection === 'orchestrations') {
              ctx.triggerBeeroomRealtimeSyncRefresh?.('hot-runtime');
              return;
          }
          ctx.triggerRealtimePulseRefresh?.('hot-runtime');
          return;
      }
      if (previousHot && !hot) {
          void ctx.loadRunningAgents({ force: true });
      }
  });

  watch(() => ctx.hasHotBeeroomRuntimeState.value, (hot) => {
      if (!hot || !['swarms', 'orchestrations'].includes(ctx.sessionHub.activeSection))
          return;
      ctx.triggerBeeroomRealtimeSyncRefresh?.('hot-beeroom');
  });

  watch(() => [ctx.filteredContacts.value.length, ctx.sessionHub.activeSection, ctx.userWorldPermissionDenied.value], () => {
      if (ctx.sessionHub.activeSection !== 'users' || ctx.userWorldPermissionDenied.value)
          return;
      void nextTick(ctx.syncContactVirtualMetrics);
  });

  watch(() => [ctx.keyword.value, ctx.selectedContactUnitId.value], () => {
      if (ctx.sessionHub.activeSection !== 'users' || ctx.userWorldPermissionDenied.value)
          return;
      ctx.resetContactVirtualScroll();
      void nextTick(ctx.syncContactVirtualMetrics);
  });

  watch(ctx.agentHiveTreeRows, (rows) => {
      if (!ctx.selectedAgentHiveGroupId.value)
          return;
      const exists = rows.some((row) => row.id === ctx.selectedAgentHiveGroupId.value);
      if (!exists) {
          ctx.selectedAgentHiveGroupId.value = '';
      }
  });

  watch(ctx.visibleAgentIdsForSelection, () => {
      if (ctx.sessionHub.activeSection !== 'agents')
          return;
      ctx.ensureSectionSelection();
  });

  watch(() => ctx.filteredGroups.value.map((item) => String(item?.group_id || '')).join('|'), () => {
      if (ctx.sessionHub.activeSection !== 'groups')
          return;
      ctx.ensureSectionSelection();
  });

  watch(() => ctx.filteredBeeroomGroupsOrdered.value
      .map((item) => String(item?.group_id || item?.hive_id || ''))
      .join('|'), () => {
      if (!['swarms', 'orchestrations'].includes(ctx.sessionHub.activeSection))
          return;
      ctx.ensureSectionSelection();
  });

  watch(() => ctx.filteredPlazaItems.value.map((item) => String(item?.item_id || '')).join('|'), () => {
      if (ctx.sessionHub.activeSection !== 'plaza')
          return;
      ctx.ensureSectionSelection();
  });

  watch(() => [
      ctx.sessionHub.activeSection,
      ctx.sessionHub.activeConversationKey,
      ctx.chatStore.activeSessionId,
      ctx.chatStore.draftAgentId,
      ctx.route.query?.conversation_id
  ], () => {
      ctx.syncAgentConversationFallback();
  }, { immediate: true });

  watch(() => [
      ctx.chatStore.sessions
          .map((session) => [
          String(session?.id || ''),
          ctx.normalizeAgentId(session?.agent_id),
          session?.is_main ? '1' : '0',
          String(session?.last_message_at || session?.updated_at || session?.created_at || '')
      ].join(':'))
          .join('|'),
      ctx.sessionHub.activeConversationKey
  ], () => {
      ctx.refreshAgentMainUnreadFromSessions();
  }, { immediate: true });

  watch(() => [ctx.hasAnyMixedConversations.value, ctx.sessionHub.activeSection, ctx.sessionHub.activeConversationKey], () => {
      ctx.clearMessagePanelWhenConversationEmpty();
  }, { immediate: true });

  watch(() => [
      ctx.filteredContacts.value.length,
      ctx.filteredGroups.value.length,
      ctx.filteredPlazaItems.value.length,
      ctx.filteredOwnedAgentsOrdered.value.length,
      ctx.filteredSharedAgentsOrdered.value.length,
      ctx.showDefaultAgentEntry.value ? 1 : 0
  ], () => {
      ctx.ensureSectionSelection();
  });

  watch(() => ctx.sessionHub.activeConversationKey, () => {
      ctx.markdownCache.clear();
      ctx.clearWorkspaceResourceCache();
      ctx.pendingAssistantCenter = false;
      ctx.pendingAssistantCenterCount = 0;
      ctx.agentPlanExpanded.value = false;
      ctx.dismissedPlanMessages.value = new WeakSet<Record<string, unknown>>();
      ctx.dismissedPlanVersion.value += 1;
      ctx.agentInquirySelection.value = [];
      ctx.scheduleWorkspaceResourceHydration();
  });

  watch(() => ctx.activeAgentPlan.value, (value) => {
      if (!value) {
          ctx.agentPlanExpanded.value = false;
      }
  });

  watch(() => ctx.activeAgentInquiryPanel.value, (value) => {
      if (!value) {
          ctx.agentInquirySelection.value = [];
      }
  });

  watch(() => ctx.chatStore.activeSessionId, (value) => {
      if (!value || ctx.sessionHub.activeSection !== 'messages')
          return;
      if (ctx.activeConversation.value?.kind === 'direct' || ctx.activeConversation.value?.kind === 'group')
          return;
      const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === String(value));
      ctx.selectedAgentId.value = ctx.normalizeAgentId(session?.agent_id ?? ctx.activeAgentId.value);
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: String(value),
          agentId: ctx.normalizeAgentId(session?.agent_id ?? ctx.activeAgentId.value)
      });
  });

  watch(() => ctx.currentContainerId.value, (value) => {
      if (ctx.fileScope.value !== 'agent')
          return;
      if (ctx.sessionHub.activeSection === 'files')
          return;
      ctx.selectedFileContainerId.value = value;
  }, { immediate: true });

  watch(() => [ctx.timelineDialogVisible.value, ctx.rightPanelSessionHistory.value.map((item) => item.id).join('|')] as const, ([visible, value]) => {
      if (!visible || !value) {
          if (typeof window !== 'undefined' && ctx.timelinePrefetchTimer) {
              window.clearTimeout(ctx.timelinePrefetchTimer);
              ctx.timelinePrefetchTimer = null;
          }
          return;
      }
      if (typeof window !== 'undefined' && ctx.timelinePrefetchTimer) {
          window.clearTimeout(ctx.timelinePrefetchTimer);
          ctx.timelinePrefetchTimer = null;
      }
      const prefetchTargets = ctx.rightPanelSessionHistory.value.slice(0, 4).map((item) => item.id);
      const runPrefetch = () => {
          prefetchTargets.forEach((sessionId) => {
              void ctx.preloadTimelinePreview(sessionId);
          });
      };
      if (typeof window !== 'undefined') {
          ctx.timelinePrefetchTimer = window.setTimeout(() => {
              ctx.timelinePrefetchTimer = null;
              runPrefetch();
          }, 80);
          return;
      }
      runPrefetch();
  }, { immediate: true });

  watch(() => [
      ctx.sessionHub.activeSection,
      ctx.filteredMixedConversations.value
          .filter((item) => item.kind === 'agent')
          .slice(0, 4)
          .map((item) => `${item.agentId}:${String(item.sourceId || '').trim()}`)
          .join('|')
  ] as const, ([section, key]) => {
      if (section !== 'messages' || !key) {
          return;
      }
      ctx.filteredMixedConversations.value
          .filter((item) => item.kind === 'agent')
          .slice(0, 4)
          .forEach((item) => {
          ctx.preloadMixedConversation(item);
      });
  }, { immediate: true });

  watch(() => ctx.rightDockSkillDialogVisible.value, (visible) => {
      if (visible)
          return;
      ctx.rightDockSkillContentLoadVersion += 1;
      ctx.rightDockSkillContentLoading.value = false;
      ctx.rightDockSkillToggleSaving.value = false;
      ctx.rightDockSkillContent.value = '';
      ctx.rightDockSkillContentPath.value = '';
  });

  watch(() => [ctx.chatStore.activeSessionId, ctx.chatStore.messages.length], () => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return;
      const activeSession = (Array.isArray(ctx.chatStore.sessions)
          ? ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId)
          : null) || null;
      ctx.refreshSessionPreviewCache(sessionId, (activeSession || null) as Record<string, unknown> | null);
  });

  watch(() => ctx.showChatSettingsView.value, () => {
      ctx.scheduleMessageViewportRefresh({
          updateScrollState: true,
          measure: true
      });
  });

  watch(() => [ctx.chatStore.messages.length, ctx.userWorldStore.activeMessages.length, ctx.sessionHub.activeConversationKey], () => {
      ctx.pruneMessageVirtualHeightCache();
      void nextTick(() => {
          ctx.scheduleMessageViewportRefresh({
              measure: true
          });
      });
      ctx.scheduleWorkspaceResourceHydration();
      if (ctx.pendingAssistantCenter &&
          ctx.isAgentConversationActive.value &&
          ctx.chatStore.messages.length > ctx.pendingAssistantCenterCount) {
          const lastMessage = ctx.chatStore.messages[ctx.chatStore.messages.length - 1] as Record<string, unknown> | undefined;
          if (String(lastMessage?.role || '') === 'assistant') {
              ctx.pendingAssistantCenter = false;
              ctx.pendingAssistantCenterCount = ctx.chatStore.messages.length;
              ctx.autoStickToBottom.value = false;
              void ctx.scrollLatestAssistantToCenter();
              return;
          }
      }
      if (ctx.autoStickToBottom.value) {
          void ctx.scrollMessagesToBottom();
      }
      else {
          ctx.updateMessageScrollState();
      }
  });

  watch(() => {
      const latestMessage = ctx.chatStore.messages[ctx.chatStore.messages.length - 1] as Record<string, unknown> | undefined;
      return [
          ctx.chatStore.activeSessionId,
          ctx.latestAgentRenderableMessageKey.value,
          ctx.buildLatestAssistantLayoutSignature(latestMessage)
      ].join('::');
  }, () => {
      ctx.scheduleWorkspaceResourceHydration();
      ctx.refreshLatestAssistantMessageLayout('latest-assistant-signature');
  }, { flush: 'post' });

  watch(() => ctx.userWorldStore.activeMessages[ctx.userWorldStore.activeMessages.length - 1]?.content, () => {
      ctx.scheduleWorkspaceResourceHydration();
      const latestMessageKey = ctx.latestWorldRenderableMessageKey.value;
      ctx.scheduleMessageViewportRefresh({
          measure: true,
          measureKeys: latestMessageKey ? [latestMessageKey] : undefined
      });
  });

  watch(() => [ctx.agentRenderableMessages.value.length, ctx.worldRenderableMessages.value.length], () => {
      ctx.pruneMessageVirtualHeightCache();
      void nextTick(() => {
          ctx.scheduleMessageViewportRefresh({
              measure: true
          });
      });
  });

  watch(() => [ctx.fileScope.value, ctx.selectedFileContainerId.value, ctx.selectedFileAgentIdForApi.value], () => {
      ctx.fileContainerLatestUpdatedAt.value = 0;
      ctx.fileContainerEntryCount.value = 0;
      ctx.fileLifecycleNowTick.value = Date.now();
  });

  watch(() => ctx.isWorldConversationActive.value, (active) => {
      if (!active) {
          ctx.clearWorldQuickPanelClose();
          ctx.worldQuickPanelMode.value = '';
          void ctx.cancelWorldVoiceRecording();
          ctx.disposeWorldVoicePlayback();
          ctx.disposeMessageTtsPlayback();
      }
  });

  watch(() => [
      ctx.isAgentConversationActive.value,
      ctx.desktopMode.value,
      ctx.activeAgentId.value,
      String(ctx.chatStore.activeSessionId || '').trim(),
      ctx.showChatSettingsView.value
  ] as const, ([active, desktop, _agentId, _sessionId, showingSettings], previous) => {
      if (previous && (previous[2] !== _agentId || previous[3] !== _sessionId)) {
          ctx.disposeMessageTtsPlayback();
      }
      if (!active) {
          void ctx.cancelAgentVoiceRecording();
          ctx.disposeMessageTtsPlayback();
          return;
      }
      const forceRefresh = Boolean(previous?.[4] && !showingSettings);
      if (desktop) {
          void ctx.readDesktopDefaultModelMeta(forceRefresh);
          return;
      }
      void ctx.readServerDefaultModelName(forceRefresh);
  }, { immediate: true });

  watch(() => ctx.agentComposerDraftKey.value, (nextKey, previousKey) => {
      if (previousKey && previousKey !== nextKey) {
          void ctx.cancelAgentVoiceRecording();
      }
  });

  watch(() => Boolean(ctx.activeSessionApproval.value), (visible) => {
      if (visible) {
          void ctx.cancelAgentVoiceRecording();
      }
  });

  watch(() => ctx.activeWorldConversationId.value, (nextConversationId, previousConversationId) => {
      if (previousConversationId && previousConversationId !== nextConversationId) {
          void ctx.cancelWorldVoiceRecording();
          ctx.disposeWorldVoicePlayback();
          ctx.disposeMessageTtsPlayback();
      }
      if (previousConversationId) {
          ctx.writeWorldDraft(previousConversationId, ctx.worldDraft.value);
      }
      ctx.worldDraft.value = ctx.readWorldDraft(nextConversationId);
      ctx.clearWorldQuickPanelClose();
      ctx.worldQuickPanelMode.value = '';
      ctx.worldHistoryDialogVisible.value = false;
  });

  watch(() => ctx.worldDraft.value, (value) => {
      ctx.writeWorldDraft(ctx.activeWorldConversationId.value, value);
  });

  onUpdated(() => {
      ctx.scheduleWorkspaceResourceHydration();
  });

  onMounted(async () => {
      if (typeof window !== 'undefined') {
          ctx.viewportResizeHandler = () => {
              if (ctx.viewportResizeFrame !== null) {
                  return;
              }
              ctx.viewportResizeFrame = window.requestAnimationFrame(() => {
                  ctx.viewportResizeFrame = null;
                  ctx.refreshHostWidth();
                  ctx.closeFileContainerMenu();
                  ctx.syncContactVirtualMetrics();
                  ctx.scheduleMessageViewportRefresh({
                      updateScrollState: true,
                      measure: true
                  });
              });
          };
          ctx.viewportResizeHandler();
          window.addEventListener('resize', ctx.viewportResizeHandler);
          ctx.messengerSendKey.value = ctx.normalizeMessengerSendKey(window.localStorage.getItem(MESSENGER_SEND_KEY_STORAGE_KEY));
          ctx.uiFontSize.value = ctx.normalizeUiFontSize(window.localStorage.getItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY));
          ctx.worldComposerHeight.value = ctx.clampWorldComposerHeight(window.localStorage.getItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY));
          ctx.worldRecentEmojis.value = ctx.loadStoredStringArray(WORLD_QUICK_EMOJI_STORAGE_KEY, 12);
          window.addEventListener('pointerdown', ctx.closeWorldQuickPanelWhenOutside, true);
          document.addEventListener('scroll', ctx.closeFileContainerMenu, true);
          ctx.audioRecordingSupportHandler = () => {
              ctx.refreshAudioRecordingSupport();
          };
          window.addEventListener('focus', ctx.audioRecordingSupportHandler);
          window.addEventListener('pageshow', ctx.audioRecordingSupportHandler);
          document.addEventListener('visibilitychange', ctx.audioRecordingSupportHandler);
          ctx.refreshAudioRecordingSupport();
          if (ctx.audioRecordingSupportRetryTimer !== null) {
              window.clearTimeout(ctx.audioRecordingSupportRetryTimer);
          }
          ctx.audioRecordingSupportRetryTimer = window.setTimeout(() => {
              ctx.refreshAudioRecordingSupport();
              ctx.audioRecordingSupportRetryTimer = null;
          }, 1200);
      }
      ctx.initDesktopLaunchBehavior();
      ctx.applyUiFontSize(ctx.uiFontSize.value);
      await ctx.bootstrap();
      ctx.refreshAudioRecordingSupport();
      ctx.scheduleMessageViewportRefresh({
          updateScrollState: true,
          measure: true
      });
      ctx.scheduleWorkspaceResourceHydration();
      ctx.warmMessengerUserToolsData({
          catalog: ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools',
          skills: ctx.showAgentRightDock.value,
          summary: ctx.sessionHub.activeSection === 'agents' || ctx.showAgentRightDock.value
      });
      ctx.stopWorkspaceRefreshListener = onWorkspaceRefresh(ctx.handleWorkspaceResourceRefresh);
      ctx.stopAgentRuntimeRefreshListener = onAgentRuntimeRefresh((detail) => {
          void ctx.loadRunningAgents({ force: true });
          const targetAgentIds = new Set((Array.isArray(detail?.agentIds) ? detail.agentIds : [])
              .map((agentId) => ctx.normalizeAgentId(agentId))
              .filter(Boolean));
          if (!targetAgentIds.size)
              return;
          const sessionAgentMap = ctx.buildSessionAgentMap();
          Object.keys(ctx.chatStore.loadingBySession || {}).forEach((sessionId) => {
              const mappedAgentId = sessionAgentMap.get(sessionId);
              if (mappedAgentId && targetAgentIds.has(mappedAgentId)) {
                  delete ctx.chatStore.loadingBySession[sessionId];
              }
          });
      });
      ctx.stopUserToolsUpdatedListener = onUserToolsUpdated(ctx.handleUserToolsUpdatedEvent);
      ctx.lifecycleTimer = window.setInterval(() => {
          ctx.fileLifecycleNowTick.value = Date.now();
      }, 60000);
      const realtimePulse = createMessengerRealtimePulse({
          refreshRunningAgents: ctx.loadRunningAgents,
          refreshCronAgentIds: ctx.loadCronAgentIds,
          refreshChannelBoundAgentIds: ctx.loadChannelBoundAgentIds,
          refreshChatSessions: ctx.refreshRealtimeChatSessions,
          refreshContacts: ctx.refreshRealtimeContacts,
          isHotState: () => ctx.hasHotRuntimeState.value,
          shouldRefreshCron: () => !ctx.cronPermissionDenied.value,
          shouldRefreshChannelBoundAgentIds: ctx.shouldRefreshAgentMeta,
          shouldRefreshChatSessions: ctx.shouldRefreshRealtimeChatSessions,
          shouldRefreshContacts: () => !ctx.userWorldPermissionDenied.value &&
              (ctx.sessionHub.activeSection === 'users' || ctx.sessionHub.activeSection === 'messages')
      });
      const beeroomRealtimeSync = createBeeroomRealtimeSync({
          refreshBeeroomGroups: ctx.refreshBeeroomRealtimeGroups,
          refreshBeeroomActiveGroup: ctx.refreshBeeroomRealtimeActiveGroup,
          isHotState: () => ctx.hasHotBeeroomRuntimeState.value,
          shouldSync: () => ['swarms', 'orchestrations'].includes(ctx.sessionHub.activeSection),
          refreshRunningAgents: ctx.loadRunningAgents
      });
      ctx.startRealtimePulse = () => realtimePulse.start();
      ctx.stopRealtimePulse = () => realtimePulse.stop();
      ctx.triggerRealtimePulseRefresh = (reason = '') => realtimePulse.trigger(reason);
      ctx.startBeeroomRealtimeSync = () => beeroomRealtimeSync.start();
      ctx.stopBeeroomRealtimeSync = () => beeroomRealtimeSync.stop();
      ctx.triggerBeeroomRealtimeSyncRefresh = (reason = '') => beeroomRealtimeSync.trigger(reason);
      if (ctx.sessionHub.activeSection === 'swarms' || ctx.sessionHub.activeSection === 'orchestrations') {
          beeroomRealtimeSync.start();
      }
      else {
          realtimePulse.start();
      }
  });

  onBeforeUnmount(() => {
      ctx.sectionRouteSyncToken += 1;
      if (typeof window !== 'undefined') {
          if (ctx.messengerOrderSaveTimer.value !== null) {
              window.clearTimeout(ctx.messengerOrderSaveTimer.value);
              ctx.messengerOrderSaveTimer.value = null;
          }
          if (ctx.viewportResizeHandler) {
              window.removeEventListener('resize', ctx.viewportResizeHandler);
              ctx.viewportResizeHandler = null;
          }
          if (ctx.viewportResizeFrame !== null) {
              window.cancelAnimationFrame(ctx.viewportResizeFrame);
              ctx.viewportResizeFrame = null;
          }
          window.removeEventListener('pointerdown', ctx.closeWorldQuickPanelWhenOutside, true);
          document.removeEventListener('scroll', ctx.closeFileContainerMenu, true);
          if (ctx.audioRecordingSupportHandler) {
              window.removeEventListener('focus', ctx.audioRecordingSupportHandler);
              window.removeEventListener('pageshow', ctx.audioRecordingSupportHandler);
              document.removeEventListener('visibilitychange', ctx.audioRecordingSupportHandler);
              ctx.audioRecordingSupportHandler = null;
          }
          if (ctx.audioRecordingSupportRetryTimer !== null) {
              window.clearTimeout(ctx.audioRecordingSupportRetryTimer);
              ctx.audioRecordingSupportRetryTimer = null;
          }
      }
      ctx.clearRightDockSkillAutoRetry();
      ctx.closeFileContainerMenu();
      ctx.clearWorldQuickPanelClose();
      ctx.clearMiddlePaneOverlayHide();
      ctx.clearMiddlePanePrewarm();
      if (typeof window !== 'undefined' && ctx.rightDockEdgeHoverFrame !== null) {
          window.cancelAnimationFrame(ctx.rightDockEdgeHoverFrame);
          ctx.rightDockEdgeHoverFrame = null;
      }
      ctx.pendingRightDockPointerX = null;
      ctx.clearKeywordDebounce();
      ctx.closeImagePreview();
      ctx.stopWorldComposerResize();
      void ctx.cancelAgentVoiceRecording();
      void ctx.cancelWorldVoiceRecording();
      ctx.disposeWorldVoicePlayback();
      ctx.disposeMessageTtsPlayback();
      ctx.messageViewportRuntime?.dispose();
      if (typeof window !== 'undefined' && ctx.contactVirtualFrame !== null) {
          window.cancelAnimationFrame(ctx.contactVirtualFrame);
          ctx.contactVirtualFrame = null;
      }
      ctx.stopRealtimePulse?.();
      ctx.stopBeeroomRealtimeSync?.();
      ctx.startRealtimePulse = null;
      ctx.stopRealtimePulse = null;
      ctx.triggerRealtimePulseRefresh = null;
      ctx.startBeeroomRealtimeSync = null;
      ctx.stopBeeroomRealtimeSync = null;
      ctx.triggerBeeroomRealtimeSyncRefresh = null;
      if (ctx.lifecycleTimer) {
          window.clearInterval(ctx.lifecycleTimer);
          ctx.lifecycleTimer = null;
      }
      if (typeof window !== 'undefined' && ctx.timelinePrefetchTimer) {
          window.clearTimeout(ctx.timelinePrefetchTimer);
          ctx.timelinePrefetchTimer = null;
      }
      if (typeof window !== 'undefined' && ctx.sessionDetailPrefetchTimer !== null) {
          window.clearTimeout(ctx.sessionDetailPrefetchTimer);
          ctx.sessionDetailPrefetchTimer = null;
      }
      ctx.queuedSessionDetailPrefetchIds.clear();
      ctx.markdownCache.clear();
      ctx.messageVirtualHeightCache.clear();
      if (ctx.stopWorkspaceRefreshListener) {
          ctx.stopWorkspaceRefreshListener();
          ctx.stopWorkspaceRefreshListener = null;
      }
      if (ctx.stopAgentRuntimeRefreshListener) {
          ctx.stopAgentRuntimeRefreshListener();
          ctx.stopAgentRuntimeRefreshListener = null;
      }
      if (ctx.stopUserToolsUpdatedListener) {
          ctx.stopUserToolsUpdatedListener();
          ctx.stopUserToolsUpdatedListener = null;
      }
      ctx.clearWorkspaceResourceCache();
      ctx.timelinePreviewMap.value.clear();
      ctx.timelinePreviewLoadingSet.value.clear();
      ctx.userWorldStore.stopAllWatchers();
  });
}
