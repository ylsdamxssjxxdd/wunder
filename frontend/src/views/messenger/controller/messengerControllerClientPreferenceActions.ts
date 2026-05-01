// @ts-nocheck
// Language switching, desktop update checks, send-key/profile preferences, approvals, theme, and debug tools.
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

export function installMessengerControllerClientPreferenceActions(ctx: MessengerControllerContext): void {
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
      if (!ctx.desktopMode.value) {
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
