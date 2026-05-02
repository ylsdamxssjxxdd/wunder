// @ts-nocheck
// Markdown rendering, world voice playback, assistant resume, copy actions, and world message identity.
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
  fetchRealtimeSystemPrompt,
  synthesizeChatTts
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

export function installMessengerControllerMessageMarkdownVoice(ctx: MessengerControllerContext): void {
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
      const cacheKey = `agent:${ctx.resolveAgentMessageKey(message, index)}:c${ctx.currentContainerId.value}`;
      const streaming = Boolean(message?.stream_incomplete) ||
          Boolean(message?.workflowStreaming) ||
          Boolean(message?.reasoningStreaming);
      return ctx.renderMessageMarkdown(cacheKey, buildAssistantDisplayContent(message, ctx.t), {
          streaming,
          resolveWorkspacePath: ctx.resolveAgentMarkdownWorkspacePath,
          message
      });
  };

  ctx.renderWorldMarkdown = (message: Record<string, unknown>): string => {
      const cacheKey = `world:${ctx.resolveWorldMessageKey(message)}`;
      const content = String(message?.content || '');
      const senderUserId = String(message?.sender_user_id || '').trim();
      const patched = ctx.replaceWorldAtPathTokens(content, senderUserId);
      return ctx.renderMessageMarkdown(cacheKey, patched, {
          message,
          resolveWorkspacePath: (rawPath: string) => ctx.resolveWorldMarkdownWorkspacePath(rawPath, senderUserId)
      });
  };

  ctx.resolveWorldVoicePayloadFromMessage = (message: Record<string, unknown>) => {
      if (!isWorldVoiceContentType(message?.content_type))
          return null;
      return parseWorldVoicePayload(message?.content);
  };

  ctx.isWorldVoiceMessage = (message: Record<string, unknown>): boolean => Boolean(ctx.resolveWorldVoicePayloadFromMessage(message));

  ctx.isWorldVoicePlaying = (message: Record<string, unknown>): boolean => ctx.worldVoicePlayingMessageKey.value === ctx.resolveWorldMessageKey(message);

  ctx.isWorldVoiceLoading = (message: Record<string, unknown>): boolean => ctx.worldVoiceLoadingMessageKey.value === ctx.resolveWorldMessageKey(message);

  ctx.resolveWorldVoiceTotalDurationMs = (message: Record<string, unknown>): number => {
      const payload = ctx.resolveWorldVoicePayloadFromMessage(message);
      const payloadDuration = Number(payload?.duration_ms || 0);
      if (!Number.isFinite(payloadDuration) || payloadDuration <= 0) {
          const messageKey = ctx.resolveWorldMessageKey(message);
          if (messageKey && messageKey === ctx.worldVoicePlayingMessageKey.value) {
              return Math.max(0, Number(ctx.worldVoicePlaybackDurationMs.value || 0));
          }
          return 0;
      }
      const messageKey = ctx.resolveWorldMessageKey(message);
      if (messageKey && messageKey === ctx.worldVoicePlayingMessageKey.value) {
          return Math.max(payloadDuration, Number(ctx.worldVoicePlaybackDurationMs.value || 0));
      }
      return payloadDuration;
  };

  ctx.resolveWorldVoiceDurationLabel = (message: Record<string, unknown>): string => {
      const totalDurationMs = ctx.resolveWorldVoiceTotalDurationMs(message);
      if (!totalDurationMs) {
          return ctx.t('messenger.world.voice.durationUnknown');
      }
      if (!ctx.isWorldVoicePlaying(message)) {
          return formatWorldVoiceDuration(totalDurationMs);
      }
      const remainingMs = Math.max(0, totalDurationMs - Number(ctx.worldVoicePlaybackCurrentMs.value || 0));
      return ctx.t('messenger.world.voice.remaining', {
          duration: formatWorldVoiceDuration(remainingMs)
      });
  };

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
          ctx.setRuntimeStateOverride(targetAgentId, 'done', 8000);
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
          ElMessage.error(ctx.t('chat.message.ttsFailed'));
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
      const contact = ctx.userWorldStore.contacts.find((item) => String(item?.user_id || '') === sender);
      if (contact?.username)
          return contact.username;
      const user = ctx.authStore.user as Record<string, unknown> | null;
      if (String(user?.id || '') === sender) {
          return String(user?.username || sender);
      }
      return sender;
  };

  ctx.resolveWorldMessageKey = (message: Record<string, unknown>): string => String(message?.message_id ||
      message?.id ||
      `${message?.sender_user_id || 'peer'}-${message?.created_at || ''}`);

  ctx.resolveWorldRenderKey = (message: Record<string, unknown>, index: number): string => {
      const safeIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
      return `${ctx.resolveWorldMessageKey(message)}:${safeIndex}`;
  };

  ctx.resolveWorldMessageDomId = (message: Record<string, unknown>): string => {
      const messageId = Number.parseInt(String(message?.message_id || ''), 10);
      if (Number.isFinite(messageId) && messageId > 0) {
          return `uw-message-${messageId}`;
      }
      const fallbackKey = ctx.resolveWorldMessageKey(message).replace(/[^a-zA-Z0-9_-]/g, '_');
      return `uw-message-${fallbackKey}`;
  };

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

  ctx.resolveWorldVoiceContainerId = (value: unknown): number => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed))
          return USER_CONTAINER_ID;
      return Math.max(0, Math.round(parsed));
  };

  ctx.buildWorldVoiceResourceKey = (conversationId: string, ownerUserId: string, containerId: number, path: string): string => `${conversationId}|${ownerUserId}|${containerId}|${path}`;

  ctx.fetchWorldVoiceObjectUrl = async (message: Record<string, unknown>, payload: {
      path: string;
      container_id?: number;
      owner_user_id?: string;
  }, runtime: WorldVoicePlaybackRuntime): Promise<{
      resourceKey: string;
      objectUrl: string;
  }> => {
      const conversationId = String(message?.conversation_id || ctx.activeWorldConversationId.value || '').trim();
      if (!conversationId) {
          throw new Error(ctx.t('messenger.world.voice.playFailed'));
      }
      const path = ctx.normalizeUploadPath(payload.path);
      if (!path) {
          throw new Error(ctx.t('messenger.world.voice.playFailed'));
      }
      const ownerUserId = String(payload.owner_user_id || '').trim() ||
          String(message?.sender_user_id || '').trim() ||
          String(ctx.currentUserId.value || '').trim();
      if (!ownerUserId) {
          throw new Error(ctx.t('messenger.world.voice.playFailed'));
      }
      const containerId = ctx.resolveWorldVoiceContainerId(payload.container_id);
      const resourceKey = ctx.buildWorldVoiceResourceKey(conversationId, ownerUserId, containerId, path);
      const cached = runtime.objectUrlCache.get(resourceKey);
      if (cached) {
          return { resourceKey, objectUrl: cached };
      }
      const response = await downloadUserWorldFile({
          conversation_id: conversationId,
          owner_user_id: ownerUserId,
          container_id: containerId,
          path
      });
      const blob = response.data as Blob;
      if (!(blob instanceof Blob) || !blob.size) {
          throw new Error(ctx.t('messenger.world.voice.playFailed'));
      }
      const objectUrl = URL.createObjectURL(blob);
      runtime.objectUrlCache.set(resourceKey, objectUrl);
      return { resourceKey, objectUrl };
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
