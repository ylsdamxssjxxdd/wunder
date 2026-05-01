// @ts-nocheck
// Messenger order persistence, current-user appearance, launch behavior, middle-pane overlay, and quick agent creation.
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

export function installMessengerControllerWorkspaceOrderUiActions(ctx: MessengerControllerContext): void {
  ctx.prioritizeImportedBeeroomAgents = async (report: unknown, groupId: unknown) => {
      const normalizedGroupId = String(groupId || '').trim();
      if (!normalizedGroupId) {
          return;
      }
      const activeGroup = await ctx.beeroomStore.selectGroup(normalizedGroupId, { silent: true }).catch(() => null);
      const groupMembers = Array.isArray(activeGroup?.members) ? activeGroup.members : [];
      const reportRecord = report && typeof report === 'object' && !Array.isArray(report)
          ? (report as Record<string, unknown>)
          : {};
      const importedAgents = Array.isArray(reportRecord.agents)
          ? reportRecord.agents
              .map((item) => item && typeof item === 'object' && !Array.isArray(item)
              ? String((item as Record<string, unknown>).agent_id || '').trim()
              : '')
              .filter(Boolean)
          : [];
      const motherAgentId = String(activeGroup?.mother_agent_id || '').trim();
      const orderedImportedIds = ctx.normalizeStringListUnique([
          motherAgentId,
          ...importedAgents,
          ...groupMembers.map((item) => String(item?.agent_id || '').trim())
      ]);
      ctx.moveOwnedAgentsToFront(orderedImportedIds);
  };

  ctx.hydrateMessengerOrderPreferences = async () => {
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      ctx.messengerOrderReady.value = false;
      if (!scopedUserId) {
          chatDebugLog('messenger.order', 'hydrate-skip-no-user', {});
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
                      userId: scopedUserId,
                      current: ctx.captureMessengerOrderPreferences()
                  });
                  ctx.scheduleMessengerOrderPersist();
              }
          }
      }
  };

  ctx.persistMessengerOrderPreferences = async () => {
      if (ctx.messengerOrderHydrating.value || !ctx.messengerOrderReady.value) {
          chatDebugLog('messenger.order', 'persist-skip-not-ready', {
              hydrating: ctx.messengerOrderHydrating.value,
              ready: ctx.messengerOrderReady.value
          });
          return;
      }
      const scopedUserId = String(ctx.currentUserId.value || '').trim();
      if (!scopedUserId) {
          chatDebugLog('messenger.order', 'persist-skip-no-user', {});
          return;
      }
      const current = ctx.captureMessengerOrderPreferences();
      chatDebugLog('messenger.order', 'persist-start', {
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
          userId: scopedUserId,
          persisted
      });
  };

  ctx.scheduleMessengerOrderPersist = () => {
      if (ctx.messengerOrderHydrating.value || !ctx.messengerOrderReady.value || typeof window === 'undefined') {
          chatDebugLog('messenger.order', 'schedule-skip', {
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
      if (!ctx.desktopMode.value || typeof window === 'undefined')
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
      const maxScroll = Math.max(0, ctx.filteredContacts.value.length * ctx.CONTACT_VIRTUAL_ITEM_HEIGHT - ctx.contactVirtualViewportHeight.value);
      if (ctx.contactVirtualScrollTop.value > maxScroll) {
          ctx.contactVirtualScrollTop.value = maxScroll;
          container.scrollTop = maxScroll;
      }
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
      ctx.clearMiddlePaneOverlayPreview();
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
          ctx.clearMiddlePaneOverlayPreview();
          return;
      }
      ctx.middlePaneOverlayHideTimer = window.setTimeout(() => {
          ctx.middlePaneOverlayHideTimer = null;
          ctx.middlePaneOverlayVisible.value = false;
          ctx.clearMiddlePaneOverlayPreview();
      }, 140);
  };

  ctx.openCreatedAgentSettings = (agentId: unknown) => {
      const normalizedId = ctx.normalizeAgentId(agentId);
      if (!normalizedId) {
          return;
      }
      ctx.sessionHub.setSection('agents');
      ctx.selectedAgentId.value = normalizedId;
      ctx.agentSettingMode.value = 'agent';
      ctx.router.replace({ path: `${ctx.basePrefix.value}/home`, query: { ...ctx.route.query, section: 'agents' } })
          .catch(() => undefined);
  };

  ctx.buildQuickAgentName = () => {
      const now = new Date();
      const pad = (value: number) => String(value).padStart(2, '0');
      const suffix = `${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
      return `${ctx.t('messenger.action.newAgent')} ${suffix}`;
  };

  ctx.createAgentQuickly = async () => {
      if (ctx.quickCreatingAgent.value)
          return;
      ctx.agentQuickCreateVisible.value = true;
  };

  ctx.submitAgentQuickCreate = async (payload: {
      copy_from_agent_id?: string;
  }) => {
      const createPayload: Record<string, unknown> = {
          name: ctx.buildQuickAgentName()
      };
      const targetHiveId = String(ctx.preferredBeeroomGroupId.value || '').trim();
      if (targetHiveId) {
          createPayload.hive_id = targetHiveId;
      }
      const copyFromAgentId = String(payload?.copy_from_agent_id || DEFAULT_AGENT_KEY).trim() || DEFAULT_AGENT_KEY;
      createPayload.copy_from_agent_id = copyFromAgentId;
      const created = await ctx.submitAgentCreate(createPayload);
      if (created) {
          ctx.agentQuickCreateVisible.value = false;
      }
  };

  ctx.submitAgentCreate = async (payload: Record<string, unknown>): Promise<boolean> => {
      if (ctx.quickCreatingAgent.value)
          return false;
      ctx.quickCreatingAgent.value = true;
      try {
          const created = await ctx.agentStore.createAgent(payload);
          ElMessage.success(ctx.t('portal.agent.createSuccess'));
          const tasks: Promise<unknown>[] = [ctx.loadRunningAgents({ force: true }), ctx.beeroomStore.loadGroups()];
          if (!ctx.cronPermissionDenied.value) {
              tasks.push(ctx.loadCronAgentIds({ force: true }));
          }
          await Promise.all(tasks);
          const createdHiveId = String(created?.hive_id || payload.hive_id || '').trim();
          if (ctx.sessionHub.activeSection === 'swarms' ||
              ctx.sessionHub.activeSection === 'orchestrations' ||
              createdHiveId ||
              String(payload.hive_name || '').trim()) {
              if (createdHiveId) {
                  ctx.beeroomFirstEntryAutoSelectionPending.value = false;
                  ctx.beeroomStore.setActiveGroup(createdHiveId);
              }
              await ctx.beeroomStore.loadActiveGroup().catch(() => null);
          }
          if (created?.id) {
              ctx.openCreatedAgentSettings(created.id);
          }
          return true;
      }
      catch (error) {
          showApiError(error, ctx.t('portal.agent.saveFailed'));
          return false;
      }
      finally {
          ctx.quickCreatingAgent.value = false;
      }
  };
}
