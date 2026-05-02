// @ts-nocheck
// Store wiring, mutable refs, runtime handles, cache state, and performance tracing.
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

type MessageTtsPlaybackRuntime = {
  audio: HTMLAudioElement;
  objectUrlCache: Map<string, string>;
  currentMessageKey: string;
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

export function installMessengerControllerStateRefs(ctx: MessengerControllerContext): void {
  ctx.route = useRoute();

  ctx.router = useRouter();

  const { t } = useI18n();
  ctx.t = t;

  ctx.SUPPORTED_SKILL_ARCHIVE_SUFFIXES = [
      '.zip',
      '.skill',
      '.rar',
      '.7z',
      '.tar',
      '.tgz',
      '.tar.gz',
      '.tbz2',
      '.tar.bz2',
      '.txz',
      '.tar.xz'
  ];

  ctx.authStore = useAuthStore();

  ctx.agentStore = useAgentStore();

  ctx.chatStore = useChatStore();

  ctx.beeroomStore = useBeeroomStore();

  ctx.plazaStore = usePlazaStore();

  ctx.themeStore = useThemeStore();

  ctx.userWorldStore = useUserWorldStore();

  ctx.sessionHub = useSessionHubStore();

  ctx.DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY = 'messenger_desktop_first_launch_default_agent_hint_v1';

  ctx.bootLoading = ref(true);

  ctx.selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);

  ctx.deletingAgentSelectionSnapshot = ref<string[]>([]);

  ctx.selectedAgentHiveGroupId = ref('');

  ctx.agentOverviewMode = ref<'detail' | 'grid'>('detail');

  ctx.selectedContactUserId = ref('');

  ctx.selectedGroupId = ref('');

  ctx.plazaBrowseKind = ref<PlazaBrowseKind>('hive_pack');

  ctx.selectedPlazaItemId = ref('');

  ctx.agentQuickCreateVisible = ref(false);

  ctx.workerCardImportInputRef = ref<HTMLInputElement | null>(null);

  ctx.workerCardImporting = ref(false);

  ctx.workerCardImportOverlayVisible = ref(false);

  ctx.workerCardImportOverlayPhase = ref<'preparing' | 'creating' | 'refreshing'>('preparing');

  ctx.workerCardImportOverlayProgress = ref(0);

  ctx.workerCardImportOverlayTargetName = ref('');

  ctx.workerCardImportOverlayCurrent = ref(0);

  ctx.workerCardImportOverlayTotal = ref(0);

  ctx.selectedContactUnitId = ref('');

  ctx.selectedToolCategory = ref<'admin' | 'mcp' | 'skills' | 'knowledge' | ''>('');

  ctx.worldDraft = ref('');

  ctx.worldDraftMap = new Map<string, string>();

  ctx.dismissedAgentConversationMap = ref<Record<string, number>>({});

  ctx.dismissedAgentStorageKey = ref('');

  ctx.leftRailRef = ref<HTMLElement | null>(null);

  ctx.middlePaneRef = ref<HTMLElement | null>(null);

  ctx.rightDockRef = ref<{
      $el?: HTMLElement;
      refreshWorkspace?: (options?: {
          background?: boolean;
      }) => Promise<boolean>;
  } | null>(null);

  ctx.messengerHivePlazaPanelRef = ref<{
      openPublishDialog: () => Promise<void> | void;
      reload: () => Promise<void>;
  } | null>(null);

  ctx.worldComposerViewRef = ref<WorldComposerViewRef | null>(null);

  ctx.worldUploading = ref(false);

  ctx.worldVoiceRecording = ref(false);

  ctx.worldVoiceDurationMs = ref(0);

  ctx.agentVoiceRecording = ref(false);

  ctx.agentVoiceDurationMs = ref(0);

  ctx.worldVoicePlaybackCurrentMs = ref(0);

  ctx.worldVoicePlaybackDurationMs = ref(0);

  ctx.agentVoiceModelHearingSupported = ref<boolean | null>(null);

  ctx.desktopDefaultModelDisplayName = ref('');

  ctx.serverDefaultModelDisplayName = ref('');

  ctx.worldVoicePlayingMessageKey = ref('');

  ctx.worldVoiceLoadingMessageKey = ref('');

  ctx.messageTtsPlayingKey = ref('');

  ctx.messageTtsLoadingKey = ref('');

  ctx.worldComposerHeight = ref(188);

  ctx.worldQuickPanelMode = ref<'' | 'emoji'>('');

  ctx.worldHistoryDialogVisible = ref(false);

  ctx.helperAppsWorkspaceMode = ref(false);

  ctx.helperAppsActiveKind = ref<'offline' | 'online' | ''>('');

  ctx.helperAppsActiveKey = ref('');

  ctx.helperAppsOnlineLoading = ref(false);

  ctx.helperAppsOnlineLoaded = ref(false);

  ctx.helperAppsOnlineItems = ref<HelperAppExternalItem[]>([]);

  ctx.worldHistoryKeyword = ref('');

  ctx.worldHistoryActiveTab = ref<WorldHistoryCategory>('all');

  ctx.worldHistoryDateRange = ref<[
      string,
      string
  ] | [
  ]>([]);

  ctx.worldContainerPickerVisible = ref(false);

  ctx.worldContainerPickerLoading = ref(false);

  ctx.worldContainerPickerPath = ref('');

  ctx.worldContainerPickerKeyword = ref('');

  ctx.worldContainerPickerEntries = ref<WorldContainerPickerEntry[]>([]);

  ctx.agentPromptPreviewVisible = ref(false);

  ctx.agentPromptPreviewLoading = ref(false);

  ctx.agentPromptPreviewContent = ref('');

  ctx.agentPromptPreviewMemoryMode = ref<'none' | 'pending' | 'frozen'>('none');

  ctx.agentPromptPreviewToolingMode = ref('');

  ctx.agentPromptPreviewToolingContent = ref('');

  ctx.agentPromptPreviewToolingItems = ref<PromptToolingPreviewItem[]>([]);

  ctx.agentPromptPreviewSelectedNames = ref<string[] | null>(null);

  ctx.AGENT_PROMPT_PREVIEW_CACHE_MS = 5000;

  ctx.agentPromptPreviewPayloadPromise = null;

  ctx.agentPromptPreviewPayloadPromiseKey = '';

  ctx.agentPromptPreviewPayloadCache = null;

  ctx.imagePreviewVisible = ref(false);

  ctx.imagePreviewUrl = ref('');

  ctx.imagePreviewTitle = ref('');

  ctx.imagePreviewWorkspacePath = ref('');

  ctx.agentPromptToolSummary = ref<Record<string, unknown> | null>(null);

  ctx.agentToolSummaryLoading = ref(false);

  ctx.agentToolSummaryError = ref('');

  ctx.agentToolSummaryPromise = null;

  ctx.agentAbilityTooltipRef = ref<TooltipLike | TooltipLike[] | null>(null);

  ctx.agentAbilityTooltipVisible = ref(false);

  ctx.agentAbilityTooltipOptions = {
      strategy: 'fixed',
      modifiers: [
          { name: 'offset', options: { offset: [0, 10] } },
          { name: 'shift', options: { padding: 8 } },
          { name: 'flip', options: { padding: 8, fallbackPlacements: ['top', 'bottom', 'right', 'left'] } },
          { name: 'preventOverflow', options: { padding: 8, altAxis: true, boundary: 'viewport' } }
      ]
  };

  ctx.worldRecentEmojis = ref<string[]>([]);

  ctx.messageListRef = ref<HTMLElement | null>(null);

  ctx.chatFooterRef = ref<HTMLElement | null>(null);

  ctx.messageVirtualScrollTop = ref(0);

  ctx.messageVirtualViewportHeight = ref(0);

  ctx.messageVirtualLayoutVersion = ref(0);

  ctx.messageVirtualHeightCache = new Map<string, number>();

  ctx.agentRuntimeStateMap = ref<Map<string, AgentRuntimeState>>(new Map());

  ctx.agentUserRoundsMap = ref<Map<string, number>>(new Map());

  ctx.messengerOrderHydrating = ref(false);

  ctx.messengerOrderReady = ref(false);

  ctx.messengerOrderSaveTimer = ref<number | null>(null);

  ctx.messengerOrderSnapshot = ref<MessengerOrderPreferences>(defaultMessengerOrderPreferences());

  ctx.beeroomDispatchSessionIdsByGroup = ref<Record<string, string[]>>({});

  ctx.runtimeStateOverrides = ref<Map<string, {
      state: AgentRuntimeState;
      expiresAt: number;
  }>>(new Map());

  ctx.cronAgentIds = ref<Set<string>>(new Set());

  ctx.channelBoundAgentIds = ref<Set<string>>(new Set());

  ctx.cronPermissionDenied = ref(false);

  ctx.agentSettingMode = ref<AgentSettingMode>('agent');

  ctx.mountedAgentSettingModes = ref<Record<AgentSettingMode, boolean>>({
      agent: true,
      cron: false,
      channel: false,
      runtime: false,
      memory: false,
      archived: false
  });

  ctx.agentSettingsFocusTarget = ref<'' | 'model'>('');

  ctx.agentSettingsFocusToken = ref(0);

  ctx.settingsPanelMode = ref<SettingsPanelMode>('general');

  ctx.rightDockCollapsed = ref(false);

  ctx.rightDockEdgeHover = ref(false);

  ctx.desktopInitialSectionPinned = ref(false);

  ctx.desktopShowFirstLaunchDefaultAgentHint = ref(false);

  ctx.desktopFirstLaunchDefaultAgentHintAt = ref(0);

  ctx.usernameSaving = ref(false);

  ctx.appearanceHydrating = ref(false);

  ctx.currentUserAvatarIcon = ref('initial');

  ctx.currentUserAvatarColor = ref('#3b82f6');

  ctx.helpManualLoading = ref(false);

  ctx.toolsCatalogLoading = ref(false);

  ctx.toolsCatalogLoaded = ref(false);

  ctx.builtinTools = ref<ToolEntry[]>([]);

  ctx.mcpTools = ref<ToolEntry[]>([]);

  ctx.skillTools = ref<ToolEntry[]>([]);

  ctx.knowledgeTools = ref<ToolEntry[]>([]);

  ctx.fileScope = ref<'agent' | 'user'>('agent');

  ctx.selectedFileContainerId = ref(USER_CONTAINER_ID);

  ctx.fileContainerLatestUpdatedAt = ref(0);

  ctx.fileContainerEntryCount = ref(0);

  ctx.fileLifecycleNowTick = ref(Date.now());

  ctx.fileContainerMenuViewRef = ref<{
      getMenuElement: () => HTMLElement | null;
  } | null>(null);

  ctx.desktopContainerManagerPanelRef = ref<{
      openManager: (containerId?: number) => Promise<void> | void;
  } | null>(null);

  ctx.agentSettingsPanelRef = ref<{
      triggerReload: () => Promise<void> | void;
      triggerSave: () => Promise<void> | void;
      triggerDelete: () => Promise<void> | void;
      triggerExportWorkerCard: () => Promise<void> | void;
  } | null>(null);

  ctx.fileContainerContextMenu = ref<{
      visible: boolean;
      x: number;
      y: number;
      target: FileContainerMenuTarget | null;
  }>({
      visible: false,
      x: 0,
      y: 0,
      target: null
  });

  ctx.desktopContainerRootMap = ref<Record<number, string>>({});

  ctx.timelinePreviewMap = ref<Map<string, string>>(new Map());

  ctx.timelinePreviewLoadingSet = ref<Set<string>>(new Set());

  ctx.rightDockSkillCatalog = ref<RightDockSkillCatalogItem[]>([]);

  ctx.rightDockSkillCatalogLoading = ref(false);

  ctx.rightDockSkillDialogVisible = ref(false);

  ctx.rightDockSelectedSkillName = ref('');

  ctx.rightDockSkillContentLoading = ref(false);

  ctx.rightDockSkillContent = ref('');

  ctx.rightDockSkillContentPath = ref('');

  ctx.rightDockSkillToggleSaving = ref(false);

  ctx.timelineDialogVisible = ref(false);

  ctx.timelineDetailDialogVisible = ref(false);

  ctx.timelineDetailSessionId = ref('');

  ctx.skillDockUploading = ref(false);

  ctx.approvalResponding = ref(false);

  ctx.messengerSendKey = ref<MessengerSendKeyMode>('enter');

  ctx.uiFontSize = ref(14);

  ctx.orgUnitPathMap = ref<Record<string, string>>({});

  ctx.orgUnitTree = ref<UnitTreeNode[]>([]);

  ctx.contactUnitExpandedIds = ref<Set<string>>(new Set());

  ctx.showScrollTopButton = ref(false);

  ctx.showScrollBottomButton = ref(false);

  ctx.autoStickToBottom = ref(true);

  ctx.agentInquirySelection = ref<number[]>([]);

  ctx.agentPlanExpanded = ref(false);

  ctx.beeroomFirstEntryAutoSelectionPending = ref(true);

  ctx.dismissedPlanMessages = ref<WeakSet<Record<string, unknown>>>(new WeakSet());

  ctx.dismissedPlanVersion = ref(0);

  ctx.groupCreateVisible = ref(false);

  ctx.groupCreateName = ref('');

  ctx.groupCreateKeyword = ref('');

  ctx.groupCreateMemberIds = ref<string[]>([]);

  ctx.groupCreating = ref(false);

  ctx.creatingAgentSession = ref(false);

  const { hostRootRef: messengerRootRef, hostWidth: viewportWidth, refreshHostWidth } = useMessengerHostWidth();
  ctx.messengerRootRef = messengerRootRef;
  ctx.viewportWidth = viewportWidth;
  ctx.refreshHostWidth = refreshHostWidth;

  const {
    isBlocked: isMessengerInteractionBlocked,
    label: messengerInteractionBlockingLabel,
    activeReason: messengerInteractionBlockReason,
    runWithBlock: runWithMessengerInteractionBlock
  } = useMessengerInteractionBlocker({
      rootRef: ctx.messengerRootRef,
      resolveLabel: (reason) => (reason === 'new_session' ? ctx.t('chat.newSession') : ctx.t('common.refresh'))
  });
  ctx.isMessengerInteractionBlocked = isMessengerInteractionBlocked;
  ctx.messengerInteractionBlockingLabel = messengerInteractionBlockingLabel;
  ctx.messengerInteractionBlockReason = messengerInteractionBlockReason;
  ctx.runWithMessengerInteractionBlock = runWithMessengerInteractionBlock;

  ctx.middlePaneOverlayVisible = ref(false);

  ctx.middlePaneMounted = ref(false);

  ctx.standardNavigationCollapsed = ref(false);

  ctx.leftRailMoreExpanded = ref(false);

  ctx.quickCreatingAgent = ref(false);

  ctx.agentMainReadAtMap = ref<Record<string, number>>({});

  ctx.agentMainUnreadCountMap = ref<Record<string, number>>({});

  ctx.agentUnreadStorageKeys = ref<{
      readAt: string;
      unread: string;
  }>({ readAt: '', unread: '' });

  ctx.keywordInput = ref('');

  ctx.contactVirtualListRef = ref<HTMLElement | null>(null);

  ctx.contactVirtualScrollTop = ref(0);

  ctx.contactVirtualViewportHeight = ref(0);

  ctx.setContactVirtualListRef = (element: HTMLElement | null) => {
      ctx.contactVirtualListRef.value = element;
  };

  ctx.lifecycleTimer = null;

  ctx.worldQuickPanelCloseTimer = null;

  ctx.timelinePrefetchTimer = null;

  ctx.sessionDetailPrefetchTimer = null;

  ctx.middlePaneOverlayHideTimer = null;

  ctx.middlePanePrewarmTimer = null;

  ctx.keywordDebounceTimer = null;

  ctx.contactVirtualFrame = null;

  ctx.viewportResizeFrame = null;

  ctx.viewportResizeHandler = null;

  ctx.audioRecordingSupportHandler = null;

  ctx.audioRecordingSupportRetryTimer = null;

  ctx.startRealtimePulse = null;

  ctx.stopRealtimePulse = null;

  ctx.triggerRealtimePulseRefresh = null;

  ctx.startBeeroomRealtimeSync = null;

  ctx.stopBeeroomRealtimeSync = null;

  ctx.triggerBeeroomRealtimeSyncRefresh = null;

  ctx.messageViewportRuntime = null;

  ctx.worldComposerResizeRuntime = null;

  ctx.worldVoiceRecordingRuntime = null;

  ctx.agentVoiceRecordingRuntime = null;

  ctx.worldVoicePlaybackRuntime = null;

  ctx.messageTtsPlaybackRuntime = null;

  ctx.runningAgentsLoadVersion = 0;

  ctx.agentUserRoundsLoadVersion = 0;

  ctx.cronAgentIdsLoadVersion = 0;

  ctx.channelBoundAgentIdsLoadVersion = 0;

  ctx.runningAgentsLoadPromise = null;

  ctx.runningAgentsLoadedAt = 0;

  ctx.cronAgentIdsLoadPromise = null;

  ctx.cronAgentIdsLoadedAt = 0;

  ctx.channelBoundAgentIdsLoadPromise = null;

  ctx.channelBoundAgentIdsLoadedAt = 0;

  ctx.toolsCatalogLoadVersion = 0;

  ctx.rightDockSkillCatalogLoadVersion = 0;

  ctx.rightDockSkillContentLoadVersion = 0;

  ctx.rightDockSkillAutoRetryTimer = null;

  ctx.desktopDefaultModelMetaFetchPromise = null;

  ctx.serverDefaultModelCheckedAt = 0;

  ctx.serverDefaultModelFetchPromise = null;

  ctx.agentVoiceModelSupportCheckedAt = 0;

  ctx.beeroomGroupsLastRefreshAt = 0;

  ctx.agentUnreadRefreshInFlight = new Set<string>();

  ctx.MARKDOWN_CACHE_LIMIT = 280;

  ctx.MARKDOWN_STREAM_THROTTLE_MS = 80;

  ctx.CONTACT_VIRTUAL_ITEM_HEIGHT = 60;

  ctx.CONTACT_VIRTUAL_OVERSCAN = 8;

  ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT = 118;

  ctx.AGENT_VOICE_MODEL_SUPPORT_CACHE_MS = 30000;

  ctx.SERVER_DEFAULT_MODEL_CACHE_MS = 30000;

  ctx.AGENT_META_REQUEST_CACHE_MS = 1500;

  ctx.SESSION_DETAIL_PREFETCH_DELAY_MS = 90;

  ctx.BEEROOM_GROUPS_REFRESH_MIN_MS_HOT = 2800;

  ctx.BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE = 7000;

  ctx.markdownCache = new Map<string, {
      source: string;
      html: string;
      updatedAt: number;
  }>();

  ctx.KEYWORD_INPUT_DEBOUNCE_MS = 120;

  ctx.RIGHT_DOCK_SKILL_AUTO_RETRY_DELAY_MS = 1200;

  ctx.workspaceResourceCache = new Map<string, WorkspaceResourceCacheEntry>();

  ctx.userAttachmentResourceCache = ref(new Map<string, AttachmentResourceState>());

  ctx.workspaceResourceHydrationFrame = null;

  ctx.workspaceResourceHydrationPending = false;

  ctx.stopWorkspaceRefreshListener = null;

  ctx.stopAgentRuntimeRefreshListener = null;

  ctx.stopUserToolsUpdatedListener = null;

  ctx.pendingAssistantCenter = false;

  ctx.pendingAssistantCenterCount = 0;

  ctx.MESSENGER_PERF_TRACE_ENABLED = (() => {
      if (typeof window === 'undefined')
          return false;
      const raw = String(window.localStorage.getItem('messenger_perf_trace') || '')
          .trim()
          .toLowerCase();
      if (raw === '1' || raw === 'true' || raw === 'on')
          return true;
      return import.meta.env.DEV;
  })();

  ctx.startMessengerPerfTrace = (label: string, meta: Record<string, unknown> = {}): MessengerPerfTrace | null => {
      if (!ctx.MESSENGER_PERF_TRACE_ENABLED)
          return null;
      return {
          label,
          startedAt: performance.now(),
          marks: [],
          meta
      };
  };

  ctx.markMessengerPerfTrace = (trace: MessengerPerfTrace | null, name: string) => {
      if (!trace)
          return;
      trace.marks.push({ name, at: performance.now() });
  };

  ctx.finishMessengerPerfTrace = (trace: MessengerPerfTrace | null, status: 'ok' | 'fail' | 'pending' = 'ok', extra: Record<string, unknown> = {}) => {
      if (!trace)
          return;
      const totalMs = Number((performance.now() - trace.startedAt).toFixed(1));
      const marks = trace.marks.map((item) => ({
          name: item.name,
          sinceStartMs: Number((item.at - trace.startedAt).toFixed(1))
      }));
      console.info('[messenger-perf]', {
          label: trace.label,
          status,
          totalMs,
          ...trace.meta,
          ...extra,
          marks
      });
  };
}
