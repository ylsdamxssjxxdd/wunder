// @ts-nocheck
// Runtime metadata refreshers for agents, cron jobs, channel bindings, realtime contacts, and full refresh.
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

export function installMessengerControllerLifecycleRuntimeMeta(ctx: MessengerControllerContext): void {
  ctx.loadRunningAgents = async (options: {
      force?: boolean;
  } = {}) => {
      const force = options.force === true;
      if (!force && ctx.runningAgentsLoadPromise) {
          return ctx.runningAgentsLoadPromise;
      }
      if (ctx.shouldReuseAgentMetaResult(ctx.runningAgentsLoadedAt, force)) {
          return;
      }
      // Ignore stale responses when multiple refreshes race (manual refresh + pulse tick).
      const loadVersion = ++ctx.runningAgentsLoadVersion;
      const request = (async () => {
          try {
              const response = await listRunningAgents();
              if (loadVersion !== ctx.runningAgentsLoadVersion) {
                  return;
              }
              const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
              const stateMap = new Map<string, AgentRuntimeState>();
              items.forEach((item: Record<string, unknown>) => {
                  const key = ctx.normalizeAgentId(item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')) || DEFAULT_AGENT_KEY;
                  const state = ctx.normalizeRuntimeState(item?.state, item?.pending_question === true);
                  stateMap.set(key, state);
              });
              ctx.handleAgentRuntimeStateUpdate(stateMap);
              ctx.runningAgentsLoadedAt = Date.now();
          }
          catch (error) {
              if (loadVersion !== ctx.runningAgentsLoadVersion) {
                  return;
              }
              const status = ctx.resolveHttpStatus(error);
              if (ctx.isAuthDeniedStatus(status)) {
                  ctx.agentRuntimeStateMap.value = new Map<string, AgentRuntimeState>();
                  ctx.agentRuntimeStateSnapshot = new Map<string, AgentRuntimeState>();
                  ctx.agentRuntimeStateHydrated = false;
              }
          }
      })().finally(() => {
          ctx.runningAgentsLoadPromise = null;
      });
      ctx.runningAgentsLoadPromise = request;
      return request;
  };

  ctx.loadAgentUserRounds = async () => {
      const loadVersion = ++ctx.agentUserRoundsLoadVersion;
      try {
          const response = await listAgentUserRounds();
          if (loadVersion !== ctx.agentUserRoundsLoadVersion) {
              return;
          }
          const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
          const roundsMap = new Map<string, number>();
          items.forEach((item: Record<string, unknown>) => {
              const key = ctx.normalizeAgentUserRoundsKey(item?.agent_id);
              const raw = Number(item?.user_rounds ?? item?.rounds ?? 0);
              const value = Number.isFinite(raw) ? Math.max(0, Math.floor(raw)) : 0;
              roundsMap.set(key, value);
          });
          ctx.agentUserRoundsMap.value = roundsMap;
      }
      catch (error) {
          if (loadVersion !== ctx.agentUserRoundsLoadVersion) {
              return;
          }
          const status = ctx.resolveHttpStatus(error);
          if (ctx.isAuthDeniedStatus(status)) {
              ctx.agentUserRoundsMap.value = new Map<string, number>();
          }
      }
  };

  ctx.resolveHttpStatus = (error: unknown): number => {
      const status = Number((error as {
          response?: {
              status?: unknown;
          };
      })?.response?.status ?? 0);
      return Number.isFinite(status) ? status : 0;
  };

  ctx.isAuthDeniedStatus = (status: number): boolean => status === 401 || status === 403;

  ctx.handleCronPanelChanged = (payload?: {
      agentId?: string;
      hasJobs?: boolean;
  }) => {
      const normalizeChangedAgentId = (value: unknown): string => {
          const raw = String(value || '').trim();
          if (!raw)
              return DEFAULT_AGENT_KEY;
          const lowered = raw.toLowerCase();
          if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
              return DEFAULT_AGENT_KEY;
          }
          return ctx.normalizeAgentId(raw);
      };
      const hasJobs = payload?.hasJobs;
      if (hasJobs === true || hasJobs === false) {
          const next = new Set(ctx.cronAgentIds.value);
          const changedAgentId = normalizeChangedAgentId(payload?.agentId);
          if (hasJobs) {
              next.add(changedAgentId);
          }
          else {
              next.delete(changedAgentId);
          }
          ctx.cronAgentIds.value = next;
      }
      void ctx.loadCronAgentIds({ force: true });
  };

  ctx.loadCronAgentIds = async (options: {
      force?: boolean;
  } = {}) => {
      const force = options.force === true;
      if (!force && ctx.cronAgentIdsLoadPromise) {
          return ctx.cronAgentIdsLoadPromise;
      }
      if (ctx.shouldReuseAgentMetaResult(ctx.cronAgentIdsLoadedAt, force)) {
          return;
      }
      const loadVersion = ++ctx.cronAgentIdsLoadVersion;
      if (ctx.cronPermissionDenied.value) {
          if (loadVersion === ctx.cronAgentIdsLoadVersion) {
              ctx.cronAgentIds.value = new Set<string>();
          }
          return;
      }
      const request = (async () => {
          try {
              const normalizeCronAgentKey = (value: unknown): string => {
                  const raw = String(value || '').trim();
                  if (!raw)
                      return '';
                  const lowered = raw.toLowerCase();
                  if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
                      return DEFAULT_AGENT_KEY;
                  }
                  return ctx.normalizeAgentId(raw);
              };
              const sessionAgentMap = new Map<string, string>();
              const sessions = Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : [];
              sessions.forEach((session: Record<string, unknown>) => {
                  const sessionId = String(session?.id || '').trim();
                  if (!sessionId)
                      return;
                  const explicitAgent = normalizeCronAgentKey(session?.agent_id ?? session?.agentId);
                  const fallbackAgent = session?.is_main === true ? DEFAULT_AGENT_KEY : '';
                  const resolvedAgent = explicitAgent || fallbackAgent;
                  if (resolvedAgent) {
                      sessionAgentMap.set(sessionId, resolvedAgent);
                  }
              });
              const response = await fetchCronJobs();
              if (loadVersion !== ctx.cronAgentIdsLoadVersion) {
                  return;
              }
              const jobs = Array.isArray(response?.data?.data?.jobs)
                  ? response.data.data.jobs
                  : Array.isArray(response?.data?.data?.items)
                      ? response.data.data.items
                      : [];
              const result = new Set<string>();
              jobs.forEach((job: Record<string, unknown>) => {
                  const rawAgentId = String(job?.agent_id ??
                      job?.agentId ??
                      (job?.agent as Record<string, unknown> | undefined)?.id ??
                      (job?.agent as Record<string, unknown> | undefined)?.agent_id ??
                      '').trim();
                  const mappedSessionAgent = sessionAgentMap.get(String(job?.session_id ?? job?.sessionId ?? '').trim());
                  const target = String(job?.session_target ?? job?.sessionTarget ?? job?.session ?? '').trim().toLowerCase();
                  const defaultTarget = target === '' ||
                      target === 'main' ||
                      target === 'default' ||
                      target === 'system' ||
                      target === '__default__';
                  const resolved = rawAgentId ||
                      mappedSessionAgent ||
                      (defaultTarget ||
                          job?.is_default === true ||
                          job?.isDefault === true
                          ? DEFAULT_AGENT_KEY
                          : '');
                  if (!resolved)
                      return;
                  result.add(normalizeCronAgentKey(resolved));
              });
              if (loadVersion !== ctx.cronAgentIdsLoadVersion) {
                  return;
              }
              ctx.cronAgentIds.value = result;
              ctx.cronPermissionDenied.value = false;
              ctx.cronAgentIdsLoadedAt = Date.now();
          }
          catch (error) {
              if (loadVersion !== ctx.cronAgentIdsLoadVersion) {
                  return;
              }
              const status = ctx.resolveHttpStatus(error);
              if (ctx.isAuthDeniedStatus(status)) {
                  ctx.cronPermissionDenied.value = true;
                  ctx.cronAgentIds.value = new Set<string>();
                  return;
              }
          }
      })().finally(() => {
          ctx.cronAgentIdsLoadPromise = null;
      });
      ctx.cronAgentIdsLoadPromise = request;
      return request;
  };

  ctx.loadChannelBoundAgentIds = async (options: {
      force?: boolean;
  } = {}) => {
      const force = options.force === true;
      if (!force && ctx.channelBoundAgentIdsLoadPromise) {
          return ctx.channelBoundAgentIdsLoadPromise;
      }
      if (ctx.shouldReuseAgentMetaResult(ctx.channelBoundAgentIdsLoadedAt, force)) {
          return;
      }
      const loadVersion = ++ctx.channelBoundAgentIdsLoadVersion;
      const request = (async () => {
          try {
              const normalizeChannelAgentKey = (value: unknown): string => {
                  const raw = String(value || '').trim();
                  if (!raw)
                      return DEFAULT_AGENT_KEY;
                  const lowered = raw.toLowerCase();
                  if (lowered === 'default' || lowered === '__default__' || lowered === 'system') {
                      return DEFAULT_AGENT_KEY;
                  }
                  return ctx.normalizeAgentId(raw);
              };
              const response = await listChannelBindings();
              if (loadVersion !== ctx.channelBoundAgentIdsLoadVersion) {
                  return;
              }
              const items = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
              const bound = new Set<string>();
              items.forEach((item: Record<string, unknown>) => {
                  const agentId = normalizeChannelAgentKey(item?.agent_id ??
                      item?.agentId ??
                      (item?.agent as Record<string, unknown> | undefined)?.id ??
                      (item?.agent as Record<string, unknown> | undefined)?.agent_id ??
                      (item?.config as Record<string, unknown> | undefined)?.agent_id ??
                      (item?.raw_config as Record<string, unknown> | undefined)?.agent_id ??
                      '');
                  bound.add(agentId);
              });
              if (loadVersion !== ctx.channelBoundAgentIdsLoadVersion) {
                  return;
              }
              ctx.channelBoundAgentIds.value = bound;
              ctx.channelBoundAgentIdsLoadedAt = Date.now();
          }
          catch (error) {
              if (loadVersion !== ctx.channelBoundAgentIdsLoadVersion) {
                  return;
              }
              const status = ctx.resolveHttpStatus(error);
              if (ctx.isAuthDeniedStatus(status)) {
                  ctx.channelBoundAgentIds.value = new Set<string>();
                  return;
              }
          }
      })().finally(() => {
          ctx.channelBoundAgentIdsLoadPromise = null;
      });
      ctx.channelBoundAgentIdsLoadPromise = request;
      return request;
  };

  ctx.refreshRealtimeChatSessions = async () => {
      await ctx.chatStore.loadSessions();
  };

  ctx.REALTIME_CONTACT_REFRESH_MIN_MS = 7000;

  ctx.refreshRealtimeContacts = async () => {
      const lastRefreshedAt = Number(ctx.userWorldStore.lastContactRealtimeRefreshAt || 0);
      if (lastRefreshedAt > 0 && Date.now() - lastRefreshedAt < ctx.REALTIME_CONTACT_REFRESH_MIN_MS) {
          return;
      }
      await ctx.userWorldStore.refreshContacts('', {
          shouldApply: () => ctx.sessionHub.activeSection === 'users' || ctx.sessionHub.activeSection === 'messages'
      });
  };

  ctx.shouldRefreshRealtimeChatSessions = () => ctx.sessionHub.activeSection === 'messages';

  ctx.shouldRefreshAgentMeta = () => ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools';

  ctx.refreshAll = async () => {
      const tasks: Promise<unknown>[] = [
          ctx.agentStore.loadAgents(),
          ctx.beeroomStore.loadGroups(),
          ctx.chatStore.loadSessions(),
          ctx.userWorldStore.bootstrap(true),
          ctx.loadOrgUnits(),
          ctx.loadRunningAgents({ force: true }),
          ctx.loadAgentUserRounds(),
          ctx.loadToolsCatalog(),
          ctx.loadChannelBoundAgentIds({ force: true })
      ];
      if (!ctx.cronPermissionDenied.value) {
          tasks.push(ctx.loadCronAgentIds({ force: true }));
      }
      await Promise.allSettled(tasks);
      ctx.ensureSectionSelection();
      ElMessage.success(ctx.t('common.refreshSuccess'));
  };
}
