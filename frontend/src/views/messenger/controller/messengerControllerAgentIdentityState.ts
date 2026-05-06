// @ts-nocheck
// Agent identity, active session model display, approval mode, and default profile state.
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

export function installMessengerControllerAgentIdentityState(ctx: MessengerControllerContext): void {
  ctx.DEFAULT_BEEROOM_GROUP_ID = 'default';

  ctx.ownedAgents = computed(() => (Array.isArray(ctx.agentStore.agents) ? ctx.agentStore.agents : []));

  ctx.sharedAgents = computed(() => (Array.isArray(ctx.agentStore.sharedAgents) ? ctx.agentStore.sharedAgents : []));

  ctx.normalizeAgentHiveGroupId = (value: unknown): string => {
      const normalized = String(value || '').trim();
      return normalized || ctx.DEFAULT_BEEROOM_GROUP_ID;
  };

  ctx.defaultBeeroomGroupId = computed(() => {
      const defaultGroup = ctx.beeroomStore.groups.find((item) => item.is_default);
      const normalized = String(defaultGroup?.group_id || defaultGroup?.hive_id || '').trim();
      return normalized || ctx.DEFAULT_BEEROOM_GROUP_ID;
  });

  ctx.resolveAgentHiveGroupId = (agent: unknown): string => {
      if (!agent || typeof agent !== 'object') {
          return ctx.defaultBeeroomGroupId.value;
      }
      const source = agent as Record<string, unknown>;
      return ctx.normalizeAgentHiveGroupId(source.hive_id || source.hiveId || ctx.defaultBeeroomGroupId.value);
  };

  ctx.agentHiveLabelMap = computed(() => {
      const map = new Map<string, string>();
      map.set(ctx.defaultBeeroomGroupId.value, ctx.t('messenger.agentGroup.defaultOption'));
      (Array.isArray(ctx.beeroomStore.groups) ? ctx.beeroomStore.groups : []).forEach((item) => {
          const hiveId = String(item?.group_id || item?.hive_id || '').trim();
          if (!hiveId)
              return;
          const label = String(item?.is_default ? ctx.t('messenger.agentGroup.defaultOption') : (item?.name || hiveId)).trim();
          if (label) {
              map.set(hiveId, label);
          }
      });
      [...ctx.ownedAgents.value, ...ctx.sharedAgents.value].forEach((agent) => {
          const hiveId = ctx.resolveAgentHiveGroupId(agent);
          if (map.has(hiveId))
              return;
          const source = agent as Record<string, unknown>;
          const label = String(hiveId === ctx.defaultBeeroomGroupId.value
              ? ctx.t('messenger.agentGroup.defaultOption')
              : (source.hive_name || source.hiveName || source.hive_id || source.hiveId || hiveId)).trim();
          if (label) {
              map.set(hiveId, label);
          }
      });
      return map;
  });

  ctx.agentHiveEntries = computed(() => {
      const entries: Array<{
          agentId: string;
          hiveId: string;
      }> = [
          {
              agentId: DEFAULT_AGENT_KEY,
              hiveId: ctx.defaultBeeroomGroupId.value
          }
      ];
      const seenAgentIds = new Set<string>([DEFAULT_AGENT_KEY]);
      [...ctx.ownedAgents.value, ...ctx.sharedAgents.value].forEach((agent) => {
          const agentId = ctx.normalizeAgentId(agent?.id);
          if (!agentId || seenAgentIds.has(agentId))
              return;
          seenAgentIds.add(agentId);
          entries.push({
              agentId,
              hiveId: ctx.resolveAgentHiveGroupId(agent)
          });
      });
      return entries;
  });

  ctx.agentHiveTotalCount = computed(() => ctx.agentHiveEntries.value.length);

  ctx.agentHiveTreeRows = computed(() => {
      const countMap = new Map<string, number>();
      ctx.agentHiveEntries.value.forEach((entry) => {
          countMap.set(entry.hiveId, (countMap.get(entry.hiveId) || 0) + 1);
      });
      return Array.from(countMap.entries())
          .filter(([, count]) => count > 0)
          .map(([id, count]) => ({
          id,
          label: ctx.agentHiveLabelMap.value.get(id) || id,
          count,
          depth: 0,
          expanded: false,
          hasChildren: false
      }))
          .sort((left, right) => {
          if (left.id === ctx.defaultBeeroomGroupId.value)
              return -1;
          if (right.id === ctx.defaultBeeroomGroupId.value)
              return 1;
          return String(left.label || left.id).localeCompare(String(right.label || right.id), 'zh-Hans-CN');
      });
  });

  ctx.matchesAgentKeyword = (agent: unknown, text: string) => {
      const source = agent && typeof agent === 'object' ? (agent as Record<string, unknown>) : {};
      const id = String(source.id || '').toLowerCase();
      const name = String(source.name || '').toLowerCase();
      const desc = String(source.description || '').toLowerCase();
      const hiveId = String(ctx.resolveAgentHiveGroupId(source) || '').toLowerCase();
      const hiveLabel = String(ctx.agentHiveLabelMap.value.get(ctx.resolveAgentHiveGroupId(source)) || ctx.resolveAgentHiveGroupId(source)).toLowerCase();
      return !text || id.includes(text) || name.includes(text) || desc.includes(text) || hiveId.includes(text) || hiveLabel.includes(text);
  };

  ctx.matchesAgentHiveSelection = (agent: unknown) => {
      const selectedHiveId = String(ctx.selectedAgentHiveGroupId.value || '').trim();
      if (!selectedHiveId)
          return true;
      return ctx.resolveAgentHiveGroupId(agent) === ctx.normalizeAgentHiveGroupId(selectedHiveId);
  };

  ctx.defaultAgentMatchesKeyword = computed(() => ctx.matchesAgentKeyword({
      id: DEFAULT_AGENT_KEY,
      name: ctx.t('messenger.defaultAgent'),
      description: ctx.t('messenger.defaultAgentDesc'),
      hive_id: ctx.defaultBeeroomGroupId.value
  }, ctx.keyword.value.toLowerCase()));

  ctx.showDefaultAgentEntry = computed(() => ctx.defaultAgentMatchesKeyword.value &&
      (!ctx.selectedAgentHiveGroupId.value ||
          ctx.normalizeAgentHiveGroupId(ctx.selectedAgentHiveGroupId.value) === ctx.defaultBeeroomGroupId.value));

  ctx.defaultAgentApprovalMode = computed(() => 'full_auto');

  ctx.agentMap = computed(() => {
      const map = new Map<string, Record<string, unknown>>();
      map.set(DEFAULT_AGENT_KEY, {
          id: DEFAULT_AGENT_KEY,
          name: ctx.t('messenger.defaultAgent'),
          description: ctx.t('messenger.defaultAgentDesc'),
          sandbox_container_id: 1,
          approval_mode: ctx.defaultAgentApprovalMode.value,
          silent: false,
          prefer_mother: false
      });
      ctx.ownedAgents.value.forEach((item) => {
          const id = ctx.normalizeAgentId(item?.id);
          map.set(id, item as Record<string, unknown>);
      });
      ctx.sharedAgents.value.forEach((item) => {
          const id = ctx.normalizeAgentId(item?.id);
          if (!map.has(id)) {
              map.set(id, item as Record<string, unknown>);
          }
      });
      return map;
  });

  ctx.quickCreateCopyFromAgents = computed(() => {
      const items: Array<{
          id: string;
          name: string;
      }> = [
          {
              id: DEFAULT_AGENT_KEY,
              name: ctx.t('messenger.defaultAgent')
          }
      ];
      const seenIds = new Set<string>([DEFAULT_AGENT_KEY]);
      ctx.ownedAgents.value.forEach((item) => {
          const id = ctx.normalizeAgentId(item?.id);
          if (!id || seenIds.has(id))
              return;
          seenIds.add(id);
          items.push({
              id,
              name: String(item?.name || item?.id || id).trim()
          });
      });
      return items;
  });

  ctx.isSilentAgent = (agentId: unknown): boolean => {
      const normalized = ctx.normalizeAgentId(agentId);
      if (!normalized)
          return false;
      return Boolean(ctx.agentMap.value.get(normalized)?.silent);
  };

  ctx.activeConversation = computed(() => ctx.sessionHub.activeConversation);

  ctx.resolvedMessageConversationKind = computed<'agent' | 'world' | ''>(() => {
      if (ctx.sessionHub.activeSection !== 'messages') {
          return '';
      }
      const identity = ctx.activeConversation.value;
      if (identity?.kind === 'agent')
          return 'agent';
      if (identity?.kind === 'direct' || identity?.kind === 'group')
          return 'world';
      const queryConversationId = String(ctx.route.query?.conversation_id || '').trim();
      if (queryConversationId)
          return 'world';
      const querySessionId = String(ctx.route.query?.session_id || '').trim();
      const queryAgentId = String(ctx.route.query?.agent_id || '').trim();
      const queryEntry = String(ctx.route.query?.entry || '')
          .trim()
          .toLowerCase();
      if (querySessionId || queryAgentId || queryEntry === 'default')
          return 'agent';
      if (String(ctx.chatStore.activeSessionId || '').trim() || String(ctx.chatStore.draftAgentId || '').trim()) {
          return 'agent';
      }
      return '';
  });

  ctx.isAgentConversationActive = computed(() => ctx.resolvedMessageConversationKind.value === 'agent');

  ctx.isWorldConversationActive = computed(() => ctx.resolvedMessageConversationKind.value === 'world');

  ctx.activeAgentId = computed(() => {
      const identity = ctx.activeConversation.value;
      if (identity?.kind === 'agent') {
          if (identity.agentId) {
              return ctx.normalizeAgentId(identity.agentId);
          }
          if (identity.id.startsWith('draft:')) {
              return ctx.normalizeAgentId(identity.id.slice('draft:'.length));
          }
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === identity.id);
          return ctx.normalizeAgentId(session?.agent_id || ctx.chatStore.draftAgentId);
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (sessionId) {
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
          return ctx.normalizeAgentId(session?.agent_id || ctx.chatStore.draftAgentId);
      }
      if (String(ctx.chatStore.draftAgentId || '').trim()) {
          return ctx.normalizeAgentId(ctx.chatStore.draftAgentId);
      }
      return ctx.normalizeAgentId(ctx.selectedAgentId.value);
  });

  ctx.activeAgent = computed(() => ctx.agentMap.value.get(ctx.activeAgentId.value) || null);

  ctx.activeAgentDetailProfile = ref<Record<string, unknown> | null>(null);

  ctx.defaultAgentProfile = ref<Record<string, unknown> | null>(null);

  ctx.activeAgentIdForApi = computed(() => ctx.activeAgentId.value === DEFAULT_AGENT_KEY ? '' : ctx.activeAgentId.value);

  ctx.activeAgentPresetQuestions = computed(() => {
      if (ctx.activeAgentId.value === DEFAULT_AGENT_KEY) {
          return normalizeAgentPresetQuestions(ctx.defaultAgentProfile.value?.preset_questions);
      }
      return normalizeAgentPresetQuestions((ctx.activeAgent.value as Record<string, unknown> | null)?.preset_questions);
  });

  ctx.activeAgentName = computed(() => String((ctx.activeAgent.value as Record<string, unknown> | null)?.name || ctx.t('messenger.defaultAgent')));

  ctx.activeAgentIcon = computed(() => ctx.activeAgentId.value === DEFAULT_AGENT_KEY
      ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)?.icon
      : (ctx.activeAgent.value as Record<string, unknown> | null)?.icon);

  ctx.activeAgentGreetingOverride = computed(() => {
      if (ctx.activeAgentId.value === DEFAULT_AGENT_KEY) {
          return String((ctx.defaultAgentProfile.value as Record<string, unknown> | null)?.description || '').trim();
      }
      const profile = (ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
          (ctx.activeAgent.value as Record<string, unknown> | null);
      return String(profile?.description || '').trim();
  });

  ctx.resolveAgentIconForDisplay = (agentId: string, fallback: Record<string, unknown> | null = null): unknown => {
      const normalized = ctx.normalizeAgentId(agentId);
      if (normalized === DEFAULT_AGENT_KEY) {
          return (ctx.defaultAgentProfile.value as Record<string, unknown> | null)?.icon ?? fallback?.icon;
      }
      return fallback?.icon;
  };

  ctx.loadDefaultAgentProfile = async () => {
      ctx.defaultAgentProfile.value =
          ((await ctx.agentStore.getAgent(DEFAULT_AGENT_KEY, { force: true }).catch(() => null)) as Record<string, unknown> | null) || null;
  };

  watch(() => ctx.activeAgentId.value, (value) => {
      if (value === DEFAULT_AGENT_KEY) {
          ctx.activeAgentDetailProfile.value = null;
          void ctx.loadDefaultAgentProfile();
          return;
      }
      const targetAgentId = ctx.normalizeAgentId(value);
      if (!targetAgentId) {
          ctx.activeAgentDetailProfile.value = null;
          return;
      }
      void ctx.agentStore.getAgent(targetAgentId, { force: true })
          .then((profile) => {
          if (ctx.normalizeAgentId(ctx.activeAgentId.value) !== targetAgentId)
              return;
          ctx.activeAgentDetailProfile.value =
              (profile as Record<string, unknown> | null) || null;
      })
          .catch(() => null);
  }, { immediate: true });

  watch(() => [ctx.chatStore.activeSessionId, ctx.activeAgentId.value, ctx.selectedAgentId.value, ctx.chatStore.draftAgentId] as const, () => {
      ctx.agentPromptPreviewPayloadCache = null;
      if (ctx.agentPromptPreviewPayloadPromise) {
          ctx.agentPromptPreviewPayloadPromiseKey = '';
      }
  });

  watch(() => ctx.activeAgentGreetingOverride.value, (value, oldValue) => {
      if (value === oldValue)
          return;
      ctx.chatStore.setGreetingOverride(value);
  }, { immediate: true });

  watch([() => ctx.chatStore.activeSessionId, () => ctx.activeAgentId.value], () => {
      ctx.agentPromptPreviewSelectedNames.value = null;
  });

  ctx.activeAgentPromptPreviewText = computed(() => String(ctx.agentPromptPreviewContent.value || '').trim() || ctx.t('chat.systemPrompt.empty'));

  ctx.activeAgentSession = computed(() => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return null;
      return (ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId) || null);
  });

  ctx.asObjectRecord = (value: unknown): Record<string, unknown> => value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

  ctx.tryParseJsonRecord = (value: unknown): Record<string, unknown> | null => {
      if (typeof value !== 'string')
          return null;
      const text = value.trim();
      if (!text || !text.startsWith('{'))
          return null;
      try {
          const parsed = JSON.parse(text);
          return parsed && typeof parsed === 'object' && !Array.isArray(parsed)
              ? (parsed as Record<string, unknown>)
              : null;
      }
      catch {
          return null;
      }
  };

  ctx.resolveModelNameFromRecord = (value: unknown): string => {
      const source = ctx.tryParseJsonRecord(value) || ctx.asObjectRecord(value);
      if (!Object.keys(source).length)
          return '';
      const directKeys = [
          'model_name',
          'modelName',
          'model',
          'llm_model',
          'llmModel',
          'llm_model_name',
          'llmModelName'
      ] as const;
      for (const key of directKeys) {
          const candidate = source[key];
          if (typeof candidate === 'string' || typeof candidate === 'number') {
              const text = String(candidate).trim();
              if (text)
                  return text;
              const parsed = ctx.tryParseJsonRecord(candidate);
              if (parsed) {
                  const parsedName = ctx.resolveModelNameFromRecord(parsed);
                  if (parsedName)
                      return parsedName;
              }
              continue;
          }
          const nested = ctx.asObjectRecord(candidate);
          const nestedText = String(nested.name || nested.model || nested.id || '').trim();
          if (nestedText)
              return nestedText;
          const nestedName = ctx.resolveModelNameFromRecord(nested);
          if (nestedName)
              return nestedName;
      }
      const nestedContainerKeys = ['payload', 'data', 'request', 'response', 'detail', 'args'] as const;
      for (const key of nestedContainerKeys) {
          const nestedName = ctx.resolveModelNameFromRecord(source[key]);
          if (nestedName)
              return nestedName;
      }
      const meta = source.meta;
      if (meta && typeof meta === 'object' && meta !== value) {
          const nested = ctx.resolveModelNameFromRecord(meta);
          if (nested)
              return nested;
      }
      return '';
  };

  ctx.resolveMessageModelName = (message: Record<string, unknown>): string => {
      const direct = ctx.resolveModelNameFromRecord(message);
      if (direct)
          return direct;
      const workflowItems = Array.isArray(message.workflowItems)
          ? (message.workflowItems as unknown[])
          : [];
      for (let cursor = workflowItems.length - 1; cursor >= 0; cursor -= 1) {
          const item = workflowItems[cursor];
          const fromItem = ctx.resolveModelNameFromRecord(item);
          if (fromItem) {
              return fromItem;
          }
          const fromDetail = ctx.resolveModelNameFromRecord(ctx.asObjectRecord(item).detail);
          if (fromDetail) {
              return fromDetail;
          }
      }
      return '';
  };

  ctx.activeAgentSessionModelName = computed(() => ctx.resolveModelNameFromRecord(ctx.activeAgentSession.value));

  ctx.activeAgentRuntimeModelName = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return '';
      const messages = Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [];
      for (let cursor = messages.length - 1; cursor >= 0; cursor -= 1) {
          const message = ctx.asObjectRecord(messages[cursor]);
          if (String(message.role || '').trim().toLowerCase() !== 'assistant') {
              continue;
          }
          const modelName = ctx.resolveMessageModelName(message);
          if (modelName)
              return modelName;
      }
      return '';
  });

  ctx.activeAgentProfileForModelResolution = computed(() => ctx.activeAgentId.value === DEFAULT_AGENT_KEY ? ctx.defaultAgentProfile.value : ctx.activeAgent.value);

  ctx.isDefaultModelSelectorValue = (value: unknown): boolean => {
      const lowered = String(value || '').trim().toLowerCase();
      return !lowered || lowered === 'default' || lowered === '__default__' || lowered === 'system';
  };

  ctx.isSameModelName = (left: unknown, right: unknown): boolean => {
      const leftValue = String(left || '').trim();
      const rightValue = String(right || '').trim();
      if (!leftValue || !rightValue)
          return false;
      return leftValue.toLowerCase() === rightValue.toLowerCase();
  };

  ctx.resolveExplicitAgentModelName = (profileValue: unknown): string => {
      const profile = ctx.asObjectRecord(profileValue);
      const configuredRaw = profile.configured_model_name ?? profile.configuredModelName;
      const configuredResolved = ctx.resolveModelNameFromRecord(configuredRaw);
      const configured = configuredResolved || String(configuredRaw || '').trim();
      if (!ctx.isDefaultModelSelectorValue(configured)) {
          return configured;
      }
      const fallback = ctx.resolveModelNameFromRecord(profile);
      if (ctx.isDefaultModelSelectorValue(fallback))
          return '';
      // API fallback may contain effective default model_name when agent has no explicit model.
      if (ctx.desktopLocalMode.value && ctx.isSameModelName(fallback, ctx.desktopDefaultModelDisplayName.value)) {
          return '';
      }
      if (!ctx.desktopLocalMode.value && ctx.isSameModelName(fallback, ctx.serverDefaultModelDisplayName.value)) {
          return '';
      }
      return fallback;
  };

  ctx.activeAgentDirectConfiguredModelName = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return '';
      return ctx.resolveExplicitAgentModelName(ctx.activeAgentProfileForModelResolution.value);
  });

  ctx.activeAgentConfiguredModelName = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return '';
      const directModelName = ctx.activeAgentDirectConfiguredModelName.value;
      if (directModelName)
          return directModelName;
      if (ctx.desktopMode.value) {
          return String(ctx.desktopDefaultModelDisplayName.value || '').trim();
      }
      return String(ctx.serverDefaultModelDisplayName.value || '').trim();
  });

  ctx.activeAgentUsingDesktopDefaultModel = computed(() => ctx.desktopLocalMode.value &&
      ctx.isAgentConversationActive.value &&
      !String(ctx.activeAgentDirectConfiguredModelName.value || '').trim());

  ctx.agentHeaderModelDisplayName = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return '';
      const configuredModelName = ctx.activeAgentConfiguredModelName.value;
      // Keep composer label stable by preferring configured model alias over runtime model id.
      if (configuredModelName)
          return configuredModelName;
      const sessionModelName = ctx.activeAgentSessionModelName.value;
      if (sessionModelName)
          return sessionModelName;
      const runtimeModelName = ctx.activeAgentRuntimeModelName.value;
      if (runtimeModelName)
          return runtimeModelName;
      if (ctx.desktopMode.value && ctx.desktopLocalMode.value) {
          return ctx.t('desktop.system.modelUnnamed');
      }
      return ctx.t('common.unknown');
  });

  ctx.agentHeaderModelJumpEnabled = computed(() => ctx.desktopMode.value || ctx.route.path.startsWith('/desktop'));

  ctx.activeAgentApprovalMode = computed<AgentApprovalMode>(() => {
      if (ctx.activeAgentId.value === DEFAULT_AGENT_KEY) {
          return 'full_auto';
      }
      const agent = ctx.asObjectRecord(ctx.activeAgent.value);
      const agentMode = String(agent.approval_mode || agent.approvalMode || '').trim();
      if (agentMode) {
          return normalizeAgentApprovalMode(agentMode);
      }
      const session = ctx.asObjectRecord(ctx.activeAgentSession.value);
      const sessionMode = String(session.approval_mode || session.approvalMode || '').trim();
      if (sessionMode) {
          return normalizeAgentApprovalMode(sessionMode);
      }
      return 'full_auto';
  });

  ctx.resolveCompactApprovalOptionLabel = (value: string): string => {
      const source = String(value || '').trim();
      if (!source)
          return '';
      const splitIndex = ['\uff08', '(']
          .map((marker) => source.indexOf(marker))
          .filter((index) => index > 0)
          .sort((left, right) => left - right)[0];
      return typeof splitIndex === 'number' ? source.slice(0, splitIndex).trim() : source;
  };

  ctx.agentComposerApprovalModeOptions = computed(() => buildAgentApprovalOptions((mode) => {
      const optionLabel = ctx.t(`portal.agent.permission.option.${mode}`);
      return ctx.resolveCompactApprovalOptionLabel(optionLabel) || optionLabel;
  }));

  ctx.showAgentComposerApprovalSelector = computed(() => ctx.isAgentConversationActive.value);

  ctx.resolveComposerApprovalPersistAgentId = () => ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId) ||
      DEFAULT_AGENT_KEY;

  const {
    composerApprovalMode,
    composerApprovalModeSyncing,
    updateComposerApprovalMode
  } = useComposerApprovalMode({
      isAgentConversationActive: ctx.isAgentConversationActive,
      activeAgentId: ctx.activeAgentId,
      activeAgentApprovalMode: ctx.activeAgentApprovalMode,
      resolvePersistAgentId: ctx.resolveComposerApprovalPersistAgentId,
      persistApprovalMode: async (agentId, mode) => {
          await ctx.agentStore.updateAgent(agentId, { approval_mode: mode });
          if (agentId === DEFAULT_AGENT_KEY) {
              await ctx.loadDefaultAgentProfile().catch(() => null);
          }
      },
      onPersistError: (error) => {
          showApiError(error, ctx.t('portal.agent.saveFailed'));
      }
  });
  ctx.composerApprovalMode = composerApprovalMode;
  ctx.composerApprovalModeSyncing = composerApprovalModeSyncing;
  ctx.updateComposerApprovalMode = updateComposerApprovalMode;

  ctx.agentComposerApprovalHintMode = computed<AgentApprovalMode>(() => ctx.showAgentComposerApprovalSelector.value ? ctx.composerApprovalMode.value : ctx.activeAgentApprovalMode.value);

  ctx.agentComposerApprovalHintLabel = computed(() => {
      const optionLabel = ctx.t(`portal.agent.permission.option.${ctx.agentComposerApprovalHintMode.value}`);
      const compactOption = ctx.resolveCompactApprovalOptionLabel(optionLabel) || optionLabel;
      return `${ctx.t('portal.agent.permission.title')}: ${compactOption}`;
  });

  ctx.showAgentComposerApprovalHint = computed(() => ctx.isAgentConversationActive.value);

  ctx.activeSessionApproval = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return null;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId || !Array.isArray(ctx.chatStore.pendingApprovals))
          return null;
      return (ctx.chatStore.pendingApprovals.find((item) => String(item?.session_id || '').trim() === sessionId) || null);
  });

  ctx.activeSessionRecord = computed<Record<string, unknown> | null>(() => {
      if (!ctx.isAgentConversationActive.value)
          return null;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return null;
      return ((Array.isArray(ctx.chatStore.sessions)
          ? ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId)
          : null) || null) as Record<string, unknown> | null;
  });

  ctx.activeSessionOrchestrationLock = computed<Record<string, unknown> | null>(() => {
      const session = ctx.activeSessionRecord.value;
      const lock = session && typeof session === 'object' && !Array.isArray(session)
          ? (session.orchestration_lock as Record<string, unknown> | null | undefined)
          : null;
      if (!lock || typeof lock !== 'object' || Array.isArray(lock)) {
          return null;
      }
      return lock.active === true ? lock : null;
  });

  ctx.activeSessionOrchestrationLocked = computed(() => Boolean(ctx.activeSessionOrchestrationLock.value));

  ctx.activeSessionGoalLocked = computed(() => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      return Boolean(sessionId && ctx.chatStore.isSessionGoalLocked?.(sessionId));
  });

  ctx.activeSessionGoal = computed(() => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return null;
      return typeof ctx.chatStore.sessionGoal === 'function'
          ? ctx.chatStore.sessionGoal(sessionId)
          : null;
  });

  ctx.isAgentOrchestrationActive = (agentId: unknown): boolean => {
      const normalizedAgentId = ctx.normalizeAgentId(agentId);
      if (!normalizedAgentId)
          return false;
      return (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).some((sessionRaw) => {
          const session = (sessionRaw || {}) as Record<string, unknown>;
          if (ctx.normalizeAgentId(session?.agent_id || (session?.is_default === true ? DEFAULT_AGENT_KEY : '')) !== normalizedAgentId) {
              return false;
          }
          const lock = session && typeof session === 'object' && !Array.isArray(session)
              ? (session.orchestration_lock as Record<string, unknown> | null | undefined)
              : null;
          return Boolean(lock && typeof lock === 'object' && !Array.isArray(lock) && lock.active === true);
      });
  };

  ctx.isAgentGoalActive = (agentId: unknown): boolean => {
      const normalizedAgentId = ctx.normalizeAgentId(agentId);
      if (!normalizedAgentId)
          return false;
      return (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).some((sessionRaw) => {
          const session = (sessionRaw || {}) as Record<string, unknown>;
          if (ctx.normalizeAgentId(session?.agent_id || (session?.is_default === true ? DEFAULT_AGENT_KEY : '')) !== normalizedAgentId) {
              return false;
          }
          const sessionId = String(session?.id || session?.session_id || '').trim();
          return Boolean(sessionId && ctx.chatStore.isSessionGoalLocked?.(sessionId));
      });
  };

  ctx.buildSessionAgentMap = (): Map<string, string> => {
      const sessionAgentMap = new Map<string, string>();
      (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).forEach((sessionRaw) => {
          const session = (sessionRaw || {}) as Record<string, unknown>;
          const sessionId = String(session?.id || '').trim();
          if (!sessionId)
              return;
          const resolvedAgentId = ctx.normalizeAgentId(session?.agent_id || (session?.is_default === true ? DEFAULT_AGENT_KEY : '')) || DEFAULT_AGENT_KEY;
          sessionAgentMap.set(sessionId, resolvedAgentId);
      });
      return sessionAgentMap;
  };
}
