// @ts-nocheck
// Message keys, route sync, section switching, middle-pane delegates, appearance, ordering, and beeroom caches.
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

export function installMessengerControllerMessageRoutingPreferences(ctx: MessengerControllerContext): void {
  ctx.resolveAgentMessageKey = (message: Record<string, unknown>, index: number): string => {
      const base = String(message?.id || message?.message_id || message?.request_id || message?.role || 'm');
      const safeIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
      return `${base}:${safeIndex}`;
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
          String(item?.detail || '').length
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

  ctx.isMixedConversationActive = (item: MixedConversation): boolean => {
      const identity = ctx.activeConversation.value;
      if (!identity)
          return false;
      if (item.kind === 'agent') {
          if (identity.kind !== 'agent')
              return false;
          const currentAgentId = ctx.normalizeAgentId(identity.agentId ||
              (identity.id.startsWith('draft:')
                  ? identity.id.slice('draft:'.length)
                  : ctx.chatStore.sessions.find((session) => String(session?.id || '') === identity.id)?.agent_id));
          return currentAgentId === item.agentId;
      }
      return identity.kind === item.kind && identity.id === item.sourceId;
  };

  ctx.canDeleteMixedConversation = (item: MixedConversation): boolean => item?.kind === 'agent' || Boolean(item?.sourceId);

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

  ctx.deleteMixedConversation = async (item: MixedConversation) => {
      const sourceId = String(item?.sourceId || '').trim();
      if (!sourceId)
          return;
      const confirmed = await confirmWithFallback(ctx.t('chat.history.confirmDelete'), ctx.t('chat.history.confirmTitle'), {
          type: 'warning',
          confirmButtonText: ctx.t('common.confirm'),
          cancelButtonText: ctx.t('common.cancel')
      });
      if (!confirmed) {
          return;
      }
      try {
          if (item.kind === 'agent') {
              const agentId = ctx.normalizeAgentId(item.agentId);
              ctx.markAgentConversationDismissed(agentId);
              if (sourceId) {
                  ctx.timelinePreviewMap.value.delete(sourceId);
              }
              if (ctx.isMixedConversationActive(item)) {
                  const fallback = ctx.mixedConversations.value.find((entry) => entry.key !== item.key);
                  if (fallback) {
                      await ctx.openMixedConversation(fallback);
                  }
                  else {
                      ctx.sessionHub.clearActiveConversation();
                      const nextQuery = {
                          ...ctx.route.query,
                          section: 'messages'
                      } as Record<string, any>;
                      delete nextQuery.conversation_id;
                      delete nextQuery.session_id;
                      delete nextQuery.agent_id;
                      delete nextQuery.entry;
                      ctx.router.replace({ path: ctx.resolveChatShellPath(), query: nextQuery }).catch(() => undefined);
                  }
              }
          }
          else {
              await ctx.userWorldStore.dismissConversation(sourceId);
          }
          ctx.triggerRealtimePulseRefresh?.('delete-mixed-conversation');
          ElMessage.success(ctx.t('chat.history.delete'));
      }
      catch (error) {
          showApiError(error, ctx.t('chat.sessions.deleteFailed'));
      }
  };

  ctx.switchSection = (section: MessengerSection, options: {
      preserveHelperWorkspace?: boolean;
      panelHint?: string;
      helperWorkspace?: boolean;
      settingsPanelMode?: string;
  } = {}) => {
      const preserveHelperWorkspace = options.preserveHelperWorkspace === true;
      const panelHint = String(options.panelHint || '').trim().toLowerCase();
      const explicitSettingsPanelMode = ctx.normalizeSettingsPanelMode(options.settingsPanelMode);
      const helperWorkspace = options.helperWorkspace === true;
      ctx.closeLeftRailMoreMenu();
      ctx.closeFileContainerMenu();
      ctx.openMiddlePaneOverlay();
      if (!preserveHelperWorkspace) {
          ctx.helperAppsWorkspaceMode.value = false;
      }
      else if (helperWorkspace) {
          ctx.helperAppsWorkspaceMode.value = true;
      }
      ctx.sessionHub.setSection(section);
      ctx.sessionHub.setKeyword('');
      ctx.worldHistoryDialogVisible.value = false;
      ctx.agentPromptPreviewVisible.value = false;
      if (section === 'more') {
          void preloadMessengerSettingsPanels({ desktopMode: ctx.desktopMode.value });
          ctx.settingsPanelMode.value =
              explicitSettingsPanelMode !== 'general'
                  ? explicitSettingsPanelMode
                  : ctx.desktopMode.value && panelHint === 'desktop-models'
                      ? 'desktop-models'
                      : ctx.desktopMode.value && panelHint === 'desktop-lan'
                          ? 'desktop-lan'
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
      if (section !== 'tools') {
          ctx.selectedToolCategory.value = '';
      }
      if (section !== 'users') {
          ctx.selectedContactUserId.value = '';
          ctx.selectedContactUnitId.value = '';
      }
      if (section !== 'groups') {
          ctx.selectedGroupId.value = '';
      }
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
      if (section === 'groups' && helperWorkspace) {
          nextQuery.helper = '1';
      }
      else {
          delete nextQuery.helper;
      }
      if (section !== 'messages') {
          delete nextQuery.session_id;
          delete nextQuery.agent_id;
          delete nextQuery.entry;
      }
      if (section !== 'users' && section !== 'groups') {
          delete nextQuery.conversation_id;
      }
      ctx.scheduleSectionRouteSync(targetPath, nextQuery);
      if (section === 'tools') {
          ctx.loadToolsCatalog();
      }
      ctx.ensureSectionSelection();
  };

  ctx.ensureMiddlePaneSection = (section: MessengerSection, options: {
      helperWorkspace?: boolean;
      panelHint?: string;
      settingsPanelMode?: SettingsPanelMode;
  } = {}) => {
      const helperWorkspace = section === 'groups' && options.helperWorkspace === true;
      const nextSettingsPanelMode = ctx.normalizeSettingsPanelMode(options.settingsPanelMode);
      const helperWorkspaceChanged = section === 'groups' && ctx.helperAppsWorkspaceMode.value !== helperWorkspace;
      const settingsModeChanged = section === 'more' && ctx.settingsPanelMode.value !== nextSettingsPanelMode;
      if (ctx.sessionHub.activeSection === section &&
          !helperWorkspaceChanged &&
          !settingsModeChanged) {
          return;
      }
      ctx.switchSection(section, {
          preserveHelperWorkspace: helperWorkspace,
          helperWorkspace,
          panelHint: section === 'more'
              ? String(options.panelHint || nextSettingsPanelMode).trim()
              : '',
          settingsPanelMode: section === 'more' ? nextSettingsPanelMode : undefined
      });
  };

  ctx.handleMiddlePaneContactUnitIdUpdate = (value: string) => {
      ctx.ensureMiddlePaneSection('users');
      ctx.selectedContactUnitId.value = value;
  };

  ctx.handleMiddlePaneAgentHiveGroupIdUpdate = (value: string) => {
      ctx.ensureMiddlePaneSection('agents');
      ctx.selectedAgentHiveGroupId.value = value;
  };

  ctx.selectHelperAppFromMiddlePane = (kind: 'offline' | 'online', key: string) => {
      ctx.ensureMiddlePaneSection('groups', { helperWorkspace: true });
      ctx.selectHelperApp(kind, key);
  };

  ctx.selectContactFromMiddlePane = (contact: Record<string, unknown>) => {
      ctx.ensureMiddlePaneSection('users');
      ctx.selectContact(contact);
  };

  ctx.selectGroupFromMiddlePane = (group: Record<string, unknown>) => {
      ctx.ensureMiddlePaneSection('groups');
      ctx.selectGroup(group);
  };

  ctx.selectPlazaBrowseKindFromMiddlePane = (kind: PlazaBrowseKind) => {
      ctx.ensureMiddlePaneSection('plaza');
      ctx.plazaBrowseKind.value = normalizePlazaBrowseKind(kind);
      ctx.selectedPlazaItemId.value = '';
  };

  ctx.selectBeeroomGroupFromMiddlePane = async (group: Record<string, unknown>) => {
      const currentBeeroomSection = ctx.sessionHub.activeSection === 'orchestrations' ? 'orchestrations' : 'swarms';
      ctx.ensureMiddlePaneSection(currentBeeroomSection);
      await ctx.selectBeeroomGroup(group);
  };

  ctx.selectAgentForSettingsFromMiddlePane = (agentId: unknown) => {
      ctx.ensureMiddlePaneSection('agents');
      ctx.selectAgentForSettings(agentId);
  };

  ctx.selectToolCategoryFromMiddlePane = (category: 'admin' | 'mcp' | 'skills' | 'knowledge') => {
      ctx.ensureMiddlePaneSection('tools');
      ctx.selectToolCategory(category);
  };

  ctx.selectContainerFromMiddlePane = (containerId: number | 'user') => {
      ctx.ensureMiddlePaneSection('files');
      ctx.selectContainer(containerId);
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
      // Commit the overlay preview to the real section before updating the settings panel,
      // otherwise the middle pane changes while the main content stays on the old section.
      if (ctx.sessionHub.activeSection !== 'more' || ctx.helperAppsWorkspaceMode.value) {
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
      if (!ctx.agentHeaderModelJumpEnabled.value)
          return;
      if (ctx.activeAgentUsingDesktopDefaultModel.value) {
          ctx.activateSettingsPanel('desktop-models');
          return;
      }
      ctx.openActiveAgentSettings({ focusSection: 'model' });
  };

  ctx.openProfilePage = () => {
      ctx.closeFileContainerMenu();
      ctx.activateSettingsPanel('profile');
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
      ctx.orderedMixedConversationsState.orderedKeys.value = value.messages.slice();
      ctx.orderedOwnedAgentsState.orderedKeys.value = value.agentsOwned.slice();
      ctx.orderedSharedAgentsState.orderedKeys.value = value.agentsShared.slice();
      ctx.orderedBeeroomGroupsState.orderedKeys.value = value.swarms.slice();
      ctx.messengerOrderSnapshot.value = {
          messages: value.messages.slice(),
          agentsOwned: value.agentsOwned.slice(),
          agentsShared: value.agentsShared.slice(),
          swarms: value.swarms.slice(),
          updatedAt: value.updatedAt
      };
      ctx.messengerOrderHydrating.value = false;
      chatDebugLog('messenger.order', 'apply', {
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
      messages: ctx.orderedMixedConversationsState.orderedKeys.value.slice(),
      agentsOwned: ctx.orderedOwnedAgentsState.orderedKeys.value.slice(),
      agentsShared: ctx.orderedSharedAgentsState.orderedKeys.value.slice(),
      swarms: ctx.orderedBeeroomGroupsState.orderedKeys.value.slice(),
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

  ctx.clearBeeroomRuntimeCachesByGroup = (groupId: unknown) => {
      const normalizedGroupId = String(groupId || '').trim();
      if (!normalizedGroupId) {
          return;
      }
      clearBeeroomMissionCanvasState(normalizedGroupId);
      clearBeeroomMissionCanvasState(`chat:${normalizedGroupId}`);
      clearBeeroomMissionCanvasState(`runtime:${normalizedGroupId}`);
      clearBeeroomMissionChatState(`runtime:${normalizedGroupId}`);
      const sessionIds = Array.isArray(ctx.beeroomDispatchSessionIdsByGroup.value[normalizedGroupId])
          ? ctx.beeroomDispatchSessionIdsByGroup.value[normalizedGroupId]
          : [];
      sessionIds.forEach((sessionId) => {
          clearCachedDispatchPreview(sessionId);
      });
      if (sessionIds.length > 0) {
          const next = { ...ctx.beeroomDispatchSessionIdsByGroup.value };
          delete next[normalizedGroupId];
          ctx.beeroomDispatchSessionIdsByGroup.value = next;
      }
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
