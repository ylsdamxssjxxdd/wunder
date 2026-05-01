// @ts-nocheck
// Agent main-session unread state, preferred session prefetching, and unread persistence.
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

export function installMessengerControllerAgentUnreadRuntime(ctx: MessengerControllerContext): void {
  ctx.ensureAgentUnreadState = (force = false) => {
      if (typeof window === 'undefined') {
          ctx.agentMainReadAtMap.value = {};
          ctx.agentMainUnreadCountMap.value = {};
          ctx.agentUnreadStorageKeys.value = { readAt: '', unread: '' };
          return;
      }
      const targetKeys = ctx.resolveAgentUnreadStorageKeys(ctx.currentUserId.value);
      const currentKeys = ctx.agentUnreadStorageKeys.value;
      if (!force && currentKeys.readAt === targetKeys.readAt && currentKeys.unread === targetKeys.unread) {
          return;
      }
      ctx.agentUnreadStorageKeys.value = targetKeys;
      try {
          const readRaw = window.localStorage.getItem(targetKeys.readAt);
          const unreadRaw = window.localStorage.getItem(targetKeys.unread);
          ctx.agentMainReadAtMap.value = readRaw ? ctx.normalizeNumericMap(JSON.parse(readRaw)) : {};
          ctx.agentMainUnreadCountMap.value = unreadRaw ? ctx.normalizeNumericMap(JSON.parse(unreadRaw)) : {};
      }
      catch {
          ctx.agentMainReadAtMap.value = {};
          ctx.agentMainUnreadCountMap.value = {};
      }
  };

  ctx.collectMainAgentSessionEntries = (): AgentMainSessionEntry[] => {
      const grouped = new Map<string, Array<Record<string, unknown>>>();
      (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).forEach((sessionRaw) => {
          const session = (sessionRaw || {}) as Record<string, unknown>;
          const agentId = ctx.normalizeAgentId(session.agent_id);
          if (!grouped.has(agentId)) {
              grouped.set(agentId, []);
          }
          grouped.get(agentId)?.push(session);
      });
      return Array.from(grouped.entries())
          .map(([agentId, sessions]) => {
          const sorted = [...sessions].sort((left, right) => ctx.resolveSessionActivityTimestamp(right) -
              ctx.resolveSessionActivityTimestamp(left));
          const main = sorted.find((item) => Boolean(item?.is_main)) || sorted[0];
          const sessionId = String(main?.id || '').trim();
          if (!sessionId) {
              return null;
          }
          return {
              agentId,
              sessionId,
              lastAt: ctx.resolveSessionActivityTimestamp(main as Record<string, unknown>)
          } as AgentMainSessionEntry;
      })
          .filter((item): item is AgentMainSessionEntry => Boolean(item));
  };

  ctx.resolvePreferredAgentSessionId = (agentId: unknown): string => {
      const normalizedAgentId = ctx.normalizeAgentId(agentId);
      const sessions = Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : [];
      return ctx.chatStore.resolveInitialSessionId(normalizedAgentId, sessions);
  };

  ctx.queuedSessionDetailPrefetchIds = new Set<string>();

  ctx.flushSessionDetailPrefetchQueue = () => {
      if (typeof window !== 'undefined' && ctx.sessionDetailPrefetchTimer !== null) {
          window.clearTimeout(ctx.sessionDetailPrefetchTimer);
          ctx.sessionDetailPrefetchTimer = null;
      }
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const sessionIds = Array.from(ctx.queuedSessionDetailPrefetchIds);
      ctx.queuedSessionDetailPrefetchIds.clear();
      sessionIds.forEach((sessionId) => {
          if (!sessionId || sessionId === activeSessionId) {
              return;
          }
          void ctx.chatStore.preloadSessionDetail(sessionId).catch(() => undefined);
      });
  };

  ctx.queueSessionDetailPrefetch = (sessionId: unknown) => {
      const normalizedSessionId = String(sessionId || '').trim();
      if (!normalizedSessionId) {
          return;
      }
      if (normalizedSessionId === String(ctx.chatStore.activeSessionId || '').trim()) {
          return;
      }
      ctx.queuedSessionDetailPrefetchIds.add(normalizedSessionId);
      if (typeof window === 'undefined') {
          ctx.flushSessionDetailPrefetchQueue();
          return;
      }
      if (ctx.sessionDetailPrefetchTimer !== null) {
          return;
      }
      ctx.sessionDetailPrefetchTimer = window.setTimeout(() => {
          ctx.flushSessionDetailPrefetchQueue();
      }, ctx.SESSION_DETAIL_PREFETCH_DELAY_MS);
  };

  ctx.preloadAgentById = (agentId: unknown) => {
      const sessionId = ctx.resolvePreferredAgentSessionId(agentId);
      if (!sessionId) {
          return;
      }
      ctx.queueSessionDetailPrefetch(sessionId);
  };

  ctx.preloadMixedConversation = (item: MixedConversation | null | undefined) => {
      if (!item || item.kind !== 'agent') {
          return;
      }
      const sessionId = String(item.sourceId || '').trim() || ctx.resolvePreferredAgentSessionId(item.agentId);
      if (!sessionId) {
          return;
      }
      ctx.queueSessionDetailPrefetch(sessionId);
  };

  ctx.setAgentMainUnreadCount = (agentId: string, count: number) => {
      const normalizedAgentId = ctx.normalizeAgentId(agentId);
      const normalizedCount = Math.max(0, Math.floor(Number(count) || 0));
      const current = Math.max(0, Math.floor(Number(ctx.agentMainUnreadCountMap.value[normalizedAgentId] || 0)));
      if (current === normalizedCount)
          return;
      ctx.agentMainUnreadCountMap.value = {
          ...ctx.agentMainUnreadCountMap.value,
          [normalizedAgentId]: normalizedCount
      };
  };

  ctx.setAgentMainReadAt = (agentId: string, timestamp: number) => {
      const normalizedAgentId = ctx.normalizeAgentId(agentId);
      const normalizedTimestamp = Math.max(0, Math.floor(Number(timestamp) || 0));
      if (!normalizedTimestamp)
          return;
      const current = Math.max(0, Math.floor(Number(ctx.agentMainReadAtMap.value[normalizedAgentId] || 0)));
      if (current >= normalizedTimestamp)
          return;
      ctx.agentMainReadAtMap.value = {
          ...ctx.agentMainReadAtMap.value,
          [normalizedAgentId]: normalizedTimestamp
      };
  };

  ctx.trimAgentMainUnreadState = (entries: AgentMainSessionEntry[]) => {
      const validAgentIds = new Set(entries.map((item) => item.agentId));
      const trimmedReadAt = Object.entries(ctx.agentMainReadAtMap.value).reduce<Record<string, number>>((acc, [key, raw]) => {
          const agentId = ctx.normalizeAgentId(key);
          if (!validAgentIds.has(agentId))
              return acc;
          const value = Math.max(0, Math.floor(Number(raw) || 0));
          if (!value)
              return acc;
          acc[agentId] = value;
          return acc;
      }, {});
      const trimmedUnread = Object.entries(ctx.agentMainUnreadCountMap.value).reduce<Record<string, number>>((acc, [key, raw]) => {
          const agentId = ctx.normalizeAgentId(key);
          if (!validAgentIds.has(agentId))
              return acc;
          const value = Math.max(0, Math.floor(Number(raw) || 0));
          if (!value)
              return acc;
          acc[agentId] = value;
          return acc;
      }, {});
      ctx.agentMainReadAtMap.value = trimmedReadAt;
      ctx.agentMainUnreadCountMap.value = trimmedUnread;
  };

  ctx.refreshAgentMainUnreadCount = async (entry: AgentMainSessionEntry, readAt: number) => {
      const requestKey = `${entry.agentId}:${entry.sessionId}:${readAt}`;
      if (ctx.agentUnreadRefreshInFlight.has(requestKey)) {
          return;
      }
      ctx.agentUnreadRefreshInFlight.add(requestKey);
      try {
          const response = await getChatSessionApi(entry.sessionId);
          const messages = Array.isArray(response?.data?.data?.messages) ? response.data.data.messages : [];
          const unreadCount = messages.filter((message: Record<string, unknown>) => {
              if (String(message?.role || '') !== 'assistant') {
                  return false;
              }
              const timestamp = ctx.normalizeTimestamp(message?.created_at);
              return timestamp > readAt;
          }).length;
          const activeEntries = ctx.collectMainAgentSessionEntries();
          const currentMain = activeEntries.find((item) => item.agentId === entry.agentId);
          if (!currentMain || currentMain.sessionId !== entry.sessionId) {
              return;
          }
          const currentReadAt = Math.max(0, Math.floor(Number(ctx.agentMainReadAtMap.value[entry.agentId] || 0)));
          if (currentReadAt !== readAt) {
              return;
          }
          if (currentMain.lastAt <= currentReadAt) {
              ctx.setAgentMainUnreadCount(entry.agentId, 0);
              ctx.persistAgentUnreadState();
              return;
          }
          ctx.setAgentMainUnreadCount(entry.agentId, unreadCount);
          ctx.persistAgentUnreadState();
      }
      catch {
      }
      finally {
          ctx.agentUnreadRefreshInFlight.delete(requestKey);
      }
  };

  ctx.refreshAgentMainUnreadFromSessions = () => {
      const entries = ctx.collectMainAgentSessionEntries();
      ctx.trimAgentMainUnreadState(entries);
      const identity = ctx.activeConversation.value;
      entries.forEach((entry) => {
          const isViewingMain = identity?.kind === 'agent' &&
              String(identity?.id || '').trim() === entry.sessionId &&
              ctx.normalizeAgentId(identity?.agentId) === entry.agentId;
          if (isViewingMain) {
              const targetReadAt = entry.lastAt || Date.now();
              ctx.setAgentMainReadAt(entry.agentId, targetReadAt);
              ctx.setAgentMainUnreadCount(entry.agentId, 0);
              return;
          }
          const readAt = Math.max(0, Math.floor(Number(ctx.agentMainReadAtMap.value[entry.agentId] || 0)));
          if (!readAt) {
              ctx.setAgentMainReadAt(entry.agentId, entry.lastAt || Date.now());
              ctx.setAgentMainUnreadCount(entry.agentId, 0);
              return;
          }
          if (entry.lastAt <= readAt) {
              ctx.setAgentMainUnreadCount(entry.agentId, 0);
              return;
          }
          void ctx.refreshAgentMainUnreadCount(entry, readAt);
      });
      ctx.persistAgentUnreadState();
  };
}
