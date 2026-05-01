// @ts-nocheck
// Route restoration, bootstrap loading, keyword synchronization, middle-pane overlay syncing, and route-driven view state.
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

export function installMessengerControllerLifecycleRouteBootstrap(ctx: MessengerControllerContext): void {
  ctx.restoreConversationFromRoute = async () => {
      const query = ctx.route.query;
      const querySection = resolveSectionFromRoute(ctx.route.path, query.section);
      const queryAgentId = String(query?.agent_id || '').trim();
      if (ctx.isEmbeddedChatRoute.value && querySection === 'agents' && queryAgentId) {
          ctx.agentOverviewMode.value = 'detail';
          ctx.selectedAgentId.value = ctx.normalizeAgentId(queryAgentId);
          ctx.sessionHub.setSection('agents');
          return;
      }
      const queryConversationId = String(query?.conversation_id || '').trim();
      if (queryConversationId) {
          if (ctx.userWorldPermissionDenied.value) {
              const nextQuery = { ...ctx.route.query } as Record<string, any>;
              delete nextQuery.conversation_id;
              ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
          }
          const conversation = ctx.userWorldStore.conversations.find((item) => String(item?.conversation_id || '') === queryConversationId);
          if (conversation) {
              const kind = String(conversation?.conversation_type || '').toLowerCase() === 'group' ? 'group' : 'direct';
              if (ctx.route.path.includes('/chat')) {
                  await ctx.userWorldStore.setActiveConversation(queryConversationId);
                  ctx.sessionHub.setActiveConversation({ kind, id: queryConversationId });
                  await ctx.scrollMessagesToBottom(true);
              }
              else {
                  await ctx.openWorldConversation(queryConversationId, kind);
              }
              return;
          }
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.conversation_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
      }
      const querySessionId = String(query?.session_id || '').trim();
      if (querySessionId) {
          const session = ctx.chatStore.sessions.find((item) => String(item?.id || '') === querySessionId);
          if (session) {
              await ctx.openAgentSession(querySessionId, ctx.normalizeAgentId(session?.agent_id));
              return;
          }
          const nextQuery = { ...ctx.route.query } as Record<string, any>;
          delete nextQuery.session_id;
          ctx.router.replace({ path: ctx.route.path, query: nextQuery }).catch(() => undefined);
      }
      const queryEntry = String(query?.entry || '').trim().toLowerCase();
      if (queryAgentId || queryEntry === 'default') {
          await ctx.openAgentById(queryAgentId || DEFAULT_AGENT_KEY);
          return;
      }
      const preferredSection = ctx.desktopMode.value
          ? ('messages' as MessengerSection)
          : resolveSectionFromRoute(ctx.route.path, query.section);
      if (preferredSection === 'messages') {
          const first = ctx.mixedConversations.value[0];
          if (first) {
              await ctx.openMixedConversation(first);
              return;
          }
      }
      ctx.clearMessagePanelWhenConversationEmpty();
  };

  ctx.bootstrap = async () => {
      ctx.bootLoading.value = true;
      if (!ctx.authStore.user && ctx.authStore.token) {
          try {
              await ctx.authStore.loadProfile();
          }
          catch (error) {
              const status = ctx.resolveHttpStatus(error);
              if (ctx.isAuthDeniedStatus(status)) {
                  ctx.authStore.logout();
                  ctx.bootLoading.value = false;
                  ctx.router.replace('/login').catch(() => undefined);
                  return;
              }
          }
      }
      await Promise.all([ctx.hydrateCurrentUserAppearance(), ctx.hydrateMessengerOrderPreferences()]);
      const initialSection = ctx.desktopMode.value
          ? ('messages' as MessengerSection)
          : resolveSectionFromRoute(ctx.route.path, ctx.route.query.section);
      const initialQuerySessionId = String(ctx.route.query.session_id || '').trim();
      const initialQueryConversationId = String(ctx.route.query.conversation_id || '').trim();
      const initialQueryAgentId = String(ctx.route.query.agent_id || '').trim();
      const initialQueryEntry = String(ctx.route.query.entry || '').trim().toLowerCase();
      const shouldPrioritizeWorldBootstrap = initialSection === 'messages' &&
          Boolean(initialQueryConversationId);
      const { critical, background } = splitMessengerBootstrapTasks(initialSection, [
          {
              sections: ['messages', 'agents', 'files', 'swarms'],
              run: () => ctx.agentStore.loadAgents()
          },
          {
              critical: true,
              sections: ['swarms'],
              run: () => ctx.beeroomStore.loadGroups()
          },
          {
              sections: ['plaza'],
              run: () => ctx.plazaStore.loadItems()
          },
          {
              sections: ['messages'],
              run: () => ctx.chatStore.loadSessions()
          },
          {
              sections: shouldPrioritizeWorldBootstrap ? ['messages', 'users', 'groups'] : ['users', 'groups'],
              run: () => ctx.userWorldStore.bootstrap()
          },
          {
              sections: ['users', 'groups'],
              run: () => ctx.loadOrgUnits()
          },
          {
              run: () => ctx.loadRunningAgents()
          },
          {
              run: () => ctx.loadAgentUserRounds()
          }
      ]);
      await settleMessengerBootstrapTasks(critical);
      ctx.ensureSectionSelection();
      ctx.bootLoading.value = false;
      void ctx.restoreConversationFromRoute();
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
          ctx.clearMiddlePaneOverlayPreview();
      }
  }, { immediate: true });

  watch(() => ctx.middlePaneOverlayVisible.value, (visible) => {
      if (visible) {
          ctx.middlePaneMounted.value = true;
          return;
      }
      if (!visible) {
          ctx.clearMiddlePaneOverlayPreview();
      }
  });

  ctx.syncRouteDrivenMessengerViewState = () => {
      ctx.settingsPanelMode.value = ctx.resolveRouteSettingsPanelMode(ctx.route.path, ctx.route.query.panel, ctx.desktopMode.value);
      const sectionHint = String(ctx.route.query.section || '').trim().toLowerCase();
      const helperWorkspaceEnabled = ctx.resolveRouteHelperWorkspaceEnabled(ctx.route.query.section, ctx.route.query.helper);
      ctx.helperAppsWorkspaceMode.value = helperWorkspaceEnabled;
      if (helperWorkspaceEnabled) {
          ctx.ensureHelperAppsSelection();
          void ctx.loadHelperExternalApps();
      }
      if (ctx.desktopMode.value && !ctx.desktopInitialSectionPinned.value) {
          ctx.desktopInitialSectionPinned.value = true;
          ctx.sessionHub.setSection(ctx.isEmbeddedChatRoute.value ? resolveSectionFromRoute(ctx.route.path, ctx.route.query.section) : 'messages');
          return;
      }
      if (ctx.route.path.includes('/user-world') && sectionHint === 'groups') {
          ctx.sessionHub.setSection('groups');
          return;
      }
      ctx.sessionHub.setSection(resolveSectionFromRoute(ctx.route.path, ctx.route.query.section));
  };

  ctx.syncRouteDrivenMessengerViewState();

  watch(() => [ctx.route.path, ctx.route.query.section, ctx.route.query.panel, ctx.route.query.helper], ctx.syncRouteDrivenMessengerViewState);
}
