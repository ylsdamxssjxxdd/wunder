// @ts-nocheck
// File container selection, tool catalog loading, organization units, and active agent refresh helpers.
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

export function installMessengerControllerFileToolSettings(ctx: MessengerControllerContext): void {
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

  ctx.SESSION_OPEN_RECOVERY_ATTEMPTS = 2;

  ctx.resolveSessionBusyRecoveryMessage = (status: SessionBusyRecoveryStatus): string => {
      if (status === 'runtime_busy') {
          return ctx.t('chat.session.running');
      }
      if (status === 'unsettled') {
          return ctx.t('common.requestFailed');
      }
      return ctx.t('common.refreshSuccess');
  };

  ctx.refreshActiveAgentConversation = async () => {
      if (!ctx.isAgentConversationActive.value)
          return;
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return;
      const activeAgent = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId);
      try {
          const runResult = await ctx.runWithMessengerInteractionBlock('refresh', async () => {
              await ctx.openAgentSession(sessionId, activeAgent || DEFAULT_AGENT_KEY);
              const recoveryStatus = await settleAgentSessionBusyAfterRefresh({
                  sessionId,
                  isSessionBusy: (targetSessionId) => ctx.resolveEffectiveSessionBusy(targetSessionId),
                  resolveRuntimeStatus: (targetSessionId) => ctx.resolveSessionRuntimeStatus(String(targetSessionId || '').trim()),
                  loadSessionDetail: (targetSessionId, options) => ctx.chatStore.loadSessionDetail(String(targetSessionId || '').trim(), options),
                  attempts: ctx.SESSION_OPEN_RECOVERY_ATTEMPTS
              });
              return recoveryStatus;
          });
          const status = runResult || 'settled';
          if (status === 'runtime_busy') {
              ElMessage.info(ctx.resolveSessionBusyRecoveryMessage(status));
              return;
          }
          if (status === 'unsettled') {
              ElMessage.warning(ctx.resolveSessionBusyRecoveryMessage(status));
              return;
          }
          ElMessage.success(ctx.resolveSessionBusyRecoveryMessage(status));
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
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

  ctx.loadOrgUnits = async () => {
      try {
          const { data } = await fetchOrgUnits();
          const sourceTree = Array.isArray(data?.data?.tree) ? data.data.tree : [];
          const sourceItems = Array.isArray(data?.data?.items)
              ? data.data.items
              : Array.isArray(data?.data)
                  ? data.data
                  : sourceTree;
          const normalized = sourceItems
              .map((item) => normalizeUnitNode(item))
              .filter((item): item is UnitTreeNode => Boolean(item));
          const flatNodes = flattenUnitNodes(normalized);
          const tree = buildUnitTreeFromFlat(flatNodes);
          const nextMap: Record<string, string> = {};
          const allNodeIds = new Set<string>();
          const rootIds = new Set<string>();
          const walk = (nodes: UnitTreeNode[]) => {
              nodes.forEach((node) => {
                  nextMap[node.id] = node.label;
                  allNodeIds.add(node.id);
                  if (node.children.length) {
                      walk(node.children);
                  }
              });
          };
          tree.forEach((node) => {
              rootIds.add(node.id);
          });
          walk(tree);
          const retainedExpanded = new Set<string>();
          ctx.contactUnitExpandedIds.value.forEach((unitId) => {
              if (allNodeIds.has(unitId)) {
                  retainedExpanded.add(unitId);
              }
          });
          ctx.orgUnitPathMap.value = nextMap;
          ctx.orgUnitTree.value = tree;
          ctx.contactUnitExpandedIds.value = retainedExpanded.size > 0 ? retainedExpanded : rootIds;
      }
      catch {
          if (ctx.orgUnitTree.value.length > 0) {
              return;
          }
          const fallbackTree = ctx.buildCurrentUserFallbackUnitTree();
          if (!fallbackTree.length) {
              ctx.orgUnitPathMap.value = {};
              ctx.orgUnitTree.value = [];
              ctx.contactUnitExpandedIds.value = new Set();
              return;
          }
          const fallbackMap: Record<string, string> = {};
          fallbackTree.forEach((node) => {
              fallbackMap[node.id] = node.label;
          });
          ctx.orgUnitPathMap.value = fallbackMap;
          ctx.orgUnitTree.value = fallbackTree;
          ctx.contactUnitExpandedIds.value = new Set(fallbackTree.map((node) => node.id));
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
