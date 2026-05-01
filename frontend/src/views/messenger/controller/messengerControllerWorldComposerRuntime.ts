// @ts-nocheck
// World drafts, history filters, composer resizing, emoji picker, container picker, and quick panel behavior.
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

export function installMessengerControllerWorldComposerRuntime(ctx: MessengerControllerContext): void {
  ctx.loadStoredStringArray = (storageKey: string, maxCount: number): string[] => {
      if (typeof window === 'undefined')
          return [];
      try {
          const raw = window.localStorage.getItem(storageKey);
          if (!raw)
              return [];
          const parsed = JSON.parse(raw) as unknown;
          if (!Array.isArray(parsed))
              return [];
          return parsed
              .map((item) => String(item || '').trim())
              .filter(Boolean)
              .slice(0, maxCount);
      }
      catch {
          return [];
      }
  };

  ctx.saveStoredStringArray = (storageKey: string, items: string[]) => {
      if (typeof window === 'undefined')
          return;
      try {
          window.localStorage.setItem(storageKey, JSON.stringify(items));
      }
      catch {
      }
  };

  ctx.activeWorldConversationId = computed(() => {
      if (!ctx.isWorldConversationActive.value)
          return '';
      return String(ctx.activeConversation.value?.id || '').trim();
  });

  ctx.buildWorldDraftKey = (conversationId: unknown): string => {
      const normalizedConversationId = String(conversationId || '').trim();
      if (!normalizedConversationId)
          return '';
      return `messenger:world:${ctx.resolveCurrentUserScope()}:${normalizedConversationId}`;
  };

  ctx.readWorldDraft = (conversationId: unknown): string => {
      const draftKey = ctx.buildWorldDraftKey(conversationId);
      if (!draftKey)
          return '';
      return String(ctx.worldDraftMap.get(draftKey) || '');
  };

  ctx.writeWorldDraft = (conversationId: unknown, value: unknown) => {
      const draftKey = ctx.buildWorldDraftKey(conversationId);
      if (!draftKey)
          return;
      const normalized = String(value || '');
      if (!normalized.trim()) {
          ctx.worldDraftMap.delete(draftKey);
          return;
      }
      ctx.worldDraftMap.set(draftKey, normalized);
  };

  ctx.normalizeWorldMessageTimestamp = (value: unknown): number => {
      const numeric = Number(value);
      if (Number.isFinite(numeric) && numeric > 0) {
          return numeric < 1000000000000 ? Math.floor(numeric * 1000) : Math.floor(numeric);
      }
      const parsed = new Date(value as string | number).getTime();
      if (Number.isFinite(parsed) && parsed > 0)
          return parsed;
      return 0;
  };

  ctx.worldHistoryRecords = computed<WorldHistoryRecord[]>(() => {
      const messages = Array.isArray(ctx.userWorldStore.activeMessages) ? ctx.userWorldStore.activeMessages : [];
      return messages
          .slice()
          .reverse()
          .map((item, index) => {
          const source = item as Record<string, unknown>;
          const rawContent = String(source.content || '').trim();
          if (!rawContent)
              return null;
          const category = classifyWorldHistoryMessage(source);
          const preview = normalizeWorldHistoryText(rawContent).slice(0, 260) || ctx.t('messenger.preview.empty');
          const messageId = Number.parseInt(String(source.message_id || ''), 10);
          const createdAt = ctx.normalizeWorldMessageTimestamp(source.created_at);
          return {
              key: `history:${source.message_id || index}:${createdAt}`,
              messageId: Number.isFinite(messageId) ? messageId : 0,
              sender: ctx.resolveWorldMessageSender(source),
              createdAt,
              preview,
              rawContent,
              category,
              icon: resolveWorldHistoryIcon(category)
          } as WorldHistoryRecord;
      })
          .filter((item): item is WorldHistoryRecord => Boolean(item));
  });

  ctx.worldHistoryTabOptions = computed(() => [
      { key: 'all' as WorldHistoryCategory, label: ctx.t('messenger.world.historyTabAll') },
      { key: 'media' as WorldHistoryCategory, label: ctx.t('messenger.world.historyTabMedia') },
      { key: 'document' as WorldHistoryCategory, label: ctx.t('messenger.world.historyTabDocument') },
      { key: 'other_file' as WorldHistoryCategory, label: ctx.t('messenger.world.historyTabOtherFile') }
  ]);

  ctx.filteredWorldHistoryRecords = computed(() => {
      const keyword = String(ctx.worldHistoryKeyword.value || '').trim().toLowerCase();
      const [rangeStartRaw, rangeEndRaw] = Array.isArray(ctx.worldHistoryDateRange.value)
          ? ctx.worldHistoryDateRange.value
          : [];
      const rangeStart = Number(rangeStartRaw);
      const rangeEnd = Number(rangeEndRaw);
      const hasDateRange = Number.isFinite(rangeStart) && Number.isFinite(rangeEnd);
      return ctx.worldHistoryRecords.value.filter((item) => {
          if (ctx.worldHistoryActiveTab.value !== 'all' && item.category !== ctx.worldHistoryActiveTab.value) {
              return false;
          }
          if (keyword) {
              const haystack = `${item.preview}\n${item.rawContent}\n${item.sender}`.toLowerCase();
              if (!haystack.includes(keyword)) {
                  return false;
              }
          }
          if (hasDateRange && item.createdAt > 0) {
              const safeStart = Math.min(rangeStart, rangeEnd);
              const safeEnd = Math.max(rangeStart, rangeEnd) + 24 * 60 * 60 * 1000 - 1;
              if (item.createdAt < safeStart || item.createdAt > safeEnd) {
                  return false;
              }
          }
          return true;
      });
  });

  ctx.worldEmojiCatalog = computed(() => WORLD_EMOJI_CATALOG.filter((emoji) => !ctx.worldRecentEmojis.value.includes(emoji)));

  ctx.clampWorldComposerHeight = (value: unknown): number => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed))
          return 188;
      return Math.min(340, Math.max(168, Math.round(parsed)));
  };

  ctx.worldComposerStyle = computed<Record<string, string>>(() => ({
      '--messenger-world-composer-height': `${ctx.worldComposerHeight.value}px`
  }));

  ctx.persistWorldComposerHeight = () => {
      if (typeof window === 'undefined')
          return;
      try {
          window.localStorage.setItem(WORLD_COMPOSER_HEIGHT_STORAGE_KEY, String(ctx.clampWorldComposerHeight(ctx.worldComposerHeight.value)));
      }
      catch {
      }
  };

  ctx.handleWorldComposerResizeMove = (event: MouseEvent) => {
      if (!ctx.worldComposerResizeRuntime)
          return;
      const delta = ctx.worldComposerResizeRuntime.startY - event.clientY;
      ctx.worldComposerHeight.value = ctx.clampWorldComposerHeight(ctx.worldComposerResizeRuntime.startHeight + delta);
  };

  ctx.stopWorldComposerResize = () => {
      if (typeof window !== 'undefined') {
          window.removeEventListener('mousemove', ctx.handleWorldComposerResizeMove);
          window.removeEventListener('mouseup', ctx.stopWorldComposerResize);
      }
      if (!ctx.worldComposerResizeRuntime)
          return;
      ctx.worldComposerResizeRuntime = null;
      ctx.persistWorldComposerHeight();
  };

  ctx.startWorldComposerResize = (event: MouseEvent) => {
      if (event.button !== 0)
          return;
      ctx.worldComposerResizeRuntime = {
          startY: event.clientY,
          startHeight: ctx.worldComposerHeight.value
      };
      if (typeof window !== 'undefined') {
          window.addEventListener('mousemove', ctx.handleWorldComposerResizeMove);
          window.addEventListener('mouseup', ctx.stopWorldComposerResize);
      }
  };

  ctx.clearWorldQuickPanelClose = () => {
      if (ctx.worldQuickPanelCloseTimer) {
          window.clearTimeout(ctx.worldQuickPanelCloseTimer);
          ctx.worldQuickPanelCloseTimer = null;
      }
  };

  ctx.scheduleWorldQuickPanelClose = () => {
      ctx.clearWorldQuickPanelClose();
      ctx.worldQuickPanelCloseTimer = window.setTimeout(() => {
          ctx.worldQuickPanelMode.value = '';
          ctx.worldQuickPanelCloseTimer = null;
      }, 120);
  };

  ctx.openWorldQuickPanel = (mode: 'emoji') => {
      ctx.clearWorldQuickPanelClose();
      ctx.worldQuickPanelMode.value = mode;
  };

  ctx.toggleWorldQuickPanel = (mode: 'emoji') => {
      ctx.clearWorldQuickPanelClose();
      ctx.worldQuickPanelMode.value = ctx.worldQuickPanelMode.value === mode ? '' : mode;
  };

  ctx.openWorldHistoryDialog = () => {
      ctx.clearWorldQuickPanelClose();
      ctx.worldQuickPanelMode.value = '';
      ctx.worldHistoryKeyword.value = '';
      ctx.worldHistoryActiveTab.value = 'all';
      ctx.worldHistoryDateRange.value = [];
      ctx.worldHistoryDialogVisible.value = true;
  };

  ctx.resolveWorldContainerPickerParent = (path: string): string => {
      const normalized = ctx.normalizeUploadPath(path);
      if (!normalized)
          return '';
      const pivot = normalized.lastIndexOf('/');
      if (pivot < 0)
          return '';
      return normalized.slice(0, pivot);
  };

  ctx.normalizeWorldContainerPickerEntry = (raw: unknown): WorldContainerPickerEntry | null => {
      if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
          return null;
      }
      const source = raw as Record<string, unknown>;
      const path = ctx.normalizeUploadPath(source.path);
      if (!path) {
          return null;
      }
      const rawName = String(source.name || '').trim();
      const fallbackName = path.split('/').pop() || path;
      const normalizedType = String(source.type || '').toLowerCase();
      const isDirectory = normalizedType === 'dir' || normalizedType === 'directory' || normalizedType === 'folder';
      return {
          path,
          name: rawName || fallbackName,
          type: isDirectory ? 'dir' : 'file'
      };
  };

  ctx.sortWorldContainerPickerEntries = (left: WorldContainerPickerEntry, right: WorldContainerPickerEntry): number => {
      if (left.type !== right.type) {
          return left.type === 'dir' ? -1 : 1;
      }
      return left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' });
  };

  ctx.loadWorldContainerPickerEntries = async (path: string) => {
      const normalizedPath = ctx.normalizeUploadPath(path);
      ctx.worldContainerPickerLoading.value = true;
      try {
          const { data } = await fetchWunderWorkspaceContent({
              path: normalizedPath,
              include_content: true,
              depth: 1,
              container_id: USER_CONTAINER_ID
          });
          const payload = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
          const payloadRecord = payload as Record<string, unknown>;
          ctx.worldContainerPickerPath.value = ctx.normalizeUploadPath(payloadRecord.path ?? normalizedPath);
          const rawEntries = payloadRecord.entries;
          const entries = Array.isArray(rawEntries) ? rawEntries : [];
          ctx.worldContainerPickerEntries.value = entries
              .map((entry) => ctx.normalizeWorldContainerPickerEntry(entry))
              .filter((entry): entry is WorldContainerPickerEntry => Boolean(entry))
              .sort(ctx.sortWorldContainerPickerEntries);
      }
      catch (error) {
          ctx.worldContainerPickerEntries.value = [];
          showApiError(error, ctx.t('userWorld.attachments.pickFailed'));
      }
      finally {
          ctx.worldContainerPickerLoading.value = false;
      }
  };

  ctx.openWorldContainerPickerPath = async (path: string) => {
      ctx.worldContainerPickerKeyword.value = '';
      await ctx.loadWorldContainerPickerEntries(path);
  };

  ctx.openWorldContainerPicker = async () => {
      if (!ctx.isWorldConversationActive.value || ctx.worldUploading.value)
          return;
      ctx.worldQuickPanelMode.value = '';
      ctx.worldContainerPickerVisible.value = true;
      await ctx.openWorldContainerPickerPath(ctx.worldContainerPickerPath.value);
  };

  ctx.openWorldContainerPickerParent = () => {
      if (ctx.worldContainerPickerLoading.value || !ctx.worldContainerPickerPath.value)
          return;
      const parentPath = ctx.resolveWorldContainerPickerParent(ctx.worldContainerPickerPath.value);
      void ctx.openWorldContainerPickerPath(parentPath);
  };

  ctx.refreshWorldContainerPicker = () => {
      if (ctx.worldContainerPickerLoading.value)
          return;
      void ctx.loadWorldContainerPickerEntries(ctx.worldContainerPickerPath.value);
  };

  ctx.handleWorldContainerPickerEntry = (entry: WorldContainerPickerEntry) => {
      if (entry.type === 'dir') {
          void ctx.openWorldContainerPickerPath(entry.path);
          return;
      }
      ctx.appendWorldAttachmentTokens([entry.path]);
      ctx.worldContainerPickerVisible.value = false;
      ctx.focusWorldTextareaToEnd();
  };

  ctx.rememberWorldEmoji = (emoji: string) => {
      const cleaned = String(emoji || '').trim();
      if (!cleaned)
          return;
      ctx.worldRecentEmojis.value = [cleaned, ...ctx.worldRecentEmojis.value.filter((item) => item !== cleaned)].slice(0, 12);
      ctx.saveStoredStringArray(WORLD_QUICK_EMOJI_STORAGE_KEY, ctx.worldRecentEmojis.value);
  };

  ctx.focusWorldTextareaToEnd = () => {
      nextTick(() => {
          const textarea = ctx.worldComposerViewRef.value?.getTextareaElement() || null;
          if (!textarea)
              return;
          if (typeof textarea.focus === 'function') {
              textarea.focus();
          }
          const cursor = String(ctx.worldDraft.value || '').length;
          if (typeof textarea.setSelectionRange === 'function') {
              textarea.setSelectionRange(cursor, cursor);
          }
      });
  };

  ctx.insertWorldEmoji = (emoji: string) => {
      const cleaned = String(emoji || '').trim();
      if (!cleaned)
          return;
      ctx.worldDraft.value = `${ctx.worldDraft.value}${cleaned}`;
      ctx.rememberWorldEmoji(cleaned);
      ctx.worldQuickPanelMode.value = '';
      ctx.focusWorldTextareaToEnd();
  };

  ctx.locateWorldHistoryMessage = async (entry: WorldHistoryRecord) => {
      const targetId = ctx.resolveWorldMessageDomId({ message_id: entry.messageId });
      ctx.worldHistoryDialogVisible.value = false;
      if (ctx.shouldVirtualizeMessages.value && ctx.isWorldConversationActive.value) {
          const targetIndex = ctx.worldRenderableMessages.value.findIndex((item) => item.domId === targetId);
          if (targetIndex >= 0) {
              ctx.scrollVirtualMessageToIndex(ctx.worldRenderableMessages.value.map((item) => item.key), targetIndex, 'center');
              await nextTick();
          }
      }
      await nextTick();
      const target = typeof document !== 'undefined' ? document.getElementById(targetId) : null;
      if (!target)
          return;
      target.scrollIntoView({ behavior: 'smooth', block: 'center' });
      target.classList.add('is-history-target');
      window.setTimeout(() => {
          target.classList.remove('is-history-target');
      }, 1400);
      ctx.scheduleMessageVirtualMeasure();
  };

  ctx.closeWorldQuickPanelWhenOutside = (event: Event) => {
      const target = event.target as Node | null;
      if (!target) {
          return;
      }
      const isInLeftRail = Boolean(ctx.leftRailRef.value?.contains(target));
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
      if (ctx.isRightDockOverlay.value && ctx.showRightDock.value && !ctx.rightDockCollapsed.value) {
          const pointerEvent = event as PointerEvent | null;
          const isSecondaryClick = Boolean(pointerEvent && typeof pointerEvent.button === 'number' && pointerEvent.button === 2);
          const targetElement = target instanceof Element ? target : null;
          const rightDockElement = ctx.rightDockRef.value?.$el || null;
          const hitInsideRightDock = Boolean((rightDockElement && rightDockElement.contains(target)) ||
              targetElement?.closest('.messenger-right-dock') ||
              targetElement?.closest('.messenger-files-context-menu') ||
              targetElement?.closest('.workspace-context-menu'));
          if (!isSecondaryClick && !hitInsideRightDock) {
              ctx.rightDockCollapsed.value = true;
          }
      }
      if (ctx.leftRailMoreExpanded.value && !isInLeftRail) {
          ctx.closeLeftRailMoreMenu();
      }
      if (ctx.isMiddlePaneOverlay.value && ctx.middlePaneOverlayVisible.value) {
          const isInMiddlePane = Boolean(ctx.middlePaneRef.value?.contains(target));
          if (!isInMiddlePane && !isInLeftRail) {
              ctx.clearMiddlePaneOverlayHide();
              ctx.middlePaneOverlayVisible.value = false;
          }
      }
  };
}
