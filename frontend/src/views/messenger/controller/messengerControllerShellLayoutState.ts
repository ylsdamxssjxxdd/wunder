// @ts-nocheck
// Messenger shell navigation, desktop mode, responsive panes, and host layout state.
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

export function installMessengerControllerShellLayoutState(ctx: MessengerControllerContext): void {
  ctx.sectionOptions = computed(() => {
      return [
          { key: 'messages' as MessengerSection, icon: 'fa-solid fa-comment-dots', label: ctx.t('messenger.section.messages') },
          { key: 'agents' as MessengerSection, icon: 'fa-solid fa-robot', label: ctx.t('messenger.section.agents') },
          { key: 'swarms' as MessengerSection, icon: 'fa-solid fa-bee', label: ctx.t('messenger.section.swarms') },
          {
              key: 'orchestrations' as MessengerSection,
              icon: 'fa-solid fa-diagram-project',
              label: ctx.t('messenger.section.orchestrations')
          },
          { key: 'users' as MessengerSection, icon: 'fa-solid fa-user-group', label: ctx.t('messenger.section.users') },
          { key: 'groups' as MessengerSection, icon: 'fa-solid fa-comments', label: ctx.t('messenger.section.groups') },
          { key: 'tools' as MessengerSection, icon: 'fa-solid fa-wrench', label: ctx.t('messenger.section.tools') },
          { key: 'files' as MessengerSection, icon: 'fa-solid fa-folder-open', label: ctx.t('messenger.section.files') },
          { key: 'plaza' as MessengerSection, icon: 'fa-solid fa-store', label: ctx.t('messenger.section.plaza') },
          { key: 'more' as MessengerSection, icon: 'fa-solid fa-gear', label: ctx.t('messenger.section.settings') }
      ];
  });

  ctx.leftRailMainSectionOptions = computed(() => ctx.sectionOptions.value.filter((item) => item.key === 'messages' ||
      item.key === 'agents' ||
      item.key === 'tools' ||
      item.key === 'files'));

  ctx.leftRailSocialSectionOptions = computed(() => ctx.sectionOptions.value.filter((item) => item.key === 'swarms' ||
      item.key === 'orchestrations' ||
      item.key === 'users' ||
      item.key === 'groups'));

  ctx.isLeftNavSectionActive = (section: MessengerSection): boolean => {
      return ctx.isSectionButtonActive(section);
  };

  ctx.resolveLeftNavButtonLabel = (section: MessengerSection): string => {
      switch (section) {
          case 'messages':
              return ctx.t('messenger.section.messages');
          case 'agents':
              return ctx.t('messenger.section.agents.short');
          case 'swarms':
              return ctx.t('messenger.section.swarms');
          case 'orchestrations':
              return ctx.t('messenger.section.orchestrations');
          case 'plaza':
              return ctx.t('messenger.section.plaza.short');
          case 'tools':
              return ctx.t('messenger.section.tools');
          case 'files':
              return ctx.t('messenger.section.files');
          default:
              return '';
      }
  };

  ctx.closeLeftRailMoreMenu = () => {
      ctx.leftRailMoreExpanded.value = false;
      ctx.clearMiddlePaneOverlayPreview();
  };

  ctx.toggleLeftRailMoreMenu = () => {
      ctx.clearMiddlePaneOverlayHide();
      ctx.clearMiddlePaneOverlayPreview();
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

  ctx.desktopMode = computed(() => isDesktopModeEnabled());

  ctx.desktopLocalMode = computed(() => ctx.desktopMode.value);

  ctx.settingsLogoutDisabled = computed(() => ctx.desktopMode.value);

  ctx.debugToolsAvailable = computed(() => typeof ctx.getDesktopBridge()?.toggleDevTools === 'function');

  ctx.desktopUpdateAvailable = computed(() => typeof ctx.getDesktopBridge()?.checkForUpdates === 'function');

  ctx.worldDesktopScreenshotSupported = computed(() => ctx.desktopMode.value && typeof ctx.getDesktopBridge()?.captureScreenshot === 'function');

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

  ctx.worldVoiceSupported = computed(() => ctx.audioRecordingSupported.value);

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

  ctx.userWorldPermissionDenied = computed(() => ctx.userWorldStore.permissionDenied === true);

  ctx.activeSectionTitle = computed(() => {
      if (ctx.helperAppsWorkspaceMode.value && ctx.sessionHub.activeSection === 'groups') {
          return ctx.t('userWorld.helperApps.title');
      }
      return ctx.sessionHub.activeSection === 'more'
          ? ctx.t('messenger.section.settings')
          : ctx.t(`messenger.section.${ctx.sessionHub.activeSection}`);
  });

  ctx.activeSectionSubtitle = computed(() => {
      if (ctx.helperAppsWorkspaceMode.value && ctx.sessionHub.activeSection === 'groups') {
          return ctx.t('userWorld.helperApps.subtitle');
      }
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

  const {
    isRightDockResizing,
    rightDockResizable,
    rightDockStyle,
    resetRightDockWidth,
    nudgeRightDockWidth,
    startRightDockResize
  } = useMessengerRightDockResize({
      hostWidth: ctx.viewportWidth,
      isOverlay: ctx.isRightDockOverlay,
      isMiddlePaneOverlay: ctx.isMiddlePaneOverlay,
      navigationPaneCollapsed: ctx.navigationPaneCollapsed,
      collapsed: ctx.rightDockCollapsed,
      storageKey: MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY
  });
  ctx.isRightDockResizing = isRightDockResizing;
  ctx.rightDockResizable = rightDockResizable;
  ctx.rightDockStyle = rightDockStyle;
  ctx.resetRightDockWidth = resetRightDockWidth;
  ctx.nudgeRightDockWidth = nudgeRightDockWidth;
  ctx.startRightDockResize = startRightDockResize;

  ctx.messengerViewStyle = computed(() => ctx.rightDockStyle.value);

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

  const {
    clearMiddlePaneOverlayPreview,
    effectiveHelperAppsWorkspace: showMiddlePaneHelperAppsWorkspace,
    effectiveSearchPlaceholder: middlePaneSearchPlaceholder,
    effectiveSection: middlePaneActiveSection,
    effectiveSectionSubtitle: middlePaneActiveSectionSubtitle,
    effectiveSectionTitle: middlePaneActiveSectionTitle,
    isHelperWorkspaceButtonActive: isHelperAppsMiddlePaneActive,
    isSectionButtonActive,
    queuePreviewMiddlePaneSection,
    previewMiddlePaneSection
  } = useMiddlePaneOverlayPreview({
      activeSection: computed(() => ctx.sessionHub.activeSection),
      helperAppsWorkspaceMode: ctx.helperAppsWorkspaceMode,
      isMiddlePaneOverlay: ctx.isMiddlePaneOverlay,
      middlePaneOverlayVisible: ctx.middlePaneOverlayVisible,
      t: ctx.t
  });
  ctx.clearMiddlePaneOverlayPreview = clearMiddlePaneOverlayPreview;
  ctx.showMiddlePaneHelperAppsWorkspace = showMiddlePaneHelperAppsWorkspace;
  ctx.middlePaneSearchPlaceholder = middlePaneSearchPlaceholder;
  ctx.middlePaneActiveSection = middlePaneActiveSection;
  ctx.middlePaneActiveSectionSubtitle = middlePaneActiveSectionSubtitle;
  ctx.middlePaneActiveSectionTitle = middlePaneActiveSectionTitle;
  ctx.isHelperAppsMiddlePaneActive = isHelperAppsMiddlePaneActive;
  ctx.isSectionButtonActive = isSectionButtonActive;
  ctx.queuePreviewMiddlePaneSection = queuePreviewMiddlePaneSection;
  ctx.previewMiddlePaneSection = previewMiddlePaneSection;

  ctx.isLeftRailMoreActive = computed(() => ctx.leftRailMoreExpanded.value ||
      ctx.isLeftNavSectionActive('swarms') ||
      ctx.isLeftNavSectionActive('orchestrations') ||
      ctx.isLeftNavSectionActive('users') ||
      ctx.isLeftNavSectionActive('groups') ||
      ctx.isLeftNavSectionActive('more') ||
      ctx.isHelperAppsMiddlePaneActive.value);

  ctx.leftRailMoreToggleTitle = computed(() => `${ctx.t('common.more')} · ${ctx.t(ctx.leftRailMoreExpanded.value ? 'common.collapse' : 'common.expand')}`);
}
