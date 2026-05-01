// @ts-nocheck
// Beeroom refreshes, agent mutation refreshes, worker-card import, and batch agent actions.
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

export function installMessengerControllerBeeroomAgentMutations(ctx: MessengerControllerContext): void {
  ctx.refreshActiveBeeroom = async () => {
      if (ctx.sessionHub.activeSection !== 'swarms') {
          return;
      }
      try {
          if (String(ctx.beeroomStore.activeGroupId || '').trim()) {
              await ctx.beeroomStore.loadActiveGroup({ silent: true });
              return;
          }
          await ctx.beeroomStore.loadGroups();
          if (String(ctx.beeroomStore.activeGroupId || '').trim()) {
              await ctx.beeroomStore.loadActiveGroup({ silent: true });
          }
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.refreshActiveOrchestration = async () => {
      if (ctx.sessionHub.activeSection !== 'orchestrations') {
          return;
      }
      try {
          if (String(ctx.beeroomStore.activeGroupId || '').trim()) {
              await ctx.beeroomStore.loadActiveGroup({ silent: true });
              return;
          }
          await ctx.beeroomStore.loadGroups();
          if (String(ctx.beeroomStore.activeGroupId || '').trim()) {
              await ctx.beeroomStore.loadActiveGroup({ silent: true });
          }
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.refreshBeeroomRealtimeGroups = async () => {
      const now = Date.now();
      const minInterval = ctx.hasHotBeeroomRuntimeState.value
          ? ctx.BEEROOM_GROUPS_REFRESH_MIN_MS_HOT : ctx.BEEROOM_GROUPS_REFRESH_MIN_MS_IDLE;
      const shouldRefreshGroups = now - ctx.beeroomGroupsLastRefreshAt >= minInterval;
      if (shouldRefreshGroups) {
          await ctx.beeroomStore.loadGroups();
          ctx.beeroomGroupsLastRefreshAt = Date.now();
      }
      await ctx.loadRunningAgents();
  };

  ctx.refreshBeeroomRealtimeActiveGroup = async () => {
      const activeGroupId = String(ctx.beeroomStore.activeGroupId || '').trim();
      if (!activeGroupId) {
          return;
      }
      await ctx.beeroomStore.loadActiveGroup({ silent: true });
  };

  ctx.handleBeeroomMoveAgents = async (agentIds: string[]) => {
      const groupId = String(ctx.beeroomStore.activeGroupId || '').trim();
      if (!groupId || !agentIds.length)
          return;
      try {
          await ctx.beeroomStore.moveAgents(groupId, agentIds);
          await ctx.agentStore.loadAgents();
          ElMessage.success(ctx.t('beeroom.message.agentMoved'));
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.refreshAgentMutationState = async () => {
      const tasks: Promise<unknown>[] = [
          ctx.agentStore.loadAgents(),
          ctx.loadRunningAgents({ force: true }),
          ctx.beeroomStore.loadGroups()
      ];
      if (!ctx.cronPermissionDenied.value) {
          tasks.push(ctx.loadCronAgentIds({ force: true }));
      }
      await Promise.all(tasks);
  };

  ctx.normalizeWorkerCardImportProgress = (value: number) => Math.max(0, Math.min(100, Math.round(value)));

  ctx.resetWorkerCardImportOverlay = () => {
      ctx.workerCardImportOverlayVisible.value = false;
      ctx.workerCardImportOverlayPhase.value = 'preparing';
      ctx.workerCardImportOverlayProgress.value = 0;
      ctx.workerCardImportOverlayTargetName.value = '';
      ctx.workerCardImportOverlayCurrent.value = 0;
      ctx.workerCardImportOverlayTotal.value = 0;
  };

  ctx.beginWorkerCardImportOverlay = (targetName) => {
      ctx.workerCardImportOverlayVisible.value = true;
      ctx.workerCardImportOverlayPhase.value = 'preparing';
      ctx.workerCardImportOverlayProgress.value = 6;
      ctx.workerCardImportOverlayTargetName.value = String(targetName || '').trim();
      ctx.workerCardImportOverlayCurrent.value = 0;
      ctx.workerCardImportOverlayTotal.value = 0;
  };

  ctx.setWorkerCardImportCreatingOverlay = (targetName, current, total) => {
      const safeTotal = Math.max(1, Number(total || 0));
      const safeCurrent = Math.max(1, Math.min(safeTotal, Number(current || 0)));
      ctx.workerCardImportOverlayVisible.value = true;
      ctx.workerCardImportOverlayPhase.value = 'creating';
      ctx.workerCardImportOverlayTargetName.value = String(targetName || '').trim();
      ctx.workerCardImportOverlayCurrent.value = safeCurrent;
      ctx.workerCardImportOverlayTotal.value = safeTotal;
      ctx.workerCardImportOverlayProgress.value = ctx.normalizeWorkerCardImportProgress(18 + ((safeCurrent - 1) / safeTotal) * 64);
  };

  ctx.setWorkerCardImportRefreshingOverlay = (targetName, total) => {
      const safeTotal = Math.max(0, Number(total || 0));
      ctx.workerCardImportOverlayVisible.value = true;
      ctx.workerCardImportOverlayPhase.value = 'refreshing';
      ctx.workerCardImportOverlayTargetName.value = String(targetName || '').trim();
      ctx.workerCardImportOverlayCurrent.value = safeTotal;
      ctx.workerCardImportOverlayTotal.value = safeTotal;
      ctx.workerCardImportOverlayProgress.value = 92;
  };

  ctx.openWorkerCardImportPicker = () => {
      if (ctx.workerCardImporting.value || ctx.quickCreatingAgent.value) {
          return;
      }
      ctx.workerCardImportInputRef.value?.click();
  };

  ctx.handleWorkerCardImportInput = async (event) => {
      const input = event?.target as HTMLInputElement | null;
      const file = input?.files?.[0];
      if (!file || ctx.quickCreatingAgent.value || ctx.workerCardImporting.value)
          return;
      ctx.workerCardImporting.value = true;
      ctx.beginWorkerCardImportOverlay(file.name);
      try {
          const dependencyCatalog = await loadUserToolsSummaryCache().catch(() => null);
          const documents = parseWorkerCardText(await file.text());
          ctx.workerCardImportOverlayTotal.value = documents.length;
          ctx.workerCardImportOverlayProgress.value = ctx.normalizeWorkerCardImportProgress(documents.length > 0 ? 12 : 18);
          const createdItems: Record<string, unknown>[] = [];
          const warnings: string[] = [];
          for (const [index, document] of documents.entries()) {
              ctx.setWorkerCardImportCreatingOverlay(document.metadata.name || file.name, index + 1, documents.length);
              const dependencyStatus = resolveAgentDependencyStatus({
                  declared_tool_names: document.abilities.tool_names,
                  declared_skill_names: document.abilities.skills
              }, dependencyCatalog);
              const response = await createAgentApi(workerCardToAgentPayload(document));
              const created = response?.data?.data;
              if (created) {
                  createdItems.push(created);
              }
              if (dependencyStatus.missingToolNames.length || dependencyStatus.missingSkillNames.length) {
                  warnings.push(ctx.t('portal.agent.workerCardImportMissingSummary', {
                      name: document.metadata.name,
                      tools: dependencyStatus.missingToolNames.length,
                      skills: dependencyStatus.missingSkillNames.length
                  }));
              }
          }
          ctx.setWorkerCardImportRefreshingOverlay(documents.length === 1 ? documents[0].metadata.name || file.name : file.name, documents.length);
          await ctx.refreshAgentMutationState();
          await ctx.loadAgentToolSummary({ force: true });
          ctx.workerCardImportOverlayProgress.value = 100;
          if (createdItems[0]?.id) {
              ctx.openCreatedAgentSettings(createdItems[0].id);
          }
          ElMessage.success(documents.length === 1
              ? ctx.t('portal.agent.workerCardImportSuccess', { name: documents[0].metadata.name })
              : ctx.t('portal.agent.workerCardImportBatchSuccess', { count: documents.length }));
          if (warnings.length) {
              ElMessage.warning(warnings.join('\uff1b'));
          }
      }
      catch (error) {
          showApiError(error, ctx.t('portal.agent.workerCardImportFailed'));
      }
      finally {
          ctx.workerCardImporting.value = false;
          ctx.resetWorkerCardImportOverlay();
          if (input) {
              input.value = '';
          }
      }
  };

  ctx.handleAgentBatchExport = async (agentIds: string[]) => {
      const normalizedIds = Array.from(new Set(agentIds.map((item) => ctx.normalizeAgentId(item)).filter(Boolean)));
      if (!normalizedIds.length)
          return;
      try {
          const records: Record<string, unknown>[] = [];
          for (const agentId of normalizedIds) {
              const agent = await ctx.agentStore.getAgent(agentId, { force: true });
              if (agent) {
                  records.push(agent as Record<string, unknown>);
              }
          }
          if (!records.length) {
              ElMessage.warning(ctx.t('portal.agent.loadingFailed'));
              return;
          }
          const filename = downloadWorkerCardBundle(records);
          ElMessage.success(ctx.t('portal.agent.workerCardExportSuccess', { name: filename }));
      }
      catch (error) {
          showApiError(error, ctx.t('portal.agent.saveFailed'));
      }
  };

  ctx.handleAgentBatchDelete = async (agentIds: string[]) => {
      const normalizedIds = Array.from(new Set(agentIds.map((item) => ctx.normalizeAgentId(item)).filter(Boolean)));
      const ownedIds = new Set((Array.isArray(ctx.agentStore.agents) ? ctx.agentStore.agents : []).map((agent) => ctx.normalizeAgentId(agent?.id)));
      const deletableIds = normalizedIds.filter((agentId) => agentId !== DEFAULT_AGENT_KEY && ownedIds.has(agentId));
      if (!deletableIds.length) {
          ElMessage.warning(ctx.t('portal.agent.deleteBatchUnavailable'));
          return;
      }
      try {
          await ElMessageBox.confirm(ctx.t('portal.agent.deleteBatchConfirm', { count: deletableIds.length }), ctx.t('common.notice'), {
              confirmButtonText: ctx.t('portal.agent.delete'),
              cancelButtonText: ctx.t('portal.agent.cancel'),
              type: 'warning'
          });
      }
      catch {
          return;
      }
      const results = await Promise.allSettled(deletableIds.map((agentId) => deleteAgentApi(agentId)));
      const successCount = results.filter((item) => item.status === 'fulfilled').length;
      const failedCount = results.length - successCount;
      if (successCount > 0) {
          await ctx.refreshAgentMutationState();
      }
      if (failedCount === 0) {
          ElMessage.success(ctx.t('portal.agent.deleteBatchSuccess', { count: successCount }));
          return;
      }
      if (successCount > 0) {
          ElMessage.warning(ctx.t('portal.agent.deleteBatchPartial', { success: successCount, failed: failedCount }));
          return;
      }
      const firstRejected = results.find((item) => item.status === 'rejected');
      showApiError((firstRejected as PromiseRejectedResult | undefined)?.reason, ctx.t('portal.agent.deleteFailed'));
  };
}
