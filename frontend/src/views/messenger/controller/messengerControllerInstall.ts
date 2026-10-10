// @ts-nocheck
// Messenger view controller: single consolidated installer for the chat shell.
// Each part below owns one concern of the messenger view; parts are executed in
// order and publish state, computed values and handlers onto the shared context.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus';
import type { MessengerControllerContext } from './messengerControllerContext';
import { listAgentUserRounds, listRunningAgents } from '@/api/agents';
import { updateProfile } from '@/api/auth';
import { listChannelBindings } from '@/api/channels';
import { fetchRealtimeSystemPrompt, fetchSessionSystemPrompt, getSession as getChatSessionApi, synthesizeChatTts } from '@/api/chat';
import { fetchCronJobs } from '@/api/cron';
import { fetchUserSkillContent, uploadUserSkillZip } from '@/api/userTools';
import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent } from '@/api/workspace';
import { resolveRuntimeMessageContentSource } from '@/components/chat/messageRuntimeContent';
import { getRuntimeConfig } from '@/config/runtime';
import { getCurrentLanguage, setLanguage, useI18n } from '@/i18n';
import { flushBackgroundPublication } from '@/realtime/chat/chatBackgroundPublication';
import { resolveChatRuntimeRenderableKey } from '@/realtime/chat/chatRuntimeMessageKeys';
import { resolveChatRuntimeMessageRenderKey } from '@/realtime/chat/chatRuntimeRenderAdapter';
import { buildChatThreadMaterializedSlots } from '@/realtime/chat/chatThreadRuntime';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { bindRuntimeMessageToUserRound, buildRuntimeDebugSnapshot, cacheSessionMessages, getRuntime, notifySessionSnapshot, settleTerminalSessionRuntime, touchSessionUpdatedAt } from '@/stores/chatRuntimeState';
import { isSessionUnavailable } from '@/stores/chatSessionAvailability';
import { resolveSectionFromRoute, useSessionHubStore } from '@/stores/sessionHub';
import type { MessengerSection } from '@/stores/sessionHub';
import { useThemeStore } from '@/stores/theme';
import { buildDeclaredDependencyPayload } from '@/utils/agentDependencyStatus';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { showApiError, resolveApiError } from '@/utils/apiError';
import { buildAssistantDisplayContent, resolveAssistantFailureNotice } from '@/utils/assistantFailureNotice';
import { hasAssistantWaitingForCurrentOutput, normalizeAssistantMessageRuntimeState, resolveAssistantMessageRuntimeState } from '@/utils/assistantMessageRuntime';
import { isAudioRecordingSupported } from '@/utils/audioRecorder';
import type { AudioRecordingSession } from '@/utils/audioRecorder';
import { redirectToLoginAfterLogout } from '@/utils/authNavigation';
import { PROFILE_AVATAR_COLORS, PROFILE_AVATAR_IMAGE_KEYS, PROFILE_AVATAR_IMAGE_MAP, PROFILE_AVATAR_OPTION_KEYS } from '@/utils/avatarCatalog';
import { resolveBlobApiErrorMessage } from '@/utils/blobApiError';
import { isCompactionRunningFromWorkflowItems } from '@/utils/chatCompactionWorkflow';
import { chatDebugLog, isChatDebugEnabled, isChatDebugVerboseEnabled } from '@/utils/chatDebug';
import { hasRunningAssistantMessage, hasStreamingAssistantMessage, isThreadRuntimeBusy } from '@/utils/chatSessionRuntime';
import { copyText } from '@/utils/clipboard';
import { confirmWithFallback } from '@/utils/confirm';
import { hydrateExternalMarkdownImages, renderMarkdown } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { buildWorkspacePublicPath, normalizeWorkspaceOwnerId, resolveMarkdownWorkspacePath } from '@/utils/messageWorkspacePath';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import { extractPromptToolingPreview } from '@/utils/promptToolingPreview';
import type { PromptToolingPreviewItem } from '@/utils/promptToolingPreview';
import { hasActiveSubagentItems } from '@/utils/subagentRuntime';
import { collectAbilityDetails, collectAbilityGroupDetails, collectAbilityNames } from '@/utils/toolSummary';
import { normalizeAvatarColor, normalizeAvatarIcon, normalizeThemePalette } from '@/utils/userPreferences';
import type { ThemePalette, UserAppearancePreferences } from '@/utils/userPreferences';
import { invalidateAllUserToolsCaches, invalidateUserSkillsCache, invalidateUserToolsCatalogCache, invalidateUserToolsSummaryCache, loadUserSkillsCache, loadUserToolsCatalogCache, loadUserToolsSummaryCache } from '@/utils/userToolsCache';
import { emitUserToolsUpdated, onUserToolsUpdated } from '@/utils/userToolsEvents';

import { claimAgentRuntimeAgentCompletion, claimAgentRuntimeCompletion, emitWorkspaceRefresh, onAgentRuntimeRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { createWorkspaceHydrationBatch } from '@/utils/workspaceHydrationBatch';
import { extractWorkspaceRefreshPaths, isWorkspacePathAffected } from '@/utils/workspaceRefresh';
import { bindWorkspaceImagePreviewState, getFilenameFromHeaders, hydrateWorkspaceResourceErrorDiagnostics, markWorkspaceImageCardError, normalizeWorkspaceImageResponseBlob, resetWorkspaceImageCardState, resolveWorkspaceResourceErrorDiagnostics, saveObjectUrlAsFile, scheduleWorkspaceLoadingLabel } from '@/utils/workspaceResourceCards';
import { WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES, decodeWorkspaceResourceLabel, extractWorkspaceResourceExtension, normalizeWorkspacePreviewBlob, normalizeWorkspacePreviewFilename, resolveWorkspacePreviewTooLargeHint, resolveWorkspacePreviewUnsupportedHint, resolveWorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';
import { buildWorkspaceResourceRequestParams } from '@/utils/workspaceResourceRequest';
import { isImagePath, isMetafileImagePath, parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
import { installActiveChatRealtimeRecovery } from '@/views/messenger/activeChatRealtimeRecovery';
import { isAgentAlreadyOpen } from '@/views/messenger/agentOpenState';
import { resolveAgentConfiguredAbilityNames, resolveAgentOverviewAbilityCounts } from '@/views/messenger/agentOverviewAbilities';
import { buildDefaultAgentOverviewSource } from '@/views/messenger/agentOverviewCards';
import { TERMINAL_SESSION_RUNTIME_STATUS_SET, WAITING_SESSION_RUNTIME_STATUS_SET, hasAgentTerminalSettlementEvidence, isWaitingMessengerRuntimeStatus, resolveAgentRuntimeStateFromSignals, resolveAgentRuntimeTerminalStateFromSessionStatus, shouldNotifyAgentTaskCompletion, shouldPreserveMissingAgentRuntimeState, shouldSettleAgentRuntimeFromTerminalSession, shouldSettleAgentSessionsFromRuntimeState } from '@/views/messenger/agentRuntimeState';
import { resolveAgentSelectionAfterRemoval } from '@/views/messenger/agentSelection';
import { scheduleMessengerBootstrapBackgroundTasks, settleMessengerBootstrapTasks, shouldUseNonBlockingDesktopMessageBootstrap, splitMessengerBootstrapTasks } from '@/views/messenger/bootstrap';
import { buildAgentApprovalOptions, normalizeAgentApprovalMode, useComposerApprovalMode } from '@/views/messenger/composerApprovalMode';
import type { AgentApprovalMode } from '@/views/messenger/composerApprovalMode';
import { resolveFileContainerLifecycleText } from '@/views/messenger/fileWorkspacePresentation';
import { useMessengerHostWidth } from '@/views/messenger/hostWidth';
import { useMessengerInteractionBlocker } from '@/views/messenger/interactionBlocker';
import { preloadAgentSettingsPanels, preloadMessengerSettingsPanels } from '@/views/messenger/lazyPanels';
import { MessageConversationKind, hasRetainedAgentConversationContext, hasRetainedMessageConversationContext, hasRetainedMessageConversationContext as resolveRetainedMessageConversationContext, resolveMessageConversationKind } from '@/views/messenger/messageConversationRetention';
import { createMessageViewportRuntime } from '@/views/messenger/messageViewportRuntime';
import { buildMessageVirtualWindow, resolveVirtualOffsetTop } from '@/views/messenger/messageVirtualWindow';
import { defaultMessengerOrderPreferences, loadMessengerOrderPreferences, saveMessengerOrderPreferences } from '@/views/messenger/messengerOrderSync';
import type { MessengerOrderPreferences } from '@/views/messenger/messengerOrderSync';
import { AGENT_CONTAINER_IDS, AGENT_MAIN_READ_AT_STORAGE_PREFIX, AGENT_MAIN_UNREAD_STORAGE_PREFIX, AGENT_TOOL_OVERRIDE_NONE, DEFAULT_AGENT_KEY, DISMISSED_AGENT_STORAGE_PREFIX, MESSENGER_SEND_KEY_STORAGE_KEY, MESSENGER_UI_FONT_SIZE_STORAGE_KEY, USER_CONTAINER_ID, WORLD_COMPOSER_HEIGHT_STORAGE_KEY, sectionRouteMap } from '@/views/messenger/model';
import type { AgentFileContainer, AgentLocalCommand, AgentOverviewCard, AgentRuntimeState, DesktopBridge, DesktopInstallResult, DesktopUpdateState, FileContainerMenuTarget, MessengerPerfTrace, MessengerSendKeyMode, MixedConversation, ToolEntry, UnitTreeNode, WorldComposerViewRef } from '@/views/messenger/model';
import { createMessengerRealtimePulse } from '@/views/messenger/realtimePulse';
import { buildRecentAgentSelection, readRecentAgentSelection, writeRecentAgentSelection } from '@/views/messenger/recentAgentSelection';
import { shouldShowAgentSettingsPanelForSection } from '@/views/messenger/settingsPanelVisibility';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import { captureStopRunSnapshot, validateStopRunSnapshot } from '@/views/messenger/stopRunGuard';
import { buildTaskList } from '@/views/messenger/taskList';
import { loadUserAppearance, saveUserAppearance } from '@/views/messenger/userAppearanceSync';

// Deferred realtime-pulse start handle shared by the lifecycle installers.
let desktopRealtimePulseStartTimer: number | null = null;

function installPart0(ctx: any): void {
// Store wiring, mutable refs, runtime handles, cache state, and performance tracing.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerStateRefs(ctx: MessengerControllerContext): void {
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





  ctx.themeStore = useThemeStore();



  ctx.sessionHub = useSessionHubStore();

  ctx.DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY = 'messenger_desktop_first_launch_default_agent_hint_v1';
  ctx.recentAgentSelection = ref(readRecentAgentSelection());
  ctx.rememberRecentAgentSelection = (agentId: unknown, sessionId: unknown = '') => {
      const selection = buildRecentAgentSelection(agentId || DEFAULT_AGENT_KEY, sessionId);
      ctx.recentAgentSelection.value = selection;
      writeRecentAgentSelection(selection);
  };
  ctx.resolveRecentAgentSelection = () => ctx.recentAgentSelection.value || readRecentAgentSelection();

  ctx.bootLoading = ref(true);

  ctx.selectedAgentId = ref<string>(DEFAULT_AGENT_KEY);

  ctx.selectedAgentHiveGroupId = ref('');

  ctx.agentOverviewMode = ref<'detail' | 'grid'>('detail');

  ctx.selectedContactUserId = ref('');

  ctx.selectedGroupId = ref('');



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


  ctx.worldComposerViewRef = ref<WorldComposerViewRef | null>(null);

  ctx.agentComposerViewRef = ref<{
      appendTextToComposer?: (value: string) => void;
      focusComposerInputAtEnd?: () => void;
  } | null>(null);





  ctx.agentVoiceRecording = ref(false);

  ctx.agentVoiceDurationMs = ref(0);

  ctx.agentVoiceTranscribing = ref(false);

  ctx.worldVoicePlaybackCurrentMs = ref(0);

  ctx.worldVoicePlaybackDurationMs = ref(0);

  ctx.agentVoiceModelHearingSupported = ref<boolean | null>(null);

  ctx.desktopDefaultModelDisplayName = ref('');

  ctx.desktopDefaultModelMaxContext = ref<number | null>(null);

  ctx.serverDefaultModelDisplayName = ref('');

  ctx.worldVoicePlayingMessageKey = ref('');

  ctx.worldVoiceLoadingMessageKey = ref('');

  ctx.messageTtsPlayingKey = ref('');

  ctx.messageTtsLoadingKey = ref('');

  ctx.worldComposerHeight = ref(188);

  ctx.worldQuickPanelMode = ref<'' | 'emoji'>('');


  const MIDDLE_PANE_SEARCHABLE_SECTIONS = new Set(['messages']);

  ctx.showMiddlePaneSearch = computed(() =>
    MIDDLE_PANE_SEARCHABLE_SECTIONS.has(String(ctx.sessionHub.activeSection || '').trim())
  );











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

  ctx.resourcePreviewVisible = ref(false);

  ctx.resourcePreviewLoading = ref(false);

  ctx.resourcePreviewUrl = ref('');

  ctx.resourcePreviewTitle = ref('');

  ctx.resourcePreviewMeta = ref('');

  ctx.resourcePreviewHint = ref('');

  ctx.resourcePreviewContent = ref('');

  ctx.resourcePreviewWorkspacePath = ref('');

  ctx.resourcePreviewKind = ref('image');

  ctx.resourcePreviewUserId = ref('');

  ctx.onlyOfficeVisible = ref(false);

  ctx.onlyOfficePath = ref('');

  ctx.onlyOfficeUserId = ref('');

  ctx.onlyOfficeAgentId = ref('');

  ctx.onlyOfficeContainerId = ref<number | null>(null);

  ctx.drawioVisible = ref(false);

  ctx.drawioPath = ref('');

  ctx.drawioUserId = ref('');

  ctx.drawioAgentId = ref('');

  ctx.drawioContainerId = ref<number | null>(null);

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
  ctx.selectedDesktopModelKey = ref('');
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

  ctx.chatWorkspaceBindingDialogVisible = ref(false);

  ctx.chatWorkspaceBindingCurrentPath = ref('/');

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

  ctx.rightDockSkillCatalog = ref<RightDockSkillCatalogItem[]>([]);

  ctx.rightDockSkillCatalogLoading = ref(false);

  ctx.rightDockSkillDialogVisible = ref(false);

  ctx.rightDockSelectedSkillName = ref('');

  ctx.rightDockSkillContentLoading = ref(false);

  ctx.rightDockSkillContent = ref('');

  ctx.rightDockSkillContentPath = ref('');

  ctx.rightDockSkillToggleSaving = ref(false);


  ctx.timelineDetailDialogVisible = ref(false);

  ctx.timelineDetailSessionId = ref('');

  ctx.messengerSessionRefreshTraceId = ref('');

  ctx.messengerSessionRefreshTraceSource = ref('');

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

  ctx.agentInquirySelection = ref<AgentInquiryPanelAnswer[]>([]);

  ctx.dismissedPlanMessages = ref<WeakSet<Record<string, unknown>>>(new WeakSet());

  ctx.dismissedPlanVersion = ref(0);





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

  ctx.agentSendForegroundLock = ref(false);

  ctx.agentSendForegroundLockSessionId = ref('');

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

  // ---------------------------------------------------------------------------
  // Shell runtime helpers for the two-column messenger.
  // The previous controller split lived in one module per concern; the remaining
  // helpers below are the ones the surviving shell still calls. Surfaces that no
  // longer exist (helper apps, beeroom groups, contact directory) deliberately
  // install nothing.

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
      }
  };

  ctx.cancelAgentVoiceRecording = async () => {
      const runtime = ctx.agentVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.agentVoiceRecordingRuntime = null;
      if (runtime.timerId !== null) {
          window.clearInterval(runtime.timerId);
      }
      ctx.agentVoiceRecording.value = false;
      ctx.agentVoiceDurationMs.value = 0;
      ctx.agentVoiceTranscribing.value = false;
      await runtime.session.cancel().catch(() => undefined);
  };

  ctx.cancelWorldVoiceRecording = async () => {
      const runtime = ctx.worldVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.worldVoiceRecordingRuntime = null;
      if (runtime.timerId !== null) {
          window.clearInterval(runtime.timerId);
      }
      await runtime.session.cancel().catch(() => undefined);
  };

  // The world composer keeps one draft per conversation; the key is the
  // conversation id and drafts never outlive the session.


  ctx.loadStoredStringArray = (storageKey: string, maxCount: number): string[] => {
      if (typeof window === 'undefined')
          return [];
      try {
          const parsed = JSON.parse(String(window.localStorage.getItem(storageKey) || 'null'));
          if (!Array.isArray(parsed))
              return [];
          return parsed.map((item) => String(item || '').trim()).filter(Boolean).slice(0, maxCount);
      }
      catch {
          return [];
      }
  };

  ctx.clampWorldComposerHeight = (value: unknown): number => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed))
          return 188;
      return Math.min(340, Math.max(168, Math.round(parsed)));
  };

  ctx.persistWorldComposerHeight = () => {
      try {
          window.localStorage.setItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY, String(ctx.worldComposerHeight.value));
      }
      catch {
          // Preference only.
      }
  };

  ctx.clearWorldQuickPanelClose = () => {
      if (ctx.worldQuickPanelCloseTimer) {
          window.clearTimeout(ctx.worldQuickPanelCloseTimer);
          ctx.worldQuickPanelCloseTimer = null;
      }
  };

  ctx.stopWorldComposerResize = () => {
      if (!ctx.worldComposerResizeRuntime)
          return;
      ctx.worldComposerResizeRuntime = null;
      ctx.persistWorldComposerHeight();
  };

  // The shell keeps a single global pointerdown dismiss handler: it only owns
  // surfaces that still exist (context menus), never the removed side panes.
  ctx.closeWorldQuickPanelWhenOutside = (event: Event) => {
      const target = event.target as Node | null;
      if (!target)
          return;
      if (ctx.fileContainerContextMenu.value.visible) {
          const menu = ctx.fileContainerMenuViewRef.value?.getMenuElement() || null;
          if (!menu || !menu.contains(target)) {
              ctx.closeFileContainerMenu();
          }
      }
      if (ctx.worldQuickPanelMode.value) {
          const composerElement = ctx.worldComposerViewRef.value?.getComposerElement() || null;
          if (!composerElement || !composerElement.contains(target)) {
              ctx.clearWorldQuickPanelClose();
              ctx.worldQuickPanelMode.value = '';
          }
      }
  };

  ctx.normalizeUploadPath = (value: unknown): string => String(value || '').trim();

  ctx.resolveDesktopAbsoluteWorkspacePathAsync = async (
      relativePath: string,
      containerId?: number | null
  ): Promise<string> => ctx.resolveDesktopAbsoluteWorkspacePath(String(relativePath || '').trim(), containerId);

  ctx.refreshAgentMutationState = async () => {
      const tasks: Promise<unknown>[] = [
          ctx.agentStore.loadAgents(),
          ctx.loadRunningAgents({ force: true })
      ];
      if (!ctx.cronPermissionDenied.value) {
          tasks.push(ctx.loadCronAgentIds({ force: true }));
      }
      await Promise.all(tasks);
  };

  // A world voice message carries a JSON payload as its content. Parsing stays
  // tolerant because the payload is only rendered by the retained world branch.
  ctx.resolveWorldVoicePayloadFromMessage = (message: Record<string, unknown>) => {
      const contentType = String(message?.content_type ?? message?.contentType ?? '').trim().toLowerCase();
      if (!contentType.includes('voice')) {
          return null;
      }
      try {
          const parsed = JSON.parse(String(message?.content ?? ''));
          if (!parsed || typeof parsed !== 'object') {
              return null;
          }
          const record = parsed as Record<string, unknown>;
          const path = String(record.path ?? '').trim();
          return path ? record : null;
      }
      catch {
          return null;
      }
  };

  // Fetching a world voice clip used the user-world download endpoint, which was
  // removed with that surface. Nothing reachable calls this, so it fails loudly
  // instead of silently pretending the audio loaded.
  ctx.fetchWorldVoiceObjectUrl = async () => {
      throw new Error(ctx.t('messenger.world.voice.playFailed'));
  };

  ctx.readDesktopDefaultModelMeta = async () => undefined;
}
  installMessengerControllerStateRefs(ctx);
}

function installPart1(ctx: any): void {
// Messenger shell navigation, desktop mode, responsive panes, and host layout state.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerShellLayoutState(ctx: MessengerControllerContext): void {
  ctx.isLeftNavSectionActive = (section: MessengerSection): boolean => ctx.isSectionButtonActive(section);

  ctx.closeLeftRailMoreMenu = () => {
      ctx.leftRailMoreExpanded.value = false;
  };

  ctx.toggleLeftRailMoreMenu = () => {
      ctx.clearMiddlePaneOverlayHide();
      ctx.leftRailMoreExpanded.value = !ctx.leftRailMoreExpanded.value;
  };

  ctx.basePrefix = computed(() => {
      if (ctx.route.path.startsWith('/desktop'))
          return '/desktop';
      if (ctx.route.path.startsWith('/demo'))
          return '/demo';
      return '/app';
  });

  ctx.isEmbeddedChatRoute = computed(() => /\/embed\/chat$/.test(String(ctx.route.path || '').trim()));

  ctx.allowNavigationCollapse = computed(() => !ctx.isEmbeddedChatRoute.value);

  ctx.navigationPaneCollapsed = computed(() => {
      if (ctx.isEmbeddedChatRoute.value) {
          return true;
      }
      return ctx.standardNavigationCollapsed.value;
  });

  ctx.navigationPaneToggleTitle = computed(() => ctx.navigationPaneCollapsed.value ? ctx.t('common.expand') : ctx.t('common.collapse'));

  ctx.getDesktopBridge = (): DesktopBridge | null => {
      if (typeof window === 'undefined')
          return null;
      const candidate = (window as Window & {
          wunderDesktop?: DesktopBridge;
      }).wunderDesktop;
      return candidate && typeof candidate === 'object' ? candidate : null;
  };

  ctx.desktopLocalMode = computed(() => false);

  ctx.settingsLogoutDisabled = computed(() => false);

  ctx.debugToolsAvailable = computed(() => typeof ctx.getDesktopBridge()?.toggleDevTools === 'function');

  ctx.desktopUpdateAvailable = computed(() => typeof ctx.getDesktopBridge()?.checkForUpdates === 'function');


  ctx.detectAudioRecordingSupport = (): boolean => {
      try {
          return isAudioRecordingSupported();
      }
      catch {
          return false;
      }
  };

  ctx.audioRecordingSupported = ref(ctx.detectAudioRecordingSupport());

  ctx.refreshAudioRecordingSupport = () => {
      ctx.audioRecordingSupported.value = ctx.detectAudioRecordingSupport();
  };


  ctx.agentVoiceSupported = computed(() => ctx.audioRecordingSupported.value);

  ctx.resolveVoiceRecordingErrorText = (error: unknown): string => {
      const text = String((error as {
          message?: unknown;
      } | null)?.message || error || '')
          .trim()
          .toLowerCase();
      if (!text) {
          return '';
      }
      if (text.includes('microphone permission denied') ||
          text.includes('permission denied') ||
          text.includes('notallowederror') ||
          text.includes('denied permission')) {
          return ctx.t('messenger.world.voice.permissionDenied');
      }
      if (text.includes('audio recording is not supported') || text.includes('not supported')) {
          return ctx.t('messenger.world.voice.unsupported');
      }
      return '';
  };

  ctx.keyword = computed(() => ctx.sessionHub.keyword);

  ctx.currentUsername = computed(() => {
      const user = ctx.authStore.user as Record<string, unknown> | null;
      return String(user?.username || user?.id || user?.user_id || ctx.t('user.guest'));
  });

  ctx.currentUserId = computed(() => {
      const user = ctx.authStore.user as Record<string, unknown> | null;
      return String(user?.id || user?.user_id || user?.username || '');
  });

  ctx.currentUserContextInitialized = false;

  ctx.buildProfileAvatarOptionLabel = (key: string): string => {
      const match = String(key || '').trim().match(/^qq-avatar-(\d{4})$/);
      if (match) {
          return `QQ Avatar ${match[1]}`;
      }
      return `QQ Avatar ${String(key || '').trim()}`;
  };

  ctx.profileAvatarOptions = computed(() => ctx.settingsPanelMode.value === 'profile'
      ? [
          {
              key: 'initial',
              label: ctx.t('portal.agent.avatar.icon.initial')
          },
          ...PROFILE_AVATAR_IMAGE_KEYS.map((key) => ({
              key,
              label: ctx.buildProfileAvatarOptionLabel(key),
              image: PROFILE_AVATAR_IMAGE_MAP.get(key) || ''
          }))
      ]
      : []);

  ctx.profileAvatarColors = computed(() => [...PROFILE_AVATAR_COLORS]);

  ctx.currentUserAvatarImageUrl = computed(() => PROFILE_AVATAR_IMAGE_MAP.get(String(ctx.currentUserAvatarIcon.value || '').trim()) || '');

  ctx.currentUserAvatarStyle = computed(() => ({
      background: ctx.currentUserAvatarImageUrl.value
          ? 'transparent'
          : String(ctx.currentUserAvatarColor.value || '#3b82f6')
  }));

  ctx.activeSectionTitle = computed(() => {
      return ctx.sessionHub.activeSection === 'more'
          ? ctx.t('messenger.section.settings')
          : ctx.t(`messenger.section.${ctx.sessionHub.activeSection}`);
  });

  ctx.activeSectionSubtitle = computed(() => {
      if (ctx.sessionHub.activeSection === 'messages') {
          return '';
      }
      return ctx.sessionHub.activeSection === 'more'
          ? ctx.t('messenger.section.settings.desc')
          : ctx.t(`messenger.section.${ctx.sessionHub.activeSection}.desc`);
  });

  ctx.currentLanguageLabel = computed(() => getCurrentLanguage() === 'zh-CN' ? ctx.t('language.zh-CN') : ctx.t('language.en-US'));

  ctx.searchableMiddlePaneSections = new Set(['messages', 'users', 'groups', 'swarms', 'orchestrations', 'agents']);

  ctx.isSearchableMiddlePaneSection = (section: string): boolean => ctx.searchableMiddlePaneSections.has(String(section || '').trim());

  ctx.searchPlaceholder = computed(() => ctx.t(`messenger.search.${ctx.sessionHub.activeSection}`));

  ctx.MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT = 1040;

  ctx.MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT = 1040;

  ctx.MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT = 1820;

  ctx.MESSENGER_EMBEDDED_RIGHT_DOCK_OVERLAY_BREAKPOINT = 800;

  ctx.MESSENGER_TIGHT_HOST_BREAKPOINT = 900;

  ctx.isMiddlePaneOverlay = computed(() => ctx.viewportWidth.value <= ctx.MESSENGER_MIDDLE_PANE_OVERLAY_BREAKPOINT);

  ctx.isRightDockOverlay = computed(() => {
      // Embedded chat removes the navigation shell and middle pane, so the dock can
      // stay persistent until the real host width becomes much tighter.
      if (ctx.isEmbeddedChatRoute.value) {
          return ctx.viewportWidth.value <= ctx.MESSENGER_EMBEDDED_RIGHT_DOCK_OVERLAY_BREAKPOINT;
      }
      const inAgentSettingsDetail = ctx.sessionHub.activeSection === 'agents' && ctx.agentOverviewMode.value === 'detail';
      const breakpoint = inAgentSettingsDetail
          ? ctx.MESSENGER_AGENT_SETTINGS_RIGHT_DOCK_BREAKPOINT : ctx.MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT;
      return ctx.viewportWidth.value <= breakpoint;
  });

  ctx.showMiddlePane = computed(() => {
      if (ctx.isEmbeddedChatRoute.value) {
          return false;
      }
      return !ctx.navigationPaneCollapsed.value && (!ctx.isMiddlePaneOverlay.value || ctx.middlePaneOverlayVisible.value);
  });

  ctx.showNavigationCollapseToggle = computed(() => ctx.allowNavigationCollapse.value && (ctx.showMiddlePane.value || ctx.navigationPaneCollapsed.value));

  ctx.middlePaneTransitionName = computed(() => 'messenger-middle-pane-slide');

  ctx.scheduleMiddlePanePrewarm = () => {
      if (ctx.middlePaneMounted.value || ctx.isEmbeddedChatRoute.value || !ctx.isMiddlePaneOverlay.value) {
          return;
      }
      if (typeof window === 'undefined') {
          ctx.middlePaneMounted.value = true;
          return;
      }
      if (ctx.middlePanePrewarmTimer !== null) {
          return;
      }
      ctx.middlePanePrewarmTimer = window.setTimeout(() => {
          ctx.middlePanePrewarmTimer = null;
          if (ctx.isEmbeddedChatRoute.value) {
              return;
          }
          ctx.middlePaneMounted.value = true;
      }, 240);
  };


  ctx.middlePaneSettingsPanelMode = computed(() => ctx.settingsPanelMode.value);

  ctx.isSectionButtonActive = (section: MessengerSection): boolean => ctx.sessionHub.activeSection === section;

  ctx.isLeftRailMoreActive = computed(() => ctx.leftRailMoreExpanded.value ||
      ctx.isLeftNavSectionActive('more'));
}
  installMessengerControllerShellLayoutState(ctx);
}

function installPart2(ctx: any): void {
// User attachments, agent/world renderable message lists, virtualization helpers, and plan state.

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

const METAFILE_IMAGE_CONTENT_TYPES = new Set([
  'image/wmf',
  'image/emf',
  'image/x-wmf',
  'image/x-emf',
  'application/x-msmetafile',
  'application/emf',
  'application/x-emf'
]);

const isImageAttachmentContentType = (contentType: string): boolean => {
  const normalized = String(contentType || '').trim().toLowerCase();
  return normalized.startsWith('image/') || METAFILE_IMAGE_CONTENT_TYPES.has(normalized);
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

type MessageVirtualSpacer = {
  key: string;
  height: number;
};

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerRenderableMessages(ctx: MessengerControllerContext): void {
  ctx.hasMessageContent = (value: unknown): boolean => Boolean(String(value || '').trim());

  ctx.AUDIO_ATTACHMENT_EXTENSIONS = new Set(['mp3', 'wav', 'ogg', 'opus', 'aac', 'flac', 'm4a', 'webm']);

  ctx.resolveAttachmentContentType = (item: Record<string, unknown>): string => {
      const raw = String(item?.content_type ?? item?.mime_type ?? item?.mimeType ?? '')
          .trim()
          .toLowerCase();
      return raw;
  };

  ctx.resolveAttachmentPublicPath = (item: Record<string, unknown>): string => {
      const rawPublic = String(item?.public_path ?? item?.publicPath ?? '').trim();
      if (rawPublic) {
          return parseWorkspaceResourceUrl(rawPublic)?.publicPath || '';
      }
      const rawContent = String(item?.content ?? '').trim();
      if (!rawContent || rawContent.startsWith('data:'))
          return '';
      return parseWorkspaceResourceUrl(rawContent)?.publicPath || '';
  };

  ctx.isAudioPath = (path: string): boolean => {
      const value = String(path || '').trim();
      if (!value)
          return false;
      const suffix = value.split('?')[0].split('#')[0].split('.').pop();
      if (!suffix)
          return false;
      return ctx.AUDIO_ATTACHMENT_EXTENSIONS.has(suffix.toLowerCase());
  };

  ctx.getUserAttachmentResourceState = (publicPath: string): AttachmentResourceState | null => ctx.userAttachmentResourceCache.value.get(publicPath) || null;

  ctx.resolveUserImageAttachments = (message: Record<string, unknown>) => {
      const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
      return attachments
          .map((item, index) => {
          const record = (item || {}) as Record<string, unknown>;
          const content = String(record?.content || '').trim();
          const contentType = ctx.resolveAttachmentContentType(record);
          const publicPath = ctx.resolveAttachmentPublicPath(record);
          const isDataImage = content.startsWith('data:image/');
          const isWorkspaceImage = Boolean(publicPath) &&
              (isImageAttachmentContentType(contentType) || isImagePath(publicPath));
          if (!isDataImage && !isWorkspaceImage)
              return null;
          const fallbackName = `image-${index + 1}`;
          const name = String(record?.name || fallbackName).trim() || fallbackName;
          let src = '';
          if (isDataImage) {
              src = content;
          }
          if (!src && publicPath) {
              const cached = ctx.getUserAttachmentResourceState(publicPath);
              if (cached?.objectUrl) {
                  src = cached.objectUrl;
              }
              else if (cached?.error) {
                  return null;
              }
          }
          if (!src)
              return null;
          return {
              key: `${name}-${index}`,
              src,
              name,
              workspacePath: publicPath || ''
          };
      })
          .filter(Boolean);
  };

  ctx.resolveUserAudioAttachments = (message: Record<string, unknown>) => {
      const attachments = Array.isArray(message?.attachments) ? message.attachments : [];
      return attachments
          .map((item, index) => {
          const record = (item || {}) as Record<string, unknown>;
          const content = String(record?.content || '').trim();
          const contentType = ctx.resolveAttachmentContentType(record);
          const publicPath = ctx.resolveAttachmentPublicPath(record);
          const isDataAudio = content.startsWith('data:audio/');
          const isWorkspaceAudio = Boolean(publicPath) && (contentType.startsWith('audio/') || ctx.isAudioPath(publicPath));
          if (!isDataAudio && !isWorkspaceAudio)
              return null;
          const fallbackName = `audio-${index + 1}`;
          const name = String(record?.name || fallbackName).trim() || fallbackName;
          let src = '';
          if (isDataAudio) {
              src = content;
          }
          if (!src && publicPath) {
              const cached = ctx.getUserAttachmentResourceState(publicPath);
              if (cached?.objectUrl) {
                  src = cached.objectUrl;
              }
              else if (cached?.error) {
                  return null;
              }
          }
          if (!src)
              return null;
          return {
              key: `${name}-${index}`,
              src,
              name,
              workspacePath: publicPath || ''
          };
      })
          .filter(Boolean);
  };

  ctx.collectUserAttachmentWorkspacePaths = (messages: Record<string, unknown>[]): string[] => {
      const paths = new Set<string>();
      messages.forEach((message) => {
          if (String(message?.role || '') !== 'user')
              return;
          const attachments = Array.isArray(message?.attachments)
              ? (message.attachments as unknown[])
              : [];
          attachments.forEach((item) => {
              const record = (item || {}) as Record<string, unknown>;
              const publicPath = ctx.resolveAttachmentPublicPath(record);
              if (!publicPath)
                  return;
              const content = String(record?.content || '').trim();
              if (content.startsWith('data:'))
                  return;
              const contentType = ctx.resolveAttachmentContentType(record);
              const isImage = isImageAttachmentContentType(contentType) || isImagePath(publicPath);
              const isAudio = contentType.startsWith('audio/') || ctx.isAudioPath(publicPath);
              if (isImage || isAudio) {
                  paths.add(publicPath);
              }
          });
      });
      return Array.from(paths);
  };

  ctx.userAttachmentWorkspacePaths = computed(() => {
      const _currentUserId = ctx.currentUserId.value;
      if (!ctx.isAgentConversationActive.value) {
          return [];
      }
      if (ctx.shouldVirtualizeMessages?.value && ctx.agentVirtualWindow?.value?.enabled) {
          const renderable = [
              ...(ctx.visibleAgentRenderableMessages?.value || []),
              ...(ctx.pinnedAgentRenderableMessages?.value || [])
          ];
          return ctx.collectUserAttachmentWorkspacePaths(renderable.map((item) => item.message));
      }
      const renderableMessages = (ctx.agentRenderableMessages?.value || [])
          .map((item) => item.message as Record<string, unknown>);
      return ctx.collectUserAttachmentWorkspacePaths(renderableMessages);
  });

  ctx.hasUserImageAttachments = (message: Record<string, unknown>): boolean => ctx.resolveUserImageAttachments(message).length > 0;

  ctx.hasUserAudioAttachments = (message: Record<string, unknown>): boolean => ctx.resolveUserAudioAttachments(message).length > 0;

  ctx.hasWorkflowOrThinking = (message: Record<string, unknown>): boolean => Boolean(message?.workflowStreaming) ||
      Boolean(message?.reasoningStreaming) ||
      Boolean((message?.workflowItems as unknown[])?.length) ||
      hasActiveSubagentItems(message?.subagents) ||
      Boolean((message?.subagents as unknown[])?.length) ||
      ctx.hasMessageContent(message?.reasoning);

  ctx.isHiddenInternalMessage = (message: Record<string, unknown>): boolean => {
      if (Boolean(message?.hiddenInternal || message?.hidden)) {
          return true;
      }
      const meta = (message?.meta || {}) as Record<string, unknown>;
      const metaType = String(meta?.type || '').trim();
      return Boolean(
          meta?.hidden === true ||
          meta?.internal_user === true ||
          metaType === 'model_context_internal'
      );
  };

  ctx.shouldRenderAgentMessage = (message: Record<string, unknown>): boolean => {
      if (ctx.isHiddenInternalMessage(message)) {
          return false;
      }
      if (String(message?.role || '') === 'user')
          return true;
      return message.__runtime_projected === true || ctx.hasMessageContent(message?.content) || ctx.hasWorkflowOrThinking(message);
  };

  const resolveSyntheticGreetingRenderable = (): AgentRenderableMessage | null => {
      for (let sourceIndex = 0; sourceIndex < ctx.chatStore.messages.length; sourceIndex += 1) {
          const rawMessage = ctx.chatStore.messages[sourceIndex];
      const message = (rawMessage || {}) as Record<string, unknown>;
          if (!ctx.isGreetingMessage(message) || !ctx.shouldRenderAgentMessage(message)) {
              continue;
      }
          return {
          key: ctx.resolveAgentMessageKey(message, sourceIndex),
          sourceIndex,
          message
          };
      }
      return null;
  };

  const wrapSlotMessage = (message: Record<string, unknown>): AgentRenderableMessage => ({
      key: resolveChatRuntimeMessageRenderKey(message), sourceIndex: 0, message
  });

  // The page source is a list of fixed user turns, never a list of messages.
  ctx.agentTurnSlots = computed(() => {
      const version = ctx.chatStore.runtimeProjectionVersionBySession?.[ctx.chatStore.activeSessionId] || 0;
      void version;
      return (buildChatThreadMaterializedSlots(ctx.chatStore.activeSessionId) ?? []).map(slot => ({
          key: slot.key, rootTurnId: slot.rootTurnId, kind: 'turn',
          user: wrapSlotMessage(slot.user), assistant: wrapSlotMessage(slot.assistant)
      }));
  });
  ctx.agentConversationRows = computed(() => {
      const greeting = resolveSyntheticGreetingRenderable();
      return greeting
          ? [{ key: 'conversation-greeting', kind: 'greeting', assistant: greeting }, ...ctx.agentTurnSlots.value]
          : ctx.agentTurnSlots.value;
  });
  // Compatibility view for composer, attachments and diagnostics only.
  ctx.agentRenderableMessages = computed<AgentRenderableMessage[]>(() =>
      ctx.agentConversationRows.value.flatMap(row => row.kind === 'greeting'
          ? [row.assistant] : [row.user, row.assistant]));

  ctx.resolveActiveAgentRenderableMessageRecords = (): Record<string, unknown>[] => {
      const renderable = ctx.agentRenderableMessages?.value;
      if (Array.isArray(renderable)) {
          return renderable
              .map((item) => (item?.message || {}) as Record<string, unknown>)
              .filter((item) => item && typeof item === 'object' && !Array.isArray(item));
      }
      return [];
  };

  ctx.agentRenderableContextMessages = computed<Record<string, unknown>[]>(() => {
      const records = ctx.resolveActiveAgentRenderableMessageRecords();
      if (!records.length) {
          return records;
      }
      // Composer only needs recent assistant stats for context display; do not duplicate full history.
      const limit = 96;
      return records.length > limit ? records.slice(-limit) : records;
  });

  ctx.buildWorkflowSurfaceDebugSnapshot = () => {
      const renderable = ctx.agentRenderableMessages.value;
      const tailAssistant = renderable.length > 0 ? renderable[renderable.length - 1].message : null;
      const workflowItems = Array.isArray(tailAssistant?.workflowItems)
          ? (tailAssistant.workflowItems as unknown[])
          : [];
      return {
          activeSessionId: ctx.chatStore.activeSessionId,
          renderableCount: renderable.length,
          tailRole: String(tailAssistant?.role || ''),
          tailHasWorkflowOrThinking: tailAssistant ? ctx.hasWorkflowOrThinking(tailAssistant) : false,
          tailWorkflowVisible: Boolean(tailAssistant?.workflowStreaming || workflowItems.length > 0),
          tailWorkflowItemCount: workflowItems.length,
          tailWorkflowStreaming: Boolean(tailAssistant?.workflowStreaming),
          tailReasoningStreaming: Boolean(tailAssistant?.reasoningStreaming),
          tailStreamIncomplete: Boolean(tailAssistant?.stream_incomplete),
          tailContentLength: String(tailAssistant?.content || '').length,
          tailReasoningLength: String(tailAssistant?.reasoning || '').length
      };
  };

  watch(() => {
      if (!isChatDebugEnabled())
          return 'disabled';
      const snapshot = ctx.buildWorkflowSurfaceDebugSnapshot();
      return [
          snapshot.activeSessionId,
          snapshot.renderableCount,
          snapshot.tailRole,
          snapshot.tailHasWorkflowOrThinking,
          snapshot.tailWorkflowVisible,
          snapshot.tailWorkflowItemCount,
          snapshot.tailWorkflowStreaming,
          snapshot.tailReasoningStreaming,
          snapshot.tailStreamIncomplete,
          snapshot.tailContentLength,
          snapshot.tailReasoningLength
      ].join('::');
  }, () => {
      if (!isChatDebugEnabled())
          return;
      chatDebugLog('messenger.workflow-surface', 'snapshot-change', ctx.buildWorkflowSurfaceDebugSnapshot());
  }, { immediate: true });

  // Direct/group ("world") conversations were removed from the product: the
  // shell only ever renders agent threads, so this list is structurally empty.
  // It stays declared because the message viewport runtime still takes it as an
  // input; see the follow-up note about retiring the world conversation kind.
  ctx.worldRenderableMessages = computed<WorldRenderableMessage[]>(() => []);

  ctx.latestRenderableAssistantMessage = computed<Record<string, unknown> | null>(() => {
      for (let index = ctx.agentRenderableMessages.value.length - 1; index >= 0; index -= 1) {
          const message = ctx.agentRenderableMessages.value[index]?.message as Record<string, unknown> | undefined;
          if (String(message?.role || '') === 'assistant') {
              return message || null;
          }
      }
      return null;
  });

  ctx.latestAgentRenderableMessageKey = computed(() => {
      const latest = ctx.agentRenderableMessages.value[ctx.agentRenderableMessages.value.length - 1];
      return String(latest?.key || '').trim();
  });

  ctx.buildLatestAssistantLayoutSignature = (message: Record<string, unknown> | undefined): string => {
      if (!message || String(message.role || '') !== 'assistant') {
          return 'non-assistant';
      }
      const workflowItems = Array.isArray(message.workflowItems)
          ? (message.workflowItems as unknown[])
          : [];
      const lastWorkflowItem = workflowItems[workflowItems.length - 1] as Record<string, unknown> | undefined;
      const workflowSignature = lastWorkflowItem
          ? [
              workflowItems.length - 1,
              String(lastWorkflowItem.id || lastWorkflowItem.toolCallId || lastWorkflowItem.eventType || '').trim(),
              String(lastWorkflowItem.status || '').trim(),
              String(lastWorkflowItem.title || lastWorkflowItem.toolName || '').length,
              String(lastWorkflowItem.detail || '').length
          ].join(':')
          : '';
      const subagents = Array.isArray(message.subagents) ? message.subagents : [];
      const lastSubagent = subagents[subagents.length - 1] as Record<string, unknown> | undefined;
      const subagentSignature = lastSubagent
          ? [
              subagents.length - 1,
              String(lastSubagent.key || lastSubagent.run_id || lastSubagent.session_id || '').trim(),
              String(lastSubagent.status || '').trim(),
              String(lastSubagent.summary || '').length
          ].join(':')
          : '';
      return [
          ctx.latestAgentRenderableMessageKey.value,
          String(message.id || message.localId || '').trim(),
          String(message.reasoning || '').length,
          Boolean(message.workflowStreaming),
          Boolean(message.reasoningStreaming),
          Boolean(message.stream_incomplete),
          workflowItems.length,
          workflowSignature,
          subagents.length,
          subagentSignature
      ].join('::');
  };

  ctx.MESSAGE_VIRTUAL_OVERSCAN = 4;

  ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT = 4;

  ctx.retainedMessageRenderKind = computed<MessageConversationKind>(() => {
      const activeKind = String(ctx.resolvedMessageConversationKind?.value || '') as MessageConversationKind;
      if (activeKind === 'agent' || activeKind === 'world') {
          return activeKind;
      }
      if (ctx.isAgentConversationActive?.value) {
          return 'agent';
      }
      return '';
  });

  ctx.shouldVirtualizeMessages = computed(() => {
      const hasExpensiveRenderableMessage = (items: Array<{ message?: Record<string, unknown> }>): boolean =>
          items.some((item) => {
              const message = item?.message || {};
              const content = String(message.content || '');
              const reasoning = String(message.reasoning || '');
              return content.length > 12000 ||
                  reasoning.length > 4000 ||
                  (Array.isArray(message.workflowItems) && message.workflowItems.length > 0) ||
                  (Array.isArray(message.subagents) && message.subagents.length > 0) ||
                  (Array.isArray(message.attachments) && message.attachments.length > 0);
          });
      if (ctx.isAgentConversationActive.value) {
          const items = ctx.agentRenderableMessages.value;
          return items.length > 24 || (items.length > 12 && hasExpensiveRenderableMessage(items));
      }
      if (ctx.retainedMessageRenderKind?.value === 'world' || ctx.isWorldConversationActive.value) {
          const items = ctx.worldRenderableMessages.value;
          return items.length > 24 || (items.length > 12 && hasExpensiveRenderableMessage(items));
      }
      return false;
  });

  ctx.resolveVirtualMessageHeight = (key: string): number => {
      const normalized = String(key || '').trim();
      if (!normalized) {
          return ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
      }
      return ctx.messageVirtualHeightCache.get(normalized) || ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT * (normalized.startsWith('turn:') ? 2 : 1);
  };

  ctx.estimateVirtualOffsetTop = (keys: string[], index: number): number => resolveVirtualOffsetTop(
      Array.isArray(keys) ? keys : [],
      index,
      ctx.resolveVirtualMessageHeight
  );

  ctx.agentVirtualWindow = computed(() => buildMessageVirtualWindow({
      items: ctx.agentConversationRows.value,
      enabled: ctx.shouldVirtualizeMessages.value && ctx.isAgentConversationActive.value,
      scrollTop: ctx.messageVirtualScrollTop.value,
      viewportHeight: ctx.messageVirtualViewportHeight.value,
      overscan: ctx.MESSAGE_VIRTUAL_OVERSCAN,
      tailPinCount: ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT,
      estimatedHeight: ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT * 2,
      resolveHeight: ctx.resolveVirtualMessageHeight,
      layoutVersion: ctx.messageVirtualLayoutVersion.value
  }));

  ctx.agentVirtualTopSpacer = computed<MessageVirtualSpacer | null>(() => ctx.agentVirtualWindow.value.enabled &&
      ctx.agentVirtualWindow.value.topPadding > 0
      ? {
          key: 'agent-virtual-top-spacer',
          height: ctx.agentVirtualWindow.value.topPadding
      }
      : null);

  ctx.agentVirtualBottomSpacer = computed<MessageVirtualSpacer | null>(() => ctx.agentVirtualWindow.value.enabled &&
      ctx.agentVirtualWindow.value.bottomPadding > 0
      ? {
          key: 'agent-virtual-bottom-spacer',
          height: ctx.agentVirtualWindow.value.bottomPadding
      }
      : null);

  const flattenConversationRows = (rows) => rows.flatMap(row => row.kind === 'greeting'
      ? [row.assistant] : [row.user, row.assistant]);
  ctx.visibleAgentRenderableMessages = computed(() => flattenConversationRows(ctx.agentVirtualWindow.value.visibleItems));
  ctx.pinnedAgentRenderableMessages = computed(() => flattenConversationRows(ctx.agentVirtualWindow.value.tailItems));
  ctx.agentVirtualRows = computed(() => {
      const window = ctx.agentVirtualWindow.value;
      const gap = ctx.agentVirtualBottomSpacer.value;
      return [...window.visibleItems, ...(gap ? [{ ...gap, kind: 'spacer' }] : []), ...window.tailItems];
  });

  ctx.buildMessageVirtualDebugSnapshot = () => {
      const agentWindow = ctx.agentVirtualWindow.value;
      const worldWindow = ctx.worldVirtualWindow?.value;
      return {
          activeSection: ctx.sessionHub.activeSection,
          activeConversationKey: ctx.sessionHub.activeConversationKey,
          conversationKind: ctx.resolvedMessageConversationKind?.value || '',
          activeSessionId: ctx.chatStore.activeSessionId,
          shouldVirtualize: Boolean(ctx.shouldVirtualizeMessages.value),
          scrollTop: ctx.messageVirtualScrollTop.value,
          viewportHeight: ctx.messageVirtualViewportHeight.value,
          agent: {
              total: ctx.agentRenderableMessages.value.length,
              visible: ctx.visibleAgentRenderableMessages.value.length,
              pinned: ctx.pinnedAgentRenderableMessages.value.length,
              startIndex: agentWindow?.startIndex ?? 0,
              endIndex: agentWindow?.endIndex ?? 0,
              tailStartIndex: agentWindow?.tailStartIndex ?? 0,
              topPadding: agentWindow?.topPadding ?? 0,
              bottomPadding: agentWindow?.bottomPadding ?? 0
          },
          world: {
              total: ctx.worldRenderableMessages.value.length,
              visible: ctx.visibleWorldRenderableMessages?.value?.length ?? 0,
              pinned: ctx.pinnedWorldRenderableMessages?.value?.length ?? 0,
              startIndex: worldWindow?.startIndex ?? 0,
              endIndex: worldWindow?.endIndex ?? 0,
              tailStartIndex: worldWindow?.tailStartIndex ?? 0,
              topPadding: worldWindow?.topPadding ?? 0,
              bottomPadding: worldWindow?.bottomPadding ?? 0
          }
      };
  };

  ctx.worldVirtualWindow = computed(() => buildMessageVirtualWindow({
      items: ctx.worldRenderableMessages.value,
      enabled: ctx.shouldVirtualizeMessages.value &&
          (ctx.retainedMessageRenderKind?.value === 'world' || ctx.isWorldConversationActive.value),
      scrollTop: ctx.messageVirtualScrollTop.value,
      viewportHeight: ctx.messageVirtualViewportHeight.value,
      overscan: ctx.MESSAGE_VIRTUAL_OVERSCAN,
      tailPinCount: ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT,
      estimatedHeight: ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT,
      resolveHeight: ctx.resolveVirtualMessageHeight,
      layoutVersion: ctx.messageVirtualLayoutVersion.value
  }));

  ctx.worldVirtualTopSpacer = computed<MessageVirtualSpacer | null>(() => ctx.worldVirtualWindow.value.enabled &&
      ctx.worldVirtualWindow.value.topPadding > 0
      ? {
          key: 'world-virtual-top-spacer',
          height: ctx.worldVirtualWindow.value.topPadding
      }
      : null);

  ctx.worldVirtualBottomSpacer = computed<MessageVirtualSpacer | null>(() => ctx.worldVirtualWindow.value.enabled &&
      ctx.worldVirtualWindow.value.bottomPadding > 0
      ? {
          key: 'world-virtual-bottom-spacer',
          height: ctx.worldVirtualWindow.value.bottomPadding
      }
      : null);

  ctx.visibleWorldRenderableMessages = computed<WorldRenderableMessage[]>(() => ctx.worldVirtualWindow.value.enabled
      ? ctx.worldVirtualWindow.value.visibleItems
      : ctx.worldRenderableMessages.value);

  ctx.pinnedWorldRenderableMessages = computed<WorldRenderableMessage[]>(() => ctx.worldVirtualWindow.value.enabled
      ? ctx.worldVirtualWindow.value.tailItems
      : []);

  ctx.worldVirtualGroups = computed<WorldRenderableMessage[][]>(() => ctx.worldVirtualWindow.value.enabled
      ? [ctx.visibleWorldRenderableMessages.value, ctx.pinnedWorldRenderableMessages.value]
      : [ctx.visibleWorldRenderableMessages.value]);

  ctx.isGreetingMessage = (message: Record<string, unknown>): boolean => String(message?.role || '') === 'assistant' && Boolean(message?.isGreeting);

  ctx.isVisibleAgentAssistantMessage = (message: Record<string, unknown>): boolean => String(message?.role || '') === 'assistant' &&
      !ctx.isHiddenInternalMessage(message);

  ctx.latestVisibleAgentAssistantMessage = computed<Record<string, unknown> | null>(() => {
      for (let index = ctx.agentRenderableMessages.value.length - 1; index >= 0; index -= 1) {
          const message = (ctx.agentRenderableMessages.value[index]?.message || {}) as Record<string, unknown>;
          if (ctx.isVisibleAgentAssistantMessage(message)) {
              return message;
          }
      }
      return null;
  });

  ctx.resolveMessageAgentAvatarState = (message: Record<string, unknown>): AgentRuntimeState => {
      if (String(message?.role || '') !== 'assistant')
          return 'idle';
      if (resolveAssistantFailureNotice(message, ctx.t))
          return 'error';
      // The durable projection owns this turn's lifecycle. Session-level
      // activity belongs to a newer turn and cannot reopen this bubble.
      if (message.__runtime_projected === true)
          return resolveAssistantMessageRuntimeState(message) as AgentRuntimeState;
      if (hasActiveSubagentItems(message.subagents))
          return 'running';
      if (hasAssistantWaitingForCurrentOutput(message))
          return 'running';
      const runtimeState = resolveAssistantMessageRuntimeState(message) as AgentRuntimeState;
      return runtimeState;
  };

  ctx.shouldShowAgentMessageBubble = (message: Record<string, unknown>): boolean => ctx.hasMessageContent(buildAssistantDisplayContent(message, ctx.t));

  ctx.shouldMountAgentMessageBubble = (message: Record<string, unknown>): boolean => ctx.shouldShowAgentMessageBubble(message);

  const resolveRuntimeAssistantProjection = (message: Record<string, unknown>) => {
      if (String(message?.role || '') !== 'assistant') {
          return null;
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId) {
          return null;
      }
      return resolveRuntimeMessageContentSource({
          projection: ctx.chatStore.runtimeProjection,
          sessionId,
          runtimeMessageId: message.__runtime_message_id || message.message_id || message.messageId,
          runtimeUserTurnId: message.__runtime_user_turn_id || message.user_turn_id || message.userTurnId,
          runtimeModelTurnId: message.__runtime_model_turn_id || message.model_turn_id || message.modelTurnId,
          message
      });
  };

  ctx.resolveAgentWorkflowSubagents = (message: Record<string, unknown>): unknown[] => {
      const projected = resolveRuntimeAssistantProjection(message);
      const projectedItems = Array.isArray(projected?.subagents) ? projected.subagents : [];
      if (projectedItems.length > 0) {
          return projectedItems;
      }
      return Array.isArray(message?.subagents) ? message.subagents : [];
  };

  ctx.shouldMountAgentWorkflow = (message: Record<string, unknown>): boolean => {
      if (String(message?.role || '') !== 'assistant') {
          return false;
      }
      const projected = resolveRuntimeAssistantProjection(message);
      const hasWorkflow = Boolean(message?.stream_incomplete) ||
          Boolean(message?.workflowStreaming) ||
          (Array.isArray(message?.workflowItems) && message.workflowItems.length > 0) ||
          (Array.isArray(message?.subagents) && message.subagents.length > 0) ||
          Boolean(projected?.workflowItems?.length) ||
          Boolean(projected?.subagents?.length);
      // Message virtualization bounds mounted shells; each shell lazily mounts details.
      return hasWorkflow;
  };

  ctx.hasPlanSteps = (plan: unknown): boolean => Array.isArray((plan as {
      steps?: unknown[];
  } | null)?.steps) &&
      ((plan as {
          steps?: unknown[];
      } | null)?.steps?.length || 0) > 0;

  ctx.isPlanMessageDismissed = (message: Record<string, unknown>): boolean => ctx.dismissedPlanMessages.value.has(message);

  ctx.markPlanMessageDismissed = (message: Record<string, unknown>) => {
      ctx.dismissedPlanMessages.value.add(message);
      ctx.dismissedPlanVersion.value += 1;
  };

  ctx.activeAgentPlanMessage = computed<Record<string, unknown> | null>(() => {
      // Trigger recompute when manual dismiss state changes.
      void ctx.dismissedPlanVersion.value;
      if (!ctx.isAgentConversationActive.value)
          return null;
      for (let index = ctx.agentRenderableMessages.value.length - 1; index >= 0; index -= 1) {
          const message = ctx.agentRenderableMessages.value[index]?.message as Record<string, unknown> | undefined;
          if (String(message?.role || '') !== 'assistant')
              continue;
          if (!ctx.hasPlanSteps(message?.plan))
              continue;
          if (message && ctx.isPlanMessageDismissed(message)) {
              return null;
          }
          return message || null;
      }
      return null;
  });

  ctx.activeAgentPlan = computed(() => {
      const message = ctx.activeAgentPlanMessage.value as {
          plan?: unknown;
      } | null;
      return message?.plan || null;
  });
}
  installMessengerControllerRenderableMessages(ctx);
}

function installPart3(ctx: any): void {
// Agent identity, active session model display, approval mode, and default profile state.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerAgentIdentityState(ctx: MessengerControllerContext): void {
  ctx.DEFAULT_BEEROOM_GROUP_ID = 'default';

  ctx.ownedAgents = computed(() => (Array.isArray(ctx.agentStore.agents) ? ctx.agentStore.agents : []));

  ctx.normalizeAgentHiveGroupId = (value: unknown): string => {
      const normalized = String(value || '').trim();
      return normalized || ctx.DEFAULT_BEEROOM_GROUP_ID;
  };

  // Agent grouping used to be fed by the removed beeroom (hive) store. With that
  // surface gone every agent belongs to the single built-in group.
  ctx.defaultBeeroomGroupId = computed(() => ctx.DEFAULT_BEEROOM_GROUP_ID);

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
      [...ctx.ownedAgents.value].forEach((agent) => {
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
      [...ctx.ownedAgents.value].forEach((agent) => {
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
      const defaultProfile = ctx.defaultAgentProfile.value as Record<string, unknown> | null;
      map.set(DEFAULT_AGENT_KEY, {
          id: DEFAULT_AGENT_KEY,
          name: String(defaultProfile?.name || ctx.t('messenger.defaultAgent')),
          description: String(defaultProfile?.description || ctx.t('messenger.defaultAgentDesc')),
          icon: defaultProfile?.icon,
          sandbox_container_id: defaultProfile?.sandbox_container_id ?? 1,
          approval_mode: defaultProfile?.approval_mode ?? ctx.defaultAgentApprovalMode.value,
          silent: Boolean(defaultProfile?.silent),
          prefer_mother: Boolean(defaultProfile?.prefer_mother)
      });
      ctx.ownedAgents.value.forEach((item) => {
          const id = ctx.normalizeAgentId(item?.id);
          map.set(id, item as Record<string, unknown>);
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
      return resolveMessageConversationKind({
          foregroundLock: ctx.agentSendForegroundLock.value,
          activeConversationKind: ctx.activeConversation.value?.kind,
          activeConversationId: ctx.activeConversation.value?.id,
          routeConversationId: ctx.route.query?.conversation_id,
          routeSessionId: ctx.route.query?.session_id,
          routeAgentId: ctx.route.query?.agent_id,
          routeEntry: ctx.route.query?.entry,
          activeSessionId: ctx.chatStore.activeSessionId,
          draftAgentId: ctx.chatStore.draftAgentId,
          messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length
      });
  });

  ctx.isAgentConversationActive = computed(() => {
      if (ctx.resolvedMessageConversationKind.value === 'agent') {
          return true;
      }
      return hasRetainedAgentConversationContext({
          foregroundLock: ctx.agentSendForegroundLock.value,
          activeConversationKind: ctx.activeConversation.value?.kind,
          activeConversationId: ctx.activeConversation.value?.id,
          routeConversationId: ctx.route.query?.conversation_id,
          routeSessionId: ctx.route.query?.session_id,
          routeAgentId: ctx.route.query?.agent_id,
          routeEntry: ctx.route.query?.entry,
          activeSessionId: ctx.chatStore.activeSessionId,
          draftAgentId: ctx.chatStore.draftAgentId,
          messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length
      });
  });

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
      const messages = ctx.resolveActiveAgentRenderableMessageRecords();
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
      if (false) {
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
      if (false && ctx.desktopLocalMode.value) {
          return ctx.t('desktop.system.modelUnnamed');
      }
      return ctx.t('common.unknown');
  });

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

  // §8.4: the three approval tiers live in the composer; the selector is enabled
  // whenever an agent conversation is active (its value persists to the agent).
  ctx.showAgentComposerApprovalSelector = computed(() => Boolean(ctx.isAgentConversationActive.value));

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

  ctx.activeSessionApproval = computed(() => {
      if (!ctx.isAgentConversationActive.value)
          return null;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId || !Array.isArray(ctx.chatStore.pendingApprovals))
          return null;
      return (ctx.chatStore.pendingApprovals.find((item) => String(item?.session_id || '').trim() === sessionId) || null);
  });

  /**
   * §8.3 model switching for the composer popover.
   *
   * The cloud contract has no per-session model field: the effective model is
   * resolved from the single user agent (`resolve_chat_model_name(config, agent)`),
   * so switching the current thread's model means updating that agent record and
   * refreshing the local profile. The next turn picks it up without a reload.
   *
   * Reasoning effort stays a per-thread setting and is written by the composer
   * itself through `POST /chat/sessions/{id}/reasoning-effort`.
   *
   * `user_default_model_name` (A3) is not delivered yet and has no setter
   * endpoint, so "设为我的默认" reports the gap instead of faking a write.
   */
  ctx.applyComposerModelSelection = async (payload: {
      modelId?: string;
      reasoningEffort?: string;
      setAsDefault?: boolean;
  } = {}) => {
      const modelId = String(payload?.modelId || '').trim();
      if (!modelId) {
          return { ok: false, message: ctx.t('chat.composer.modelUnavailable') };
      }
      const targetAgentId = ctx.resolveComposerApprovalPersistAgentId();
      if (!targetAgentId) {
          return { ok: false, message: ctx.t('chat.features.agentMissing') };
      }
      try {
          const updated = (await ctx.agentStore.updateAgent(targetAgentId, {
              model_name: modelId
          })) as Record<string, unknown> | null;
          const refreshed = updated || (await ctx.agentStore.getAgent(targetAgentId, { force: true }).catch(() => null));
          if (targetAgentId === DEFAULT_AGENT_KEY) {
              await ctx.loadDefaultAgentProfile().catch(() => null);
          }
          else if (targetAgentId === ctx.activeAgentId.value) {
              ctx.activeAgentDetailProfile.value = (refreshed as Record<string, unknown> | null) || updated;
              ctx.activeAgent.value = (refreshed as Record<string, unknown> | null) || updated;
          }
          if (payload?.setAsDefault === true) {
              // Honest degradation: the read field may exist while the write
              // endpoint is still pending on the backend track.
              ElMessage.info(ctx.t('chat.composer.modelSetDefaultUnsupported'));
          }
          return { ok: true, message: ctx.t('chat.composer.modelSwitched', { name: modelId }) };
      }
      catch (error) {
          showApiError(error, ctx.t('chat.composer.modelSwitchFailed'));
          return {
              ok: false,
              message: resolveApiError(error, ctx.t('chat.composer.modelSwitchFailed')).message ||
                  ctx.t('chat.composer.modelSwitchFailed')
          };
      }
  };

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
  installMessengerControllerAgentIdentityState(ctx);
}

function installPart4(ctx: any): void {
// Search-create routing, middle-pane selections, world conversation openers, agent sessions, and prompt previews.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerConversationOpenActions(ctx: MessengerControllerContext): void {
  const openingAgentByIdTasks = new Map<string, Promise<void>>();
  const openingAgentSessionTasks = new Map<string, Promise<void>>();

  ctx.openAgentById = async (agentId: unknown, options: { preserveSection?: boolean } = {}) => {
      const normalized = ctx.normalizeAgentId(agentId);
      // 后台补水（启动恢复）不允许把视图从用户当前 section 拉回聊天区。
      const sessionOptions = options.preserveSection === true ? { preserveSection: true } : {};
      const existingTask = openingAgentByIdTasks.get(normalized);
      if (existingTask) {
          return existingTask;
      }
      const task = (async () => {
          ctx.clearAgentConversationDismissed(normalized);
          ctx.selectedAgentId.value = normalized;
          if (isAgentAlreadyOpen(normalized, {
            activeSessionId: ctx.chatStore.activeSessionId,
            activeConversationKey: ctx.sessionHub.activeConversationKey,
            draftAgentId: ctx.chatStore.draftAgentId,
            sessions: ctx.chatStore.sessions
          })) {
              const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
              if (activeSessionId) {
                  await ctx.openAgentSession(activeSessionId, normalized, sessionOptions);
              }
              else if (options.preserveSection !== true) {
                  await ctx.openAgentDraftSessionWithScroll(normalized);
              }
              return;
          }
          const preferredSessionId = ctx.resolvePreferredAgentSessionId(normalized);
          if (preferredSessionId) {
              await ctx.openAgentSession(preferredSessionId, normalized, sessionOptions);
              return;
          }
          try {
              const freshSessionId = await ctx.openOrReuseFreshAgentSession(normalized);
              if (freshSessionId) {
                  await ctx.openAgentSession(freshSessionId, normalized, sessionOptions);
                  return;
              }
          }
          catch (error) {
              showApiError(error, ctx.t('common.requestFailed'));
          }
          // Keep navigation usable when the backend is temporarily unavailable.
          if (options.preserveSection !== true) {
              await ctx.openAgentDraftSessionWithScroll(normalized);
          }
      })().finally(() => {
          if (openingAgentByIdTasks.get(normalized) === task) {
              openingAgentByIdTasks.delete(normalized);
          }
      });
      openingAgentByIdTasks.set(normalized, task);
      return task;
  };

  ctx.openAgentDraftSession = (agentId: unknown) => {
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
      ctx.rememberRecentAgentSelection?.(normalized, '');
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
          ctx.rightDockSkillContentPath.value = String(
              payload.relative_path || payload.relativePath || payload.path || ctx.rightDockSkillContentPath.value || ''
          ).trim();
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
          const refreshedProfile = updated || (await ctx.agentStore.getAgent(targetAgentId, { force: true }).catch(() => null));
          if (targetAgentId === DEFAULT_AGENT_KEY) {
              ctx.defaultAgentProfile.value = (refreshedProfile as Record<string, unknown> | null) || updated;
          }
          else if (targetAgentId === ctx.activeAgentId.value) {
              ctx.activeAgentDetailProfile.value = (refreshedProfile as Record<string, unknown> | null) || updated;
              ctx.activeAgent.value = (refreshedProfile as Record<string, unknown> | null) || updated;
          }
          ctx.agentStore.agentMap = {
              ...ctx.agentStore.agentMap,
              [targetAgentId]: (refreshedProfile as Record<string, unknown> | null) || updated || null
          };
          await ctx.loadAgentToolSummary({ force: true });
          void ctx.loadRightDockSkills({ force: true, silent: true });
          void ctx.refreshAgentMutationState?.();
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
      // 专家页（agents 分区）的技能工具列表同样依赖 skillTools，不能只在 tools 分区时刷新。
      void ctx.loadToolsCatalog({ silent: true });
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

  ctx.openAgentSession = async (sessionId: string, agentId = '', options: { skipHydration?: boolean; preserveSection?: boolean } = {}) => {
      if (!sessionId)
          return;
      const normalizedSessionId = String(sessionId || '').trim();
      if (!normalizedSessionId)
          return;
      const existingTask = openingAgentSessionTasks.get(normalizedSessionId);
      if (existingTask) {
          return existingTask;
      }
      const task = (async () => {
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const knownSession = ctx.chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
      const fallbackAgentId = agentId
          ? ctx.normalizeAgentId(agentId)
          : ctx.resolveSessionAgentId(knownSession, ctx.chatStore.draftAgentId);
      const perfTrace = ctx.startMessengerPerfTrace('openAgentSession', { sessionId: normalizedSessionId, agentId });

      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneOverlayVisible.value = false;
      ctx.clearAgentConversationDismissed(fallbackAgentId);
      ctx.selectedAgentId.value = fallbackAgentId || DEFAULT_AGENT_KEY;
      ctx.rememberRecentAgentSelection?.(fallbackAgentId || DEFAULT_AGENT_KEY, normalizedSessionId);
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: normalizedSessionId,
          agentId: fallbackAgentId || DEFAULT_AGENT_KEY
      });
      // 后台会话补水（登录后的启动恢复、会话列表回填）不应抢走用户当前的导航：
      // 用户在会话创建期间打开设置页时，只记录会话与 agent，不改写 section。
      const preserveSection = options.preserveSection === true && ctx.sessionHub.activeSection !== 'messages';
      if (!preserveSection) {
          const nextQuery = {
              ...ctx.route.query,
              section: 'messages',
              session_id: normalizedSessionId,
              agent_id: fallbackAgentId === DEFAULT_AGENT_KEY ? '' : fallbackAgentId
          } as Record<string, any>;
          delete nextQuery.conversation_id;
          const nextPath = ctx.resolveChatShellPath();
          if (!ctx.isSameRouteLocation(nextPath, nextQuery)) {
              ctx.router.replace({
                  path: nextPath,
                  query: nextQuery
              }).catch(() => undefined);
          }
      }
      // createSession already installed the empty-thread greeting and watcher.
      // Re-opening it immediately would start a second hydration pipeline
      // (detail + events + workflow + thread snapshot) while the first watcher
      // is being mounted, which can monopolize the old page's render loop.
      if (options.skipHydration === true) {
          ctx.finishMessengerPerfTrace(perfTrace, 'ok', { created: true, hydrationSkipped: true });
          return;
      }
      const isForegroundSession = () => String(ctx.chatStore.activeSessionId || '').trim() === normalizedSessionId;
      try {
          ctx.markMessengerPerfTrace(perfTrace, 'beforeLoadSessionDetail');

          let sessionDetail = null;
          let sessionDetailError: unknown = null;
          const sessionDetailTask = ctx.chatStore.loadSessionDetail(normalizedSessionId, {
              preserveWatcher: true,
              forceHydrateForeground: true
          })
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

          if (!sessionDetail && isSessionUnavailable(ctx.chatStore, normalizedSessionId)) {
              // Purging clears activeSessionId. Use navigation intent to report
              // failure without mistaking deletion for a subsequent user switch.
              if (ctx.sessionHub.activeConversation?.kind === 'agent' &&
                  ctx.sessionHub.activeConversation.id === normalizedSessionId) {
                  ctx.openAgentDraftSession(fallbackAgentId);
                  ElMessage.warning(ctx.t('chat.session.unavailable'));
              }
              ctx.finishMessengerPerfTrace(perfTrace, 'fail', { reason: 'sessionUnavailable' });
              return;
          }
          if (!isForegroundSession()) {
              ctx.finishMessengerPerfTrace(perfTrace, 'ok', { stale: true });
              return;
          }
          if (!sessionDetail) {
              ctx.finishMessengerPerfTrace(perfTrace, 'fail', { reason: 'sessionDetailMissing' });
              ElMessage.warning(ctx.t('messenger.error.openConversation'));
              return;
          }
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === normalizedSessionId);
          const targetAgentId = ctx.normalizeAgentId(session?.agent_id ?? fallbackAgentId);
          ctx.refreshSessionPreviewCache(normalizedSessionId, (session || sessionDetail || null) as Record<string, unknown> | null);
          ctx.selectedAgentId.value = targetAgentId || DEFAULT_AGENT_KEY;
          ctx.rememberRecentAgentSelection?.(targetAgentId || DEFAULT_AGENT_KEY, normalizedSessionId);
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
      })().finally(() => {
          if (openingAgentSessionTasks.get(normalizedSessionId) === task) {
              openingAgentSessionTasks.delete(normalizedSessionId);
          }
      });
      openingAgentSessionTasks.set(normalizedSessionId, task);
      return task;
  };
}
  installMessengerControllerConversationOpenActions(ctx);
}

function installPart5(ctx: any): void {
// Message keys, route sync, section switching, middle-pane delegates, appearance, ordering, and beeroom caches.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerMessageRoutingPreferences(ctx: MessengerControllerContext): void {
  ctx.resolveAgentMessageKey = (message: Record<string, unknown>, index: number): string => {
      return resolveChatRuntimeRenderableKey(message, index);
  };

  ctx.resolveMessageWorkflowStateKey = (
      message: Record<string, unknown>,
      index: number
  ): string => {
      const stableId = String(
          message?.__runtime_message_id ||
          message?.message_id ||
          message?.messageId ||
          message?.id ||
          message?.__runtime_model_turn_id ||
          message?.model_turn_id ||
          message?.modelTurnId ||
          ''
      ).trim();
      return stableId
          ? `workflow-message:${stableId}`
          : ctx.resolveAgentMessageKey(message, index);
  };

  ctx.resolveMessageWorkflowStateAliases = (
      message: Record<string, unknown>,
      index: number,
      renderKey = ''
  ): string[] => {
      const aliases = [
          ctx.resolveAgentMessageKey(message, index),
          String(renderKey || '').trim(),
          String(message?.__runtime_render_key || '').trim()
      ];
      const messageId = String(
          message?.__runtime_message_id ||
          message?.message_id ||
          message?.messageId ||
          message?.id ||
          ''
      ).trim();
      const modelTurnId = String(
          message?.__runtime_model_turn_id ||
          message?.model_turn_id ||
          message?.modelTurnId ||
          ''
      ).trim();
      if (messageId) aliases.push(`workflow-message:${messageId}`);
      if (modelTurnId) aliases.push(`workflow-model-turn:${modelTurnId}`, `workflow-message:${modelTurnId}`);
      const firstWorkflowItem = Array.isArray(message?.workflowItems)
          ? (message.workflowItems[0] || {}) as Record<string, unknown>
          : {};
      const firstWorkflowRef = String(
          firstWorkflowItem?.toolCallId ||
          firstWorkflowItem?.tool_call_id ||
          firstWorkflowItem?.callId ||
          firstWorkflowItem?.call_id ||
          firstWorkflowItem?.commandSessionId ||
          firstWorkflowItem?.command_session_id ||
          firstWorkflowItem?.id ||
          firstWorkflowItem?.itemId ||
          firstWorkflowItem?.item_id ||
          ''
      ).trim();
      if (firstWorkflowRef) aliases.push(`workflow-first-item:${firstWorkflowRef}`);
      return Array.from(new Set(aliases.filter(Boolean)));
  };

  ctx.buildMessageWorkflowRenderVersion = (message: Record<string, unknown>): string => {
      const items = Array.isArray(message?.workflowItems) ? (message.workflowItems as Array<Record<string, unknown>>) : [];
      const tail = items
          .slice(-8)
          .map((item) => [
          String(item?.id || item?.itemId || item?.item_id || ''),
          String(item?.eventType || item?.event || item?.event_type || ''),
          String(item?.toolCallId || item?.tool_call_id || item?.callId || item?.call_id || ''),
          String(item?.status || ''),
          String(item?.title || ''),
          String(item?.context_occupancy_tokens || item?.contextTokens || item?.context_tokens || ''),
          String(item?.updatedSeq || item?.updated_seq || '')
      ].join(':'))
          .join('|');
      return [
          items.length,
          message?.workflowStreaming === true ? 1 : 0,
          message?.reasoningStreaming === true ? 1 : 0,
          message?.stream_incomplete === true ? 1 : 0,
          tail
      ].join('::');
  };

  ctx.sectionRouteSyncToken = 0;



  ctx.normalizeRouteQueryValue = (value: unknown): string[] => {
      if (Array.isArray(value)) {
          return value.map((item) => String(item ?? '').trim());
      }
      if (value === undefined || value === null) {
          return [];
      }
      return [String(value).trim()];
  };

  ctx.buildRouteQuerySignature = (query: Record<string, any>): string => Object.keys(query)
      .sort((left, right) => left.localeCompare(right))
      .map((key) => {
      const values = ctx.normalizeRouteQueryValue(query[key]).join(',');
      return `${key}=${values}`;
  })
      .join('&');

  ctx.isSameRouteLocation = (path: string, query: Record<string, any>): boolean => {
      const currentPath = String(ctx.route.path || '').trim();
      if (currentPath !== path)
          return false;
      const currentQuery = ctx.route.query as Record<string, any>;
      return ctx.buildRouteQuerySignature(currentQuery) === ctx.buildRouteQuerySignature(query);
  };

  ctx.scheduleSectionRouteSync = (path: string, query: Record<string, any>) => {
      const normalizedPath = String(path || '').trim();
      if (!normalizedPath)
          return;
      const normalizedQuery = { ...query } as Record<string, any>;
      const ticket = ++ctx.sectionRouteSyncToken;
      Promise.resolve().then(() => {
          if (ticket !== ctx.sectionRouteSyncToken)
              return;
          if (ctx.isSameRouteLocation(normalizedPath, normalizedQuery))
              return;
          ctx.router.replace({ path: normalizedPath, query: normalizedQuery }).catch(() => undefined);
      });
  };

  ctx.switchSection = (section: MessengerSection, options: {
      preserveHelperWorkspace?: boolean;
      panelHint?: string;
      helperWorkspace?: boolean;
      settingsPanelMode?: string;
  } = {}) => {
      const panelHint = String(options.panelHint || '').trim().toLowerCase();
      const explicitSettingsPanelMode = ctx.normalizeSettingsPanelMode(options.settingsPanelMode);
      ctx.closeFileContainerMenu();
      ctx.sessionHub.setSection(section);
      ctx.sessionHub.setKeyword('');
      ctx.agentPromptPreviewVisible.value = false;
      if (section === 'more') {
          void preloadMessengerSettingsPanels();
          ctx.settingsPanelMode.value =
              explicitSettingsPanelMode !== 'general'
                  ? explicitSettingsPanelMode
                  : panelHint === 'profile'
                              ? 'profile'
                              : panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt'
                                  ? 'prompts'
                                  : panelHint === 'help-manual' ||
                                      panelHint === 'manual' ||
                                      panelHint === 'help' ||
                                      panelHint === 'docs' ||
                                      panelHint === 'docs-site'
                                      ? 'help-manual'
                                      : 'general';
      }
      // Section-scoped selections are cleared unconditionally: the shell only has
      // the messages/agents/files/more sections, so nothing survives a switch.
      ctx.selectedToolCategory.value = '';
      ctx.selectedContactUserId.value = '';
      ctx.selectedContactUnitId.value = '';
      ctx.selectedGroupId.value = '';
      if (section === 'agents') {
          ctx.agentSettingMode.value = 'agent';
      }
      if (section === 'files') {
          if (ctx.fileScope.value === 'user') {
              ctx.selectedFileContainerId.value = USER_CONTAINER_ID;
          }
          else if (!ctx.agentFileContainers.value.some((item) => item.id === ctx.selectedFileContainerId.value)) {
              ctx.selectedFileContainerId.value = ctx.agentFileContainers.value[0]?.id ?? USER_CONTAINER_ID;
          }
      }
      const normalizedCurrentPath = String(ctx.route.path || '').trim();
      const normalizedBasePrefix = String(ctx.basePrefix.value || '').trim();
      // Keep navigation inside current messenger shell route to avoid route-level remount churn.
      const targetPath = normalizedCurrentPath.startsWith(`${normalizedBasePrefix}/`)
          ? normalizedCurrentPath
          : `${ctx.basePrefix.value}/${sectionRouteMap[section]}`;
      const nextQuery = { ...ctx.route.query, section } as Record<string, any>;
      if (panelHint && section === 'more') {
          nextQuery.panel = panelHint;
      }
      else {
          delete nextQuery.panel;
      }
      delete nextQuery.helper;
      if (section !== 'messages') {
          delete nextQuery.session_id;
          delete nextQuery.agent_id;
          delete nextQuery.entry;
      }
      else if (!nextQuery.session_id && !nextQuery.agent_id && !nextQuery.entry) {
          const recent = ctx.resolveRecentAgentSelection?.();
          const activeConversationKey = String(ctx.sessionHub.activeConversationKey || '').trim();
          const fallbackActiveAgentId = activeConversationKey.startsWith('agent:')
              ? ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.chatStore.draftAgentId || '')
              : '';
          const recentAgentRaw = String(recent?.agentId || '').trim();
          const recentAgentId = recentAgentRaw ? ctx.normalizeAgentId(recentAgentRaw) : fallbackActiveAgentId;
          const recentSessionId = String(recent?.sessionId || '').trim();
          const recentSessionKnown = recentSessionId
              ? ctx.chatStore.sessions.some((item) => String(item?.id || item?.session_id || '').trim() === recentSessionId)
              : false;
          if (recentSessionId && recentSessionKnown) {
              nextQuery.session_id = recentSessionId;
              nextQuery.agent_id = recentAgentId === DEFAULT_AGENT_KEY ? '' : recentAgentId;
          }
          else if (recentAgentId) {
              nextQuery.agent_id = recentAgentId === DEFAULT_AGENT_KEY ? '' : recentAgentId;
              nextQuery.entry = recentAgentId === DEFAULT_AGENT_KEY ? 'default' : undefined;
          }
      }
      delete nextQuery.conversation_id;
      ctx.scheduleSectionRouteSync(targetPath, nextQuery);
      ctx.ensureSectionSelection();
  };

  ctx.ensureMiddlePaneSection = (section: MessengerSection, options: {
      panelHint?: string;
      settingsPanelMode?: SettingsPanelMode;
  } = {}) => {
      const nextSettingsPanelMode = ctx.normalizeSettingsPanelMode(options.settingsPanelMode);
      const settingsModeChanged = section === 'more' && ctx.settingsPanelMode.value !== nextSettingsPanelMode;
      if (ctx.sessionHub.activeSection === section && !settingsModeChanged) {
          return;
      }
      ctx.switchSection(section, {
          panelHint: section === 'more'
              ? String(options.panelHint || nextSettingsPanelMode).trim()
              : '',
          settingsPanelMode: section === 'more' ? nextSettingsPanelMode : undefined
      });
  };

  ctx.selectAgentForSettingsFromMiddlePane = (agentId: unknown) => {
      ctx.ensureMiddlePaneSection('agents');
      ctx.selectAgentForSettings(agentId);
  };

  ctx.openMoreRailSection = (section: MessengerSection) => {
      ctx.switchSection(section);
  };

  ctx.activateSettingsPanel = (panelMode: string) => {
      const nextPanelMode = ctx.normalizeSettingsPanelMode(panelMode);
      const panelHint = nextPanelMode === 'profile' ||
          nextPanelMode === 'prompts' ||
          nextPanelMode === 'help-manual' ||
          nextPanelMode === 'desktop-models' ||
          nextPanelMode === 'desktop-lan'
          ? nextPanelMode
          : '';
      // Commit the panel mode only after the settings section is the active one,
      // otherwise the main content stays on the previous section.
      if (ctx.sessionHub.activeSection !== 'more') {
          ctx.switchSection('more', { panelHint, settingsPanelMode: nextPanelMode });
          return;
      }
      ctx.settingsPanelMode.value = nextPanelMode;
  };

  ctx.openMoreRailSection = (section: MessengerSection) => {
      ctx.switchSection(section);
  };

  ctx.openSettingsPage = () => {
      ctx.activateSettingsPanel('general');
  };

  ctx.requestAgentSettingsFocus = (target: '' | 'model') => {
      if (!target)
          return;
      ctx.agentSettingsFocusTarget.value = target;
      ctx.agentSettingsFocusToken.value += 1;
  };

  ctx.handleAgentSettingsFocusConsumed = (target: string) => {
      if (String(target || '').trim() !== ctx.agentSettingsFocusTarget.value)
          return;
      ctx.agentSettingsFocusTarget.value = '';
  };

  ctx.openDesktopModelSettingsFromHeader = () => {
      if (ctx.activeAgentUsingDesktopDefaultModel.value) {
          ctx.activateSettingsPanel('desktop-models');
          return;
      }
      ctx.openActiveAgentSettings({ focusSection: 'model' });
  };

  ctx.handleSettingsLogout = () => {
      if (ctx.settingsLogoutDisabled.value) {
          return;
      }
      ctx.stopRealtimePulse?.();
      ctx.stopBeeroomRealtimeSync?.();
      ctx.authStore.logout();
      redirectToLoginAfterLogout((to) => ctx.router.replace(to));
  };

  ctx.applyCurrentUserAppearance = (appearance: UserAppearancePreferences) => {
      ctx.appearanceHydrating.value = true;
      ctx.themeStore.setPalette(normalizeThemePalette(appearance.themePalette));
      ctx.currentUserAvatarIcon.value = normalizeAvatarIcon(appearance.avatarIcon, PROFILE_AVATAR_OPTION_KEYS);
      ctx.currentUserAvatarColor.value = normalizeAvatarColor(appearance.avatarColor);
      ctx.appearanceHydrating.value = false;
  };

  ctx.resolveCurrentUserAppearance = (): UserAppearancePreferences => ({
      themePalette: normalizeThemePalette(ctx.themeStore.palette),
      avatarIcon: normalizeAvatarIcon(ctx.currentUserAvatarIcon.value, PROFILE_AVATAR_OPTION_KEYS),
      avatarColor: normalizeAvatarColor(ctx.currentUserAvatarColor.value),
      updatedAt: 0
  });

  ctx.hydrateCurrentUserAppearance = async () => {
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      if (!scopedUserId) {
          ctx.applyCurrentUserAppearance({
              ...ctx.resolveCurrentUserAppearance(),
              avatarIcon: 'initial',
              avatarColor: '#3b82f6'
          });
          return;
      }
      ctx.appearanceHydrating.value = true;
      try {
          const appearance = await loadUserAppearance(scopedUserId, PROFILE_AVATAR_OPTION_KEYS);
          if (String(ctx.currentUserId.value || '').trim() !== scopedUserId)
              return;
          ctx.applyCurrentUserAppearance(appearance);
      }
      finally {
          ctx.appearanceHydrating.value = false;
      }
  };

  ctx.persistCurrentUserAppearance = async () => {
      if (ctx.appearanceHydrating.value)
          return;
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      if (!scopedUserId)
          return;
      const appearance = ctx.resolveCurrentUserAppearance();
      const persisted = await saveUserAppearance(scopedUserId, appearance, PROFILE_AVATAR_OPTION_KEYS);
      if (String(ctx.currentUserId.value || '').trim() !== scopedUserId)
          return;
      ctx.applyCurrentUserAppearance(persisted);
  };

  ctx.applyMessengerOrderPreferences = (value: MessengerOrderPreferences) => {
      ctx.messengerOrderHydrating.value = true;
      ctx.orderedOwnedAgentsState.orderedKeys.value = value.agentsOwned.slice();
      ctx.messengerOrderSnapshot.value = {
          messages: value.messages.slice(),
          agentsOwned: value.agentsOwned.slice(),
          agentsShared: value.agentsShared.slice(),
          swarms: value.swarms.slice(),
          updatedAt: value.updatedAt
      };
      ctx.messengerOrderHydrating.value = false;
      chatDebugLog('messenger.order', 'apply', {
          traceId: String(ctx.messengerSessionRefreshTraceId.value || '').trim(),
          traceSource: String(ctx.messengerSessionRefreshTraceSource.value || '').trim(),
          messages: value.messages.slice(),
          agentsOwned: value.agentsOwned.slice(),
          agentsShared: value.agentsShared.slice(),
          swarms: value.swarms.slice(),
          updatedAt: value.updatedAt
      });
  };

  ctx.hasMessengerOrderEntries = (value: MessengerOrderPreferences): boolean => value.messages.length > 0 ||
      value.agentsOwned.length > 0 ||
      value.agentsShared.length > 0 ||
      value.swarms.length > 0;

  ctx.captureMessengerOrderPreferences = (): MessengerOrderPreferences => ({
      messages: [],
      agentsOwned: ctx.orderedOwnedAgentsState.orderedKeys.value.slice(),
      // 共享智能体入口已下线：不再持久化该列表，历史值由归一化逻辑丢弃。
      agentsShared: [],
      swarms: [],
      updatedAt: 0
  });

  ctx.normalizeStringListUnique = (values: unknown[]): string[] => {
      const output: string[] = [];
      const seen = new Set<string>();
      values.forEach((value) => {
          const normalized = String(value || '').trim();
          if (!normalized || seen.has(normalized)) {
              return;
          }
          seen.add(normalized);
          output.push(normalized);
      });
      return output;
  };

  ctx.rememberBeeroomDispatchSessionIds = (groupId: unknown, values: unknown[]) => {
      const normalizedGroupId = String(groupId || '').trim();
      if (!normalizedGroupId) {
          return;
      }
      const nextIds = ctx.normalizeStringListUnique(values);
      if (!nextIds.length) {
          return;
      }
      const currentIds = Array.isArray(ctx.beeroomDispatchSessionIdsByGroup.value[normalizedGroupId])
          ? ctx.beeroomDispatchSessionIdsByGroup.value[normalizedGroupId]
          : [];
      ctx.beeroomDispatchSessionIdsByGroup.value = {
          ...ctx.beeroomDispatchSessionIdsByGroup.value,
          [normalizedGroupId]: ctx.normalizeStringListUnique([...currentIds, ...nextIds])
      };
  };



  ctx.moveOwnedAgentsToFront = (agentIds: unknown[]) => {
      const normalizedIds = ctx.normalizeStringListUnique((Array.isArray(agentIds) ? agentIds : []).map((agentId) => ctx.normalizeAgentId(agentId))).filter((agentId) => agentId && agentId !== DEFAULT_AGENT_KEY);
      if (!normalizedIds.length) {
          return;
      }
      const current = ctx.normalizeStringListUnique(ctx.orderedOwnedAgentsState.orderedKeys.value);
      const pinned = normalizedIds.filter((agentId) => current.includes(agentId));
      if (!pinned.length) {
          return;
      }
      const nextOrder = [DEFAULT_AGENT_KEY, ...pinned, ...current.filter((agentId) => agentId !== DEFAULT_AGENT_KEY && !pinned.includes(agentId))];
      ctx.orderedOwnedAgentsState.orderedKeys.value = ctx.normalizeStringListUnique(nextOrder);
  };
}
  installMessengerControllerMessageRoutingPreferences(ctx);
}

function installPart6(ctx: any): void {
// Plan and inquiry panels, avatar labels, timestamps, presence labels, and admin checks.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerMessagePanelsPresentation(ctx: MessengerControllerContext): void {
  ctx.dismissActiveAgentPlan = () => {
      const target = ctx.activeAgentPlanMessage.value;
      if (!target)
          return;
      ctx.markPlanMessageDismissed(target);
  };

  ctx.activeAgentInquiryPanel = computed<ActiveAgentInquiryPanel | null>(() => {
      if (!ctx.isAgentConversationActive.value)
          return null;
      for (let index = ctx.agentRenderableMessages.value.length - 1; index >= 0; index -= 1) {
          const message = ctx.agentRenderableMessages.value[index]?.message as Record<string, unknown> | undefined;
          if (String(message?.role || '') !== 'assistant')
              continue;
          const panel = (message?.questionPanel || null) as AgentInquiryPanelData | null;
          if (panel?.status === 'pending') {
              return {
                  message: message || {},
                  panel
              };
          }
      }
      return null;
  });

  ctx.handleAgentInquirySelection = (selected: unknown) => {
      if (!Array.isArray(selected)) {
          ctx.agentInquirySelection.value = [];
          return;
      }
      ctx.agentInquirySelection.value = selected
          .map((item) => {
              const record = (item || {}) as Record<string, unknown>;
              const labels = Array.isArray(record.labels)
                  ? record.labels.map((label) => String(label || '').trim()).filter(Boolean)
                  : [];
              return {
                  questionIndex: Number.isInteger(Number(record.questionIndex))
                      ? Number(record.questionIndex)
                      : -1,
                  labels,
                  other: String(record.other || '').trim(),
                  noPreference: record.noPreference === true
              };
          })
          .filter(
              (item) =>
                  item.labels.length > 0 || Boolean(item.other) || item.noPreference
          );
  };

  ctx.buildAgentInquiryReply = (panel: AgentInquiryPanelData, answers: AgentInquiryPanelAnswer[]): string => {
      const questions = Array.isArray(panel?.questions) ? panel.questions : [];
      if (!questions.length) {
          return '';
      }
      const join = ctx.t('chat.inquiry.answerJoin');
      const lines: string[] = [ctx.t('chat.askPanelPrefix')];
      questions.forEach((question, index) => {
          lines.push(questions.length > 1
              ? ctx.t('chat.askPanelQuestionIndexed', {
                  index: index + 1,
                  total: questions.length,
                  question: question.question
              })
              : ctx.t('chat.askPanelQuestion', { question: question.question }));
          const answer = answers.find((item) => item.questionIndex === index);
          const parts: string[] = [];
          if (answer?.labels.length) {
              parts.push(answer.labels.join(join));
          }
          if (answer?.other) {
              parts.push(answer.other);
          }
          const answerText = parts.length
              ? parts.join('；')
              : answer?.noPreference
                  ? ctx.t('chat.inquiry.noPreference')
                  : ctx.t('chat.inquiry.unanswered');
          lines.push(ctx.t('chat.askPanelAnswer', { answer: answerText }));
      });
      return lines.filter(Boolean).join('\n');
  };

  ctx.avatarLabel = (value: unknown): string => {
      const source = String(value || '').trim();
      if (!source)
          return '?';
      return source.slice(0, 1).toUpperCase();
  };

  ctx.resolveUnread = (value: unknown): number => {
      const parsed = Number.parseInt(String(value || ''), 10);
      if (!Number.isFinite(parsed))
          return 0;
      return Math.max(0, parsed);
  };

  ctx.normalizeTimestamp = (value: unknown): number => {
      if (value === null || value === undefined)
          return 0;
      if (value instanceof Date) {
          return Number.isNaN(value.getTime()) ? 0 : value.getTime();
      }
      if (typeof value === 'number') {
          if (!Number.isFinite(value))
              return 0;
          return value < 1000000000000 ? value * 1000 : value;
      }
      const text = String(value).trim();
      if (!text)
          return 0;
      if (/^-?\d+(\.\d+)?$/.test(text)) {
          const numeric = Number(text);
          if (!Number.isFinite(numeric))
              return 0;
          return numeric < 1000000000000 ? numeric * 1000 : numeric;
      }
      const date = new Date(text);
      return Number.isNaN(date.getTime()) ? 0 : date.getTime();
  };

  ctx.formatTime = (value: unknown): string => {
      const ts = ctx.normalizeTimestamp(value);
      if (!ts)
          return '';
      const date = new Date(ts);
      const now = new Date();
      const sameYear = date.getFullYear() === now.getFullYear();
      const sameDay = sameYear && date.getMonth() === now.getMonth() && date.getDate() === now.getDate();
      const hour = String(date.getHours()).padStart(2, '0');
      const minute = String(date.getMinutes()).padStart(2, '0');
      if (sameDay) {
          return `${hour}:${minute}`;
      }
      if (sameYear) {
          const month = String(date.getMonth() + 1).padStart(2, '0');
          const day = String(date.getDate()).padStart(2, '0');
          return `${month}-${day}`;
      }
      return String(date.getFullYear());
  };

  ctx.resolveOnlineFlag = (value: unknown): boolean => {
      if (typeof value === 'boolean')
          return value;
      if (typeof value === 'number')
          return Number.isFinite(value) && value > 0;
      if (typeof value === 'string') {
          const normalized = value.trim().toLowerCase();
          return normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'online';
      }
      return false;
  };

  ctx.isContactOnline = (contact: unknown): boolean => {
      const source = (contact || {}) as Record<string, unknown>;
      return ctx.resolveOnlineFlag(source.online);
  };

  ctx.formatContactPresence = (contact: unknown): string => ctx.isContactOnline(contact) ? ctx.t('presence.online') : ctx.t('presence.offline');

  ctx.isAdminUser = (user: Record<string, unknown> | null): boolean => Array.isArray(user?.roles) &&
      user.roles.some((role) => role === 'admin' || role === 'super_admin');
}
  installMessengerControllerMessagePanelsPresentation(ctx);
}

function installPart7(ctx: any): void {
// Workspace path resolution, resource fetching, markdown resource cards, image preview, and resource downloads.

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

type WorkspaceResourceInvalidation = {
  epoch: number;
  paths: string[];
};

const WORKSPACE_RESOURCE_CACHE_BUST_PARAM = '_wunder_resource_version';
const WORKSPACE_RESOURCE_CACHE_LIMIT = 48;

const buildWorkspaceResourceCacheKey = (publicPath: string, preview = '', version = 0): string => {
  const previewSuffix = preview ? `#preview=${preview}` : '';
  const versionSuffix = version > 0 ? `#version=${version}` : '';
  return `${publicPath}${previewSuffix}${versionSuffix}`;
};

const extractWorkspaceResourcePublicPathFromCacheKey = (cacheKey: string): string => {
  const text = String(cacheKey || '').trim();
  if (!text)
      return '';
  const previewIndex = text.indexOf('#preview=');
  const versionIndex = text.indexOf('#version=');
  const cutIndex = [previewIndex, versionIndex]
      .filter((index) => index >= 0)
      .reduce((min, index) => Math.min(min, index), text.length);
  return text.slice(0, cutIndex);
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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerWorkspaceResourceHydration(ctx: MessengerControllerContext): void {
  let workspaceHydrationTimeout: number | null = null;
  let workspaceResourceCacheEpoch = 0;
  const pendingHydration = createWorkspaceHydrationBatch();
  let workspaceResourceGeneration = 0;
  // Requests are page-scoped: abandoned bubbles must not keep downloads alive.
  const workspaceResourceAbortControllers = new Set<AbortController>();
  const workspaceResourceInvalidations: WorkspaceResourceInvalidation[] = [];

  const bumpWorkspaceResourceCacheEpoch = () => {
      workspaceResourceCacheEpoch += 1;
      if (!Number.isSafeInteger(workspaceResourceCacheEpoch)) {
          workspaceResourceCacheEpoch = 1;
      }
  };

  const buildCurrentWorkspaceResourceCacheKey = (publicPath: string, preview = ''): string =>
      buildWorkspaceResourceCacheKey(publicPath, preview, workspaceResourceCacheEpoch);

  const rememberWorkspaceResourceInvalidation = (paths: string[]) => {
      workspaceResourceInvalidations.push({
          epoch: workspaceResourceCacheEpoch,
          paths: Array.isArray(paths) ? paths.slice() : []
      });
      while (workspaceResourceInvalidations.length > 64) {
          workspaceResourceInvalidations.shift();
      }
  };

  const isWorkspaceResourceInvalidatedSince = (
      resource: WorkspaceResolvedResource,
      epoch: number
  ): boolean => {
      if (workspaceResourceCacheEpoch <= epoch) {
          return false;
      }
      const relativePath = String(resource?.relativePath || '').trim();
      return workspaceResourceInvalidations.some((entry) => {
          if (!entry || entry.epoch <= epoch) {
              return false;
          }
          return !entry.paths.length || isWorkspacePathAffected(relativePath, entry.paths);
      });
  };

  const buildVersionedWorkspaceResourceRequestParams = (
      resource: WorkspaceResolvedResource,
      extra: Record<string, unknown> = {}
  ) => {
      const scopedExtra = { ...extra };
      if (workspaceResourceCacheEpoch > 0) {
          scopedExtra[WORKSPACE_RESOURCE_CACHE_BUST_PARAM] = String(workspaceResourceCacheEpoch);
      }
      return buildWorkspaceResourceRequestParams(resource, scopedExtra);
  };

  const revokeUncachedWorkspaceObjectUrl = (objectUrl: string) => {
      const url = String(objectUrl || '').trim();
      if (!url.startsWith('blob:')) {
          return;
      }
      const cached = Array.from(ctx.workspaceResourceCache.values()).some((entry) => entry?.objectUrl === url);
      if (!cached) {
          URL.revokeObjectURL(url);
      }
  };

  const collectActiveWorkspaceObjectUrls = (): Set<string> => {
      const active = new Set<string>();
      const previewUrl = String(ctx.resourcePreviewUrl?.value || '').trim();
      if (previewUrl.startsWith('blob:')) {
          active.add(previewUrl);
      }
      if (typeof document === 'undefined') {
          return active;
      }
      document.querySelectorAll<HTMLImageElement>('img[src^="blob:"]').forEach((image) => {
          const url = String(image.currentSrc || image.src || '').trim();
          if (url.startsWith('blob:')) {
              active.add(url);
          }
      });
      return active;
  };

  const pruneWorkspaceResourceCache = () => {
      if (ctx.workspaceResourceCache.size <= WORKSPACE_RESOURCE_CACHE_LIMIT) {
          return;
      }
      const activeUrls = collectActiveWorkspaceObjectUrls();
      for (const [cacheKey, entry] of ctx.workspaceResourceCache.entries()) {
          if (ctx.workspaceResourceCache.size <= WORKSPACE_RESOURCE_CACHE_LIMIT) {
              break;
          }
          const objectUrl = String(entry?.objectUrl || '').trim();
          // Never revoke a URL that backs a rendered image or the open preview.
          if (!objectUrl || activeUrls.has(objectUrl) || entry?.promise) {
              continue;
          }
          URL.revokeObjectURL(objectUrl);
          ctx.workspaceResourceCache.delete(cacheKey);
      }
  };

  const abortWorkspaceResourceRequests = () => {
      workspaceResourceAbortControllers.forEach((controller) => controller.abort());
      workspaceResourceAbortControllers.clear();
  };

  ctx.resolveDesktopWorkspaceRoot = (): string => String(getRuntimeConfig().workspace_root || '').trim();

  ctx.resolveDesktopContainerRoot = (containerId?: number | null): string => {
      if (containerId !== null && Number.isFinite(Number(containerId))) {
          const mapped = String(ctx.desktopContainerRootMap.value[Number(containerId)] || '').trim();
          if (mapped)
              return mapped;
      }
      return ctx.resolveDesktopWorkspaceRoot();
  };

  ctx.resolveDesktopAbsoluteWorkspacePath = (
      relativePath: string,
      containerId?: number | null
  ): string => {
      const normalized = String(relativePath || '').replace(/\\/g, '/').replace(/^\/+/, '').trim();
      if (!normalized) {
          return '';
      }
      const root = String(ctx.resolveDesktopContainerRoot(containerId) || '').trim().replace(/[\\/]+$/, '');
      if (!root) {
          return normalized.replace(/\//g, '\\');
      }
      const looksLikeContainerScoped = /(?:^|[\\/])desktop_user(?:__c__\d+)?$/i.test(root);
      if (looksLikeContainerScoped) {
          return `${root.replace(/[\\/]+$/, '')}\\${normalized.replace(/\//g, '\\').replace(/^\\+/, '')}`;
      }
      const effectiveContainerId =
          containerId !== null && Number.isFinite(Number(containerId))
              ? Number(containerId)
              : ctx.currentContainerId.value;
      const scope = effectiveContainerId > 0 ? `desktop_user__c__${effectiveContainerId}` : 'desktop_user';
      return `${root}\\${scope}\\${normalized.replace(/\//g, '\\').replace(/^\\+/, '')}`;
  };



  ctx.resolveAgentMarkdownWorkspacePath = (rawPath: string): string => {
      const ownerId = normalizeWorkspaceOwnerId(ctx.authStore.user?.id);
      if (!ownerId)
          return '';
      return resolveMarkdownWorkspacePath({
          rawPath,
          ownerId,
          containerId: ctx.currentContainerId.value,
          desktopLocalMode: ctx.desktopLocalMode.value,
          workspaceRoot: ctx.resolveDesktopContainerRoot(ctx.currentContainerId.value)
      });
  };

  ctx.resolveWorldMarkdownWorkspacePath = (rawPath: string, senderUserId: string): string => {
      const ownerId = normalizeWorkspaceOwnerId(senderUserId);
      if (!ownerId)
          return '';
      return resolveMarkdownWorkspacePath({
          rawPath,
          ownerId,
          containerId: USER_CONTAINER_ID,
          desktopLocalMode: ctx.desktopLocalMode.value,
          workspaceRoot: ctx.resolveDesktopContainerRoot(USER_CONTAINER_ID)
      });
  };

  ctx.WORLD_AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;

  ctx.WORLD_AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?\uFF0C\u3002\uFF1B\uFF1A\uFF01\uFF1F\u300B\u3011]+)?$/;

  ctx.decodeWorldAtPathToken = (value: string): string => {
      if (!/%[0-9a-fA-F]{2}/.test(value))
          return value;
      try {
          return decodeURIComponent(value);
      }
      catch {
          return value;
      }
  };

  ctx.replaceWorldAtPathTokens = (content: string, senderUserId: string): string => {
      if (!content)
          return '';
      const ownerId = normalizeWorkspaceOwnerId(senderUserId);
      if (!ownerId)
          return content;
      return content.replace(ctx.WORLD_AT_PATH_RE, (match, prefix, token, doubleQuoted, singleQuoted) => {
          const raw = doubleQuoted ?? singleQuoted ?? token ?? '';
          if (!raw)
              return match;
          let value = raw;
          let suffix = '';
          if (!doubleQuoted && !singleQuoted) {
              const split = ctx.WORLD_AT_PATH_SUFFIX_RE.exec(value);
              if (split) {
                  value = split[1] ?? value;
                  suffix = split[2] ?? '';
              }
          }
          const decoded = ctx.decodeWorldAtPathToken(String(value || '').trim());
          const normalized = ctx.normalizeUploadPath(decoded);
          if (!normalized)
              return match;
          const pathLike = decoded.startsWith('/') ||
              decoded.startsWith('./') ||
              decoded.startsWith('../') ||
              normalized.includes('/') ||
              normalized.includes('.');
          if (!pathLike)
              return match;
          const publicPath = buildWorkspacePublicPath(ownerId, normalized, USER_CONTAINER_ID);
          if (!publicPath)
              return match;
          const label = decoded;
          const replacement = isImagePath(normalized)
              ? `![${label}](${publicPath})`
              : `[${label}](${publicPath})`;
          return `${prefix}${replacement}${suffix}`;
      });
  };

  ctx.resolveWorkspaceResource = (publicPath: string): WorkspaceResolvedResource | null => {
      const parsed = parseWorkspaceResourceUrl(publicPath);
      if (!parsed)
          return null;
      const user = ctx.authStore.user as Record<string, unknown> | null;
      const currentId = normalizeWorkspaceOwnerId(user?.id || user?.user_id || user?.username);
      const workspaceId = parsed.workspaceId || parsed.userId;
      const ownerId = parsed.ownerId || workspaceId;
      const agentId = parsed.agentId || '';
      const containerId = typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
          ? parsed.containerId
          : null;
      const isOwner = Boolean(currentId) &&
          (workspaceId === currentId ||
              workspaceId.startsWith(`${currentId}__agent__`) ||
              workspaceId.startsWith(`${currentId}__a__`) ||
              workspaceId.startsWith(`${currentId}__c__`));
      if (isOwner) {
          return {
              ...parsed,
              requestUserId: null,
              requestAgentId: agentId || null,
              requestContainerId: containerId,
              allowed: true
          };
      }
      if (user && ctx.isAdminUser(user)) {
          return {
              ...parsed,
              requestUserId: ownerId,
              requestAgentId: agentId || null,
              requestContainerId: containerId,
              allowed: true
          };
      }
      // Public workspace paths can be resolved by the backend from the bearer token even while the profile is loading.
      return {
          ...parsed,
          requestUserId: null,
          requestAgentId: agentId || null,
          requestContainerId: containerId,
          allowed: true
      };
  };

  ctx.fetchWorkspaceResource = async (
      resource: WorkspaceResolvedResource,
      options: { preview?: 'png' } = {}
  ) => {
      const preview = options.preview === 'png' ? 'png' : '';
      const requestEpoch = workspaceResourceCacheEpoch;
      const generation = workspaceResourceGeneration;
      const cacheKey = buildWorkspaceResourceCacheKey(resource.publicPath, preview, requestEpoch);
      const cached = ctx.workspaceResourceCache.get(cacheKey);
      if (cached?.objectUrl) {
          return {
              objectUrl: cached.objectUrl,
              filename: cached.filename || resource.filename || 'download'
          };
      }
      if (cached?.promise)
          return cached.promise;
      const controller = new AbortController();
      workspaceResourceAbortControllers.add(controller);
      const promise = (async () => {
          const extra: Record<string, unknown> = preview ? { preview } : {};
          if (requestEpoch > 0) {
              extra[WORKSPACE_RESOURCE_CACHE_BUST_PARAM] = String(requestEpoch);
          }
          const params = buildWorkspaceResourceRequestParams(resource, extra);
          const response = await downloadWunderWorkspaceFile(params, { signal: controller.signal });
          if (controller.signal.aborted || generation !== workspaceResourceGeneration) {
              throw new DOMException('Workspace request cancelled', 'AbortError');
          }
          try {
              const fallbackFilename = preview === 'png'
                  ? `${String(resource.filename || 'preview').replace(/\.[^.]+$/, '')}.png`
                  : resource.filename || 'download';
              const filename = getFilenameFromHeaders(response?.headers as Record<string, unknown>, fallbackFilename);
              const contentType = String((response?.headers as Record<string, unknown>)?.['content-type'] ||
                  (response?.headers as Record<string, unknown>)?.['Content-Type'] ||
                  '');
              const sourceBlob = response.data as Blob;
              const normalizedBlob = await normalizeWorkspaceImageResponseBlob(
                  sourceBlob,
                  filename,
                  contentType,
                  response
              );
              if (controller.signal.aborted || generation !== workspaceResourceGeneration) {
                  throw new DOMException('Workspace request cancelled', 'AbortError');
              }
              const objectUrl = URL.createObjectURL(normalizedBlob);
              const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
              if (isWorkspaceResourceInvalidatedSince(resource, requestEpoch)) {
                  URL.revokeObjectURL(objectUrl);
                  ctx.workspaceResourceCache.delete(cacheKey);
                  return ctx.fetchWorkspaceResource(resource, options);
              }
              const activeCacheKey = buildCurrentWorkspaceResourceCacheKey(resource.publicPath, preview);
              ctx.workspaceResourceCache.set(activeCacheKey, entry);
              if (activeCacheKey !== cacheKey) {
                  ctx.workspaceResourceCache.delete(cacheKey);
              }
              pruneWorkspaceResourceCache();
              return entry;
          }
          catch (error) {
              if (ctx.workspaceResourceCache.get(cacheKey)?.promise === promise) {
                  ctx.workspaceResourceCache.delete(cacheKey);
              }
              throw error;
          }
      })()
          .catch((error) => {
          if (ctx.workspaceResourceCache.get(cacheKey)?.promise === promise) ctx.workspaceResourceCache.delete(cacheKey);
          throw error;
      })
          .finally(() => {
          workspaceResourceAbortControllers.delete(controller);
      });
      ctx.workspaceResourceCache.set(cacheKey, { promise });
      return promise;
  };

  ctx.setUserAttachmentResourceState = (publicPath: string, state: AttachmentResourceState) => {
      const next = new Map(ctx.userAttachmentResourceCache.value);
      next.set(publicPath, state);
      ctx.userAttachmentResourceCache.value = next;
  };

  ctx.ensureUserAttachmentResource = async (publicPath: string) => {
      const normalized = String(publicPath || '').trim();
      if (!normalized)
          return;
      const existing = ctx.userAttachmentResourceCache.value.get(normalized);
      if (existing)
          return;
      const resource = ctx.resolveWorkspaceResource(normalized);
      if (!resource)
          return;
      if (!resource.allowed) {
          ctx.setUserAttachmentResourceState(normalized, { error: true });
          return;
      }
      ctx.setUserAttachmentResourceState(normalized, { loading: true });
      try {
          const preview = isMetafileImagePath(resource.filename || resource.relativePath || resource.publicPath)
              ? 'png'
              : undefined;
          const entry = await ctx.fetchWorkspaceResource(resource, { preview });
          ctx.setUserAttachmentResourceState(normalized, {
              objectUrl: entry.objectUrl,
              filename: entry.filename
          });
      }
      catch (error) {
          ctx.setUserAttachmentResourceState(normalized, { error: true });
      }
  };

  ctx.isWorkspaceResourceMissing = (error: unknown): boolean => {
      const status = Number((error as {
          response?: {
              status?: unknown;
          };
      })?.response?.status || 0);
      if (status === 404 || status === 410)
          return true;
      const raw = (error as {
          response?: {
              data?: {
                  detail?: string;
                  message?: string;
              };
          };
      })?.response?.data?.detail ||
          (error as {
              response?: {
                  data?: {
                      message?: string;
                  };
              };
          })?.response?.data?.message ||
          (error as {
              message?: string;
          })?.message ||
          '';
      const message = typeof raw === 'string' ? raw : String(raw || '');
      return /not found|no such|娑撳秴鐡ㄩ崷鈻呴幍鍙ョ瑝閸掔殬瀹告彃鍨归梽顦㈠鑼╅梽顦emoved/i.test(message);
  };

  ctx.hydrateWorkspaceResourceCard = async (card: HTMLElement) => {
      if (!card || card.dataset.workspaceState)
          return;
      const kind = String(card.dataset.workspaceKind || 'image');
      if (kind !== 'image') {
          card.dataset.workspaceState = 'ready';
          return;
      }
      const publicPath = String(card.dataset.workspacePath || '').trim();
      const status = card.querySelector('.ai-resource-status') as HTMLElement | null;
      const preview = card.querySelector('.ai-resource-preview') as HTMLImageElement | null;
      if (!publicPath || !preview)
          return;
      const resource = ctx.resolveWorkspaceResource(publicPath);
      if (!resource || !resource.allowed) {
          if (status)
              status.textContent = ctx.t('chat.resourceUnavailable');
          card.dataset.workspaceState = 'error';
          card.classList.add('is-error');
          return;
      }
      card.dataset.workspaceState = 'loading';
      card.classList.remove('is-error');
      card.classList.remove('is-ready');
      const generation = workspaceResourceGeneration;
      const hydrationEpoch = workspaceResourceCacheEpoch;
      card.dataset.workspaceHydrationEpoch = String(hydrationEpoch);
      const loadingTimerId = scheduleWorkspaceLoadingLabel(card, status, ctx.t('chat.resourceImageLoading'));
      try {
          const entry = await ctx.fetchWorkspaceResource(resource, {
              preview: isMetafileImagePath(resource.filename) ? 'png' : undefined
          });
          if (generation !== workspaceResourceGeneration || String(card.dataset.workspaceHydrationEpoch || '') !== String(hydrationEpoch)) {
              revokeUncachedWorkspaceObjectUrl(entry.objectUrl);
              return;
          }
          bindWorkspaceImagePreviewState(card, preview, entry.objectUrl, {
              status,
              loadingTimerId,
              failedLabel: ctx.t('chat.resourceImageFailed'),
              onDecodeError: () => {
                  ['', 'png'].forEach((previewKind) => {
                      const cacheEntry = ctx.workspaceResourceCache.get(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, previewKind));
                      if (cacheEntry?.objectUrl) {
                          URL.revokeObjectURL(cacheEntry.objectUrl);
                      }
                      ctx.workspaceResourceCache.delete(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, previewKind));
                  });
              }
          });
      }
      catch (error) {
          if (generation !== workspaceResourceGeneration ||
              String(card.dataset.workspaceHydrationEpoch || '') !== String(hydrationEpoch)) return;
          await hydrateWorkspaceResourceErrorDiagnostics(error);
          if (generation !== workspaceResourceGeneration ||
              String(card.dataset.workspaceHydrationEpoch || '') !== String(hydrationEpoch)) return;
          markWorkspaceImageCardError(card, status, loadingTimerId, ctx.isWorkspaceResourceMissing(error)
              ? ctx.t('chat.resourceMissing')
              : ctx.t('chat.resourceImageFailed'), resolveWorkspaceResourceErrorDiagnostics(error));
      }
  };

  ctx.hydrateWorkspaceResources = (options: {
      messageKeys?: string[];
  } = {}) => {
      const container = ctx.messageListRef.value;
      if (!container)
          return;
      const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
      const targetKeys = Array.isArray(options.messageKeys)
          ? options.messageKeys.map((key) => String(key || '').trim()).filter(Boolean)
          : [];
      const targetKeySet = targetKeys.length ? new Set(targetKeys) : null;
      const messageNodes = targetKeys.length
          ? Array.from(container.querySelectorAll('.messenger-message[data-virtual-key]'))
              .filter((node) => targetKeySet?.has(String((node as HTMLElement).dataset?.virtualKey || '').trim()))
          : Array.from(container.querySelectorAll('.messenger-message[data-virtual-key]'));
      const cards = messageNodes.length
          ? Array.from(messageNodes).flatMap((node) => Array.from(node.querySelectorAll('.ai-resource-card[data-workspace-path]')))
          : Array.from(container.querySelectorAll('.ai-resource-card[data-workspace-path]'));
      cards.forEach((card) => {
          void ctx.hydrateWorkspaceResourceCard(card as HTMLElement);
      });
      if (messageNodes.length) {
          messageNodes.forEach((node) => hydrateExternalMarkdownImages(node));
      }
      else {
          hydrateExternalMarkdownImages(container);
      }
      if (isChatDebugVerboseEnabled()) {
          const durationMs = Number(((typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt).toFixed(1));
          chatDebugLog('messenger.hydration', 'workspace-scan', {
              activeSection: ctx.sessionHub.activeSection,
              activeConversationKey: ctx.sessionHub.activeConversationKey,
              virtualized: Boolean(ctx.shouldVirtualizeMessages?.value),
              targetKeyCount: targetKeys.length,
              messageNodeCount: messageNodes.length,
              resourceCardCount: cards.length,
              durationMs
          });
      }
  };

  ctx.scheduleWorkspaceResourceHydration = (reason = '', options: {
      messageKeys?: string[];
  } = {}) => {
      if (ctx.sessionHub.activeSection !== 'messages') {
          return;
      }
      pendingHydration.add(options.messageKeys);
      if (ctx.workspaceResourceHydrationFrame !== null || ctx.workspaceResourceHydrationPending ||
          workspaceHydrationTimeout !== null)
          return;
      ctx.workspaceResourceHydrationPending = true;
      const generation = workspaceResourceGeneration;
      void nextTick(() => {
          if (generation !== workspaceResourceGeneration) return;
          ctx.workspaceResourceHydrationPending = false;
          if (ctx.workspaceResourceHydrationFrame !== null || typeof window === 'undefined')
              return;
          workspaceHydrationTimeout = window.setTimeout(() => {
              workspaceHydrationTimeout = null;
              if (ctx.sessionHub.activeSection !== 'messages') {
                  return;
              }
              ctx.workspaceResourceHydrationFrame = window.requestAnimationFrame(() => {
                  ctx.workspaceResourceHydrationFrame = null;
                  if (isChatDebugVerboseEnabled()) {
                      chatDebugLog('messenger.hydration', 'workspace-run', {
                          reason,
                          activeSection: ctx.sessionHub.activeSection,
                          activeConversationKey: ctx.sessionHub.activeConversationKey
                      });
                  }
                  ctx.hydrateWorkspaceResources(pendingHydration.take());
              });
          }, false ? 180 : 90);
      });
  };

  ctx.resetWorkspaceResourceCards = (changedPaths: string[] = []) => {
      const container = ctx.messageListRef.value;
      if (!container)
          return 0;
      let resetCount = 0;
      const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
      cards.forEach((card) => {
          const element = card as HTMLElement;
          const publicPath = String(element.dataset.workspacePath || '').trim();
          if (changedPaths.length) {
              const parsed = parseWorkspaceResourceUrl(publicPath);
              if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
                  return;
              }
          }
          if (resetWorkspaceImageCardState(element, { clearSrc: true, includeReady: true })) {
              delete element.dataset.workspaceHydrationEpoch;
              resetCount += 1;
          }
      });
      return resetCount;
  };

  ctx.clearWorkspaceResourceCache = () => {
      workspaceResourceGeneration += 1;
      pendingHydration.clear();
      abortWorkspaceResourceRequests();
      ctx.resetWorkspaceResourceCards();
      if (typeof window !== 'undefined' && workspaceHydrationTimeout !== null) {
          window.clearTimeout(workspaceHydrationTimeout);
          workspaceHydrationTimeout = null;
      }
      if (ctx.workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
          window.cancelAnimationFrame(ctx.workspaceResourceHydrationFrame);
          ctx.workspaceResourceHydrationFrame = null;
      }
      ctx.workspaceResourceHydrationPending = false;
      ctx.workspaceResourceCache.forEach((entry) => {
          if (entry?.objectUrl) {
              URL.revokeObjectURL(entry.objectUrl);
          }
      });
      ctx.workspaceResourceCache.clear();
      ctx.userAttachmentResourceCache.value = new Map();
      return 0;
  };

  ctx.clearWorkspaceResourceCacheByPaths = (changedPaths: string[] = []) => {
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          return 0;
      }
      if (typeof window !== 'undefined' && workspaceHydrationTimeout !== null) {
          window.clearTimeout(workspaceHydrationTimeout);
          workspaceHydrationTimeout = null;
      }
      if (ctx.workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
          window.cancelAnimationFrame(ctx.workspaceResourceHydrationFrame);
          ctx.workspaceResourceHydrationFrame = null;
      }
      ctx.workspaceResourceHydrationPending = false;
      let clearedCount = 0;
      Array.from(ctx.workspaceResourceCache.entries()).forEach(([cacheKey, entry]) => {
          const parsed = parseWorkspaceResourceUrl(extractWorkspaceResourcePublicPathFromCacheKey(cacheKey));
          if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
              return;
          }
          if (entry?.objectUrl) {
              URL.revokeObjectURL(entry.objectUrl);
          }
          ctx.workspaceResourceCache.delete(cacheKey);
          clearedCount += 1;
      });
      if (ctx.userAttachmentResourceCache.value.size > 0) {
          const next = new Map(ctx.userAttachmentResourceCache.value);
          Array.from(next.keys()).forEach((publicPath) => {
              const parsed = parseWorkspaceResourceUrl(publicPath);
              if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
                  return;
              }
              const entry = next.get(publicPath);
              if (entry?.objectUrl) {
                  URL.revokeObjectURL(entry.objectUrl);
              }
              next.delete(publicPath);
              clearedCount += 1;
          });
          ctx.userAttachmentResourceCache.value = next;
      }
      return clearedCount;
  };

  onBeforeUnmount(() => {
      abortWorkspaceResourceRequests();
  });

  ctx.parseWorkspaceRefreshContainerId = (value: unknown): number | null => {
      const parsed = Number.parseInt(String(value ?? ''), 10);
      return Number.isFinite(parsed) ? parsed : null;
  };

  ctx.shouldHandleWorkspaceResourceRefresh = (detail: Record<string, unknown>) => {
      const eventAgentId = ctx.normalizeAgentId(detail.agentId ?? detail.agent_id);
      const eventContainerId = ctx.parseWorkspaceRefreshContainerId(detail.containerId ?? detail.container_id);
      if (ctx.isWorldConversationActive.value) {
          if (eventAgentId)
              return false;
          return !Number.isFinite(eventContainerId) || eventContainerId === USER_CONTAINER_ID;
      }
      if (!ctx.isAgentConversationActive.value) {
          return false;
      }
      const currentAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      if (eventAgentId && eventAgentId !== currentAgentId) {
          return false;
      }
      return !Number.isFinite(eventContainerId) || eventContainerId === ctx.currentContainerId.value;
  };

  ctx.handleWorkspaceResourceRefresh = (event?: Event) => {
      const detail = (event as CustomEvent<Record<string, unknown>> | undefined)?.detail &&
          typeof (event as CustomEvent<Record<string, unknown>>).detail === 'object'
          ? ((event as CustomEvent<Record<string, unknown>>).detail as Record<string, unknown>)
          : {};
      if (!ctx.shouldHandleWorkspaceResourceRefresh(detail)) {
          return;
      }
      bumpWorkspaceResourceCacheEpoch();
      const changedPaths = extractWorkspaceRefreshPaths(detail);
      rememberWorkspaceResourceInvalidation(changedPaths);
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          ctx.resetWorkspaceResourceCards();
          ctx.scheduleWorkspaceResourceHydration('workspace-refresh');
          return;
      }
      const clearedCount = ctx.clearWorkspaceResourceCacheByPaths(changedPaths);
      const resetCount = ctx.resetWorkspaceResourceCards(changedPaths);
      if (clearedCount === 0 && resetCount === 0) {
          // A path that is not currently visible needs no global cache reset.
          // Clearing every resource here made unrelated images and GIFs reload
          // on each workspace write.
          rememberWorkspaceResourceInvalidation(changedPaths);
      }
      ctx.scheduleWorkspaceResourceHydration('workspace-refresh');
  };

  ctx.downloadWorkspaceResource = async (publicPath: string) => {
      const resource = ctx.resolveWorkspaceResource(publicPath);
      if (!resource || !resource.allowed)
          return;
      try {
          const entry = await ctx.fetchWorkspaceResource(resource);
          saveObjectUrlAsFile(entry.objectUrl, entry.filename || resource.filename || 'download');
      }
      catch (error) {
          ElMessage.error(ctx.isWorkspaceResourceMissing(error) ? ctx.t('chat.resourceMissing') : ctx.t('chat.resourceDownloadFailed'));
      }
  };

  ctx.openWorkspaceResourceWithDefaultApp = async (resourcePath: string) => {
      const normalized = String(resourcePath || '').trim();
      if (!normalized || !false) {
          return false;
      }
      const bridge = ctx.getDesktopBridge();
      if (!bridge || typeof bridge.openPathWithDefaultApp !== 'function') {
          return false;
      }
      const resolved = ctx.resolveWorkspaceResource(normalized);
      const localPath = await ctx.resolveDesktopAbsoluteWorkspacePathAsync(
          String(resolved?.relativePath || normalized).trim(),
          resolved?.requestContainerId ?? null
      );
      if (!localPath) {
          return false;
      }
      try {
          return Boolean(await bridge.openPathWithDefaultApp(localPath));
      }
      catch {
          return false;
      }
  };

  ctx.downloadExternalImage = async (src: string) => {
      const url = String(src || '').trim();
      if (!url)
          return;
      try {
          const response = await fetch(url);
          if (!response.ok) {
              throw new Error(`HTTP ${response.status}`);
          }
          const blob = await response.blob();
          const objectUrl = URL.createObjectURL(blob);
          // Extract filename from URL
          let filename = 'image';
          try {
              const pathname = new URL(url).pathname;
              const basename = pathname.split('/').pop() || '';
              if (basename && basename.includes('.')) {
                  filename = basename;
              }
              else {
                  // Determine extension from MIME type
                  const ext = blob.type.split('/')[1] || 'png';
                  filename = `image.${ext}`;
              }
          }
          catch {
              const ext = blob.type.split('/')[1] || 'png';
              filename = `image.${ext}`;
          }
          saveObjectUrlAsFile(objectUrl, filename);
          URL.revokeObjectURL(objectUrl);
      }
      catch (error) {
          ElMessage.error(ctx.t('chat.resourceDownloadFailed'));
      }
  };

  ctx.openResourcePreview = async (options: {
      src?: string;
      title?: string;
      workspacePath?: string;
      userId?: string;
      content?: string;
      hint?: string;
      meta?: string;
      kind?: string;
  } = {}) => {
      const workspacePath = String(options.workspacePath || '').trim();
      const title = String(options.title || '').trim() || ctx.t('workspace.preview.dialogTitle');
      const meta = String(options.meta || workspacePath || title).trim();
      const userId = String(options.userId || '').trim();
      const initialKind = String(options.kind || '').trim();
      const fileName = normalizeWorkspacePreviewFilename(title, workspacePath.split('/').pop() || '');
      const previewKind = initialKind || resolveWorkspaceResourcePreviewKind(fileName);
      ctx.resourcePreviewVisible.value = true;
      ctx.resourcePreviewLoading.value = false;
      ctx.resourcePreviewTitle.value = title;
      ctx.resourcePreviewMeta.value = meta;
      ctx.resourcePreviewHint.value = String(options.hint || '').trim();
      ctx.resourcePreviewContent.value = String(options.content || '').trim();
      ctx.resourcePreviewWorkspacePath.value = workspacePath;
      ctx.resourcePreviewUrl.value = String(options.src || '').trim();
      ctx.resourcePreviewKind.value = previewKind || 'image';
      ctx.resourcePreviewUserId.value = userId;
      if (!workspacePath) {
          return;
      }
      if (previewKind === 'drawio') {
          const resource = ctx.resolveWorkspaceResource(workspacePath);
          const relativePath = String(resource?.relativePath || workspacePath).trim();
          ctx.resourcePreviewVisible.value = false;
          ctx.drawioVisible.value = true;
          ctx.drawioPath.value = relativePath;
          ctx.drawioUserId.value = String(resource?.requestUserId || userId).trim();
          ctx.drawioAgentId.value = String(resource?.requestAgentId || '').trim();
          ctx.drawioContainerId.value =
              resource?.requestContainerId !== null && Number.isFinite(resource?.requestContainerId)
                  ? resource.requestContainerId
                  : null;
          return;
      }
      if (previewKind === 'onlyoffice') {
          const resource = ctx.resolveWorkspaceResource(workspacePath);
          const relativePath = String(resource?.relativePath || workspacePath).trim();
          if (false) {
              const opened = await ctx.openWorkspaceResourceWithDefaultApp(workspacePath);
              if (opened) {
                  return;
              }
          }
          ctx.resourcePreviewVisible.value = false;
          ctx.onlyOfficeVisible.value = true;
          ctx.onlyOfficePath.value = relativePath;
          ctx.onlyOfficeUserId.value = String(resource?.requestUserId || userId).trim();
          ctx.onlyOfficeAgentId.value = String(resource?.requestAgentId || '').trim();
          ctx.onlyOfficeContainerId.value =
              resource?.requestContainerId !== null && Number.isFinite(resource?.requestContainerId)
                  ? resource.requestContainerId
                  : null;
          return;
      }
      const resource = ctx.resolveWorkspaceResource(workspacePath);
      if (!resource || !resource.allowed) {
          ctx.resourcePreviewHint.value = ctx.t('chat.resourceUnavailable');
          ctx.resourcePreviewKind.value = 'unsupported';
          return;
      }
      if (previewKind === 'unsupported') {
          ctx.resourcePreviewHint.value = resolveWorkspacePreviewUnsupportedHint();
          return;
      }
      ctx.resourcePreviewLoading.value = true;
      try {
          if (previewKind === 'text') {
              const response = await fetchWunderWorkspaceContent(buildVersionedWorkspaceResourceRequestParams(resource, {
                  include_content: true,
                  max_bytes: WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES
              }));
              const payload = response.data || {};
              if (payload.truncated) {
                  ctx.resourcePreviewHint.value = ctx.t('workspace.preview.truncatedHint');
              }
              ctx.resourcePreviewContent.value = typeof payload.content === 'string'
                  ? payload.content || ctx.t('workspace.preview.emptyContent')
                  : ctx.t('workspace.preview.emptyContent');
              return;
          }
          const preview = isMetafileImagePath(resource.filename || resource.relativePath || resource.publicPath)
              ? 'png'
              : undefined;
          const entry = await ctx.fetchWorkspaceResource(resource, { preview });
          const extension = extractWorkspaceResourceExtension(fileName);
          const cacheEntry = ctx.workspaceResourceCache.get(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, preview || ''));
          if (cacheEntry?.objectUrl && previewKind !== 'pdf') {
              ctx.resourcePreviewUrl.value = cacheEntry.objectUrl;
              return;
          }
          const response = await downloadWunderWorkspaceFile(buildVersionedWorkspaceResourceRequestParams(resource));
          const blob = normalizeWorkspacePreviewBlob(response.data as Blob, previewKind as never, extension);
          ctx.resourcePreviewUrl.value = URL.createObjectURL(blob);
          ctx.resourcePreviewContent.value = '';
          ctx.resourcePreviewHint.value = '';
          void entry;
      }
      catch (error) {
          const missing = ctx.isWorkspaceResourceMissing(error);
          ctx.resourcePreviewHint.value = missing
              ? ctx.t('chat.resourceMissing')
              : previewKind === 'text'
                  ? ctx.t('workspace.preview.loadFailedHint')
                  : resolveWorkspacePreviewTooLargeHint();
          if (previewKind === 'text') {
              ctx.resourcePreviewContent.value = ctx.t('workspace.preview.empty');
          }
      }
      finally {
          ctx.resourcePreviewLoading.value = false;
      }
  };

  ctx.handleResourcePreviewDownload = async () => {
      const workspacePath = String(ctx.resourcePreviewWorkspacePath.value || '').trim();
      if (workspacePath) {
          await ctx.downloadWorkspaceResource(workspacePath);
          return;
      }
      const url = String(ctx.resourcePreviewUrl.value || '').trim();
      if (url) {
          await ctx.downloadExternalImage(url);
      }
  };

  ctx.closeResourcePreview = () => {
      const currentUrl = String(ctx.resourcePreviewUrl.value || '').trim();
      const currentWorkspacePath = String(ctx.resourcePreviewWorkspacePath.value || '').trim();
      const isCachedObjectUrl =
          currentUrl.startsWith('blob:') &&
          Array.from(ctx.workspaceResourceCache.values()).some((entry) => entry?.objectUrl === currentUrl);
      if (
          currentUrl &&
          currentWorkspacePath &&
          currentUrl.startsWith('blob:') &&
          !isCachedObjectUrl
      ) {
          URL.revokeObjectURL(currentUrl);
      }
      ctx.resourcePreviewVisible.value = false;
      ctx.resourcePreviewLoading.value = false;
      ctx.resourcePreviewUrl.value = '';
      ctx.resourcePreviewTitle.value = '';
      ctx.resourcePreviewMeta.value = '';
      ctx.resourcePreviewHint.value = '';
      ctx.resourcePreviewContent.value = '';
      ctx.resourcePreviewWorkspacePath.value = '';
      ctx.resourcePreviewKind.value = 'image';
      ctx.resourcePreviewUserId.value = '';
  };

  ctx.handleWorkspaceEditorSaved = async (payload: { path?: string } = {}) => {
      const changedPath = String(payload.path || ctx.onlyOfficePath.value || ctx.drawioPath.value || '').trim();
      if (!changedPath)
          return;
      const editorAgentId = ctx.onlyOfficeVisible.value
          ? ctx.onlyOfficeAgentId.value
          : ctx.drawioVisible.value
              ? ctx.drawioAgentId.value
              : ctx.activeAgentId.value;
      const explicitEditorContainerId = ctx.onlyOfficeVisible.value
          ? ctx.onlyOfficeContainerId.value
          : ctx.drawioVisible.value
              ? ctx.drawioContainerId.value
              : null;
      const editorContainerId =
          explicitEditorContainerId !== null && Number.isFinite(explicitEditorContainerId)
              ? explicitEditorContainerId
              : ctx.currentContainerId.value;
      emitWorkspaceRefresh({
          path: changedPath,
          changed_paths: [changedPath],
          agent_id: editorAgentId || '',
          container_id: editorContainerId
      });
      if (ctx.resourcePreviewVisible.value) {
          await ctx.openResourcePreview({
              title: ctx.resourcePreviewTitle.value,
              workspacePath: ctx.resourcePreviewWorkspacePath.value,
              userId: ctx.resourcePreviewUserId.value,
              meta: ctx.resourcePreviewMeta.value,
              kind: ctx.resourcePreviewKind.value
          });
      }
  };

  ctx.handleWorkspaceEditorFallback = async (payload: { path?: string; message?: string } = {}) => {
      const path = String(payload.path || '').trim() || ctx.onlyOfficePath.value || ctx.drawioPath.value;
      const fallbackContainerId =
          ctx.onlyOfficeVisible.value
              ? ctx.onlyOfficeContainerId.value
              : ctx.drawioVisible.value
                  ? ctx.drawioContainerId.value
                  : null;
      ctx.onlyOfficeVisible.value = false;
      ctx.drawioVisible.value = false;
      ctx.onlyOfficePath.value = '';
      ctx.drawioPath.value = '';
      ctx.onlyOfficeUserId.value = '';
      ctx.drawioUserId.value = '';
      ctx.onlyOfficeAgentId.value = '';
      ctx.drawioAgentId.value = '';
      ctx.onlyOfficeContainerId.value = null;
      ctx.drawioContainerId.value = null;
      if (!path)
          return;
      if (false) {
          const bridge = ctx.getDesktopBridge();
          if (bridge && typeof bridge.openPathWithDefaultApp === 'function') {
              try {
                  const localPath = await ctx.resolveDesktopAbsoluteWorkspacePathAsync(
                      path,
                      fallbackContainerId
                  );
                  await bridge.openPathWithDefaultApp(localPath || ctx.resolveDesktopAbsoluteWorkspacePath(path, fallbackContainerId) || path);
                  return;
              }
              catch {
                  // Fall back to in-app preview if the local app cannot open the file.
              }
          }
      }
      await ctx.openResourcePreview({
          title: ctx.resourcePreviewTitle.value || path.split('/').pop() || '',
          workspacePath: path,
          userId: ctx.resourcePreviewUserId.value,
          meta: path,
          hint: String(payload.message || '').trim(),
          kind: resolveWorkspaceResourcePreviewKind(path)
      });
  };

  ctx.handleMessageContentClick = async (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target)
          return;
      // Handle external image preview (images from external URLs)
      const externalImage = target.closest('img.ai-external-image-preview') as HTMLImageElement | null;
      if (externalImage) {
          const card = externalImage.closest('.ai-external-image-card') as HTMLElement | null;
          const src = String(card?.dataset?.externalImageSrc || externalImage.getAttribute('src') || '').trim();
          if (!src)
              return;
          const title = String(card?.dataset?.externalImageAlt || externalImage.getAttribute('alt') || '').trim();
          await ctx.openResourcePreview({
              src,
              title,
              meta: title,
              kind: 'image'
          });
          return;
      }
      // Handle workspace resource image preview
      const previewImage = target.closest('img.ai-resource-preview') as HTMLImageElement | null;
      if (previewImage) {
          const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
          if (card?.dataset?.workspaceState !== 'ready')
              return;
          const src = String(previewImage.getAttribute('src') || '').trim();
          if (!src)
              return;
          const title = String(card?.querySelector('.ai-resource-name')?.textContent || '').trim();
          const workspacePath = String(card?.dataset?.workspacePath || '').trim();
          await ctx.openResourcePreview({
              src,
              title,
              workspacePath,
              meta: workspacePath,
              kind: 'image'
          });
          return;
      }
      // Handle external image download button
      const externalImageButton = target.closest('[data-external-image-action]') as HTMLElement | null;
      if (externalImageButton) {
          const card = externalImageButton.closest('.ai-external-image-card') as HTMLElement | null;
          const src = String(card?.dataset?.externalImageSrc || '').trim();
          if (!src)
              return;
          event.preventDefault();
          await ctx.downloadExternalImage(src);
          return;
      }
      // Handle workspace resource download button
      const resourceButton = target.closest('[data-workspace-action]') as HTMLElement | null;
      if (resourceButton) {
          const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
          const publicPath = String(container?.dataset?.workspacePath || '').trim();
          if (!publicPath)
              return;
          event.preventDefault();
          const action = String(resourceButton.dataset?.workspaceAction || container?.dataset?.workspaceAction || '').trim().toLowerCase();
          if (action === 'download') {
              await ctx.downloadWorkspaceResource(publicPath);
              return;
          }
          const title = String(container?.querySelector('.ai-resource-name')?.textContent || '').trim();
          await ctx.openResourcePreview({
              title: decodeWorkspaceResourceLabel(title),
              workspacePath: publicPath,
              meta: publicPath,
              kind: resolveWorkspaceResourcePreviewKind(publicPath || title)
          });
          return;
      }
      const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
      if (resourceLink) {
          const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
          if (!publicPath)
              return;
          event.preventDefault();
          const title = String(resourceLink.textContent || '').trim();
          await ctx.openResourcePreview({
              title: decodeWorkspaceResourceLabel(title),
              workspacePath: publicPath,
              meta: publicPath,
              kind: resolveWorkspaceResourcePreviewKind(publicPath || title)
          });
          return;
      }
      const copyButton = target.closest('.ai-code-copy') as HTMLElement | null;
      if (!copyButton)
          return;
      event.preventDefault();
      const codeBlock = copyButton.closest('.ai-code-block');
      const codeText = String(codeBlock?.querySelector('code')?.textContent || '').trim();
      if (!codeText) {
          ElMessage.warning(ctx.t('chat.message.copyEmpty'));
          return;
      }
      const copied = await copyText(codeText);
      if (copied) {
          ElMessage.success(ctx.t('chat.message.copySuccess'));
      }
      else {
          ElMessage.warning(ctx.t('chat.message.copyFailed'));
      }
  };
}
  installMessengerControllerWorkspaceResourceHydration(ctx);
}

function installPart8(ctx: any): void {
// Messenger order persistence, current-user appearance, launch behavior, middle-pane overlay, and quick agent creation.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerWorkspaceOrderUiActions(ctx: MessengerControllerContext): void {

  ctx.hydrateMessengerOrderPreferences = async () => {
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      const refreshTraceId = String(ctx.messengerSessionRefreshTraceId.value || '').trim();
      const refreshTraceSource = String(ctx.messengerSessionRefreshTraceSource.value || '').trim();
      ctx.messengerOrderReady.value = false;
      if (!scopedUserId) {
          chatDebugLog('messenger.order', 'hydrate-skip-no-user', {
              traceId: refreshTraceId,
              traceSource: refreshTraceSource
          });
          ctx.applyMessengerOrderPreferences(defaultMessengerOrderPreferences());
          ctx.messengerOrderReady.value = true;
          return;
      }
      ctx.messengerOrderHydrating.value = true;
      let shouldBackfillLocalOrder = false;
      try {
          const localPreferences = ctx.captureMessengerOrderPreferences();
          const preferences = await loadMessengerOrderPreferences();
          if (String(ctx.currentUserId.value || '').trim() !== scopedUserId)
              return;
          const shouldPreferLocalFallback = !ctx.hasMessengerOrderEntries(preferences) &&
              preferences.updatedAt <= 0 &&
              ctx.hasMessengerOrderEntries(localPreferences);
          chatDebugLog('messenger.order', 'hydrate-loaded', {
              traceId: refreshTraceId,
              traceSource: refreshTraceSource,
              userId: scopedUserId,
              remote: preferences,
              local: localPreferences,
              shouldPreferLocalFallback
          });
          ctx.applyMessengerOrderPreferences(shouldPreferLocalFallback ? localPreferences : preferences);
          shouldBackfillLocalOrder = shouldPreferLocalFallback;
      }
      finally {
          ctx.messengerOrderHydrating.value = false;
          if (String(ctx.currentUserId.value || '').trim() === scopedUserId) {
              ctx.messengerOrderReady.value = true;
              if (shouldBackfillLocalOrder) {
                  chatDebugLog('messenger.order', 'hydrate-backfill-local', {
                      traceId: refreshTraceId,
                      traceSource: refreshTraceSource,
                      userId: scopedUserId,
                      current: ctx.captureMessengerOrderPreferences()
                  });
                  ctx.scheduleMessengerOrderPersist();
              }
          }
      }
  };

  ctx.persistMessengerOrderPreferences = async () => {
      const refreshTraceId = String(ctx.messengerSessionRefreshTraceId.value || '').trim();
      const refreshTraceSource = String(ctx.messengerSessionRefreshTraceSource.value || '').trim();
      if (ctx.messengerOrderHydrating.value || !ctx.messengerOrderReady.value) {
          chatDebugLog('messenger.order', 'persist-skip-not-ready', {
              traceId: refreshTraceId,
              traceSource: refreshTraceSource,
              hydrating: ctx.messengerOrderHydrating.value,
              ready: ctx.messengerOrderReady.value
          });
          return;
      }
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      if (!scopedUserId) {
          chatDebugLog('messenger.order', 'persist-skip-no-user', {
              traceId: refreshTraceId,
              traceSource: refreshTraceSource
          });
          return;
      }
      const current = ctx.captureMessengerOrderPreferences();
      chatDebugLog('messenger.order', 'persist-start', {
          traceId: refreshTraceId,
          traceSource: refreshTraceSource,
          userId: scopedUserId,
          current
      });
      const persisted = await saveMessengerOrderPreferences(current);
      if (String(ctx.currentUserId.value || '').trim() !== scopedUserId)
          return;
      ctx.messengerOrderSnapshot.value = {
          messages: persisted.messages.slice(),
          agentsOwned: persisted.agentsOwned.slice(),
          agentsShared: persisted.agentsShared.slice(),
          swarms: persisted.swarms.slice(),
          updatedAt: persisted.updatedAt
      };
      chatDebugLog('messenger.order', 'persist-finish', {
          traceId: refreshTraceId,
          traceSource: refreshTraceSource,
          userId: scopedUserId,
          persisted
      });
  };

  ctx.scheduleMessengerOrderPersist = () => {
      const refreshTraceId = String(ctx.messengerSessionRefreshTraceId.value || '').trim();
      const refreshTraceSource = String(ctx.messengerSessionRefreshTraceSource.value || '').trim();
      if (ctx.messengerOrderHydrating.value || !ctx.messengerOrderReady.value || typeof window === 'undefined') {
          chatDebugLog('messenger.order', 'schedule-skip', {
              traceId: refreshTraceId,
              traceSource: refreshTraceSource,
              hydrating: ctx.messengerOrderHydrating.value,
              ready: ctx.messengerOrderReady.value,
              hasWindow: typeof window !== 'undefined'
          });
          return;
      }
      if (ctx.messengerOrderSaveTimer.value !== null) {
          window.clearTimeout(ctx.messengerOrderSaveTimer.value);
      }
      chatDebugLog('messenger.order', 'schedule', {
          traceId: refreshTraceId,
          traceSource: refreshTraceSource,
          current: ctx.captureMessengerOrderPreferences()
      });
      ctx.messengerOrderSaveTimer.value = window.setTimeout(() => {
          ctx.messengerOrderSaveTimer.value = null;
          void ctx.persistMessengerOrderPreferences();
      }, 220);
  };

  ctx.updateCurrentUserAvatarIcon = (value: unknown) => {
      ctx.currentUserAvatarIcon.value = normalizeAvatarIcon(value, PROFILE_AVATAR_OPTION_KEYS);
      void ctx.persistCurrentUserAppearance();
  };

  ctx.updateCurrentUserAvatarColor = (value: unknown) => {
      ctx.currentUserAvatarColor.value = normalizeAvatarColor(value);
      void ctx.persistCurrentUserAppearance();
  };

  ctx.initDesktopLaunchBehavior = () => {
      ctx.desktopShowFirstLaunchDefaultAgentHint.value = false;
      ctx.desktopFirstLaunchDefaultAgentHintAt.value = 0;
      if (!false || typeof window === 'undefined')
          return;
      try {
          const alreadyShown = String(window.localStorage.getItem(ctx.DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY) || '').trim() === '1';
          if (!alreadyShown) {
              ctx.desktopShowFirstLaunchDefaultAgentHint.value = true;
              ctx.desktopFirstLaunchDefaultAgentHintAt.value = Date.now();
              window.localStorage.setItem(ctx.DESKTOP_FIRST_LAUNCH_DEFAULT_AGENT_HINT_KEY, '1');
          }
      }
      catch {
          ctx.desktopShowFirstLaunchDefaultAgentHint.value = false;
          ctx.desktopFirstLaunchDefaultAgentHintAt.value = 0;
      }
  };

  ctx.clearMiddlePaneOverlayHide = () => {
      if (typeof window !== 'undefined' && ctx.middlePaneOverlayHideTimer) {
          window.clearTimeout(ctx.middlePaneOverlayHideTimer);
          ctx.middlePaneOverlayHideTimer = null;
      }
  };

  ctx.clearMiddlePanePrewarm = () => {
      if (typeof window !== 'undefined' && ctx.middlePanePrewarmTimer !== null) {
          window.clearTimeout(ctx.middlePanePrewarmTimer);
          ctx.middlePanePrewarmTimer = null;
      }
  };

  ctx.clearKeywordDebounce = () => {
      if (typeof window === 'undefined' || ctx.keywordDebounceTimer === null)
          return;
      window.clearTimeout(ctx.keywordDebounceTimer);
      ctx.keywordDebounceTimer = null;
  };

  ctx.resetContactVirtualScroll = () => {
      ctx.contactVirtualScrollTop.value = 0;
      const container = ctx.contactVirtualListRef.value;
      if (container && container.scrollTop !== 0) {
          container.scrollTop = 0;
      }
  };

  ctx.syncContactVirtualMetrics = () => {
      const container = ctx.contactVirtualListRef.value;
      if (!container) {
          ctx.contactVirtualViewportHeight.value = 0;
          ctx.contactVirtualScrollTop.value = 0;
          return;
      }
      ctx.contactVirtualViewportHeight.value = container.clientHeight;
      ctx.contactVirtualScrollTop.value = container.scrollTop;
  };

  ctx.handleContactVirtualScroll = () => {
      if (typeof window === 'undefined') {
          ctx.syncContactVirtualMetrics();
          return;
      }
      if (ctx.contactVirtualFrame !== null)
          return;
      ctx.contactVirtualFrame = window.requestAnimationFrame(() => {
          ctx.contactVirtualFrame = null;
          ctx.syncContactVirtualMetrics();
      });
  };

  ctx.openMiddlePaneOverlay = () => {
      if (!ctx.isMiddlePaneOverlay.value)
          return;
      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneMounted.value = true;
      ctx.middlePaneOverlayVisible.value = true;
  };

  ctx.normalizeSettingsPanelMode = (value: unknown): SettingsPanelMode => {
      const normalized = String(value || '').trim().toLowerCase();
      if (normalized === 'profile' ||
          normalized === 'prompts' ||
          normalized === 'help-manual' ||
          normalized === 'desktop-models' ||
          normalized === 'desktop-lan') {
          return normalized;
      }
      return 'general';
  };

  ctx.cancelMiddlePaneOverlayHide = () => {
      ctx.clearMiddlePaneOverlayHide();
  };

  ctx.scheduleMiddlePaneOverlayHide = () => {
      if (!ctx.isMiddlePaneOverlay.value)
          return;
      ctx.clearMiddlePaneOverlayHide();
      if (typeof window === 'undefined') {
          ctx.middlePaneOverlayVisible.value = false;
          return;
      }
      ctx.middlePaneOverlayHideTimer = window.setTimeout(() => {
          ctx.middlePaneOverlayHideTimer = null;
          ctx.middlePaneOverlayVisible.value = false;
      }, 140);
  };

  // 快捷创建智能体的链路（createAgentQuickly / submitAgentQuickCreate / submitAgentCreate /
  // openCreatedAgentSettings / buildQuickAgentName）已随用户侧多智能体下线整体移除：
  // 唯一入口 handleSearchCreateAction 没有任何绑定，agentQuickCreateVisible 也没有模板消费，
  // 而 DELETE /agents/{id} 已不在服务端路由表里。
}
  installMessengerControllerWorkspaceOrderUiActions(ctx);
}

function installPart9(ctx: any): void {
// Timeline session operations and file container context-menu actions.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerTimelineFileActions(ctx: MessengerControllerContext): void {
  ctx.restoreTimelineSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const targetSession = ctx.resolveSessionRecordById(targetId);
      const targetAgentId = ctx.resolveSessionAgentId(targetSession);
      await ctx.openAgentSession(targetId, targetAgentId);
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

  ctx.handleTimelineDialogActivateSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const targetSession = ctx.resolveSessionRecordById(targetId);
      const targetAgentId = ctx.resolveSessionAgentId(targetSession);
      // Navigation is independent from execution locks: users may inspect another task
      // while it runs, while send/stop actions remain scoped to the selected session.
      await ctx.restoreTimelineSession(targetId);
  };

  ctx.renameTimelineSession = async (sessionId: string) => {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return;
      const session = ctx.resolveSessionRecordById(targetId);
      const targetAgentId = ctx.resolveSessionAgentId(session);
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
      if (false) {
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

  ctx.openChatWorkspaceBindingDialog = (payload: { containerId?: number; currentPath?: string } = {}) => {
      const nextContainerId = Math.min(10, Math.max(1, Number.parseInt(String(payload.containerId ?? ctx.currentContainerId.value), 10) || ctx.currentContainerId.value));
      ctx.chatWorkspaceBindingCurrentPath.value = String(payload.currentPath || ctx.fileContainerLocalLocation.value || '/').trim() || '/';
      ctx.selectedFileContainerId.value = nextContainerId;
      ctx.chatWorkspaceBindingDialogVisible.value = true;
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
  installMessengerControllerTimelineFileActions(ctx);
}

function installPart10(ctx: any): void {
// File container selection, tool catalog loading, organization units, and active agent refresh helpers.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerFileToolSettings(ctx: MessengerControllerContext): void {
  const applyDesktopContainerRootsFromSettings = (settings: unknown) => {
      const data = settings && typeof settings === 'object' ? (settings as Record<string, unknown>) : {};
      const roots = Array.isArray(data.container_roots)
          ? (data.container_roots as Array<Record<string, unknown>>)
          : [];
      const normalized: Record<number, string> = {};
      roots.forEach((item) => {
          const containerId = Math.min(10, Math.max(0, Number.parseInt(String(item.container_id ?? ''), 10) || 0));
          normalized[containerId] = String(item.root || '').trim();
      });
      ctx.desktopContainerRootMap.value = normalized;
  };

  ctx.handleFileContainerMenuCopyId = async () => {
      const target = ctx.fileContainerContextMenu.value.target;
      ctx.closeFileContainerMenu();
      if (!target)
          return;
      const copied = await copyText(String(target.id));
      if (copied) {
          ElMessage.success(ctx.t('messenger.files.copyIdSuccess', { id: target.id }));
      }
      else {
          ElMessage.warning(ctx.t('messenger.files.copyIdFailed'));
      }
  };

  ctx.handleFileContainerMenuSettings = () => {
      const target = ctx.fileContainerContextMenu.value.target;
      ctx.closeFileContainerMenu();
      void ctx.openDesktopContainerSettings(target?.id);
  };

  ctx.selectContainer = (containerId: number | 'user') => {
      ctx.closeFileContainerMenu();
      if (containerId === 'user') {
          ctx.fileScope.value = 'user';
          ctx.selectedFileContainerId.value = USER_CONTAINER_ID;
          ctx.fileContainerLatestUpdatedAt.value = 0;
          ctx.fileContainerEntryCount.value = 0;
          ctx.sessionHub.setSection('files');
          return;
      }
      const parsed = Math.min(10, Math.max(1, Number(containerId) || 1));
      const target = ctx.agentFileContainers.value.find((item) => item.id === parsed);
      if (!target) {
          ElMessage.warning(ctx.t('messenger.files.agentContainerEmpty'));
          return;
      }
      ctx.fileScope.value = 'agent';
      ctx.selectedFileContainerId.value = parsed;
      ctx.fileContainerLatestUpdatedAt.value = 0;
      ctx.fileContainerEntryCount.value = 0;
      ctx.sessionHub.setSection('files');
  };

  ctx.openContainerFromRightDock = (containerId: number) => {
      const normalized = Math.min(10, Math.max(1, Number.parseInt(String(containerId || 1), 10) || 1));
      ctx.switchSection('files');
      ctx.selectContainer(normalized === USER_CONTAINER_ID ? 'user' : normalized);
  };

  ctx.openContainerSettingsFromRightDock = (containerId: number) => {
      ctx.openContainerFromRightDock(containerId);
      void ctx.openDesktopContainerSettings(containerId);
  };

  ctx.handleFileWorkspaceStats = (payload: unknown) => {
      const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      ctx.fileContainerEntryCount.value = Math.max(0, Number(source.entryCount || 0));
      ctx.fileContainerLatestUpdatedAt.value = ctx.normalizeTimestamp(source.latestUpdatedAt);
      ctx.fileLifecycleNowTick.value = Date.now();
  };

  ctx.handleDesktopContainerRootsChange = (roots: Record<number, string>) => {
      const normalized: Record<number, string> = {};
      Object.entries(roots || {}).forEach(([key, value]) => {
          const containerId = Math.min(10, Math.max(0, Number.parseInt(String(key), 10) || 0));
          normalized[containerId] = String(value || '').trim();
      });
      ctx.desktopContainerRootMap.value = normalized;
  };



  ctx.normalizeToolEntry = (item: unknown): ToolEntry | null => {
      if (!item)
          return null;
      if (typeof item === 'string') {
          const name = item.trim();
          if (!name)
              return null;
          return { name, displayName: name, description: '', ownerId: '', source: {} };
      }
      const source = item as Record<string, unknown>;
      const name = String(source.runtime_name || source.runtimeName || source.name || source.tool_name || source.toolName || source.id || '').trim();
      if (!name)
          return null;
      const displayName = String(source.display_name || source.displayName || source.title || source.label || name).trim() || name;
      return {
          name,
          displayName,
          description: String(source.description || '').trim(),
          ownerId: String(source.owner_id || source.ownerId || '').trim(),
          source
      };
  };

  ctx.loadToolsCatalog = async (options: {
      silent?: boolean;
  } = {}) => {
      const loadVersion = ++ctx.toolsCatalogLoadVersion;
      const manageLoading = !options.silent || !ctx.toolsCatalogLoaded.value || ctx.toolsCatalogLoading.value;
      if (manageLoading) {
          ctx.toolsCatalogLoading.value = true;
      }
      try {
          const payload = ((await loadUserToolsCatalogCache()) || {}) as Record<string, unknown>;
          if (loadVersion !== ctx.toolsCatalogLoadVersion) {
              return;
          }
          ctx.builtinTools.value = (Array.isArray(payload.builtin_tools) ? payload.builtin_tools : [])
              .map((item) => ctx.normalizeToolEntry(item))
              .filter(Boolean) as ToolEntry[];
          ctx.mcpTools.value = (Array.isArray(payload.mcp_tools) ? payload.mcp_tools : [])
              .map((item) => ctx.normalizeToolEntry(item))
              .filter(Boolean) as ToolEntry[];
          ctx.skillTools.value = (Array.isArray(payload.skills) ? payload.skills : [])
              .map((item) => ctx.normalizeToolEntry(item))
              .filter(Boolean) as ToolEntry[];
          ctx.knowledgeTools.value = (Array.isArray(payload.knowledge_tools) ? payload.knowledge_tools : [])
              .map((item) => ctx.normalizeToolEntry(item))
              .filter(Boolean) as ToolEntry[];
          ctx.toolsCatalogLoaded.value = true;
      }
      catch (error) {
          if (loadVersion !== ctx.toolsCatalogLoadVersion) {
              return;
          }
          showApiError(error, ctx.t('toolManager.loadFailed'));
      }
      finally {
          if (manageLoading && loadVersion === ctx.toolsCatalogLoadVersion) {
              ctx.toolsCatalogLoading.value = false;
          }
      }
  };



  ctx.selectToolCategory = (category: 'admin' | 'mcp' | 'skills' | 'knowledge') => {
      ctx.selectedToolCategory.value = category;
  };

  ctx.toolCategoryLabel = (category: string) => {
      if (category === 'admin')
          return ctx.t('messenger.tools.adminTitle');
      if (category === 'mcp')
          return ctx.t('toolManager.system.mcp');
      if (category === 'skills')
          return ctx.t('toolManager.system.skills');
      if (category === 'knowledge')
          return ctx.t('toolManager.system.knowledge');
      return category;
  };
}
  installMessengerControllerFileToolSettings(ctx);
}

function installPart11(ctx: any): void {
// Conversation titles, page waiting state, chat footer state, and dismissed conversation persistence.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerPanelSummaries(ctx: MessengerControllerContext): void {
  ctx.activeConversationTitle = computed(() => {
      const identity = ctx.activeConversation.value;
      if (!identity)
          return ctx.t('messenger.empty.noConversation');
      if (identity.kind === 'agent') {
          return ctx.activeAgentName.value;
      }
      return ctx.t('messenger.empty.noConversation');
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
      return ctx.t('messenger.direct.subtitle');
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
      return ctx.resolveEffectiveSessionBusy(sessionId, ctx.resolveActiveAgentRenderableMessageRecords());
  });

  ctx.buildActiveSessionBusyDebugSnapshot = () => {
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const runtimeStatus = activeSessionId
          ? ctx.resolveSessionRuntimeStatus(activeSessionId)
          : '';
      const loadingBySession = activeSessionId ? ctx.resolveSessionLoadingFlag(activeSessionId) : false;
      const messages = ctx.resolveActiveAgentRenderableMessageRecords();
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
      () => ctx.resolveActiveAgentRenderableMessageRecords().length,
      () => Boolean(ctx.isMessengerInteractionBlocked.value)
  ], () => {
      chatDebugLog('messenger.busy', 'snapshot-change', ctx.buildActiveSessionBusyDebugSnapshot());
  }, { immediate: true });



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
  installMessengerControllerPanelSummaries(ctx);
}

function installPart12(ctx: any): void {
// Runtime busy state, prompt ability summaries, right-dock skills, file containers, and settings targets.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerRuntimeToolLists(ctx: MessengerControllerContext): void {
  ctx.pendingApprovalAgentIdSet = computed(() => {
      const approvals = Array.isArray(ctx.chatStore.pendingApprovals) ? ctx.chatStore.pendingApprovals : [];
      const result = new Set<string>();
      if (!approvals.length) {
          return result;
      }
      const sessionAgentMap = ctx.buildSessionAgentMap();
      approvals.forEach((item) => {
          const sessionId = String((item as Record<string, unknown>)?.session_id || '').trim();
          if (!sessionId)
              return;
          const fromMap = sessionAgentMap.get(sessionId);
          if (fromMap) {
              result.add(fromMap);
              return;
          }
          if (sessionId === String(ctx.chatStore.activeSessionId || '').trim()) {
              result.add(ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || DEFAULT_AGENT_KEY));
          }
      });
      return result;
  });

  ctx.isSessionBusy = (sessionId: unknown): boolean => Boolean(ctx.chatStore.isSessionBusy?.(sessionId));

  ctx.TERMINAL_RUNTIME_STATUS_SET = TERMINAL_SESSION_RUNTIME_STATUS_SET;
  ctx.WAITING_RUNTIME_STATUS_SET = WAITING_SESSION_RUNTIME_STATUS_SET;

  ctx.resolveSessionRuntimeStatus = (sessionId: string): string => String(ctx.chatStore.sessionRuntimeStatus?.(sessionId) || '')
      .trim()
      .toLowerCase();

  ctx.resolveSessionLoadingFlag = (sessionId: string): boolean => {
      const loadingBySession = (ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession
          : {}) as Record<string, unknown>;
      return Boolean(loadingBySession[sessionId]);
  };

  // Session state is reduced once in the store. Reading toolbar/list status must
  // not materialize and scan every cached transcript during a render.
  ctx.resolveEffectiveSessionBusy = (sessionId: unknown): boolean => ctx.isSessionBusy(sessionId);

  ctx.activeMessengerSessionBusy = computed(() => {
      const _projectionVersion = ctx.chatStore.runtimeProjectionVersion;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return false;
      return ctx.resolveEffectiveSessionBusy(sessionId);
  });

  ctx.waitingAgentIdSet = computed(() => {
      const _projectionVersion = ctx.chatStore.runtimeProjectionVersion;
      const sessionAgentMap = ctx.buildSessionAgentMap();
      const loadingBySession = (ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession
          : {}) as Record<string, unknown>;
      const sessionIds = new Set<string>([
          ...Array.from(sessionAgentMap.keys()),
          ...Object.keys(loadingBySession).map((id) => String(id || '').trim())
      ]);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (activeSessionId) {
          sessionIds.add(activeSessionId);
      }
      const result = new Set<string>();
      sessionIds.forEach((sessionId) => {
          if (!sessionId || !isWaitingMessengerRuntimeStatus(ctx.resolveSessionRuntimeStatus(sessionId)))
              return;
          const mappedAgentId = sessionAgentMap.get(sessionId);
          if (mappedAgentId) {
              result.add(mappedAgentId);
              return;
          }
          if (sessionId === activeSessionId) {
              const fallbackAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId) ||
                  DEFAULT_AGENT_KEY;
              result.add(fallbackAgentId);
          }
      });
      return result;
  });

  ctx.streamingAgentIdSet = computed(() => {
      const _projectionVersion = ctx.chatStore.runtimeProjectionVersion;
      const sessionAgentMap = ctx.buildSessionAgentMap();
      const loadingBySession = (ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession
          : {}) as Record<string, unknown>;
      const sessionIds = new Set<string>([
          ...Array.from(sessionAgentMap.keys()),
          ...Object.keys(loadingBySession).map((id) => String(id || '').trim())
      ]);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (activeSessionId) {
          sessionIds.add(activeSessionId);
      }
      const result = new Set<string>();
      sessionIds.forEach((sessionId) => {
          if (!sessionId)
              return;
          const runtimeStatus = ctx.resolveSessionRuntimeStatus(sessionId);
          if (isWaitingMessengerRuntimeStatus(runtimeStatus))
              return;
          if (!ctx.resolveEffectiveSessionBusy(sessionId))
              return;
          const mappedAgentId = sessionAgentMap.get(sessionId);
          if (mappedAgentId) {
              result.add(mappedAgentId);
              return;
          }
          if (sessionId === activeSessionId) {
              const fallbackAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId) ||
                  DEFAULT_AGENT_KEY;
              result.add(fallbackAgentId);
          }
      });
      return result;
  });

  ctx.resolveCurrentUserScope = (): string => String(ctx.currentUserId.value || '').trim() || 'guest';

  ctx.resolveCurrentUserScopeAliases = (): string[] => {
      const user = ctx.authStore.user as Record<string, unknown> | null;
      if (!user) {
          return ['guest'];
      }
      const rawIds = [user?.id, user?.user_id, user?.username];
      const aliases: string[] = [];
      rawIds.forEach((value) => {
          const normalized = String(value || '').trim();
          if (normalized && !aliases.includes(normalized)) {
              aliases.push(normalized);
          }
      });
      return aliases.length ? aliases : ['guest'];
  };

  ctx.createScopedStorageKeys = (prefix: string): string[] => ctx.resolveCurrentUserScopeAliases().map((scope) => `${prefix}:${scope}`);

  ctx.resolveAgentDraftIdentity = (): string => {
      const identity = ctx.activeConversation.value;
      if (identity?.kind === 'agent') {
          const conversationId = String(identity.id || '').trim();
          if (conversationId)
              return `conversation:${conversationId}`;
          const agentId = ctx.normalizeAgentId(identity.agentId || ctx.activeAgentId.value || ctx.selectedAgentId.value);
          return `draft:${agentId || DEFAULT_AGENT_KEY}`;
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (sessionId)
          return `session:${sessionId}`;
      const draftAgentId = ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.activeAgentId.value || ctx.selectedAgentId.value);
      return `draft:${draftAgentId || DEFAULT_AGENT_KEY}`;
  };

  ctx.agentComposerDraftKey = computed(() => `messenger:agent:${ctx.resolveCurrentUserScope()}:${ctx.resolveAgentDraftIdentity()}`);

  ctx.normalizeAbilityItemName = (item: unknown): string => {
      if (!item)
          return '';
      if (typeof item === 'string')
          return item.trim();
      const source = item as Record<string, unknown>;
      return String(source.name || source.tool_name || source.toolName || source.id || '').trim();
  };

  ctx.buildAbilityAllowedNameSet = (summary: Record<string, unknown>): Set<string> => {
      const names = collectAbilityNames(summary);
      return new Set<string>([...(names.tools || []), ...(names.skills || [])]);
  };

  ctx.normalizeAbilityNameList = (values: unknown): string[] => {
      if (!Array.isArray(values))
          return [];
      const output: string[] = [];
      const seen = new Set<string>();
      values.forEach((item) => {
          const name = String(item || '').trim();
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push(name);
      });
      return output;
  };

  ctx.extractPromptPreviewSelectedAbilityNames = (payload: unknown): string[] => {
      const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      const tooling = source.tooling_preview && typeof source.tooling_preview === 'object'
          ? (source.tooling_preview as Record<string, unknown>)
          : {};
      return ctx.normalizeAbilityNameList(tooling.selected_tool_names);
  };

  ctx.filterAbilitySummaryByNames = (summary: Record<string, unknown>, selectedNames: Set<string>): Record<string, unknown> => {
      const filterList = (list: unknown) => Array.isArray(list)
          ? list.filter((item) => {
              const name = ctx.normalizeAbilityItemName(item);
              return Boolean(name) && selectedNames.has(name);
          })
          : [];
      const filterUnifiedItems = (list: unknown) => Array.isArray(list)
          ? list.filter((item) => {
              if (!item || typeof item !== 'object')
                  return false;
              const source = item as Record<string, unknown>;
              const name = String(source.runtime_name ||
                  source.runtimeName ||
                  source.name ||
                  source.tool_name ||
                  source.toolName ||
                  source.id ||
                  '').trim();
              return Boolean(name) && selectedNames.has(name);
          })
          : [];
      return {
          ...summary,
          builtin_tools: filterList(summary.builtin_tools),
          mcp_tools: filterList(summary.mcp_tools),
          a2a_tools: filterList(summary.a2a_tools),
          knowledge_tools: filterList(summary.knowledge_tools),
          user_tools: filterList(summary.user_tools),
          shared_tools: filterList(summary.shared_tools),
          skills: filterList(summary.skills),
          skill_list: filterList(summary.skill_list),
          skillList: filterList(summary.skillList),
          items: filterUnifiedItems(summary.items),
          itemList: filterUnifiedItems(summary.itemList)
      };
  };

  ctx.effectiveAgentToolSummary = computed<Record<string, unknown> | null>(() => {
      const summary = ctx.agentPromptToolSummary.value;
      if (!summary)
          return null;
      const allowedSet = ctx.buildAbilityAllowedNameSet(summary);
      if (!allowedSet.size)
          return summary;
      if (ctx.agentPromptPreviewSelectedNames.value !== null) {
          const selectedNames = new Set<string>();
          ctx.agentPromptPreviewSelectedNames.value.forEach((item) => {
              const name = String(item || '').trim();
              if (name && allowedSet.has(name)) {
                  selectedNames.add(name);
              }
          });
          return ctx.filterAbilitySummaryByNames(summary, selectedNames);
      }
      const activeAgentProfile = ctx.activeAgentId.value === DEFAULT_AGENT_KEY
          ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)
          : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
              (ctx.activeAgent.value as Record<string, unknown> | null));
      const agentDefaults = ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile));
      const sourceOverrides = agentDefaults;
      if (sourceOverrides.some((item) => String(item || '').trim() === AGENT_TOOL_OVERRIDE_NONE)) {
          return ctx.filterAbilitySummaryByNames(summary, new Set<string>());
      }
      const selectedNames = new Set<string>();
      sourceOverrides.forEach((item) => {
          const name = String(item || '').trim();
          if (name && allowedSet.has(name)) {
              selectedNames.add(name);
          }
      });
      return ctx.filterAbilitySummaryByNames(summary, selectedNames);
  });

  ctx.activeAgentPromptPreviewHtml = computed(() => renderSystemPromptHighlight(ctx.activeAgentPromptPreviewText.value, (ctx.effectiveAgentToolSummary.value || {}) as Record<string, unknown>));

  ctx.agentAbilitySections = computed(() => {
      const groups = collectAbilityGroupDetails((ctx.effectiveAgentToolSummary.value || {}) as Record<string, unknown>);
      return [
          {
              key: 'skills',
              kind: 'skill',
              title: ctx.t('toolManager.system.skills'),
              emptyText: ctx.t('chat.ability.emptySkills'),
              items: groups.skills
          },
          {
              key: 'mcp',
              kind: 'tool',
              title: ctx.t('toolManager.system.mcp'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.mcp
          },
          {
              key: 'knowledge',
              kind: 'tool',
              title: ctx.t('toolManager.system.knowledge'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.knowledge
          },
          {
              key: 'a2a',
              kind: 'tool',
              title: ctx.t('toolManager.system.a2a'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.a2a
          },
          {
              key: 'builtin',
              kind: 'tool',
              title: ctx.t('toolManager.system.builtin'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.builtin
          }
      ].filter((section) => section.items.length > 0);
  });

  ctx.hasAgentAbilitySummary = computed(() => ctx.agentAbilitySections.value.some((section) => section.items.length > 0));

  ctx.normalizeRightDockSkillCatalog = (list: unknown): RightDockSkillCatalogItem[] => {
      if (!Array.isArray(list))
          return [];
      const output: RightDockSkillCatalogItem[] = [];
      const seen = new Set<string>();
      list.forEach((item) => {
          if (!item || typeof item !== 'object')
              return;
          const source = item as Record<string, unknown>;
          const name = String(source.name || source.tool_name || source.toolName || source.id || '').trim();
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push({
              name,
              description: String(source.description || source.desc || source.summary || '').trim(),
              path: String(source.path || '').trim(),
              source: String(source.source || '').trim().toLowerCase(),
              builtin: Boolean(source.builtin),
              readonly: Boolean(source.readonly)
          });
      });
      return output;
  };

  ctx.normalizeRightDockSkillSummaryItems = (list: unknown): Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> => {
      if (!Array.isArray(list))
          return [];
      const output: Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> = [];
      const seen = new Set<string>();
      list.forEach((item) => {
          if (!item || typeof item !== 'object')
              return;
          const source = item as Record<string, unknown>;
          const name = ctx.normalizeRightDockSkillRuntimeName(String(source.name || source.tool_name || source.toolName || source.id || ''));
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push({
              name,
              description: String(source.description || source.desc || source.summary || '').trim()
          });
      });
      return output;
  };

  ctx.rightDockSkillEnabledNameSet = computed<Set<string>>(() => {
      const activeAgentProfile = ctx.activeAgentId.value === DEFAULT_AGENT_KEY
          ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)
          : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
              (ctx.activeAgent.value as Record<string, unknown> | null));
      const selectedByProfile = ctx.normalizeRightDockSkillNameList(ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile)));
      return new Set(selectedByProfile);
  });

  ctx.rightDockSkillItems = computed<RightDockSkillItem[]>(() => {
      const enabledSet = ctx.rightDockSkillEnabledNameSet.value;
      const merged = new Map<string, RightDockSkillItem>();
      ctx.rightDockSkillCatalog.value.forEach((item) => {
          merged.set(item.name, {
              name: item.name,
              description: item.description,
              enabled: enabledSet.has(item.name)
          });
      });
      const allSkills = collectAbilityDetails((ctx.agentPromptToolSummary.value || {}) as Record<string, unknown>);
      ctx.normalizeRightDockSkillSummaryItems(allSkills.skills).forEach((item) => {
          const existing = merged.get(item.name);
          if (existing) {
              if (!existing.description && item.description) {
                  existing.description = item.description;
              }
              return;
          }
          merged.set(item.name, {
              name: item.name,
              description: item.description,
              enabled: enabledSet.has(item.name)
          });
      });
      return Array.from(merged.values()).sort((left, right) => left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' }));
  });

  ctx.rightDockEnabledSkills = computed<RightDockSkillItem[]>(() => ctx.rightDockSkillItems.value.filter((item) => item.enabled));

  ctx.rightDockDisabledSkills = computed<RightDockSkillItem[]>(() => ctx.rightDockSkillItems.value.filter((item) => !item.enabled));

  ctx.rightDockSkillsLoading = computed(() => ctx.rightDockSkillCatalogLoading.value && ctx.rightDockSkillItems.value.length === 0);

  ctx.rightDockSelectedSkill = computed<RightDockSkillCatalogItem | null>(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      if (!name)
          return null;
      return ctx.rightDockSkillCatalog.value.find((item) => item.name === name) || null;
  });

  ctx.rightDockSkillDialogTitle = computed(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      return name ? `技能 skill · ${name}` : '技能 skill';
  });

  ctx.rightDockSkillDialogPath = computed(() => {
      const path = String(ctx.rightDockSkillContentPath.value || ctx.rightDockSelectedSkill.value?.path || '').trim();
      return path || 'SKILL.md';
  });

  ctx.rightDockSelectedSkillEnabled = computed(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      if (!name)
          return false;
      return ctx.rightDockSkillEnabledNameSet.value.has(name);
  });

  ctx.currentContainerId = computed(() => {
      const source = ctx.activeAgentId.value === DEFAULT_AGENT_KEY
          ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null) || (ctx.activeAgent.value as Record<string, unknown> | null)
          : ctx.activeAgent.value as Record<string, unknown> | null;
      const parsed = Number.parseInt(String(source?.sandbox_container_id ?? 1), 10);
      if (!Number.isFinite(parsed))
          return 1;
      return Math.min(10, Math.max(1, parsed));
  });

  ctx.normalizeSandboxContainerId = (value: unknown): number => {
      const parsed = Number.parseInt(String(value ?? 1), 10);
      if (!Number.isFinite(parsed))
          return 1;
      return Math.min(10, Math.max(1, parsed));
  };

  ctx.agentFileContainers = computed<AgentFileContainer[]>(() => {
      const buckets = new Map<number, {
          agentIds: string[];
          agentNames: string[];
      }>();
      const seenAgentIds = new Set<string>();
      const collect = (agent: Record<string, unknown>) => {
          const normalizedId = ctx.normalizeAgentId(agent?.id);
          if (seenAgentIds.has(normalizedId))
              return;
          seenAgentIds.add(normalizedId);
          const containerId = ctx.normalizeSandboxContainerId(agent?.sandbox_container_id);
          const target = buckets.get(containerId) || { agentIds: [], agentNames: [] };
          target.agentIds.push(normalizedId);
          target.agentNames.push(String(agent?.name || normalizedId));
          buckets.set(containerId, target);
      };
      const defaultProfile = ctx.defaultAgentProfile.value as Record<string, unknown> | null;
      collect({
          id: DEFAULT_AGENT_KEY,
          name: String(defaultProfile?.name || ctx.t('messenger.defaultAgent')),
          sandbox_container_id: defaultProfile?.sandbox_container_id ?? 1
      });
      ctx.ownedAgents.value.forEach((item) => collect(item as Record<string, unknown>));
      return AGENT_CONTAINER_IDS.map((id) => {
          const bucket = buckets.get(id) || { agentIds: [], agentNames: [] };
          const names = bucket.agentNames.filter(Boolean);
          const preview = names.length === 0
              ? ctx.t('messenger.files.unboundAgentContainer')
              : names.length <= 2
                  ? names.join(' / ')
                  : `${names.slice(0, 2).join(' / ')} +${names.length - 2}`;
          const primaryAgentId = bucket.agentIds.find((agentId) => agentId !== DEFAULT_AGENT_KEY) || bucket.agentIds[0] || '';
          return {
              id,
              agentIds: bucket.agentIds,
              agentNames: names,
              preview,
              primaryAgentId
          };
      });
  });

  ctx.boundAgentFileContainers = computed(() => ctx.agentFileContainers.value.filter((item) => item.agentNames.length > 0));

  ctx.unboundAgentFileContainers = computed(() => ctx.agentFileContainers.value.filter((item) => item.agentNames.length === 0));

  ctx.selectedAgentFileContainer = computed(() => ctx.agentFileContainers.value.find((item) => item.id === ctx.selectedFileContainerId.value) || null);

  ctx.selectedFileAgentIdForApi = computed(() => {
      if (ctx.fileScope.value !== 'agent')
          return '';
      const target = ctx.selectedAgentFileContainer.value?.primaryAgentId || '';
      if (!target || target === DEFAULT_AGENT_KEY)
          return '';
      return target;
  });

  ctx.selectedFileContainerAgentLabel = computed(() => {
      if (ctx.fileScope.value !== 'agent')
          return ctx.currentUsername.value;
      const names = ctx.selectedAgentFileContainer.value?.agentNames || [];
      if (!names.length)
          return ctx.t('common.none');
      if (names.length <= 3)
          return names.join(' / ');
      return `${names.slice(0, 3).join(' / ')} +${names.length - 3}`;
  });

  ctx.resolveWorkspaceRootPrefix = (): {
      root: string;
      separator: '/' | '\\';
  } => {
      const runtimeRoot = String(getRuntimeConfig().workspace_root || '')
          .trim()
          .replace(/[\\/]+$/, '');
      const root = runtimeRoot || '/workspaces';
      return {
          root,
          separator: root.includes('\\') ? '\\' : '/'
      };
  };

  ctx.withTrailingSeparator = (path: string): string => {
      const trimmed = String(path || '').trim();
      if (!trimmed)
          return '';
      const separator = trimmed.includes('\\') ? '\\' : '/';
      if (trimmed.endsWith('/') || trimmed.endsWith('\\')) {
          return trimmed;
      }
      return `${trimmed}${separator}`;
  };

  ctx.resolveWorkspaceScopeSuffix = (): string => {
      const userId = String(ctx.currentUserId.value || '').trim() || 'anonymous';
      if (ctx.fileScope.value === 'user' || ctx.selectedFileContainerId.value === USER_CONTAINER_ID) {
          return userId;
      }
      return `${userId}__c__${ctx.selectedFileContainerId.value}`;
  };

  ctx.fileContainerCloudLocation = computed(() => {
      const { root } = ctx.resolveWorkspaceRootPrefix();
      const scope = ctx.resolveWorkspaceScopeSuffix();
      return `${root.replace(/\\/g, '/')}/${scope}/`;
  });

  ctx.fileContainerLocalLocation = computed(() => {
      if (!false) {
          return '';
      }
      const containerId = ctx.fileScope.value === 'user' || ctx.selectedFileContainerId.value === USER_CONTAINER_ID
          ? USER_CONTAINER_ID
          : ctx.selectedFileContainerId.value;
      const mapped = String(ctx.desktopContainerRootMap.value[containerId] || '').trim();
      if (mapped) {
          return ctx.withTrailingSeparator(mapped);
      }
      const { root, separator } = ctx.resolveWorkspaceRootPrefix();
      const scope = ctx.resolveWorkspaceScopeSuffix();
      return `${root}${separator}${scope}${separator}`;
  });

  ctx.workspacePanelKey = computed(() => `${ctx.fileScope.value}:${ctx.selectedFileContainerId.value}:${ctx.selectedFileAgentIdForApi.value || 'default'}`);

  ctx.fileContainerContextMenuStyle = computed(() => ({
      left: `${ctx.fileContainerContextMenu.value.x}px`,
      top: `${ctx.fileContainerContextMenu.value.y}px`
  }));

  ctx.showAgentSettingsPanel = computed(() => shouldShowAgentSettingsPanelForSection(ctx.sessionHub.activeSection));

  ctx.settingsAgentId = computed(() => {
      if (ctx.sessionHub.activeSection === 'agents') {
          return ctx.normalizeAgentId(ctx.selectedAgentId.value);
      }
      if (ctx.isAgentConversationActive.value) {
          return ctx.normalizeAgentId(ctx.activeAgentId.value);
      }
      return '';
  });

  ctx.settingsAgentIdForPanel = computed(() => ctx.normalizeAgentId(ctx.settingsAgentId.value));

  ctx.isSettingsDefaultAgentReadonly = computed(() => false);

  ctx.settingsAgentIdForApi = computed(() => {
      const value = ctx.normalizeAgentId(ctx.settingsAgentId.value);
      return value === DEFAULT_AGENT_KEY ? '' : value;
  });

  ctx.settingsRuntimeAgentIdForApi = computed(() => {
      const value = ctx.normalizeAgentId(ctx.settingsAgentId.value);
      if (value === DEFAULT_AGENT_KEY) {
          return '__default__';
      }
      return value;
  });

  ctx.showChatSettingsView = computed(() => ctx.sessionHub.activeSection !== 'messages');


  ctx.settingsPanelRenderKey = computed(() => ['settings', ctx.sessionHub.activeSection].join(':'));

  ctx.routeSectionIntent = computed<MessengerSection>(() => {
      if (false && ctx.desktopInitialSectionPinned.value) {
          return ctx.sessionHub.activeSection;
      }
      return resolveSectionFromRoute(ctx.route.path, ctx.route.query.section);
  });

  ctx.routeSettingsPanelModeIntent = computed<SettingsPanelMode>(() => ctx.resolveRouteSettingsPanelMode(ctx.route.path, ctx.route.query.panel, false));

  ctx.showHelpManualWaitingOverlay = computed(() => ctx.sessionHub.activeSection === 'more' &&
      ctx.settingsPanelMode.value === 'help-manual' &&
      ctx.helpManualLoading.value);

  ctx.suppressMessengerPageWaitingOverlay = computed(() => (ctx.routeSectionIntent.value === 'agents' &&
      ctx.agentSettingMode.value === 'agent' &&
      !ctx.showAgentGridOverview.value) ||
      (ctx.routeSectionIntent.value === 'more' &&
          ctx.routeSettingsPanelModeIntent.value === 'help-manual') ||
      ctx.showHelpManualWaitingOverlay.value);

  ctx.showChatComposerFooter = computed(() => {
      if (ctx.sessionHub.activeSection !== 'messages') {
          return false;
      }
      return !ctx.showChatSettingsView.value &&
          ['agent', 'world'].includes(String(ctx.resolvedMessageConversationKind.value || ''));
  });

  ctx.filteredOwnedAgents = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return ctx.ownedAgents.value.filter((agent) => ctx.matchesAgentHiveSelection(agent) && ctx.matchesAgentKeyword(agent, text));
  });

  ctx.fullPrimaryAgentList = computed(() => {
      const items: Array<Record<string, unknown>> = [];
      if (ctx.showDefaultAgentEntry.value) {
          items.push(buildDefaultAgentOverviewSource({
              profile: ctx.defaultAgentProfile.value as Record<string, unknown> | null,
              defaultAgentKey: DEFAULT_AGENT_KEY,
              defaultName: ctx.t('messenger.defaultAgent'),
              defaultDescription: ctx.t('messenger.defaultAgentDesc')
          }));
      }
      return [...items, ...ctx.ownedAgents.value];
  });

  ctx.orderedOwnedAgentsState = usePersistentStableListOrder(ctx.fullPrimaryAgentList, {
      getKey: (agent) => ctx.normalizeAgentId(agent?.id),
      storageKey: computed(() => `messenger:agents:owned:${ctx.resolveCurrentUserScope()}`),
      storageFallbackKeys: computed(() => ctx.createScopedStorageKeys('messenger:agents:owned'))
  });


  ctx.handleHelpManualLoadingChange = (value: boolean) => {
      ctx.helpManualLoading.value = value === true;
  };

  ctx.filteredOwnedAgentIdSet = computed(() => new Set(ctx.filteredOwnedAgents.value.map((agent) => ctx.normalizeAgentId(agent?.id)).filter(Boolean)));

  ctx.orderedPrimaryAgents = computed(() => ctx.orderedOwnedAgentsState.orderedItems.value.filter((agent) => {
      const agentId = ctx.normalizeAgentId(agent?.id);
      if (!agentId) {
          return false;
      }
      if (agentId === DEFAULT_AGENT_KEY) {
          return ctx.showDefaultAgentEntry.value;
      }
      return ctx.filteredOwnedAgentIdSet.value.has(agentId);
  }));

  ctx.filteredOwnedAgentsOrdered = computed(() => ctx.orderedPrimaryAgents.value.filter((agent) => ctx.normalizeAgentId(agent?.id) !== DEFAULT_AGENT_KEY));

  ctx.visibleAgentIdsForSelection = computed(() => {
      const ids: string[] = [];
      ctx.orderedPrimaryAgents.value.forEach((agent) => {
          const agentId = ctx.normalizeAgentId(agent?.id);
          if (agentId && !ids.includes(agentId)) {
              ids.push(agentId);
          }
      });
      return ids;
  });

  ctx.showAgentGridOverview = computed(() => ctx.sessionHub.activeSection === 'agents' && ctx.agentOverviewMode.value === 'grid');

  watch(() => [ctx.sessionHub.activeSection, ctx.showAgentGridOverview.value] as const, ([section, showGrid]) => {
      if (section !== 'agents' || showGrid) {
          return;
      }
      void preloadAgentSettingsPanels();
      ctx.warmMessengerUserToolsData({
          catalog: true,
          summary: true
      });
  }, { immediate: true });

  watch(() => [ctx.sessionHub.activeSection, ctx.settingsPanelMode.value] as const, ([section, panelMode]) => {
      if (section === 'more' && panelMode === 'help-manual') {
          ctx.helpManualLoading.value = true;
          return;
      }
      ctx.helpManualLoading.value = false;
  }, { immediate: true });

  ctx.agentOverviewCards = computed<AgentOverviewCard[]>(() => {
      const cards: AgentOverviewCard[] = [];
      const seen = new Set<string>();
      const pushCard = (agent: Record<string, unknown>, options: {
          shared?: boolean;
          isDefault?: boolean;
      } = {}) => {
          const id = ctx.normalizeAgentId(agent?.id || DEFAULT_AGENT_KEY);
          if (!id || seen.has(id))
              return;
          seen.add(id);
          const containerId = ctx.normalizeSandboxContainerId(agent?.sandbox_container_id);
          const abilityCounts = resolveAgentOverviewAbilityCounts(agent);
          cards.push({
              id,
              name: String(agent?.name || id),
              icon: agent?.icon,
              description: String(agent?.description || ''),
              shared: options.shared === true,
              isDefault: options.isDefault === true,
              runtimeState: ctx.resolveAgentRuntimeState(id),
              hasCron: ctx.hasCronTask(id),
              hasChannelBinding: ctx.channelBoundAgentIds.value.has(id),
              containerId,
              userRounds: ctx.resolveAgentUserRounds(id),
              skillCount: abilityCounts.skillCount,
              mcpCount: abilityCounts.mcpCount
          });
      };
      ctx.orderedPrimaryAgents.value.forEach((item) => pushCard(item as Record<string, unknown>, {
          isDefault: ctx.normalizeAgentId((item as Record<string, unknown>)?.id) === DEFAULT_AGENT_KEY
      }));
      return cards;
  });

  ctx.normalizeUiFontSize = (value: unknown): number => {
      // 未设置时必须是 14（方案 §11.2 的正文字号，桌面端 `font-lg = 14px * font-scale`）。
      // 注意 `Number(null) === 0` 且 `Number('') === 0`，旧写法把它们当成"用户选了 0"
      // 再夹到 12，于是**新用户**（localStorage 还没有这个键）整屏按 0.857 缩放渲染成 12px。
      const raw = typeof value === 'string' ? value.trim() : value;
      if (raw === null || raw === undefined || raw === '')
          return 14;
      const parsed = Number(raw);
      if (!Number.isFinite(parsed) || parsed <= 0)
          return 14;
      return Math.min(20, Math.max(12, Math.round(parsed)));
  };

  ctx.normalizeMessengerSendKey = (value: unknown): MessengerSendKeyMode => (() => {
      const text = String(value || '').trim().toLowerCase();
      if (text === 'enter')
          return 'enter';
      if (text === 'none' || text === 'off' || text === 'disabled')
          return 'none';
      return 'enter';
  })();

  ctx.applyUiFontSize = (value: number) => {
      if (typeof document === 'undefined')
          return;
      const normalized = ctx.normalizeUiFontSize(value);
      document.documentElement.style.setProperty('--messenger-font-size', `${normalized}px`);
      document.documentElement.style.setProperty('--messenger-font-scale', String(normalized / 14));
  };
}
  installMessengerControllerRuntimeToolLists(ctx);
}

function installPart13(ctx: any): void {
// File lifecycle text, right dock visibility, right-panel session history, and timeline preview caching.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerRightDockSessionRuntime(ctx: MessengerControllerContext): void {
  ctx.fileContainerLifecycleText = computed(() => resolveFileContainerLifecycleText({
      t: ctx.t
  }));

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

  ctx.showRightDock = computed(() => ctx.showAgentRightDock.value);

  ctx.showRightAgentPanels = computed(() => ctx.showAgentRightDock.value);

  watch(() => ctx.showAgentRightDock.value, (visible) => {
      if (!visible) {
          return;
      }
      ctx.warmMessengerUserToolsData({
          skills: false,
          summary: true
      });
  }, { immediate: true });

  ctx.RIGHT_DOCK_EDGE_HOVER_THRESHOLD = 84;

  ctx.cachedMessengerRootRight = 0;

  ctx.cachedMessengerRootWidth = 0;

  ctx.lastMessengerLayoutDebugSignature = '';

  watch(() => ctx.showRightDock.value, (visible) => {
      if (!visible) {
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
      // The messages dock follows the active conversation agent. Settings keeps
      // its own selection, but must never leak into the task list while the
      // user is working in another section.
      const source = ctx.sessionHub.activeSection === 'agents'
          ? ctx.settingsAgentId.value
          : ctx.activeAgentId.value;
      return ctx.normalizeAgentId(source);
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
           const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
           // Materializing every cached thread is expensive while the timeline is open.
           // Only the active thread needs a live projection; other rows use summaries.
           if (sessionId === activeSessionId) {
               const cachedMessages = ctx.chatStore.getCachedSessionMessages(sessionId);
               if (Array.isArray(cachedMessages) && cachedMessages.length > 0) {
                   const preview = ctx.extractLatestConversationPreview(cachedMessages as unknown[]);
                   const previewTimestamp = ctx.resolveLatestConversationMessageTimestamp(cachedMessages as unknown[]);
                   if (preview && (!fieldPreview || previewTimestamp >= fieldTimestamp)) {
                       return preview;
                   }
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

  ctx.rightPanelSessionHistory = computed(() => ctx.showAgentRightDock.value
      ? buildTaskList(ctx.chatStore.sessions || [], ctx.rightPanelAgentId.value, ctx.t('chat.newSession'))
      : []);
}
  installMessengerControllerRightDockSessionRuntime(ctx);
}

function installPart14(ctx: any): void {
// Markdown rendering, world voice playback, assistant resume, copy actions, and world message identity.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerMessageMarkdownVoice(ctx: MessengerControllerContext): void {
  ctx.trimMarkdownCache = () => {
      while (ctx.markdownCache.size > ctx.MARKDOWN_CACHE_LIMIT) {
          const oldestKey = ctx.markdownCache.keys().next().value;
          if (!oldestKey)
              break;
          ctx.markdownCache.delete(oldestKey);
      }
  };

  ctx.renderMessageMarkdown = (cacheKey: string, content: unknown, options: {
      streaming?: boolean;
      resolveWorkspacePath?: (rawPath: string) => string;
      message?: Record<string, unknown>;
  } = {}): string => {
      const source = prepareMessageMarkdownContent(content, options.message);
      const normalizedKey = String(cacheKey || '').trim();
      if (!source) {
          if (normalizedKey) {
              ctx.markdownCache.delete(normalizedKey);
          }
          return '';
      }
      if (!normalizedKey) {
          return renderMarkdown(source, { resolveWorkspacePath: options.resolveWorkspacePath });
      }
      const cached = ctx.markdownCache.get(normalizedKey);
      if (cached && cached.source === source) {
          return cached.html;
      }
      const now = Date.now();
      if (options.streaming && cached && now - cached.updatedAt < ctx.MARKDOWN_STREAM_THROTTLE_MS) {
          return cached.html;
      }
      const html = renderMarkdown(source, { resolveWorkspacePath: options.resolveWorkspacePath });
      ctx.markdownCache.set(normalizedKey, { source, html, updatedAt: now });
      ctx.trimMarkdownCache();
      return html;
  };

  ctx.renderAgentMarkdown = (message: Record<string, unknown>, index: number): string => {
      const cacheKey = `agent:${String(ctx.sessionHub.activeConversationKey || '')}:${ctx.resolveAgentMessageKey(message, index)}:c${ctx.currentContainerId.value}`;
      const streaming = Boolean(message?.stream_incomplete) ||
          Boolean(message?.workflowStreaming) ||
          Boolean(message?.reasoningStreaming);
      return ctx.renderMessageMarkdown(cacheKey, buildAssistantDisplayContent(message, ctx.t), {
          streaming,
          resolveWorkspacePath: ctx.resolveAgentMarkdownWorkspacePath,
          message
      });
  };




  ctx.isWorldVoiceMessage = (message: Record<string, unknown>): boolean => Boolean(ctx.resolveWorldVoicePayloadFromMessage(message));

  ctx.isWorldVoicePlaying = (message: Record<string, unknown>): boolean => ctx.worldVoicePlayingMessageKey.value === ctx.resolveWorldMessageKey(message);

  ctx.isWorldVoiceLoading = (message: Record<string, unknown>): boolean => ctx.worldVoiceLoadingMessageKey.value === ctx.resolveWorldMessageKey(message);




  ctx.resolveWorldVoiceActionLabel = (message: Record<string, unknown>): string => ctx.isWorldVoicePlaying(message) ? ctx.t('messenger.world.voice.pause') : ctx.t('messenger.world.voice.play');

  ctx.shouldShowAgentResumeButton = (message: Record<string, unknown>): boolean => {
      if (String(message?.role || '') !== 'assistant')
          return false;
      if (Boolean(message?.workflowStreaming) || Boolean(message?.reasoningStreaming))
          return false;
      return Boolean(message?.resume_available || message?.slow_client);
  };

  ctx.resumeAgentMessage = async (message: Record<string, unknown>) => {
      if (String(message?.role || '') !== 'assistant')
          return;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return;
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      message.resume_available = false;
      message.slow_client = false;
      ctx.autoStickToBottom.value = true;
      ctx.setRuntimeStateOverride(targetAgentId, 'running', 30000);
      try {
          await ctx.chatStore.resumeStream(sessionId, message, { force: true });
          await ctx.scrollMessagesToBottom();
      }
      catch (error) {
          message.resume_available = true;
          ctx.setRuntimeStateOverride(targetAgentId, 'error', 8000);
          showApiError(error, ctx.t('chat.error.resumeFailed'));
      }
  };

  ctx.copyMessageContent = async (payload: unknown) => {
      const message = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : null;
      const text = prepareMessageMarkdownContent(message?.content ?? payload, message).trim();
      if (!text)
          return;
      const copied = await copyText(text);
      if (copied) {
          ElMessage.success(ctx.t('chat.message.copySuccess'));
      }
      else {
          ElMessage.warning(ctx.t('chat.message.copyFailed'));
      }
  };

  ctx.resolveMessageTtsText = (message: Record<string, unknown>): string => prepareMessageMarkdownContent(message?.content, message).trim();

  ctx.resolveMessageTtsKey = (message: Record<string, unknown>, index = 0, scope = 'agent'): string => {
      const sourceIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
      if (scope === 'world') {
          return `world:${ctx.resolveWorldMessageKey(message)}:${sourceIndex}`;
      }
      return `agent:${ctx.resolveAgentMessageKey(message, sourceIndex)}`;
  };

  ctx.isMessageTtsPlaying = (message: Record<string, unknown>, index = 0, scope = 'agent'): boolean => ctx.messageTtsPlayingKey.value === ctx.resolveMessageTtsKey(message, index, scope);

  ctx.isMessageTtsLoading = (message: Record<string, unknown>, index = 0, scope = 'agent'): boolean => ctx.messageTtsLoadingKey.value === ctx.resolveMessageTtsKey(message, index, scope);

  ctx.resolveMessageTtsActionLabel = (message: Record<string, unknown>, index = 0, scope = 'agent'): string => {
      if (ctx.isMessageTtsLoading(message, index, scope))
          return ctx.t('chat.message.ttsLoading');
      if (ctx.isMessageTtsPlaying(message, index, scope))
          return ctx.t('chat.message.pauseVoice');
      return ctx.t('chat.message.playVoice');
  };

  ctx.ensureMessageTtsPlaybackRuntime = () => {
      if (typeof Audio === 'undefined')
          return null;
      if (ctx.messageTtsPlaybackRuntime)
          return ctx.messageTtsPlaybackRuntime;
      const audio = new Audio();
      audio.preload = 'none';
      audio.addEventListener('ended', () => {
          ctx.messageTtsPlayingKey.value = '';
          if (ctx.messageTtsPlaybackRuntime) {
              ctx.messageTtsPlaybackRuntime.currentMessageKey = '';
          }
      });
      audio.addEventListener('pause', () => {
          if (audio.ended)
              return;
          ctx.messageTtsPlayingKey.value = '';
          if (ctx.messageTtsPlaybackRuntime) {
              ctx.messageTtsPlaybackRuntime.currentMessageKey = '';
          }
      });
      ctx.messageTtsPlaybackRuntime = {
          audio,
          objectUrlCache: new Map<string, string>(),
          currentMessageKey: ''
      };
      return ctx.messageTtsPlaybackRuntime;
  };

  ctx.stopMessageTtsPlayback = () => {
      const runtime = ctx.messageTtsPlaybackRuntime;
      if (!runtime)
          return;
      runtime.audio.pause();
      runtime.audio.removeAttribute('src');
      try {
          runtime.audio.load();
      }
      catch {
      }
      runtime.currentMessageKey = '';
      ctx.messageTtsPlayingKey.value = '';
      ctx.messageTtsLoadingKey.value = '';
  };

  ctx.disposeMessageTtsPlayback = () => {
      const runtime = ctx.messageTtsPlaybackRuntime;
      if (!runtime) {
          ctx.messageTtsPlayingKey.value = '';
          ctx.messageTtsLoadingKey.value = '';
          return;
      }
      ctx.stopMessageTtsPlayback();
      runtime.objectUrlCache.forEach((objectUrl) => URL.revokeObjectURL(objectUrl));
      runtime.objectUrlCache.clear();
      ctx.messageTtsPlaybackRuntime = null;
  };

  ctx.toggleMessageTtsPlayback = async (message: Record<string, unknown>, index = 0, scope = 'agent') => {
      const messageKey = ctx.resolveMessageTtsKey(message, index, scope);
      if (!messageKey || ctx.messageTtsLoadingKey.value === messageKey)
          return;
      const runtime = ctx.ensureMessageTtsPlaybackRuntime();
      if (!runtime) {
          ElMessage.warning(ctx.t('chat.message.ttsUnsupported'));
          return;
      }
      if (runtime.currentMessageKey === messageKey && !runtime.audio.paused) {
          runtime.audio.pause();
          return;
      }
      const text = ctx.resolveMessageTtsText(message);
      if (!text) {
          ElMessage.warning(ctx.t('chat.message.ttsEmpty'));
          return;
      }
      ctx.messageTtsLoadingKey.value = messageKey;
      try {
          let objectUrl = runtime.objectUrlCache.get(messageKey);
          if (!objectUrl) {
              const response = await synthesizeChatTts({
                  text,
                  response_format: 'wav'
              });
              const blob = response?.data as Blob;
              if (!(blob instanceof Blob) || blob.size <= 0) {
                  throw new Error(ctx.t('chat.message.ttsFailed'));
              }
              objectUrl = URL.createObjectURL(blob);
              runtime.objectUrlCache.set(messageKey, objectUrl);
          }
          if (runtime.audio.src !== objectUrl) {
              runtime.audio.pause();
              runtime.audio.src = objectUrl;
          }
          runtime.currentMessageKey = messageKey;
          await runtime.audio.play();
          ctx.messageTtsPlayingKey.value = messageKey;
      }
      catch (error) {
          console.error(error);
          ctx.messageTtsPlayingKey.value = '';
          ElMessage.error(await resolveBlobApiErrorMessage(error, ctx.t('chat.message.ttsFailed')));
      }
      finally {
          if (ctx.messageTtsLoadingKey.value === messageKey) {
              ctx.messageTtsLoadingKey.value = '';
          }
      }
  };

  ctx.isOwnMessage = (message: Record<string, unknown>): boolean => {
      const sender = String(message?.sender_user_id || '').trim();
      const user = ctx.authStore.user as Record<string, unknown> | null;
      const current = String(user?.id || '').trim();
      return Boolean(sender && current && sender === current);
  };

  ctx.resolveWorldMessageSender = (message: Record<string, unknown>): string => {
      const sender = String(message?.sender_user_id || '').trim();
      if (!sender)
          return ctx.t('user.guest');
      const user = ctx.authStore.user as Record<string, unknown> | null;
      if (String(user?.id || '') === sender) {
          return String(user?.username || sender);
      }
      return sender;
  };

  ctx.resolveWorldMessageKey = (message: Record<string, unknown>): string => String(message?.message_id ||
      message?.id ||
      `${message?.sender_user_id || 'peer'}-${message?.created_at || ''}`);



  ctx.resetWorldVoicePlaybackProgress = () => {
      ctx.worldVoicePlaybackCurrentMs.value = 0;
      ctx.worldVoicePlaybackDurationMs.value = 0;
  };

  ctx.syncWorldVoicePlaybackProgress = (audio: HTMLAudioElement) => {
      const currentMs = Number(audio.currentTime);
      ctx.worldVoicePlaybackCurrentMs.value =
          Number.isFinite(currentMs) && currentMs > 0 ? Math.round(currentMs * 1000) : 0;
      const durationMs = Number(audio.duration);
      if (Number.isFinite(durationMs) && durationMs > 0) {
          ctx.worldVoicePlaybackDurationMs.value = Math.round(durationMs * 1000);
      }
  };

  ctx.ensureWorldVoicePlaybackRuntime = (): WorldVoicePlaybackRuntime | null => {
      if (typeof Audio === 'undefined')
          return null;
      if (ctx.worldVoicePlaybackRuntime)
          return ctx.worldVoicePlaybackRuntime;
      const audio = new Audio();
      audio.preload = 'none';
      audio.addEventListener('loadedmetadata', () => {
          ctx.syncWorldVoicePlaybackProgress(audio);
      });
      audio.addEventListener('durationchange', () => {
          ctx.syncWorldVoicePlaybackProgress(audio);
      });
      audio.addEventListener('timeupdate', () => {
          ctx.syncWorldVoicePlaybackProgress(audio);
      });
      audio.addEventListener('ended', () => {
          ctx.resetWorldVoicePlaybackProgress();
          ctx.worldVoicePlayingMessageKey.value = '';
          if (ctx.worldVoicePlaybackRuntime) {
              ctx.worldVoicePlaybackRuntime.currentMessageKey = '';
          }
      });
      audio.addEventListener('pause', () => {
          if (audio.ended)
              return;
          ctx.worldVoicePlaybackCurrentMs.value = 0;
          ctx.worldVoicePlayingMessageKey.value = '';
          if (ctx.worldVoicePlaybackRuntime) {
              ctx.worldVoicePlaybackRuntime.currentMessageKey = '';
          }
      });
      ctx.worldVoicePlaybackRuntime = {
          audio,
          objectUrlCache: new Map<string, string>(),
          currentMessageKey: '',
          currentResourceKey: ''
      };
      return ctx.worldVoicePlaybackRuntime;
  };





  ctx.stopWorldVoicePlayback = () => {
      const runtime = ctx.worldVoicePlaybackRuntime;
      if (!runtime)
          return;
      runtime.audio.pause();
      runtime.currentMessageKey = '';
      ctx.resetWorldVoicePlaybackProgress();
      ctx.worldVoicePlayingMessageKey.value = '';
      ctx.worldVoiceLoadingMessageKey.value = '';
  };

  ctx.disposeWorldVoicePlayback = () => {
      const runtime = ctx.worldVoicePlaybackRuntime;
      if (!runtime) {
          ctx.resetWorldVoicePlaybackProgress();
          return;
      }
      ctx.stopWorldVoicePlayback();
      runtime.currentResourceKey = '';
      runtime.objectUrlCache.forEach((objectUrl) => {
          URL.revokeObjectURL(objectUrl);
      });
      runtime.objectUrlCache.clear();
      runtime.audio.removeAttribute('src');
      try {
          runtime.audio.load();
      }
      catch {
      }
      ctx.resetWorldVoicePlaybackProgress();
      ctx.worldVoicePlaybackRuntime = null;
  };

  ctx.toggleWorldVoicePlayback = async (message: Record<string, unknown>) => {
      if (!ctx.isWorldConversationActive.value)
          return;
      const payload = ctx.resolveWorldVoicePayloadFromMessage(message);
      if (!payload)
          return;
      const messageKey = ctx.resolveWorldMessageKey(message);
      if (!messageKey || ctx.worldVoiceLoadingMessageKey.value === messageKey)
          return;
      const runtime = ctx.ensureWorldVoicePlaybackRuntime();
      if (!runtime) {
          ElMessage.warning(ctx.t('messenger.world.voice.unsupported'));
          return;
      }
      if (runtime.currentMessageKey === messageKey && !runtime.audio.paused) {
          runtime.audio.pause();
          return;
      }
      ctx.worldVoiceLoadingMessageKey.value = messageKey;
      try {
          const { resourceKey, objectUrl } = await ctx.fetchWorldVoiceObjectUrl(message, payload, runtime);
          if (runtime.currentResourceKey !== resourceKey || runtime.audio.src !== objectUrl) {
              runtime.audio.pause();
              runtime.audio.src = objectUrl;
              runtime.currentResourceKey = resourceKey;
          }
          runtime.currentMessageKey = messageKey;
          await runtime.audio.play();
          ctx.syncWorldVoicePlaybackProgress(runtime.audio);
          ctx.worldVoicePlayingMessageKey.value = messageKey;
      }
      catch (error) {
          ctx.worldVoicePlayingMessageKey.value = '';
          showApiError(error, ctx.t('messenger.world.voice.playFailed'));
      }
      finally {
          if (ctx.worldVoiceLoadingMessageKey.value === messageKey) {
              ctx.worldVoiceLoadingMessageKey.value = '';
          }
      }
  };
}
  installMessengerControllerMessageMarkdownVoice(ctx);
}

function installPart15(ctx: any): void {
// Route restoration, bootstrap loading, keyword synchronization, middle-pane overlay syncing, and route-driven view state.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerLifecycleRouteBootstrap(ctx: MessengerControllerContext): void {
  let routeRestorePromise: Promise<void> | null = null;
  let routeRestoreSignature = '';
  const buildRouteRestoreSignature = (): string => {
      const query = ctx.route.query;
      return [
          String(ctx.route.path || '').trim(),
          String(query?.section || '').trim(),
          String(query?.conversation_id || '').trim(),
          String(query?.session_id || '').trim(),
          String(query?.agent_id || '').trim(),
          String(query?.entry || '').trim().toLowerCase()
      ].join('|');
  };
  const runRouteRestore = async (signature: string): Promise<void> => {
      const query = ctx.route.query;
      const querySection = resolveSectionFromRoute(ctx.route.path, query.section);
      const queryAgentId = String(query?.agent_id || '').trim();
      const queryConversationId = String(query?.conversation_id || '').trim();
      if (querySection !== 'messages') {
          ctx.sessionHub.setSection(querySection);
          if (querySection === 'agents' && queryAgentId) {
              ctx.agentOverviewMode.value = 'detail';
              ctx.selectedAgentId.value = ctx.normalizeAgentId(queryAgentId);
          }
          return;
      }
      if (queryConversationId) {
          // No surface resolves `conversation_id` any more: drop the stale query
          // so boot can continue instead of looping on an unknown conversation.
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.conversation_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
          return;
      }
      const querySessionId = String(query?.session_id || '').trim();
      if (querySessionId) {
          const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (activeSessionId === querySessionId && ctx.sessionHub.activeConversationKey === `agent:${querySessionId}`) {
              return;
          }
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === querySessionId);
          if (session) {
              await ctx.openAgentSession(querySessionId, ctx.normalizeAgentId(session?.agent_id), {
                  preserveSection: true
              });
              return;
          }
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.session_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
          return;
      }
      const queryEntry = String(query?.entry || '').trim().toLowerCase();
      if (queryAgentId || queryEntry === 'default') {
          const targetAgentId = ctx.normalizeAgentId(queryAgentId || DEFAULT_AGENT_KEY);
          const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
          const activeDraftAgentId = String(ctx.chatStore.draftAgentId || '').trim();
          if (ctx.sessionHub.activeSection === 'messages') {
              if (activeSessionId) {
                  const activeSession = ctx.chatStore.sessions.find((item) => String(item?.id || '') === activeSessionId);
                  if (ctx.resolveSessionAgentId(activeSession || activeSessionId, activeDraftAgentId) === targetAgentId) {
                      return;
                  }
              }
              if (!activeSessionId && activeDraftAgentId && ctx.normalizeAgentId(activeDraftAgentId) === targetAgentId) {
                  return;
              }
          }
          await ctx.openAgentById(targetAgentId, { preserveSection: true });
          return;
      }
      const preferredSection = false
          ? ('messages' as MessengerSection)
          : resolveSectionFromRoute(ctx.route.path, query.section);
      if (preferredSection === 'messages') {
          const recent = ctx.resolveRecentAgentSelection?.();
          const recentSessionId = String(recent?.sessionId || '').trim();
          const recentAgentRaw = String(recent?.agentId || '').trim();
          const recentAgentId = recentAgentRaw ? ctx.normalizeAgentId(recentAgentRaw) : '';
          if (recentSessionId) {
              const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === recentSessionId);
              if (session) {
                  await ctx.openAgentSession(recentSessionId, ctx.normalizeAgentId(session?.agent_id || recentAgentId), {
                      preserveSection: true
                  });
                  return;
              }
          }
          if (recentAgentId) {
              await ctx.openAgentById(recentAgentId, { preserveSection: true });
              return;
          }
      }
      ctx.clearMessagePanelWhenConversationEmpty();
  };
  ctx.restoreConversationFromRoute = async () => {
      const signature = buildRouteRestoreSignature();
      if (routeRestorePromise && routeRestoreSignature === signature) {
          return routeRestorePromise;
      }
      routeRestoreSignature = signature;
      routeRestorePromise = runRouteRestore(signature).finally(() => {
          if (routeRestoreSignature === signature) {
              routeRestorePromise = null;
          }
      });
      return routeRestorePromise;
  };

  ctx.bootstrap = async () => {
      ctx.bootLoading.value = true;
      const initialSection = false
          ? ('messages' as MessengerSection)
          : resolveSectionFromRoute(ctx.route.path, ctx.route.query.section);
      const useNonBlockingDesktopBootstrap = shouldUseNonBlockingDesktopMessageBootstrap(
          false,
          initialSection
      );
      let profileAuthDenied = false;
      let profileHydrationPromise: Promise<unknown> = Promise.resolve(ctx.authStore.user);
      if (!ctx.authStore.user && ctx.authStore.token) {

          profileHydrationPromise = ctx.authStore.loadProfile().then((profile) => {

              return profile;
          }).catch((error) => {
              const status = ctx.resolveHttpStatus(error);
              if (ctx.isAuthDeniedStatus(status)) {
                  profileAuthDenied = true;
                  void ctx.authStore.logout();
                  ctx.bootLoading.value = false;
                  ctx.router.replace('/login').catch(() => undefined);
              }

              return null;
          });
          if (!useNonBlockingDesktopBootstrap) {
              await profileHydrationPromise;
              if (profileAuthDenied) {
                  return;
              }
          }
      }
      const deferredShellTasks = [
          {
              run: async () => {
                  await profileHydrationPromise;
                  if (!profileAuthDenied) {
                      await ctx.hydrateCurrentUserAppearance();
                  }
              }
          },
          {
              run: async () => {
                  await profileHydrationPromise;
                  if (!profileAuthDenied) {
                      await ctx.hydrateMessengerOrderPreferences();
                  }
              }
          }
      ];
      if (!useNonBlockingDesktopBootstrap) {
          await Promise.all([ctx.hydrateCurrentUserAppearance(), ctx.hydrateMessengerOrderPreferences()]);
      }
      const initialQuerySessionId = String(ctx.route.query.session_id || '').trim();
      const initialQueryAgentId = String(ctx.route.query.agent_id || '').trim();
      const initialQueryEntry = String(ctx.route.query.entry || '').trim().toLowerCase();
      const sessionListTask = {
          run: () => {
              if (false) {
                  return Promise.resolve();
              }
              return ctx.chatStore.loadSessions({
                  preferCache: true,
                  backgroundRefresh: true,
                  maxCacheAgeMs: 5 * 60 * 1000,
                  traceSource: 'bootstrap'
              });
          }
      };
      const { critical, background } = splitMessengerBootstrapTasks(initialSection, [
          {
              // Agent metadata enriches the navigation but is not needed to
              // render or resume the initial message workspace.
              run: () => ctx.agentStore.loadAgents()
          },
          ...(useNonBlockingDesktopBootstrap
              ? []
              : [{ ...sessionListTask, sections: ['messages'] as MessengerSection[] }]),
          {
              run: () => ctx.loadRunningAgents()
          },
          {
              run: () => ctx.loadAgentUserRounds()
          }
      ]);
      if (useNonBlockingDesktopBootstrap) {
          // The desktop shell is already painted. Keep the global surface
          // interactive while the session list and recent conversation hydrate.
          const sessionListPromise = settleMessengerBootstrapTasks([sessionListTask]);
          ctx.ensureSectionSelection();
          ctx.bootLoading.value = false;

          const scheduleFollowupTasks = () => {
              scheduleMessengerBootstrapBackgroundTasks([
                  ...deferredShellTasks,
                  ...background
              ]);
          };
          void (async () => {
              try {
                  await sessionListPromise;

                  ctx.ensureSectionSelection();
                  if (!false) {
                      await ctx.restoreConversationFromRoute();
                  }
              }
              finally {

                  scheduleFollowupTasks();
              }
          })().catch(() => undefined);
          return;
      }
      await settleMessengerBootstrapTasks(critical);
      ctx.ensureSectionSelection();
      ctx.bootLoading.value = false;
      if (!false) {
          void ctx.restoreConversationFromRoute();
      }
      scheduleMessengerBootstrapBackgroundTasks(background);
  };

  watch(() => ctx.sessionHub.keyword, (value) => {
      const normalized = String(value || '');
      if (ctx.keywordInput.value !== normalized) {
          ctx.keywordInput.value = normalized;
      }
  }, { immediate: true });

  watch(ctx.keywordInput, (value) => {
      const normalized = String(value || '').trimStart();
      if (typeof window === 'undefined') {
          ctx.sessionHub.setKeyword(normalized);
          return;
      }
      ctx.clearKeywordDebounce();
      ctx.keywordDebounceTimer = window.setTimeout(() => {
          ctx.keywordDebounceTimer = null;
          ctx.sessionHub.setKeyword(normalized);
      }, ctx.KEYWORD_INPUT_DEBOUNCE_MS);
  });

  watch(() => [ctx.isEmbeddedChatRoute.value, ctx.isMiddlePaneOverlay.value, ctx.showMiddlePane.value] as const, ([embedded, overlay, visible]) => {
      if (embedded) {
          ctx.clearMiddlePanePrewarm();
          ctx.middlePaneMounted.value = false;
          return;
      }
      if (visible || !overlay) {
          ctx.clearMiddlePanePrewarm();
          ctx.middlePaneMounted.value = true;
          return;
      }
      ctx.scheduleMiddlePanePrewarm();
  }, { immediate: true });

  watch(() => ctx.isMiddlePaneOverlay.value, (overlay) => {
      if (!overlay) {
          ctx.clearMiddlePaneOverlayHide();
          ctx.middlePaneOverlayVisible.value = false;
      }
  }, { immediate: true });

  watch(() => ctx.middlePaneOverlayVisible.value, (visible) => {
      if (visible) {
          ctx.middlePaneMounted.value = true;
          return;
      }
      if (!visible) {
      }
  });

  ctx.syncRouteDrivenMessengerViewState = () => {
      ctx.settingsPanelMode.value = ctx.resolveRouteSettingsPanelMode(ctx.route.path, ctx.route.query.panel, false);
      ctx.sessionHub.setSection(resolveSectionFromRoute(ctx.route.path, ctx.route.query.section));
  };

  ctx.syncRouteDrivenMessengerViewState();

  watch(() => [ctx.route.path, ctx.route.query.section, ctx.route.query.panel, ctx.route.query.helper], ctx.syncRouteDrivenMessengerViewState);
  watch(
      () => [
          ctx.route.path,
          ctx.route.query.section,
          ctx.route.query.session_id,
          ctx.route.query.conversation_id,
          ctx.route.query.agent_id,
          ctx.route.query.entry
      ] as const,
      () => {
          if (false) {
              return;
          }
          if (ctx.bootLoading.value || !String(ctx.route.path || '').includes('/chat')) {
              return;
          }
          if (resolveSectionFromRoute(ctx.route.path, ctx.route.query.section) !== 'messages') {
              return;
          }
          void ctx.restoreConversationFromRoute();
      }
  );
}
  installMessengerControllerLifecycleRouteBootstrap(ctx);
}

function installPart16(ctx: any): void {
// Message viewport runtime wrappers, virtual measurement, scroll controls, and latest assistant layout refresh.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerLifecycleMessageViewport(ctx: MessengerControllerContext): void {
  let lightweightMarkdownStickTimer: ReturnType<typeof setTimeout> | null = null;
  let lastLightweightMarkdownStickAt = 0;
  const LIGHTWEIGHT_MARKDOWN_STICK_MIN_MS = 160;

  const clearLightweightMarkdownStickTimer = () => {
      if (lightweightMarkdownStickTimer !== null) {
          clearTimeout(lightweightMarkdownStickTimer);
          lightweightMarkdownStickTimer = null;
      }
  };

  const scheduleLightweightMarkdownStickToBottom = () => {
      if (typeof window === 'undefined') {
          void ctx.scrollMessagesToBottom();
          return;
      }
      const elapsedMs = Date.now() - lastLightweightMarkdownStickAt;
      const waitMs = Math.max(0, LIGHTWEIGHT_MARKDOWN_STICK_MIN_MS - elapsedMs);
      if (waitMs <= 0) {
          lastLightweightMarkdownStickAt = Date.now();
          void ctx.scrollMessagesToBottom();
          return;
      }
      if (lightweightMarkdownStickTimer !== null)
          return;
      lightweightMarkdownStickTimer = window.setTimeout(() => {
          lightweightMarkdownStickTimer = null;
          lastLightweightMarkdownStickAt = Date.now();
          void ctx.scrollMessagesToBottom();
      }, waitMs);
  };

  onBeforeUnmount(() => {
      clearLightweightMarkdownStickTimer();
  });

  ctx.syncMessageVirtualMetrics = () => {
      ctx.messageViewportRuntime?.syncMessageVirtualMetrics();
  };

  ctx.pruneMessageVirtualHeightCache = () => {
      ctx.messageViewportRuntime?.pruneMessageVirtualHeightCache();
  };

  ctx.scheduleMessageViewportRefresh = (options: {
      updateScrollState?: boolean;
      measure?: boolean;
      measureKeys?: string[];
      reason?: string;
  } = {}) => {
      ctx.messageViewportRuntime?.scheduleMessageViewportRefresh(options);
  };

  ctx.scheduleMessageVirtualMeasure = (measureKeys?: string[]) => {
      ctx.messageViewportRuntime?.scheduleMessageVirtualMeasure(measureKeys);
  };

  ctx.handleMessageWorkflowLayoutChange = (messageKey?: string) => {
      ctx.messageViewportRuntime?.handleWorkflowLayoutChange(messageKey);
      if (ctx.autoStickToBottom.value &&
          messageKey &&
          String(messageKey).trim() === ctx.latestAgentRenderableMessageKey.value) {
          void ctx.scrollMessagesToBottom();
      }
  };

  ctx.handleMessageMarkdownRendered = (messageKey?: string, payload: {
      streaming?: boolean;
      contentLength?: number;
      needsHydration?: boolean;
      lightweight?: boolean;
      } = {}) => {
      const normalizedKey = String(messageKey || '').trim();
      const lightweightStreaming = payload.streaming === true && payload.lightweight === true;
      if (!lightweightStreaming) {
          ctx.scheduleMessageViewportRefresh({
              updateScrollState: true,
              measure: true,
              measureKeys: normalizedKey ? [normalizedKey] : undefined,
              reason: payload.streaming ? 'streaming-markdown-rendered' : 'markdown-rendered'
          });
      }
      if (
          ctx.autoStickToBottom.value &&
          normalizedKey &&
          normalizedKey === ctx.latestAgentRenderableMessageKey.value
      ) {
          if (lightweightStreaming) {
              scheduleLightweightMarkdownStickToBottom();
          }
          else {
              clearLightweightMarkdownStickTimer();
              lastLightweightMarkdownStickAt = Date.now();
              void ctx.scrollMessagesToBottom();
          }
      }
      if (payload.needsHydration === true) {
          ctx.scheduleWorkspaceResourceHydration('markdown-rendered-resources', normalizedKey ? { messageKeys: [normalizedKey] } : {});
      }
  };

  ctx.updateMessageScrollState = () => {
      ctx.messageViewportRuntime?.updateMessageScrollState();
  };

  ctx.handleMessageListScroll = () => {
      ctx.messageViewportRuntime?.handleMessageListScroll();
  };

  ctx.restoreConversationScroll = async (options: { deferMeasure?: boolean } = {}) => {
      return ctx.messageViewportRuntime?.restoreConversationScroll(options) ?? Promise.resolve(false);
  };

  ctx.rememberCurrentMessageScroll = () => {
      ctx.messageViewportRuntime?.rememberCurrentScroll();
  };

  ctx.rememberMessageScrollForKey = (key: string) => {
      ctx.messageViewportRuntime?.rememberScrollForKey(key);
  };

  ctx.scrollMessagesToBottom = async (force = false) => {
      return ctx.messageViewportRuntime?.scrollMessagesToBottom(force) ?? Promise.resolve();
  };

  ctx.jumpToMessageBottom = async () => {
      return ctx.messageViewportRuntime?.jumpToMessageBottom() ?? Promise.resolve();
  };

  ctx.jumpToMessageTop = async () => {
      return ctx.messageViewportRuntime?.jumpToMessageTop() ?? Promise.resolve();
  };

  ctx.scrollVirtualMessageToIndex = (keys: string[], index: number, align: 'center' | 'start' = 'center') => {
      ctx.messageViewportRuntime?.scrollVirtualMessageToIndex(keys, index, align);
  };

  ctx.scrollLatestAssistantToCenter = async () => {
      return ctx.messageViewportRuntime?.scrollLatestAssistantToCenter() ?? Promise.resolve();
  };

  ctx.refreshLatestAssistantMessageLayout = (reason: string) => {
      if (!ctx.isAgentConversationActive.value) {
          return;
      }
      const latestMessage = ctx.agentRenderableMessages.value[ctx.agentRenderableMessages.value.length - 1]?.message as Record<string, unknown> | undefined;
      if (!latestMessage || String(latestMessage.role || '') !== 'assistant') {
          return;
      }
      const latestMessageKey = ctx.latestAgentRenderableMessageKey.value;
      if (isChatDebugEnabled()) {
          const workflowItems = Array.isArray(latestMessage.workflowItems)
              ? (latestMessage.workflowItems as unknown[])
              : [];
          chatDebugLog('messenger.viewport', 'latest-assistant-layout-refresh', {
              reason,
              activeSessionId: ctx.chatStore.activeSessionId,
              messageKey: latestMessageKey,
              shouldVirtualize: ctx.shouldVirtualizeMessages.value,
              autoStickToBottom: ctx.autoStickToBottom.value,
              workflowItemCount: workflowItems.length,
              workflowStreaming: Boolean(latestMessage.workflowStreaming),
              reasoningStreaming: Boolean(latestMessage.reasoningStreaming),
              streamIncomplete: Boolean(latestMessage.stream_incomplete),
              contentLength: String(latestMessage.content || '').length,
              reasoningLength: String(latestMessage.reasoning || '').length
          });
      }
      void nextTick(() => {
          ctx.scheduleMessageViewportRefresh({
              updateScrollState: true,
              measure: true,
              measureKeys: latestMessageKey ? [latestMessageKey] : undefined,
              reason
          });
          if (ctx.autoStickToBottom.value) {
              void ctx.scrollMessagesToBottom();
          }
      });
  };

  ctx.messageViewportRuntime = createMessageViewportRuntime({
      messageListRef: ctx.messageListRef,
      showChatSettingsView: ctx.showChatSettingsView,
      autoStickToBottom: ctx.autoStickToBottom,
      showScrollTopButton: ctx.showScrollTopButton,
      showScrollBottomButton: ctx.showScrollBottomButton,
      isAgentConversationActive: ctx.isAgentConversationActive,
      isWorldConversationActive: ctx.isWorldConversationActive,
      activeConversationKey: computed(() => String(ctx.sessionHub.activeConversationKey || '')),
      shouldVirtualizeMessages: ctx.shouldVirtualizeMessages,
      agentRenderableMessages: computed(() => ctx.agentConversationRows.value.map(row => ({ key: row.key, message: row.assistant.message }))),
      worldRenderableMessages: ctx.worldRenderableMessages,
      messageVirtualHeightCache: ctx.messageVirtualHeightCache,
      messageVirtualLayoutVersion: ctx.messageVirtualLayoutVersion,
      messageVirtualScrollTop: ctx.messageVirtualScrollTop,
      messageVirtualViewportHeight: ctx.messageVirtualViewportHeight,
      estimateVirtualOffsetTop: ctx.estimateVirtualOffsetTop,
      resolveVirtualMessageHeight: ctx.resolveVirtualMessageHeight,
      loadOlderHistory: async () => {
          if (!ctx.isAgentConversationActive.value) {
              return [];
          }
          const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (!sessionId) {
              return [];
          }
          return ctx.chatStore.loadOlderHistory(sessionId);
      }
  });
}
  installMessengerControllerLifecycleMessageViewport(ctx);
}

function installPart17(ctx: any): void {
// Cross-domain watchers, mounted listeners, realtime pulse wiring, and unmount cleanup.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerLifecycleReactiveEffects(ctx: MessengerControllerContext): void {
  installActiveChatRealtimeRecovery(ctx);
  watch(() => [ctx.sessionHub.activeSection, ctx.chatStore.activeSessionId], () => {
    ctx.chatStore.foregroundChatSessionId = ctx.sessionHub.activeSection === 'messages'
      ? String(ctx.chatStore.activeSessionId || '') : '';
    if (ctx.chatStore.foregroundChatSessionId) flushBackgroundPublication(ctx.chatStore);
  }, { immediate: true, flush: 'sync' });
  const clearDesktopRealtimePulseStartTimer = () => {
    return;
      if (typeof window === 'undefined' || desktopRealtimePulseStartTimer === null) {
          return;
      }
      window.clearTimeout(desktopRealtimePulseStartTimer);
      desktopRealtimePulseStartTimer = null;
  };

  watch(() => ctx.currentUserId.value, (value, previousValue) => {
      const changed = String(value || '') !== String(previousValue || '');
      const shouldClearConversationState = changed && ctx.currentUserContextInitialized && !ctx.bootLoading.value;
      ctx.currentUserContextInitialized = true;
      if (changed) {
          ctx.chatStore.resetState();
      }
      if (shouldClearConversationState) {
          ctx.sessionHub.clearActiveConversation();
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.session_id;
          delete nextQuery.conversation_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
      }
      ctx.beeroomGroupsLastRefreshAt = 0;
      ctx.selectedAgentHiveGroupId.value = '';
      void ctx.hydrateCurrentUserAppearance();
      void ctx.hydrateMessengerOrderPreferences();
      ctx.cronPermissionDenied.value = false;
      ctx.cronAgentIds.value = new Set<string>();
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
      ctx.agentToolSummaryPromise = null;
      invalidateAllUserToolsCaches();
      ctx.clearWorkspaceResourceCache();
      ctx.ensureDismissedAgentConversationState(true);
      ctx.ensureAgentUnreadState(true);
      ctx.refreshAgentMainUnreadFromSessions();
      ctx.warmMessengerUserToolsData({
          catalog: ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools',
          skills: false,
          summary: ctx.sessionHub.activeSection === 'agents' || ctx.showAgentRightDock.value
      });
      ctx.scheduleWorkspaceResourceHydration('profile-change');
  }, { immediate: true });

  watch(() => ctx.userAttachmentWorkspacePaths.value, (paths) => {
      if (isChatDebugVerboseEnabled()) {
          chatDebugLog('messenger.hydration', 'attachment-paths', {
              activeSessionId: ctx.chatStore.activeSessionId,
              activeConversationKey: ctx.sessionHub.activeConversationKey,
              pathCount: paths.length,
              virtualized: Boolean(ctx.shouldVirtualizeMessages?.value),
              snapshot: ctx.buildMessageVirtualDebugSnapshot?.()
          });
      }
      paths.forEach((path) => {
          void ctx.ensureUserAttachmentResource(path);
      });
  }, { immediate: true });

  watch(() => [ctx.themeStore.palette], () => {
      if (ctx.appearanceHydrating.value)
          return;
      void ctx.persistCurrentUserAppearance();
  });

  watch(() => ctx.sessionHub.activeSection, (section, previousSection) => {
      if (previousSection === 'messages' && section !== 'messages') {
          ctx.rememberCurrentMessageScroll?.();
      }
      ctx.closeFileContainerMenu();
      if (!ctx.isSearchableMiddlePaneSection(section) && (ctx.keywordInput.value || ctx.sessionHub.keyword)) {
          ctx.clearKeywordDebounce();
          ctx.keywordInput.value = '';
          ctx.sessionHub.setKeyword('');
      }
      if (section === 'swarms') {
          clearDesktopRealtimePulseStartTimer();
          ctx.stopRealtimePulse?.();
          ctx.beeroomGroupsLastRefreshAt = 0;
          ctx.startBeeroomRealtimeSync?.();
          ctx.triggerBeeroomRealtimeSyncRefresh?.('enter-swarms');
      }
      else {
          ctx.stopBeeroomRealtimeSync?.();
          if (false) {
              clearDesktopRealtimePulseStartTimer();
              if (!false && typeof window !== 'undefined') {
                  desktopRealtimePulseStartTimer = window.setTimeout(() => {
                      desktopRealtimePulseStartTimer = null;
                      ctx.startRealtimePulse?.();
                      ctx.triggerRealtimePulseRefresh?.(`enter-${section}`);
                  }, 2500);
              }
          }
          else {
              ctx.startRealtimePulse?.();
              ctx.triggerRealtimePulseRefresh?.(`enter-${section}`);
          }
          if (section === 'messages') {
              if (isChatDebugEnabled()) {
                  chatDebugLog('messenger.enter', 'messages-section', {
                      activeConversationKey: ctx.sessionHub.activeConversationKey,
                      renderKind: String(ctx.retainedMessageRenderKind?.value || ''),
                      shouldVirtualize: Boolean(ctx.shouldVirtualizeMessages?.value),
                      agentCount: ctx.agentRenderableMessages.value.length,
                      worldCount: ctx.worldRenderableMessages.value.length,
                      snapshot: isChatDebugVerboseEnabled()
                          ? ctx.buildMessageVirtualDebugSnapshot?.()
                          : undefined
                  });
              }
              void nextTick(async () => {
                  const restored = await ctx.restoreConversationScroll?.({ deferMeasure: true });
                  if (isChatDebugEnabled()) {
                      chatDebugLog('messenger.enter', 'restore-scroll', {
                          restored,
                          renderKind: String(ctx.retainedMessageRenderKind?.value || ''),
                          shouldVirtualize: Boolean(ctx.shouldVirtualizeMessages?.value),
                          snapshot: isChatDebugVerboseEnabled()
                              ? ctx.buildMessageVirtualDebugSnapshot?.()
                              : undefined
                      });
                  }
                  if (!restored) {
                      await ctx.scrollMessagesToBottom(true);
                  }
              });
              return;
          }
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
      if (section === 'more' && !false) {
          void preloadMessengerSettingsPanels();
      }
      ctx.ensureSectionSelection();
  }, { immediate: true });

  watch(() => ctx.showAgentGridOverview.value, (visible) => {
      if (visible) {
          ctx.loadAgentUserRounds();
          void ctx.loadDefaultAgentProfile();
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

  watch(() => [ctx.keyword.value], () => {
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

  watch(() => ctx.filteredOwnedAgentsOrdered.value
      .map((item) => String(item?.id || item?.agent_id || ''))
      .join('|'), () => {
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
          String(session?.last_message_at || session?.updated_at || session?.created_at || '')
      ].join(':'))
          .join('|'),
      ctx.sessionHub.activeConversationKey
  ], () => {
      ctx.refreshAgentMainUnreadFromSessions();
  }, { immediate: true });

  watch(() => [ctx.sessionHub.activeSection, ctx.sessionHub.activeConversationKey], () => {
      ctx.clearMessagePanelWhenConversationEmpty();
  }, { immediate: true });

  watch(() => [
      ctx.filteredOwnedAgentsOrdered.value.length,
      ctx.showDefaultAgentEntry.value ? 1 : 0
  ], () => {
      ctx.ensureSectionSelection();
  });

  watch(() => ctx.sessionHub.activeConversationKey, (_value, oldValue) => {
      if (oldValue) {
          ctx.rememberMessageScrollForKey?.(String(oldValue));
      }
      ctx.clearWorkspaceResourceCache();
      ctx.pendingAssistantCenter = false;
      ctx.pendingAssistantCenterCount = 0;
      ctx.dismissedPlanMessages.value = new WeakSet<Record<string, unknown>>();
      ctx.dismissedPlanVersion.value += 1;
      ctx.agentInquirySelection.value = [];
      ctx.scheduleWorkspaceResourceHydration('conversation-key-change');
      if (isChatDebugVerboseEnabled()) {
          chatDebugLog('messenger.virtual', 'conversation-key-change', ctx.buildMessageVirtualDebugSnapshot?.());
      }
      if (ctx.sessionHub.activeSection === 'messages') {
          void nextTick(async () => {
              const restored = await ctx.restoreConversationScroll?.();
              if (!restored) {
                  await ctx.scrollMessagesToBottom(true);
              }
          });
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

  watch(() => ctx.rightDockSkillDialogVisible.value, (visible) => {
      if (visible)
          return;
      ctx.rightDockSkillContentLoadVersion += 1;
      ctx.rightDockSkillContentLoading.value = false;
      ctx.rightDockSkillToggleSaving.value = false;
      ctx.rightDockSkillContent.value = '';
      ctx.rightDockSkillContentPath.value = '';
  });

  watch(() => [ctx.chatStore.activeSessionId, ctx.resolveActiveAgentRenderableMessageRecords().length], () => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return;
      const activeSession = (Array.isArray(ctx.chatStore.sessions)
          ? ctx.chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId)
          : null) || null;
      ctx.refreshSessionPreviewCache(sessionId, (activeSession || null) as Record<string, unknown> | null);
  });

  watch(() => ctx.showChatSettingsView.value, (settingsView) => {
      if (settingsView) {
          ctx.rememberCurrentMessageScroll?.();
          return;
      }
      ctx.scheduleMessageViewportRefresh({
          updateScrollState: true,
          reason: 'settings-view-enter-messages'
      });
  });

  watch(() => [
      ctx.agentRenderableMessages.value.length,
      ctx.retainedMessageRenderKind.value,
      ctx.sessionHub.activeConversationKey
  ], () => {
      if (ctx.sessionHub.activeSection !== 'messages') return;
      ctx.pruneMessageVirtualHeightCache();
      void nextTick(() => {
          ctx.scheduleMessageViewportRefresh({
              measure: true,
              reason: 'message-structure-change'
          });
      });
      ctx.scheduleWorkspaceResourceHydration('message-structure-change');
      if (isChatDebugVerboseEnabled()) {
          chatDebugLog('messenger.virtual', 'message-structure-change', ctx.buildMessageVirtualDebugSnapshot?.());
      }
      if (ctx.pendingAssistantCenter &&
          ctx.isAgentConversationActive.value &&
          ctx.agentRenderableMessages.value.length > ctx.pendingAssistantCenterCount) {
          const lastMessage = ctx.agentRenderableMessages.value[ctx.agentRenderableMessages.value.length - 1]?.message as Record<string, unknown> | undefined;
          if (String(lastMessage?.role || '') === 'assistant') {
              ctx.pendingAssistantCenter = false;
              ctx.pendingAssistantCenterCount = ctx.agentRenderableMessages.value.length;
              ctx.agentSendForegroundLock.value = false;
              ctx.agentSendForegroundLockSessionId.value = '';
              ctx.autoStickToBottom.value = false;
              void ctx.scrollLatestAssistantToCenter();
              return;
          }
      }
      if (
          ctx.sessionHub.activeSection === 'messages' &&
          !ctx.autoStickToBottom.value &&
          ctx.messageListRef.value &&
          ctx.messageListRef.value.scrollTop <= 1
      ) {
          void nextTick(() => {
              void ctx.restoreConversationScroll?.();
          });
      }
      if (ctx.autoStickToBottom.value) {
          void ctx.scrollMessagesToBottom();
      }
      else {
          ctx.updateMessageScrollState();
      }
  });

  watch(() => {
      const latestMessage = ctx.agentRenderableMessages.value[ctx.agentRenderableMessages.value.length - 1]?.message as Record<string, unknown> | undefined;
      return [
          ctx.chatStore.activeSessionId,
          ctx.latestAgentRenderableMessageKey.value,
          ctx.buildLatestAssistantLayoutSignature(latestMessage)
      ].join('::');
  }, () => {
      if (ctx.sessionHub.activeSection !== 'messages') return;
      ctx.refreshLatestAssistantMessageLayout('latest-assistant-signature');
  }, { flush: 'post' });

  watch(() => {
      const latestMessage = ctx.agentRenderableMessages.value[ctx.agentRenderableMessages.value.length - 1]?.message as Record<string, unknown> | undefined;
      if (!latestMessage) {
          return '';
      }
      const workflowItems = Array.isArray(latestMessage.workflowItems)
          ? (latestMessage.workflowItems as unknown[])
          : [];
      const lastWorkflowItem = workflowItems[workflowItems.length - 1] as Record<string, unknown> | undefined;
      return [
          ctx.chatStore.activeSessionId,
          ctx.latestAgentRenderableMessageKey.value,
          workflowItems.length,
          String(lastWorkflowItem?.id || lastWorkflowItem?.toolCallId || lastWorkflowItem?.eventType || '').trim(),
          String(lastWorkflowItem?.status || '').trim()
      ].join('::');
  }, () => {
      if (ctx.sessionHub.activeSection !== 'messages') return;
      const latestKey = ctx.latestAgentRenderableMessageKey.value;
      ctx.scheduleWorkspaceResourceHydration('latest-assistant-workflow-resources', latestKey ? { messageKeys: [latestKey] } : {});
  }, { flush: 'post' });

  watch(() => [ctx.agentRenderableMessages.value.length, ctx.worldRenderableMessages.value.length], () => {
      if (ctx.sessionHub.activeSection !== 'messages') return;
      ctx.pruneMessageVirtualHeightCache();
      void nextTick(() => {
          ctx.scheduleMessageViewportRefresh({
              measure: true,
              reason: 'renderable-length-change'
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
      false,
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
                      measure: true,
                      reason: 'viewport-resize'
                  });
              });
          };
          ctx.viewportResizeHandler();
          window.addEventListener('resize', ctx.viewportResizeHandler);
          ctx.messengerSendKey.value = ctx.normalizeMessengerSendKey(window.localStorage.getItem(MESSENGER_SEND_KEY_STORAGE_KEY));
          ctx.uiFontSize.value = ctx.normalizeUiFontSize(window.localStorage.getItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY));
          ctx.worldComposerHeight.value = ctx.clampWorldComposerHeight(window.localStorage.getItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY));
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

      // Show the rendered messenger shell as soon as its static layout has
      // painted. Session and agent hydration continues without a blank page.
      await nextTick();
      if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
          await new Promise<void>((resolve) => {
              window.requestAnimationFrame(() => window.requestAnimationFrame(resolve));
          });
      }

      await ctx.bootstrap();

      // Keep this paint boundary for timing and future non-desktop callers.
      await nextTick();
      if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
          await new Promise<void>((resolve) => {
              window.requestAnimationFrame(() => window.requestAnimationFrame(resolve));
          });
      }

      ctx.refreshAudioRecordingSupport();
      ctx.scheduleMessageViewportRefresh({
          updateScrollState: true,
          measure: true,
          reason: 'mounted'
      });
      ctx.scheduleWorkspaceResourceHydration('mounted');
      ctx.warmMessengerUserToolsData({
          catalog: ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools',
          skills: false,
          summary: ctx.sessionHub.activeSection === 'agents' || ctx.showAgentRightDock.value
      });
      ctx.stopWorkspaceRefreshListener = onWorkspaceRefresh(ctx.handleWorkspaceResourceRefresh);
      ctx.completedAgentNoticeSuppression ??= new Map<string, number>();
      const completedTurnNoticeKeys = new Set<string>();
      ctx.stopAgentRuntimeRefreshListener = onAgentRuntimeRefresh((detail) => {
          // A terminal durable frame is authoritative for this acknowledgement.
          // Do this before the aggregate poll: polling can legitimately lag or
          // omit an idle agent, which used to make the completion toast vanish.
          for (const completion of detail?.completedTurns ?? []) {
              const sessionId = String(completion?.sessionId || '').trim();
              const turnId = String(completion?.turnId || '').trim();
              const terminalStatus = completion?.status || 'completed';
              const catalogSession = ctx.chatStore.sessions?.find((session) => String(session?.id || '').trim() === sessionId);
              if (catalogSession) {
                  catalogSession.runtime_status = terminalStatus;
                  catalogSession.runtimeStatus = terminalStatus;
                  catalogSession.thread_status = terminalStatus;
                  catalogSession.threadStatus = terminalStatus;
              }
              const agentId = ctx.normalizeAgentId(completion?.agentId ||
                  ctx.buildSessionAgentMap().get(sessionId));
              const noticeKey = `${sessionId}:${turnId}`;
              if (!sessionId || !turnId || !agentId || completedTurnNoticeKeys.has(noticeKey) ||
                  !claimAgentRuntimeCompletion(sessionId, turnId)) {
                  continue;
              }
              completedTurnNoticeKeys.add(noticeKey);
              ctx.completedAgentNoticeSuppression.set(agentId, Date.now() + 10_000);
              void ctx.notifyAgentTaskCompleted(agentId);
          }
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
          runSequentially: false,
          shouldDefer: () => ctx.isActiveChatInteractiveStream?.() === true,
          isHotState: () => ctx.hasHotRuntimeState.value,
          shouldRefreshCron: () => !ctx.cronPermissionDenied.value,
          shouldRefreshChannelBoundAgentIds: ctx.shouldRefreshAgentMeta,
          shouldRefreshChatSessions: ctx.shouldRefreshRealtimeChatSessions
      });
      ctx.startRealtimePulse = () => realtimePulse.start();
      ctx.stopRealtimePulse = () => realtimePulse.stop();
      ctx.triggerRealtimePulseRefresh = (reason = '') => realtimePulse.trigger(reason);
      {
          realtimePulse.start();
          realtimePulse.trigger('mounted');
      }
  });

  onBeforeUnmount(() => {
      ctx.chatStore.foregroundChatSessionId = '';
      ctx.sectionRouteSyncToken += 1;
      clearDesktopRealtimePulseStartTimer();
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
      ctx.clearKeywordDebounce();
      ctx.closeResourcePreview();
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
  });
}
  installMessengerControllerLifecycleReactiveEffects(ctx);
}

function installPart18(ctx: any): void {
// Runtime metadata refreshers for agents, cron jobs, channel bindings, realtime contacts, and full refresh.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

type ActiveAgentInquiryPanel = { message: Record<string, unknown>; panel: AgentInquiryPanelData };

type AgentRuntimeRemoteStatus = {
  agentId: string;
  sessionId: string;
  previousSessionId: string;
  state: AgentRuntimeState;
  previousState: AgentRuntimeState;
};

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

function installMessengerControllerLifecycleRuntimeMeta(ctx: MessengerControllerContext): void {
  let agentRuntimeSessionSnapshot = new Map<string, string>();

  const collectAgentRuntimeSessionIds = (
      explicitSessionId: string,
      previousSessionId: string
  ): Set<string> => {
      const result = new Set<string>();
      if (explicitSessionId) {
          result.add(explicitSessionId);
      }
      if (previousSessionId) {
          result.add(previousSessionId);
      }
      return result;
  };

  const clearSettledAgentRuntimeOverride = (agentId: string) => {
      const key = ctx.normalizeAgentId(agentId) || DEFAULT_AGENT_KEY;
      const overrides = ctx.runtimeStateOverrides?.value;
      if (!overrides || !overrides.has(key))
          return;
      overrides.delete(key);
      ctx.runtimeStateOverrides.value = new Map(overrides);
  };

  const localTerminalRuntimeSessionSnapshot = new Map<string, string>();

  const resolveAgentIdForRuntimeSession = (
      sessionId: string,
      sessionAgentMap: Map<string, string>
  ): string => {
      const targetSessionId = String(sessionId || '').trim();
      if (!targetSessionId)
          return '';
      const mappedAgentId = sessionAgentMap.get(targetSessionId);
      if (mappedAgentId) {
          return ctx.normalizeAgentId(mappedAgentId) || DEFAULT_AGENT_KEY;
      }
      for (const [agentId, runtimeSessionId] of agentRuntimeSessionSnapshot.entries()) {
          if (String(runtimeSessionId || '').trim() === targetSessionId) {
              return ctx.normalizeAgentId(agentId) || DEFAULT_AGENT_KEY;
          }
      }
      if (targetSessionId === String(ctx.chatStore.activeSessionId || '').trim()) {
          return ctx.normalizeAgentId(
              ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId || DEFAULT_AGENT_KEY
          ) || DEFAULT_AGENT_KEY;
      }
      return '';
  };

  const settleAgentRuntimeStateFromTerminalSession = (
      sessionId: string,
      runtimeStatus: string,
      reason: string,
      sessionAgentMap: Map<string, string>,
      fallbackStateMap: Map<string, AgentRuntimeState> | null = null
  ) => {
      const targetSessionId = String(sessionId || '').trim();
      if (!targetSessionId)
          return;
      const terminalState = resolveAgentRuntimeTerminalStateFromSessionStatus(runtimeStatus);
      if (!terminalState) {
          localTerminalRuntimeSessionSnapshot.delete(targetSessionId);
          return;
      }
      const agentId = resolveAgentIdForRuntimeSession(targetSessionId, sessionAgentMap);
      if (!agentId)
          return;
      const currentRuntimeSessionId = String(agentRuntimeSessionSnapshot.get(agentId) || '').trim();
      if (currentRuntimeSessionId && currentRuntimeSessionId !== targetSessionId) {
          return;
      }
      const runtime = getRuntime(targetSessionId);
      const currentState =
          ctx.agentRuntimeStateMap.value.get(agentId) ||
          fallbackStateMap?.get(agentId) ||
          'idle';
      const override = ctx.runtimeStateOverrides?.value?.get(agentId);
      const localStreaming = Boolean(ctx.streamingAgentIdSet?.value?.has(agentId));
      const localWaiting = Boolean(ctx.waitingAgentIdSet?.value?.has(agentId));
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const hasLocalRuntimeEvidence = hasAgentTerminalSettlementEvidence({
          targetSessionId,
          currentRuntimeSessionId,
          activeSessionId,
          hasRuntimeActivity: Boolean(
              currentRuntimeSessionId === targetSessionId ||
              runtime?.lastThreadStatusAt ||
              runtime?.sendController ||
              runtime?.resumeController ||
              runtime?.compactController ||
              ctx.chatStore.loadingBySession?.[targetSessionId]
          ),
          currentState,
          overrideState: override?.state ?? null,
          localStreaming,
          localWaiting
      });
      if (!hasLocalRuntimeEvidence) {
          return;
      }
      if (!shouldSettleAgentRuntimeFromTerminalSession({
          sessionStatus: runtimeStatus,
          currentState,
          localStreaming,
          localWaiting,
          overrideState: override?.state ?? null
      })) {
          return;
      }
      const signature = `${agentId}:${terminalState}:${runtimeStatus}`;
      if (
          localTerminalRuntimeSessionSnapshot.get(targetSessionId) === signature &&
          currentState === terminalState
      ) {
          return;
      }
      clearSettledAgentRuntimeOverride(agentId);
      const nextStateMap = new Map<string, AgentRuntimeState>(ctx.agentRuntimeStateMap.value);
      nextStateMap.set(agentId, terminalState);
      ctx.handleAgentRuntimeStateUpdate(nextStateMap);
      localTerminalRuntimeSessionSnapshot.set(targetSessionId, signature);
      chatDebugLog('messenger.agent-runtime', 'settle-agent-from-session-runtime', {
          agentId,
          sessionId: targetSessionId,
          runtimeStatus,
          state: terminalState,
          reason,
          previousState: currentState,
          runtime: buildRuntimeDebugSnapshot(runtime)
      });
  };

  const reconcileTerminalAgentRuntimeStatesFromSessions = (
      reason: string,
      fallbackStateMap: Map<string, AgentRuntimeState> | null = null
  ) => {
      const sessionAgentMap = ctx.buildSessionAgentMap();
      const loadingBySession = ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession as Record<string, unknown>
          : {};
      const sessionIds = new Set<string>([
          ...Array.from(sessionAgentMap.keys()),
          ...Array.from(agentRuntimeSessionSnapshot.values()).map((id) => String(id || '').trim()),
          ...Object.keys(loadingBySession).map((id) => String(id || '').trim())
      ]);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (activeSessionId) {
          sessionIds.add(activeSessionId);
      }
      sessionIds.forEach((sessionId) => {
          if (!sessionId)
              return;
          const runtimeStatus = String(ctx.resolveSessionRuntimeStatus?.(sessionId) || '').trim().toLowerCase();
          settleAgentRuntimeStateFromTerminalSession(
              sessionId,
              runtimeStatus,
              reason,
              sessionAgentMap,
              fallbackStateMap
          );
      });
  };

  const settleAgentRuntimeSessionFromMeta = (
      agentId: string,
      sessionId: string,
      state: AgentRuntimeState,
      reason: string
  ) => {
      const targetSessionId = String(sessionId || '').trim();
      if (!targetSessionId)
          return;
      const runtimeBefore = getRuntime(targetSessionId);
      const runtimeBeforeSnapshot = buildRuntimeDebugSnapshot(runtimeBefore);
      const statusBefore = ctx.resolveSessionRuntimeStatus?.(targetSessionId) || '';
      const loadingBefore = Boolean(ctx.chatStore.loadingBySession?.[targetSessionId]);
      const busyBefore = Boolean(ctx.chatStore.isSessionBusy?.(targetSessionId) || ctx.chatStore.isSessionLoading?.(targetSessionId));
      const hasControllerBefore = Boolean(runtimeBefore?.sendController || runtimeBefore?.resumeController || runtimeBefore?.compactController);
      // A stale aggregate poll may report idle while the canonical session is
      // already rendering the current turn. Never settle that turn from the
      // aggregate response; its terminal stream event is authoritative.
      if (hasRunningAssistantMessage(messages) || hasStreamingAssistantMessage(messages) ||
          isThreadRuntimeBusy(statusBefore)) {
          return;
      }
      if (!loadingBefore && !busyBefore && !hasControllerBefore) {
          return;
      }
      if (runtimeBefore) {
          runtimeBefore.loaded = true;
          runtimeBefore.threadStatus = state === 'error' ? 'system_error' : 'completed';
      }
      const settled = settleTerminalSessionRuntime(ctx.chatStore, targetSessionId, {
          eventType: `agent_runtime_${reason}`,
          failed: state === 'error'
      });
      if (settled) {
          chatDebugLog('messenger.agent-runtime', 'settle-session-from-agent-meta', {
              agentId,
              sessionId: targetSessionId,
              state,
              reason,
              statusBefore,
              loadingBefore,
              busyBefore,
              runtimeBefore: runtimeBeforeSnapshot,
              runtimeAfter: buildRuntimeDebugSnapshot(getRuntime(targetSessionId))
          });
      }
  };

  const reconcileSettledAgentRuntimeSessions = (items: AgentRuntimeRemoteStatus[]) => {
      items.forEach((item) => {
          if (!shouldSettleAgentSessionsFromRuntimeState({
              previousState: item.previousState,
              nextState: item.state
          })) {
              return;
          }
          clearSettledAgentRuntimeOverride(item.agentId);
          const reason = item.state === 'idle' ? 'idle_reconcile' : item.state;
          collectAgentRuntimeSessionIds(item.sessionId, item.previousSessionId).forEach((sessionId) => {
              settleAgentRuntimeSessionFromMeta(item.agentId, sessionId, item.state, reason);
          });
      });
  };

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
              const previousStateMap = new Map<string, AgentRuntimeState>(
                  ctx.agentRuntimeStateHydrated
                      ? ctx.agentRuntimeStateSnapshot
                      : ctx.agentRuntimeStateMap.value
              );
              const previousSessionMap = new Map(agentRuntimeSessionSnapshot);
              const stateMap = new Map<string, AgentRuntimeState>();
              const nextSessionMap = new Map<string, string>();
              const runtimeItems: AgentRuntimeRemoteStatus[] = [];
              items.forEach((item: Record<string, unknown>) => {
                  const key = ctx.normalizeAgentId(item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')) || DEFAULT_AGENT_KEY;
                  const state = ctx.normalizeRuntimeState(item?.state, item?.pending_question === true);
                  const sessionId = String(item?.session_id ?? item?.sessionId ?? '').trim();
                  stateMap.set(key, state);
                  if (sessionId) {
                      nextSessionMap.set(key, sessionId);
                  }
                  runtimeItems.push({
                      agentId: key,
                      sessionId,
                      previousSessionId: previousSessionMap.get(key) ?? '',
                      state,
                      previousState: previousStateMap.get(key) ?? 'idle'
                  });
              });
              agentRuntimeSessionSnapshot = nextSessionMap;
              reconcileSettledAgentRuntimeSessions(runtimeItems);
              // The aggregate runtime endpoint is also authoritative for
              // detached work-thread rows. Keep the catalog row in sync even
              // when its websocket is not mounted in the foreground.
              runtimeItems.forEach((item) => {
                  const sessionId = String(item.sessionId || '').trim();
                  if (!sessionId) return;
                  const session = ctx.chatStore.sessions?.find((entry) => String(entry?.id || '').trim() === sessionId);
                  if (!session) return;
                  const catalogStatus = item.state === 'running'
                      ? 'running'
                      : item.state === 'pending'
                          ? 'queued'
                          : item.state === 'error'
                              ? 'failed'
                              : item.state === 'done'
                                  ? 'completed'
                                  : '';
                  if (!catalogStatus) return;
                  session.runtime_status = catalogStatus;
                  session.runtimeStatus = catalogStatus;
                  session.thread_status = catalogStatus;
                  session.threadStatus = catalogStatus;
              });
              ctx.handleAgentRuntimeStateUpdate(stateMap);
              reconcileTerminalAgentRuntimeStatesFromSessions('running-agents-refresh', previousStateMap);
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

  watch(
      () => [
          ctx.chatStore.runtimeProjectionVersion,
          String(ctx.chatStore.activeSessionId || ''),
          Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions.length : 0,
          Object.keys(ctx.chatStore.loadingBySession || {}).sort().join('|')
      ],
      () => {
          reconcileTerminalAgentRuntimeStatesFromSessions('session-runtime');
      },
      { flush: 'post' }
  );

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
                  const fallbackAgent = DEFAULT_AGENT_KEY;
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
      const traceId = `sess-refresh-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
      ctx.messengerSessionRefreshTraceId.value = traceId;
      ctx.messengerSessionRefreshTraceSource.value = 'realtime-pulse';
      if (ctx.isActiveChatInteractiveStream?.()) {
          chatDebugLog('messenger.conversation', 'session-refresh-skip-interactive-stream', {
              traceId,
              source: ctx.messengerSessionRefreshTraceSource.value,
              activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
              sessionCount: Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions.length : 0,
              runtime: buildRuntimeDebugSnapshot(getRuntime(ctx.chatStore.activeSessionId))
          });
          ctx.messengerSessionRefreshTraceId.value = '';
          ctx.messengerSessionRefreshTraceSource.value = '';
          return;
      }
      chatDebugLog('messenger.conversation', 'session-refresh-start', {
          traceId,
          source: ctx.messengerSessionRefreshTraceSource.value,
          activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
          sessionCount: Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions.length : 0
      });
      try {
      await ctx.chatStore.loadSessions({
              traceId,
              traceSource: ctx.messengerSessionRefreshTraceSource.value,
              force: true
          });
          await ctx.chatStore.ensureActiveSessionRealtime?.({
              reason: 'realtime-pulse',
              hydrateIfCold: true
          });
          await ctx.loadRunningAgents({ force: true });
      }
      finally {
          chatDebugLog('messenger.conversation', 'session-refresh-finish', {
              traceId,
              source: ctx.messengerSessionRefreshTraceSource.value,
              activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
              sessionCount: Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions.length : 0
          });
          if (ctx.messengerSessionRefreshTraceId.value === traceId) {
              ctx.messengerSessionRefreshTraceId.value = '';
              ctx.messengerSessionRefreshTraceSource.value = '';
          }
      }
  };

  ctx.isActiveChatInteractiveStream = () => {
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!activeSessionId) {
          return false;
      }
      const runtime = getRuntime(activeSessionId);
      return Boolean(runtime?.sendController || runtime?.resumeController);
  };

  ctx.shouldRefreshRealtimeChatSessions = () => {
      if (ctx.sessionHub.activeSection !== 'messages') {
          return false;
      }
      if (!ctx.isActiveChatInteractiveStream?.()) {
          return true;
      }
      chatDebugLog('messenger.conversation', 'session-refresh-skip-interactive-stream', {
          source: 'realtime-pulse',
          activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
          sessionCount: Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions.length : 0,
          runtime: buildRuntimeDebugSnapshot(getRuntime(ctx.chatStore.activeSessionId))
      });
      return false;
  };

  ctx.shouldRefreshAgentMeta = () => ctx.sessionHub.activeSection === 'agents' || ctx.sessionHub.activeSection === 'tools';

  ctx.refreshAll = async () => {
      const tasks: Promise<unknown>[] = [
          ctx.agentStore.loadAgents(),
          ctx.chatStore.loadSessions(),
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
  installMessengerControllerLifecycleRuntimeMeta(ctx);
}

function installPart19(ctx: any): void {
// Cron badges, agent runtime state normalization, hot-state detection, and completion notifications.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerAgentRuntimeSignals(ctx: MessengerControllerContext): void {
  // Durable terminal frames can acknowledge a task before the aggregate
  // running-agent poll observes idle. The lifecycle controller records that
  // acknowledgement here so the later running → idle edge cannot show a
  // second toast.
  ctx.completedAgentNoticeSuppression ??= new Map<string, number>();
  ctx.hasCronTask = (agentId: unknown): boolean => ctx.cronAgentIds.value.has(ctx.normalizeAgentId(agentId));

  ctx.normalizeRuntimeState = (state: unknown, pendingQuestion = false): AgentRuntimeState => normalizeAssistantMessageRuntimeState(state, pendingQuestion) as AgentRuntimeState;

  ctx.setRuntimeStateOverride = (agentId: unknown, state: AgentRuntimeState, ttlMs = 0) => {
      const key = ctx.normalizeAgentId(agentId);
      if (ttlMs <= 0) {
          ctx.runtimeStateOverrides.value.delete(key);
          ctx.triggerRealtimePulseRefresh?.('runtime-override-clear');
          return;
      }
      ctx.runtimeStateOverrides.value.set(key, {
          state,
          expiresAt: Date.now() + ttlMs
      });
      ctx.triggerRealtimePulseRefresh?.('runtime-override');
  };

  ctx.resolveAgentRuntimeState = (agentId: unknown): AgentRuntimeState => {
      const key = ctx.normalizeAgentId(agentId);
      const inquiryAgentId = ctx.activeAgentInquiryPanel.value
          ? ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value)
          : '';
      const remoteState = ctx.agentRuntimeStateMap.value.get(key) || 'idle';
      const now = Date.now();
      const override = ctx.runtimeStateOverrides.value.get(key);
      if (override && override.expiresAt <= now) {
          ctx.runtimeStateOverrides.value.delete(key);
      }
      return resolveAgentRuntimeStateFromSignals({
          pendingApproval: ctx.pendingApprovalAgentIdSet.value.has(key),
          pendingInquiry: Boolean(inquiryAgentId && inquiryAgentId === key),
          localWaiting: Boolean(ctx.waitingAgentIdSet?.value?.has(key)),
          localStreaming: ctx.streamingAgentIdSet.value.has(key),
          activeBlockingSwarm: false,
          remoteState,
          overrideState: override && override.expiresAt > now ? override.state : null
      });
  };

  ctx.hasHotRuntimeState = computed(() => {
      if (ctx.pendingApprovalAgentIdSet.value.size > 0 ||
          ctx.waitingAgentIdSet?.value?.size > 0 ||
          ctx.streamingAgentIdSet.value.size > 0) {
          return true;
      }
      const now = Date.now();
      for (const state of ctx.agentRuntimeStateMap.value.values()) {
          if (state === 'running' || state === 'pending') {
              return true;
          }
      }
      for (const override of ctx.runtimeStateOverrides.value.values()) {
          if (override.expiresAt <= now) {
              continue;
          }
          if (override.state === 'running' || override.state === 'pending') {
              return true;
          }
      }
      return false;
  });

  ctx.normalizeAgentUserRoundsKey = (value: unknown): string => {
      const raw = String(value || '').trim();
      if (!raw)
          return DEFAULT_AGENT_KEY;
      return ctx.normalizeAgentId(raw) || DEFAULT_AGENT_KEY;
  };

  ctx.resolveAgentUserRounds = (agentId: unknown): number => {
      const key = ctx.normalizeAgentUserRoundsKey(agentId);
      return ctx.agentUserRoundsMap.value.get(key) ?? 0;
  };

  ctx.formatUserRounds = (value: number): string => {
      const normalized = Number.isFinite(value) ? Math.max(0, Math.floor(value)) : 0;
      return normalized.toLocaleString();
  };

  ctx.formatAgentRuntimeState = (state: AgentRuntimeState): string => {
      if (state === 'running')
          return ctx.t('portal.card.running');
      if (state === 'pending')
          return ctx.t('portal.card.waiting');
      if (state === 'done')
          return ctx.t('portal.card.done');
      if (state === 'error')
          return ctx.t('portal.card.error');
      return ctx.t('portal.card.idle');
  };

  ctx.agentRuntimeStateSnapshot = new Map<string, AgentRuntimeState>();

  ctx.agentRuntimeStateHydrated = false;

  ctx.systemNotificationPermissionRequested = false;

  ctx.resolveAgentDisplayName = (agentId: string): string => {
      const normalized = ctx.normalizeAgentId(agentId);
      const agent = ctx.agentMap.value.get(normalized);
      const name = String(agent?.name || '').trim();
      if (name)
          return name;
      if (normalized === DEFAULT_AGENT_KEY)
          return ctx.t('messenger.defaultAgent');
      return normalized || ctx.t('messenger.defaultAgent');
  };

  ctx.requestSystemNotificationPermission = async (): Promise<NotificationPermission | ''> => {
      if (ctx.systemNotificationPermissionRequested) {
          return typeof window !== 'undefined' ? window.Notification?.permission ?? '' : '';
      }
      ctx.systemNotificationPermissionRequested = true;
      if (typeof window === 'undefined' || !('Notification' in window))
          return '';
      try {
          return await window.Notification.requestPermission();
      }
      catch {
          return '';
      }
  };

  ctx.sendDesktopNotification = async (title: string, body: string): Promise<boolean> => {
      const bridge = ctx.getDesktopBridge();
      if (!bridge || typeof bridge.notify !== 'function')
          return false;
      try {
          const result = await bridge.notify({ title, body });
          return result === true;
      }
      catch {
          return false;
      }
  };

  ctx.sendSystemNotification = async (title: string, body: string): Promise<boolean> => {
      const desktopNotified = await ctx.sendDesktopNotification(title, body);
      if (desktopNotified)
          return true;
      if (typeof window === 'undefined' || !('Notification' in window))
          return false;
      try {
          if (window.Notification.permission === 'granted') {
              new window.Notification(title, { body });
              return true;
          }
          if (window.Notification.permission === 'default') {
              const permission = await ctx.requestSystemNotificationPermission();
              if (permission === 'granted') {
                  new window.Notification(title, { body });
                  return true;
              }
          }
      }
      catch {
          return false;
      }
      return false;
  };

  ctx.notifyAgentTaskCompleted = async (agentId: string) => {
      const title = ctx.t('messenger.agent.taskCompletedTitle');
      const message = ctx.t('messenger.agent.taskCompleted', { name: ctx.resolveAgentDisplayName(agentId) });
      // `sendSystemNotification` already no-ops when the browser has no
      // notification support; the in-page toast stays the reliable signal.
      void ctx.sendSystemNotification(title, message);
      ElMessage.success(message);
  };

  ctx.shouldNotifyAgentCompletion = (previousState: AgentRuntimeState, nextState: AgentRuntimeState): boolean => {
      return shouldNotifyAgentTaskCompletion({ previousState, nextState });
  };

  ctx.handleAgentRuntimeStateUpdate = (stateMap: Map<string, AgentRuntimeState>) => {
      const reconciledStateMap = new Map(stateMap);
      if (ctx.agentRuntimeStateHydrated) {
          const keys = new Set<string>([
              ...Array.from(ctx.agentRuntimeStateSnapshot.keys()),
              ...Array.from(stateMap.keys())
          ]);
          keys.forEach((agentId) => {
              const previousState = ctx.agentRuntimeStateSnapshot.get(agentId) ?? 'idle';
              // A polling gap is not evidence that a task completed.
              const preserveMissing = shouldPreserveMissingAgentRuntimeState({
                  previousState,
                  remoteHasRow: stateMap.has(agentId)
              }) && Boolean(
                  ctx.streamingAgentIdSet?.value?.has(agentId) ||
                  ctx.waitingAgentIdSet?.value?.has(agentId)
              );
              const remoteState = stateMap.get(agentId) ?? 'idle';
              const localTurnStillActive = Boolean(
                  ctx.streamingAgentIdSet?.value?.has(agentId) ||
                  ctx.waitingAgentIdSet?.value?.has(agentId)
              );
              const preserveActiveTurn = shouldNotifyAgentTaskCompletion({
                  previousState,
                  nextState: remoteState
              }) && localTurnStillActive;
              const nextState = preserveMissing || preserveActiveTurn ? previousState : remoteState;
              if (preserveMissing || preserveActiveTurn) reconciledStateMap.set(agentId, previousState);
              if (previousState === nextState)
                  return;
              if (ctx.shouldNotifyAgentCompletion(previousState, nextState)) {
                  const suppressionUntil = Number(ctx.completedAgentNoticeSuppression.get(agentId) || 0);
                  if (suppressionUntil > Date.now()) {
                      ctx.completedAgentNoticeSuppression.delete(agentId);
                      return;
                  }
                  ctx.completedAgentNoticeSuppression.delete(agentId);
                  if (claimAgentRuntimeAgentCompletion(agentId)) {
                      void ctx.notifyAgentTaskCompleted(agentId);
                  }
              }
          });
      }
      ctx.agentRuntimeStateSnapshot = new Map(reconciledStateMap);
      ctx.agentRuntimeStateHydrated = true;
      ctx.agentRuntimeStateMap.value = reconciledStateMap;
  };
}
  installMessengerControllerAgentRuntimeSignals(ctx);
}

function installPart20(ctx: any): void {
// Agent main-session unread state, preferred session prefetching, and unread persistence.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerAgentUnreadRuntime(ctx: MessengerControllerContext): void {
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
          const main = sorted[0];
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
      if (false) {
          ctx.queuedSessionDetailPrefetchIds.clear();
          return;
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
      if (false) {
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
          const cachedMessages = ctx.chatStore.getCachedSessionMessages(entry.sessionId);
          if (false && !Array.isArray(cachedMessages)) {
              return;
          }
          const messages = Array.isArray(cachedMessages) ? cachedMessages : [];
          if (!messages.length && false) {
              return;
          }
          const countUnread = (items: Record<string, unknown>[]): number =>
            items.filter((message: Record<string, unknown>) => {
              if (String(message?.role || '') !== 'assistant') {
                  return false;
              }
              const timestamp = ctx.normalizeTimestamp(message?.created_at);
              return timestamp > readAt;
            }).length;
          let unreadCount = countUnread(messages);
          if (!messages.length) {
              const response = await getChatSessionApi(entry.sessionId);
              const transcript = Array.isArray(response?.data?.data?.transcript)
                  ? response.data.data.transcript
                  : [];
              unreadCount = countUnread(transcript);
          }
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
  installMessengerControllerAgentUnreadRuntime(ctx);
}

function installPart21(ctx: any): void {
// Agent settings save/delete reactions, section selection fallback, local commands, agent send, and stop actions.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerAgentMessageCommands(ctx: MessengerControllerContext): void {
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

  const hasRetainedMessageConversationContext = (options: {
      includeActiveConversation?: boolean;
  } = {}): boolean => {
      return resolveRetainedMessageConversationContext({
          foregroundLock: ctx.agentSendForegroundLock.value,
          activeConversationId: options.includeActiveConversation === true
              ? String(ctx.sessionHub.activeConversation?.id || '').trim()
              : '',
          routeConversationId: ctx.route.query?.conversation_id,
          routeSessionId: ctx.route.query?.session_id,
          routeAgentId: ctx.route.query?.agent_id,
          routeEntry: ctx.route.query?.entry,
          activeSessionId: ctx.chatStore.activeSessionId,
          draftAgentId: ctx.chatStore.draftAgentId,
          messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length
      });
  };

  ctx.hasRetainedMessageConversationContext = computed(() => hasRetainedMessageConversationContext());

  ctx.clearMessagePanelWhenConversationEmpty = () => {
      if (ctx.sessionHub.activeSection !== 'messages')
          return;
      if (hasRetainedMessageConversationContext()) {
          chatDebugLog('messenger.conversation', 'skip-clear-empty-panel', {
              activeConversation: ctx.sessionHub.activeConversation,
              activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
              draftAgentId: String(ctx.chatStore.draftAgentId || '').trim(),
              messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length
          });
          return;
      }
      if (ctx.sessionHub.activeConversation) {
          ctx.sessionHub.clearActiveConversation();
      }
      if (String(ctx.chatStore.activeSessionId || '').trim() ||
          String(ctx.chatStore.draftAgentId || '').trim() ||
          ctx.resolveActiveAgentRenderableMessageRecords().length > 0) {
          ctx.chatStore.activeSessionId = null;
          ctx.chatStore.draftAgentId = '';
          ctx.chatStore.draftToolOverrides = null;
          ctx.chatStore.messages = [];
      }
  };



  ctx.syncAgentConversationFallback = () => {
      if (ctx.sessionHub.activeSection !== 'messages')
          return;
      if (ctx.sessionHub.activeConversation)
          return;
      const routeConversationId = String(ctx.route.query?.conversation_id || '').trim();
      if (routeConversationId)
          return;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (sessionId) {
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === sessionId);
          ctx.sessionHub.setActiveConversation({
              kind: 'agent',
              id: sessionId,
              agentId: ctx.normalizeAgentId(session?.agent_id ?? ctx.chatStore.draftAgentId)
          });
          chatDebugLog('messenger.conversation', 'restore-agent-session', {
              sessionId,
              agentId: ctx.normalizeAgentId(session?.agent_id ?? ctx.chatStore.draftAgentId)
          });
          return;
      }
      if (!String(ctx.chatStore.draftAgentId || '').trim() && !ctx.resolveActiveAgentRenderableMessageRecords().length) {
          ctx.clearMessagePanelWhenConversationEmpty();
          return;
      }
      const draftAgent = ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.selectedAgentId.value);
      ctx.sessionHub.setActiveConversation({
          kind: 'agent',
          id: `draft:${draftAgent}`,
          agentId: draftAgent
      });
      chatDebugLog('messenger.conversation', 'restore-agent-draft', {
          draftAgentId: draftAgent,
          messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length
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

  ctx.resolveCommandErrorMessage = (error: unknown): string => {
      const data = (error as {
          response?: {
              data?: {
                  detail?: string | { message?: string };
                  error?: { message?: string };
              };
          };
          message?: string;
      })?.response?.data;
      // Backend error payloads wrap the message in an object; unwrap it so the
      // toast never renders "[object Object]".
      const detail = data?.detail;
      const detailMessage = typeof detail === 'string' ? detail : String(detail?.message || '').trim();
      return String(detailMessage || data?.error?.message || (error as { message?: string })?.message || ctx.t('common.requestFailed')).trim();
  };

  ctx.appendAgentLocalCommandMessages = async (commandText: string, replyText: string) => {
      let sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId) {
          const targetAgent = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
          sessionId = await ctx.openOrReuseFreshAgentSession(targetAgent, {
              reuseScope: 'active_only'
          });
          if (sessionId) {
              void ctx.openAgentSession(sessionId, targetAgent);
          }
      }
      if (!sessionId) {
          ElMessage.info(replyText);
          return;
      }
      const localTurnId = `local-command-turn:${sessionId || 'draft'}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
      const localModelTurnId = `${localTurnId}:model`;
      ctx.chatStore.appendLocalMessage('user', commandText, { sessionId, localTurnId });
      ctx.chatStore.appendLocalMessage('assistant', replyText, { sessionId, localTurnId, localModelTurnId });
  };

  let goalSubmitting = false;

  const applyGoalCommandUserRound = (sessionId: string, commandMessage, userRound) => {
      const acceptedRound = Number(userRound);
      if (!commandMessage || !Number.isFinite(acceptedRound) || acceptedRound <= 0) {
          return;
      }
      const round = Math.trunc(acceptedRound);
      const canonicalTurnId = `user-turn:${sessionId}:round:${round}`;
      commandMessage.user_turn_id = canonicalTurnId;
      commandMessage.userTurnId = canonicalTurnId;
      commandMessage.user_round = round;
      commandMessage.stream_round = round;
      bindRuntimeMessageToUserRound(ctx.chatStore, sessionId, commandMessage.message_id, round);
      const targetMessages = Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [];
      cacheSessionMessages(sessionId, targetMessages);
      touchSessionUpdatedAt(ctx.chatStore, sessionId, Date.now());
      notifySessionSnapshot(ctx.chatStore, sessionId, targetMessages, true);
  };

  ctx.submitGoalDialog = async (objectiveOverride = '', commandText = '') => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const objective = String(objectiveOverride || '').trim();
      if (!sessionId) {
          ElMessage.warning(ctx.t('chat.command.goalMissingSession'));
          return;
      }
      if (!objective) {
          ElMessage.warning(ctx.t('chat.goal.objectiveRequired'));
          return;
      }
      if (goalSubmitting) {
          return;
      }
      goalSubmitting = true;
      try {
          const currentGoal = typeof ctx.chatStore.sessionGoal === 'function'
              ? ctx.chatStore.sessionGoal(sessionId)
              : null;
          const currentObjective = String(currentGoal?.objective || '').trim();
          const currentStatus = String(currentGoal?.status || '').trim().toLowerCase();
          if (currentStatus === 'active' && currentObjective && currentObjective !== objective) {
              await ctx.chatStore.stopStream();
          }
          const commandLabel = String(commandText || '').trim();
          const commandMessage = commandLabel
              ? ctx.chatStore.appendLocalMessage('user', commandLabel, { sessionId, goalCommand: true })
              : null;
          const result = await ctx.chatStore.setSessionGoal(sessionId, { objective, approval_mode: 'full_auto' });
          applyGoalCommandUserRound(sessionId, commandMessage, result?.user_round);
          const savedObjective = String(result?.goal?.objective || objective).trim();
          ElMessage.success(ctx.t('chat.command.goalSet', { objective: savedObjective }));
      }
      catch (error) {
          ElMessage.warning(ctx.t('chat.command.goalFailed', { message: ctx.resolveCommandErrorMessage(error) }));
      }
      finally {
          goalSubmitting = false;
      }
  };

  ctx.handleAgentLocalCommand = async (command: AgentLocalCommand, rawText: string) => {
      if (command === 'help') {
          await ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.help'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'new') {
          try {
              await ctx.runStartNewSession({ notify: true });
          }
          catch (error) {
              await ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.newFailed', { message: ctx.resolveCommandErrorMessage(error) }));
          }
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'stop') {
          const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (!sessionId) {
              await ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.stopNoSession'));
              await ctx.scrollMessagesToBottom();
              return;
          }
          const cancelled = await ctx.chatStore.stopStream();
          await ctx.appendAgentLocalCommandMessages(rawText, cancelled ? ctx.t('chat.command.stopRequested') : ctx.t('chat.command.stopNoRunning'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      if (command === 'goal') {
          const goalSessionId = String(ctx.chatStore.activeSessionId || '').trim();
          if (!goalSessionId) {
              ElMessage.warning(ctx.t('chat.command.goalMissingSession'));
              return;
          }
          const args = rawText.replace(/^\/+goal\b/i, '').trim();
          const action = args.split(/\s+/, 1)[0].trim().toLowerCase();
          if (action === 'pause' || action === 'clear') {
              ElMessage.warning(ctx.t('chat.command.goalExitViaStop'));
              return;
          }
          if (action === 'resume') {
              if (goalSubmitting) {
                  return;
              }
              goalSubmitting = true;
              const commandMessage = ctx.chatStore.appendLocalMessage('user', rawText, {
                  sessionId: goalSessionId,
                  goalCommand: true
              });
              try {
                  const result = await ctx.chatStore.setSessionGoal(goalSessionId, { status: 'active', approval_mode: 'full_auto' });
                  applyGoalCommandUserRound(goalSessionId, commandMessage, result?.user_round);
                  ElMessage.success(ctx.t('chat.command.goalResumed'));
              }
              catch (error) {
                  ElMessage.warning(ctx.t('chat.command.goalFailed', { message: ctx.resolveCommandErrorMessage(error) }));
              }
              finally {
                  goalSubmitting = false;
              }
              await ctx.scrollMessagesToBottom();
              return;
          }
          if (!args) {
              try {
                  const goal = await ctx.chatStore.refreshSessionGoal(goalSessionId);
                  const objective = String(goal?.objective || '').trim();
                  if (objective) {
                      ElMessage.info(ctx.t('chat.command.goalStatus', {
                          objective,
                          status: String(goal?.status || '').trim() || '-'
                      }));
                  }
                  else {
                      ElMessage.info(ctx.t('chat.command.goalNone'));
                  }
              }
              catch (error) {
                  ElMessage.warning(ctx.t('chat.command.goalFailed', { message: ctx.resolveCommandErrorMessage(error) }));
              }
              return;
          }
          await ctx.submitGoalDialog(args, rawText);
          await ctx.scrollMessagesToBottom();
          return;
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId) {
          await ctx.appendAgentLocalCommandMessages(rawText, ctx.t('chat.command.compactMissingSession'));
          await ctx.scrollMessagesToBottom();
          return;
      }
      const commandMessage = ctx.chatStore.appendLocalMessage('user', rawText, {
          sessionId,
          manualCompactionCommand: true
      });
      try {
          await ctx.chatStore.compactSession(
              sessionId,
              {},
              String(commandMessage?.message_id || '')
          );
      }
      catch { }
      await ctx.scrollMessagesToBottom();
  };

  ctx.sendAgentMessage = async (payload: {
      content?: string;
      attachments?: unknown[];
      reasoningEffort?: string;
      approvalMode?: string;
  }) => {
      if (ctx.isMessengerInteractionBlocked.value) {
          chatDebugLog('messenger.send', 'blocked-send-during-interaction-lock', ctx.buildActiveSessionBusyDebugSnapshot());
          return;
      }
      const content = String(payload?.content || '').trim();
      const attachments = Array.isArray(payload?.attachments) ? payload.attachments : [];
      const reasoningEffort = String(payload?.reasoningEffort || '').trim();
      // §8.4: the composer tier wins over the agent default so a switch applies
      // to the tool calls of this very turn (backend: request > agent > config).
      const approvalMode = normalizeAgentApprovalMode(
          payload?.approvalMode || ctx.composerApprovalMode?.value || 'full_auto'
      );
      const activeInquiry = ctx.activeAgentInquiryPanel.value;
      const inquiryAnswers = ctx.agentInquirySelection.value;
      const hasInquirySelection = inquiryAnswers.length > 0;
      if (!content && attachments.length === 0 && !hasInquirySelection)
          return;
      const localCommand = ctx.parseAgentLocalCommand(content);
      if (localCommand && !hasInquirySelection) {
          if (activeInquiry) {
              ctx.chatStore.resolveInquiryPanel(activeInquiry.message, { status: 'dismissed' });
          }
          if (attachments.length > 0) {
              await ctx.appendAgentLocalCommandMessages(content, ctx.t('chat.command.attachmentsUnsupported'));
              ctx.agentInquirySelection.value = [];
              await ctx.scrollMessagesToBottom();
              return;
          }
          if (ctx.activeSessionOrchestrationLocked.value && localCommand !== 'stop') {
              ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
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
      let finalContent = content;
      if (activeInquiry) {
          if (hasInquirySelection) {
              ctx.chatStore.resolveInquiryPanel(activeInquiry.message, {
                  status: 'answered',
                  selected: inquiryAnswers.flatMap((item) =>
                      item.labels.length ? item.labels : [item.other || ctx.t('chat.inquiry.noPreference')]
                  ),
                  answers: inquiryAnswers
              });
              const selectionText = ctx.buildAgentInquiryReply(activeInquiry.panel, inquiryAnswers);
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
      chatDebugLog('messenger.send', 'controller-send-start', {
          activeSessionId: String(ctx.chatStore.activeSessionId || '').trim(),
          draftAgentId: String(ctx.chatStore.draftAgentId || '').trim(),
          activeConversation: ctx.activeConversation.value,
          targetAgentId,
          messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length,
          contentLength: finalContent.length,
          attachmentCount: attachments.length
      });
      const activeSessionIdBeforeSend = String(ctx.chatStore.activeSessionId || '').trim();
      ctx.agentSendForegroundLock.value = true;
      ctx.agentSendForegroundLockSessionId.value = activeSessionIdBeforeSend || `draft:${targetAgentId}`;
      ctx.autoStickToBottom.value = true;
      ctx.setRuntimeStateOverride(targetAgentId, 'running', 30000);
      // A short task can finish before the next runtime poll observes it. Record
      // the local transition so its terminal snapshot still produces one notice.
      const localRuntimeState = new Map(ctx.agentRuntimeStateMap.value);
      localRuntimeState.set(targetAgentId, 'running');
      ctx.handleAgentRuntimeStateUpdate(localRuntimeState);
      ctx.pendingAssistantCenter = true;
      ctx.pendingAssistantCenterCount = ctx.resolveActiveAgentRenderableMessageRecords().length;
      try {
          await ctx.chatStore.sendMessage(finalContent, {
              attachments,
              suppressQueuedNotice: hasInquirySelection,
              approvalMode,
              ...(reasoningEffort ? { reasoningEffort } : {})
          });
          if (ctx.chatStore.activeSessionId) {
              ctx.sessionHub.setActiveConversation({
                  kind: 'agent',
                  id: String(ctx.chatStore.activeSessionId),
                  agentId: ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.activeAgentId.value)
              });
          }
          await ctx.scrollMessagesToBottom();
          void ctx.loadRunningAgents({ force: true });
      }
      catch (error) {
          ctx.pendingAssistantCenter = false;
          ctx.pendingAssistantCenterCount = 0;
          ctx.setRuntimeStateOverride(targetAgentId, 'error', 8000);
          const failedRuntimeState = new Map(ctx.agentRuntimeStateMap.value);
          failedRuntimeState.set(targetAgentId, 'error');
          ctx.handleAgentRuntimeStateUpdate(failedRuntimeState);
          showApiError(error, ctx.t('chat.error.requestFailed'));
      }
      finally {
          ctx.agentSendForegroundLock.value = false;
          ctx.agentSendForegroundLockSessionId.value = '';
          ctx.agentInquirySelection.value = [];
      }
  };

  ctx.stopAgentMessage = async () => {
      if (ctx.isMessengerInteractionBlocked.value) {
          chatDebugLog('messenger.send', 'blocked-stop-during-interaction-lock', ctx.buildActiveSessionBusyDebugSnapshot());
          return;
      }
      const targetSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const resolveStopSnapshot = () => {
          const currentSessionId = String(ctx.chatStore.activeSessionId || '').trim();
          const targetMessages = targetSessionId && targetSessionId === currentSessionId
              ? ctx.resolveActiveAgentRenderableMessageRecords()
              : ctx.chatStore.getCachedSessionMessages(targetSessionId);
          return captureStopRunSnapshot({
              sessionId: targetSessionId,
              messages: targetMessages,
              busy: targetSessionId ? (ctx.resolveEffectiveSessionBusy(targetSessionId, targetMessages) ||
                  ctx.chatStore.isSessionGoalLocked?.(targetSessionId) === true) : false
          });
      };
      const stopSnapshot = resolveStopSnapshot();
      chatDebugLog('messenger.send', 'manual-stop-click', {
          ...ctx.buildActiveSessionBusyDebugSnapshot(),
          stopSnapshot
      });
      if (!stopSnapshot.sessionId || !stopSnapshot.busy) {
          ElMessage.info(ctx.t('chat.command.stopNoRunning'));
          return;
      }
      const currentStopSnapshot = resolveStopSnapshot();
      const stopDecision = validateStopRunSnapshot(
          stopSnapshot,
          currentStopSnapshot,
          ctx.chatStore.activeSessionId
      );
      if (!stopDecision.ok) {
          chatDebugLog('messenger.send', 'manual-stop-stale-confirmation', {
              reason: stopDecision.reason,
              expected: stopSnapshot,
              current: currentStopSnapshot
          });
          if (stopDecision.reason === 'session_changed') {
              ElMessage.info(ctx.t('chat.command.stopNoRunning'));
              return;
          }
      }
      const targetAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      ctx.setRuntimeStateOverride(targetAgentId, 'done', 20000);
      ctx.pendingAssistantCenter = false;
      ctx.pendingAssistantCenterCount = 0;
      try {
          const stopped = await ctx.chatStore.stopSessionActivity(targetSessionId, { terminateSubagents: true });
          chatDebugLog('messenger.send', 'manual-stop-finish', {
              sessionId: targetSessionId,
              stopped,
              stopSnapshot,
              currentStopSnapshot
          });
      }
      catch (error) {
          chatDebugLog('messenger.send', 'manual-stop-failed', {
              sessionId: targetSessionId,
              message: String((error as { message?: unknown })?.message || '')
          });
      }
  };
}
  installMessengerControllerAgentMessageCommands(ctx);
}

function installPart22(ctx: any): void {
// Language switching, desktop update checks, send-key/profile preferences, approvals, theme, and debug tools.

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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerClientPreferenceActions(ctx: MessengerControllerContext): void {
  ctx.toggleLanguage = async () => {
      const next = getCurrentLanguage() === 'zh-CN' ? 'en-US' : 'zh-CN';
      await setLanguage(next);
      ElMessage.success(ctx.t('messenger.more.languageChanged'));
  };

  ctx.normalizeDesktopUpdatePhase = (state?: DesktopUpdateState | null) => String(state?.phase || '')
      .trim()
      .toLowerCase();

  ctx.resolveDesktopUpdateProgress = (state?: DesktopUpdateState | null) => {
      const raw = Number(state?.progress);
      if (!Number.isFinite(raw)) {
          return 0;
      }
      return Math.max(0, Math.min(100, Math.round(raw)));
  };

  ctx.isDesktopUpdatePending = (phase: string) => phase === 'checking' || phase === 'available' || phase === 'downloading';

  ctx.isDesktopUpdateTerminal = (phase: string) => phase === 'downloaded' ||
      phase === 'error' ||
      phase === 'not-available' ||
      phase === 'idle' ||
      phase === 'unsupported';

  ctx.buildDesktopUpdateStatusText = (state?: DesktopUpdateState | null) => {
      const phase = ctx.normalizeDesktopUpdatePhase(state);
      if (phase === 'checking') {
          return ctx.t('desktop.settings.checkingUpdate');
      }
      if (phase === 'downloading' || phase === 'available') {
          const progress = ctx.resolveDesktopUpdateProgress(state);
          if (progress > 0) {
              return ctx.t('desktop.settings.updateDownloadingProgress', { progress });
          }
          return ctx.t('desktop.settings.updateDownloading');
      }
      return ctx.t('desktop.settings.updateDownloading');
  };

  ctx.wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  ctx.pollDesktopUpdateState = async (bridge: DesktopBridge, initialState: DesktopUpdateState, onTick: (state: DesktopUpdateState) => void) => {
      if (typeof bridge.getUpdateState !== 'function') {
          onTick(initialState);
          return initialState;
      }
      let state = initialState;
      const started = Date.now();
      const timeoutMs = 15 * 60 * 1000;
      while (Date.now() - started < timeoutMs) {
          onTick(state);
          const phase = ctx.normalizeDesktopUpdatePhase(state);
          if (ctx.isDesktopUpdateTerminal(phase) || !ctx.isDesktopUpdatePending(phase)) {
              return state;
          }
          await ctx.wait(700);
          try {
              state = await bridge.getUpdateState();
          }
          catch {
              return state;
          }
      }
      return state;
  };

  ctx.checkClientUpdate = async () => {
      if (!false) {
          ElMessage.success(ctx.t('common.refreshSuccess'));
          return;
      }
      const bridge = ctx.getDesktopBridge();
      if (!bridge || typeof bridge.checkForUpdates !== 'function') {
          ElMessage.warning(ctx.t('desktop.settings.updateUnsupported'));
          return;
      }
      const loading = ElLoading.service({
          lock: false,
          text: ctx.t('desktop.settings.checkingUpdate'),
          background: 'rgba(0, 0, 0, 0.06)'
      });
      try {
          let state = await bridge.checkForUpdates();
          let lastStatusText = '';
          const updateLoadingText = (nextState: DesktopUpdateState) => {
              const nextText = ctx.buildDesktopUpdateStatusText(nextState);
              if (nextText && nextText !== lastStatusText) {
                  loading.setText(nextText);
                  lastStatusText = nextText;
              }
          };
          state = await ctx.pollDesktopUpdateState(bridge, state, updateLoadingText);
          loading.close();
          const phase = String(state?.phase || '').trim().toLowerCase();
          const latestVersion = String(state?.latestVersion || '').trim();
          if (phase === 'not-available' || phase === 'idle') {
              ElMessage.success(ctx.t('desktop.settings.updateNotAvailable'));
              return;
          }
          if (phase === 'unsupported') {
              ElMessage.warning(ctx.t('desktop.settings.updateUnsupported'));
              return;
          }
          if (phase === 'error') {
              const reason = String(state?.message || '').trim() || ctx.t('common.unknown');
              ElMessage.error(ctx.t('desktop.settings.updateCheckFailed', { reason }));
              return;
          }
          if (phase === 'downloading' || phase === 'available' || phase === 'checking') {
              const progress = ctx.resolveDesktopUpdateProgress(state);
              if (progress > 0) {
                  ElMessage.info(ctx.t('desktop.settings.updateDownloadingProgress', { progress }));
              }
              else {
                  ElMessage.info(ctx.t('desktop.settings.updateDownloading'));
              }
              return;
          }
          if (phase !== 'downloaded') {
              ElMessage.info(ctx.t('desktop.settings.updateUnknownState'));
              return;
          }
          const versionText = latestVersion || String(state?.currentVersion || '-');
          const confirmed = await confirmWithFallback(ctx.t('desktop.settings.updateReadyConfirm', { version: versionText }), ctx.t('desktop.settings.update'), {
              type: 'warning',
              confirmButtonText: ctx.t('desktop.settings.installNow'),
              cancelButtonText: ctx.t('common.cancel')
          });
          if (!confirmed) {
              ElMessage.info(ctx.t('desktop.settings.updateReadyLater'));
              return;
          }
          if (typeof bridge.installUpdate !== 'function') {
              ElMessage.warning(ctx.t('desktop.settings.updateUnsupported'));
              return;
          }
          const installResult = await bridge.installUpdate();
          const installOk = typeof installResult === 'boolean' ? installResult : Boolean((installResult as DesktopInstallResult)?.ok);
          if (!installOk) {
              ElMessage.warning(ctx.t('desktop.settings.updateInstallFailed'));
              return;
          }
          ElMessage.success(ctx.t('desktop.settings.updateInstalling'));
      }
      catch (error) {
          loading.close();
          const reason = String((error as {
              message?: unknown;
          })?.message || '').trim() || ctx.t('common.unknown');
          ElMessage.error(ctx.t('desktop.settings.updateCheckFailed', { reason }));
      }
  };

  ctx.updateSendKey = (value: MessengerSendKeyMode) => {
      const normalized = ctx.normalizeMessengerSendKey(value);
      ctx.messengerSendKey.value = normalized;
      if (typeof window !== 'undefined') {
          window.localStorage.setItem(MESSENGER_SEND_KEY_STORAGE_KEY, normalized);
      }
  };

  ctx.updateCurrentUsername = async (value: string) => {
      const normalized = String(value || '').trim();
      if (!normalized) {
          ElMessage.warning(ctx.t('profile.edit.usernameRequired'));
          return;
      }
      const current = String((ctx.authStore.user as Record<string, unknown> | null)?.username || '').trim();
      if (current === normalized || ctx.usernameSaving.value) {
          return;
      }
      ctx.usernameSaving.value = true;
      try {
          const { data } = await updateProfile({ username: normalized });
          const profile = data?.data;
          if (profile && typeof profile === 'object') {
              ctx.authStore.user = profile;
          }
          else {
              await ctx.authStore.loadProfile();
          }
          ElMessage.success(ctx.t('profile.edit.saved'));
      }
      catch (error) {
          showApiError(error, ctx.t('profile.edit.saveFailed'));
      }
      finally {
          ctx.usernameSaving.value = false;
      }
  };

  ctx.handleSessionApprovalDecision = async (decision: 'approve_once' | 'approve_session' | 'deny') => {
      const approval = ctx.activeSessionApproval.value;
      if (!approval || ctx.approvalResponding.value)
          return;
      ctx.approvalResponding.value = true;
      try {
          await ctx.chatStore.respondApproval(decision, approval.approval_id);
          if (decision !== 'deny') {
              ElMessage.success(ctx.t('chat.approval.sent'));
          }
      }
      catch (error) {
          showApiError(error, ctx.t('chat.approval.sendFailed'));
      }
      finally {
          ctx.approvalResponding.value = false;
      }
  };

  ctx.updateThemePalette = (value: ThemePalette) => {
      ctx.themeStore.setPalette(normalizeThemePalette(value));
  };

  ctx.updateUiFontSize = (value: number) => {
      const normalized = ctx.normalizeUiFontSize(value);
      ctx.uiFontSize.value = normalized;
      if (typeof window !== 'undefined') {
          window.localStorage.setItem(MESSENGER_UI_FONT_SIZE_STORAGE_KEY, String(normalized));
      }
      ctx.applyUiFontSize(normalized);
  };

  ctx.openDebugTools = async () => {
      if (typeof window === 'undefined')
          return;
      try {
          const bridge = ctx.getDesktopBridge();
          if (typeof bridge?.toggleDevTools === 'function') {
              await bridge.toggleDevTools();
              return;
          }
      }
      catch {
          ElMessage.warning(ctx.t('desktop.common.saveFailed'));
          return;
      }
      ElMessage.info(ctx.t('messenger.settings.debugHint'));
  };

  ctx.shouldReuseAgentMetaResult = (loadedAt: number, force = false): boolean => !force && loadedAt > 0 && Date.now() - loadedAt < ctx.AGENT_META_REQUEST_CACHE_MS;
}
  installMessengerControllerClientPreferenceActions(ctx);
}

function installPart23(ctx: any): void {
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

type AgentInquiryPanelOption = { label: string; description?: string; recommended?: boolean };

type AgentInquiryPanelQuestion = {
  question: string;
  options: AgentInquiryPanelOption[];
  multiple: boolean;
};

/** 一题的作答结果：选中项标签、自由文本，或明确「无偏好」。 */
type AgentInquiryPanelAnswer = {
  questionIndex: number;
  labels: string[];
  other: string;
  noPreference: boolean;
};

type AgentInquiryPanelData = {
  questions?: AgentInquiryPanelQuestion[];
  status?: string;
  selected?: string[];
  answers?: AgentInquiryPanelAnswer[];
};

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

function installMessengerControllerSharedHelpers(ctx: MessengerControllerContext): void {
  ctx.resolveRouteSettingsPanelMode = function resolveRouteSettingsPanelMode(routePath: string, panelValue: unknown, desktopEnabled: boolean): SettingsPanelMode {
      const path = String(routePath || '').trim().toLowerCase();
      const panelHint = String(panelValue || '').trim().toLowerCase();
      if (path.includes('/profile')) {
          return 'profile';
      }
      if (panelHint === 'profile') {
          return 'profile';
      }
      if (panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt') {
          return 'prompts';
      }
      if (panelHint === 'help-manual' ||
          panelHint === 'manual' ||
          panelHint === 'help' ||
          panelHint === 'docs' ||
          panelHint === 'docs-site') {
          return 'help-manual';
      }
      if (desktopEnabled && panelHint === 'desktop-models') {
          return 'desktop-models';
      }
      if (desktopEnabled && panelHint === 'desktop-lan') {
          return 'desktop-lan';
      }
      return 'general';
  };

  ctx.resolveRouteHelperWorkspaceEnabled = function resolveRouteHelperWorkspaceEnabled(sectionValue: unknown, helperValue: unknown): boolean {
      const sectionHint = String(sectionValue || '').trim().toLowerCase();
      const helperHint = String(helperValue || '').trim().toLowerCase();
      return (sectionHint === 'groups' &&
          (helperHint === '1' || helperHint === 'true' || helperHint === 'yes'));
  };

  ctx.setNavigationPaneCollapsed = function setNavigationPaneCollapsed(collapsed: boolean): void {
      if (!ctx.allowNavigationCollapse.value) {
          ctx.standardNavigationCollapsed.value = false;
          return;
      }
      ctx.standardNavigationCollapsed.value = collapsed;
      if (collapsed) {
          ctx.leftRailMoreExpanded.value = false;
          ctx.clearMiddlePaneOverlayHide();
          ctx.middlePaneOverlayVisible.value = false;
          return;
      }
      if (ctx.isMiddlePaneOverlay.value) {
          ctx.openMiddlePaneOverlay();
      }
  };

  ctx.toggleNavigationPaneCollapsed = function toggleNavigationPaneCollapsed(): void {
      ctx.setNavigationPaneCollapsed(!ctx.navigationPaneCollapsed.value);
  };

  ctx.resolveChatShellPath = function resolveChatShellPath(): string {
      return ctx.isEmbeddedChatRoute.value ? String(ctx.route.path || '').trim() : `${ctx.basePrefix.value}/chat`;
  };

  ctx.readServerDefaultModelName = async function readServerDefaultModelName(force = false): Promise<string> {
      if (false) {
          ctx.serverDefaultModelDisplayName.value = '';
          return '';
      }
      const now = Date.now();
      if (!force &&
          String(ctx.serverDefaultModelDisplayName.value || '').trim() &&
          now - ctx.serverDefaultModelCheckedAt <= ctx.SERVER_DEFAULT_MODEL_CACHE_MS) {
          return String(ctx.serverDefaultModelDisplayName.value || '').trim();
      }
      if (ctx.serverDefaultModelFetchPromise) {
          return ctx.serverDefaultModelFetchPromise;
      }
      ctx.serverDefaultModelFetchPromise = (async () => {
          try {
              const profile = ((await ctx.agentStore.getAgent(DEFAULT_AGENT_KEY, { force }).catch(() => null)) as Record<string, unknown> | null) || null;
              if (profile) {
                  ctx.defaultAgentProfile.value = profile;
              }
              const resolved = String(ctx.resolveModelNameFromRecord(profile) || '').trim();
              ctx.serverDefaultModelDisplayName.value = resolved;
              return resolved;
          }
          finally {
              ctx.serverDefaultModelCheckedAt = Date.now();
              ctx.serverDefaultModelFetchPromise = null;
          }
      })();
      return ctx.serverDefaultModelFetchPromise;
  };

  ctx.normalizeRightDockSkillRuntimeName = function normalizeRightDockSkillRuntimeName(value: unknown): string {
      const normalized = String(value || '').trim();
      if (!normalized)
          return '';
      if (ctx.rightDockSkillCatalog.value.some((item) => item.name === normalized)) {
          return normalized;
      }
      const separatorIndex = normalized.indexOf('@');
      if (separatorIndex <= 0 || separatorIndex >= normalized.length - 1) {
          return normalized;
      }
      const legacyName = normalized.slice(separatorIndex + 1).trim();
      if (!legacyName) {
          return normalized;
      }
      return ctx.rightDockSkillCatalog.value.some((item) => item.name === legacyName)
          ? legacyName
          : normalized;
  };

  ctx.normalizeRightDockSkillNameList = function normalizeRightDockSkillNameList(values: string[]): string[] {
      const output: string[] = [];
      const seen = new Set<string>();
      values.forEach((value) => {
          const normalized = ctx.normalizeRightDockSkillRuntimeName(value);
          if (!normalized || seen.has(normalized)) {
              return;
          }
          seen.add(normalized);
          output.push(normalized);
      });
      return output;
  };

  ctx.resolveSessionActivityTimestamp = function resolveSessionActivityTimestamp(session: Record<string, unknown>): number {
      // Keep conversation ordering aligned to real message activity to avoid list jumps on UI-only updates.
      const fieldTimestamp = ctx.normalizeTimestamp(session.last_message_at || session.updated_at || session.created_at);
      const sessionId = String(session?.id || session?.session_id || '').trim();
      if (!sessionId) {
          return fieldTimestamp;
      }
      const cachedMessages = ctx.chatStore.getCachedSessionMessages(sessionId);
      const messageTimestamp = ctx.resolveLatestConversationMessageTimestamp(
          Array.isArray(cachedMessages) ? cachedMessages as unknown[] : []
      );
      return Math.max(fieldTimestamp, messageTimestamp);
  };

  ctx.resolveSessionRecordById = function resolveSessionRecordById(sessionId: unknown): Record<string, unknown> | null {
      const targetId = String(sessionId || '').trim();
      if (!targetId)
          return null;
      return ((Array.isArray(ctx.chatStore.sessions)
          ? ctx.chatStore.sessions.find((item) => String(item?.id || item?.session_id || '').trim() === targetId)
          : null) || null) as Record<string, unknown> | null;
  };

  ctx.resolveSessionAgentId = function resolveSessionAgentId(sessionOrId: unknown, fallbackAgentId: unknown = ''): string {
      const session = typeof sessionOrId === 'string'
          ? ctx.resolveSessionRecordById(sessionOrId)
          : (sessionOrId && typeof sessionOrId === 'object' && !Array.isArray(sessionOrId)
              ? (sessionOrId as Record<string, unknown>)
              : null);
      return ctx.normalizeAgentId(session?.agent_id || (session?.is_default === true ? DEFAULT_AGENT_KEY : '') || fallbackAgentId || DEFAULT_AGENT_KEY);
  };

  ctx.resolveMessengerRootElement = function resolveMessengerRootElement(): HTMLElement | null {
      const root = ctx.messengerRootRef.value as unknown;
      if (!root)
          return null;
      if (root instanceof HTMLElement)
          return root;
      const candidate = (root as {
          $el?: unknown;
      }).$el;
      return candidate instanceof HTMLElement ? candidate : null;
  };

  ctx.measureMessengerLayoutElement = function measureMessengerLayoutElement(element: Element | null): {
      width: number;
      left: number;
      right: number;
  } | null {
      if (!(element instanceof HTMLElement))
          return null;
      const rect = element.getBoundingClientRect();
      const width = Number.isFinite(rect.width) ? Math.round(rect.width) : 0;
      const left = Number.isFinite(rect.left) ? Math.round(rect.left) : 0;
      const right = Number.isFinite(rect.right) ? Math.round(rect.right) : 0;
      return { width, left, right };
  };

  ctx.reportMessengerLayoutAnomaly = function reportMessengerLayoutAnomaly(reason: string): void {
      if (typeof window === 'undefined')
          return;
      const root = ctx.resolveMessengerRootElement();
      if (!root)
          return;
      const rootRect = ctx.measureMessengerLayoutElement(root);
      const parentRect = ctx.measureMessengerLayoutElement(root.parentElement);
      const leftRailRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-left-rail'));
      const middlePaneRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-middle-pane'));
      const chatRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
      const chatBodyRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-chat-body'));
      const footerRect = ctx.measureMessengerLayoutElement(ctx.chatFooterRef.value);
      const composerRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
      const rightDockRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
      const sandboxPanelRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--sandbox'));
      const skillsPanelRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--skills'));
      const snapshot = {
          reason,
          route: ctx.route.fullPath,
          section: ctx.sessionHub.activeSection,
          windowWidth: Math.round(window.innerWidth || 0),
          viewportWidth: Math.round(ctx.viewportWidth.value || 0),
          root: rootRect,
          parent: parentRect,
          leftRail: leftRailRect,
          middlePane: middlePaneRect,
          chat: chatRect,
          chatBody: chatBodyRect,
          footer: footerRect,
          composer: composerRect,
          rightDock: rightDockRect,
          sandboxPanel: sandboxPanelRect,
          skillsPanel: skillsPanelRect,
          showMiddlePane: ctx.showMiddlePane.value,
          showRightDock: ctx.showRightDock.value,
          rightDockCollapsed: ctx.rightDockCollapsed.value,
          navigationPaneCollapsed: ctx.navigationPaneCollapsed.value,
          isMiddlePaneOverlay: ctx.isMiddlePaneOverlay.value,
          isRightDockOverlay: ctx.isRightDockOverlay.value,
          rootClasses: Array.from(root.classList.values()),
          gridTemplateColumns: window.getComputedStyle(root).gridTemplateColumns
      };
      const signature = JSON.stringify({
          reason,
          windowWidth: snapshot.windowWidth,
          viewportWidth: snapshot.viewportWidth,
          root: rootRect,
          chat: chatRect,
          footer: footerRect,
          composer: composerRect,
          rightDock: rightDockRect,
          gridTemplateColumns: snapshot.gridTemplateColumns,
          section: snapshot.section,
          showMiddlePane: snapshot.showMiddlePane,
          showRightDock: snapshot.showRightDock,
          rightDockCollapsed: snapshot.rightDockCollapsed,
          navigationPaneCollapsed: snapshot.navigationPaneCollapsed,
          isMiddlePaneOverlay: snapshot.isMiddlePaneOverlay,
          isRightDockOverlay: snapshot.isRightDockOverlay
      });
      if (signature === ctx.lastMessengerLayoutDebugSignature)
          return;
      ctx.lastMessengerLayoutDebugSignature = signature;
      if (isChatDebugEnabled()) {
          chatDebugLog('messenger.layout', 'anomaly', snapshot);
          console.warn('[messenger-layout-anomaly]', snapshot);
      }
  };

  ctx.detectMessengerLayoutAnomaly = function detectMessengerLayoutAnomaly(): void {
      if (typeof window === 'undefined')
          return;
      const root = ctx.resolveMessengerRootElement();
      if (!root)
          return;
      const rootRect = ctx.measureMessengerLayoutElement(root);
      const chatRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
      const footerRect = ctx.measureMessengerLayoutElement(ctx.chatFooterRef.value);
      const composerRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
      const rightDockRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
      const windowWidth = Math.round(window.innerWidth || 0);
      if (windowWidth <= 0 || !rootRect)
          return;
      if (rootRect.width > 0 && rootRect.width < windowWidth - 240) {
          ctx.reportMessengerLayoutAnomaly('root-too-narrow');
          return;
      }
      if (windowWidth >= 900 &&
          ((chatRect && chatRect.width > 0 && chatRect.width < 220) ||
              (footerRect && footerRect.width > 0 && footerRect.width < 220) ||
              (composerRect && composerRect.width > 0 && composerRect.width < 220))) {
          ctx.reportMessengerLayoutAnomaly('chat-too-narrow');
          return;
      }
      if (ctx.isRightDockOverlay.value &&
          rightDockRect &&
          rightDockRect.width > 0 &&
          !ctx.rightDockCollapsed.value &&
          windowWidth >= ctx.MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT &&
          rootRect.width >= windowWidth - 80 &&
          rightDockRect.left < Math.round(windowWidth * 0.72) &&
          rightDockRect.right < windowWidth - 12) {
          ctx.reportMessengerLayoutAnomaly('overlay-dock-shifted-left');
      }
  };

  ctx.refreshMessengerRootBounds = function refreshMessengerRootBounds(): void {
      const root = ctx.resolveMessengerRootElement();
      if (!root) {
          ctx.cachedMessengerRootRight = 0;
          ctx.cachedMessengerRootWidth = 0;
          return;
      }
      const rect = root.getBoundingClientRect();
      ctx.cachedMessengerRootRight = Number.isFinite(rect.right) ? rect.right : 0;
      ctx.cachedMessengerRootWidth = Number.isFinite(rect.width) ? rect.width : 0;
      ctx.detectMessengerLayoutAnomaly();
  };

  ctx.setRightDockEdgeHover = function setRightDockEdgeHover(next: boolean): void {
      if (ctx.rightDockEdgeHover.value === next)
          return;
      ctx.rightDockEdgeHover.value = next;
  };

  ctx.handleMessengerRootPointerMove = function handleMessengerRootPointerMove(event: PointerEvent | MouseEvent): void {
      if (!ctx.showRightDock.value) {
          ctx.setRightDockEdgeHover(false);
          return;
      }
      const pointerX = Number(event.clientX);
      if (!Number.isFinite(pointerX)) {
          ctx.setRightDockEdgeHover(false);
          return;
      }
      // Root bounds are refreshed when the viewport or dock layout changes. Reading
      // geometry for every pointer frame forces layout and makes long workflow cards jank.
      if (!Number.isFinite(ctx.cachedMessengerRootRight) || ctx.cachedMessengerRootWidth <= 0) {
          ctx.refreshMessengerRootBounds();
      }
      if (!Number.isFinite(ctx.cachedMessengerRootRight) || ctx.cachedMessengerRootWidth <= 0) {
          ctx.setRightDockEdgeHover(false);
          return;
      }
      ctx.setRightDockEdgeHover(pointerX >= ctx.cachedMessengerRootRight - ctx.RIGHT_DOCK_EDGE_HOVER_THRESHOLD);
  };

  ctx.handleMessengerRootPointerLeave = function handleMessengerRootPointerLeave(): void {
      ctx.setRightDockEdgeHover(false);
  };

  ctx.loadAgentToolSummary = async function loadAgentToolSummary(options: {
      force?: boolean;
  } = {}) {
      const force = options.force === true;
      if (ctx.agentToolSummaryPromise) {
          return ctx.agentToolSummaryPromise;
      }
      if (!force && ctx.agentPromptToolSummary.value) {
          return ctx.agentPromptToolSummary.value;
      }
      ctx.agentToolSummaryLoading.value = true;
      ctx.agentToolSummaryError.value = '';
      ctx.agentToolSummaryPromise = (async () => {
          try {
              const summary = (await loadUserToolsSummaryCache({ force })) as Record<string, unknown> | null;
              ctx.agentPromptToolSummary.value = summary;
              return summary;
          }
          catch (error) {
              ctx.agentToolSummaryError.value =
                  (error as {
                      response?: {
                          data?: {
                              detail?: string;
                          };
                      };
                      message?: string;
                  })?.response?.data?.detail ||
                      ctx.t('chat.toolSummaryFailed');
              return null;
          }
          finally {
              ctx.agentToolSummaryLoading.value = false;
              ctx.agentToolSummaryPromise = null;
              if (ctx.agentAbilityTooltipVisible.value) {
                  await ctx.updateAgentAbilityTooltip();
              }
          }
      })();
      return ctx.agentToolSummaryPromise;
  };

  ctx.loadRightDockSkills = async function loadRightDockSkills(options: {
      force?: boolean;
      silent?: boolean;
  } = {}) {
      const force = options.force === true;
      const silent = options.silent !== false;
      if (force) {
          ctx.clearRightDockSkillAutoRetry();
      }
      if (ctx.rightDockSkillCatalogLoading.value && !force) {
          return false;
      }
      const currentVersion = ++ctx.rightDockSkillCatalogLoadVersion;
      ctx.rightDockSkillCatalogLoading.value = true;
      try {
          const skills = await loadUserSkillsCache({ force });
          if (currentVersion !== ctx.rightDockSkillCatalogLoadVersion)
              return;
          ctx.rightDockSkillCatalog.value = ctx.normalizeRightDockSkillCatalog(skills);
          if (!force && ctx.rightDockSkillCatalog.value.length === 0) {
              // First pass may race with startup auth/cache warmup and return empty transiently.
              ctx.scheduleRightDockSkillAutoRetry();
          }
          return true;
      }
      catch (error) {
          if (currentVersion !== ctx.rightDockSkillCatalogLoadVersion)
              return;
          if (!silent) {
              showApiError(error, ctx.t('userTools.skills.loadFailed'));
          }
          if (!force) {
              ctx.scheduleRightDockSkillAutoRetry();
          }
          return false;
      }
      finally {
          if (currentVersion === ctx.rightDockSkillCatalogLoadVersion) {
              ctx.rightDockSkillCatalogLoading.value = false;
          }
      }
  };

  ctx.warmMessengerUserToolsData = function warmMessengerUserToolsData(options: {
      catalog?: boolean;
      skills?: boolean;
      summary?: boolean;
  } = {}) {
      if (options.catalog === true) {
          void loadUserToolsCatalogCache();
      }
      if (options.summary === true) {
          void ctx.loadAgentToolSummary();
      }
      if (options.skills === true) {
          void ctx.loadRightDockSkills({ silent: true });
      }
  };

  ctx.handleDesktopModelMetaChanged = function handleDesktopModelMetaChanged(): void {
      if (!false)
          return;
      ctx.agentVoiceModelSupportCheckedAt = 0;
      ctx.desktopDefaultModelMetaFetchPromise = null;
      void ctx.readDesktopDefaultModelMeta(true);
  };

  ctx.resolveReusableFreshAgentSessionId = function resolveReusableFreshAgentSessionId(targetAgentId: string, options: {
      activeOnly?: boolean;
  } = {}): string {
      return ctx.chatStore.resolveReusableFreshSessionId(targetAgentId, options);
  };

  ctx.openOrReuseFreshAgentSession = async function openOrReuseFreshAgentSession(targetAgentId: string, options: {
      reuseScope?: 'any' | 'active_only' | 'none';
  } = {}): Promise<string> {
      const reuseScope = options.reuseScope || 'any';
      const reusableSessionId = reuseScope === 'none'
          ? ''
          : ctx.resolveReusableFreshAgentSessionId(targetAgentId, {
              activeOnly: reuseScope === 'active_only'
          });
      if (reusableSessionId) {
          return reusableSessionId;
      }
      const payloadAgentId = targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId;
      const session = await ctx.chatStore.createSession(payloadAgentId ? { agent_id: payloadAgentId } : {});
      const sessionId = String((session as Record<string, unknown> | null)?.id || '').trim();
      if (!sessionId)
          return '';
      return sessionId;
  };

  ctx.runStartNewSession = async function runStartNewSession(options: {
      notify?: boolean;
  } = {}): Promise<StartNewSessionOutcome> {
      if (ctx.creatingAgentSession.value || ctx.isMessengerInteractionBlocked.value) {
          return 'noop';
      }
      const targetAgent = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const reusableSessionId = ctx.resolveReusableFreshAgentSessionId(targetAgent);
      if (activeSessionId && reusableSessionId && activeSessionId === reusableSessionId) {
          if (options.notify === true) {
              ElMessage.info(ctx.t('chat.newSessionAlreadyCurrent'));
          }
          return 'already_current';
      }
      // A session write can be delayed by an unavailable local service. Keep
      // only the action button busy: a whole-page blocker makes the shell and
      // its recovery controls unreachable while the request is pending.
      ctx.creatingAgentSession.value = true;
      try {
          const sessionId = await ctx.openOrReuseFreshAgentSession(targetAgent, {
              reuseScope: 'any'
          });
          if (!sessionId)
              return 'noop';
          if (options.notify === true) {
              ElMessage.success(ctx.t('chat.newSessionOpened'));
          }
          // createSession has already installed the new empty thread, greeting,
          // cache entry and realtime watcher. Only synchronize the route here;
          // reopening the same session would duplicate its hydration pipeline.
          void ctx.openAgentSession(sessionId, targetAgent, { skipHydration: true });
          return 'opened';
      }
      finally {
          ctx.creatingAgentSession.value = false;
      }
  };

  ctx.startNewSession = async function startNewSession() {
      try {
          await ctx.runStartNewSession({ notify: true });
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.normalizeAgentId = function normalizeAgentId(value: unknown): string {
      const text = String(value || '').trim();
      return text || DEFAULT_AGENT_KEY;
  };
}
  installMessengerControllerSharedHelpers(ctx);
}

// Install order is load-bearing: several parts call helpers synchronously while
// they are being installed (for example the route bootstrap syncs the settings
// panel mode right away), so a part must never run before the part that defines
// the helpers it uses. The order below mirrors the pre-consolidation installer
// chain: shared helpers -> core state -> navigation lists -> conversation runtime
// -> message resources -> workspace actions -> messaging settings -> lifecycle.
const installParts: Array<(ctx: any) => void> = [
  installPart23,
  installPart0,
  installPart1,
  installPart2,
  installPart3,
  installPart12,
  installPart11,
  installPart20,
  installPart13,
  installPart19,
  installPart6,
  installPart7,
  installPart14,
  installPart5,
  installPart8,
  installPart4,
  installPart9,
  installPart10,
  installPart21,
  installPart22,
  installPart18,
  installPart16,
  installPart15,
  installPart17
];

export function installMessengerController(ctx: MessengerControllerContext): void {
  for (const install of installParts) {
    install(ctx);
  }
}
