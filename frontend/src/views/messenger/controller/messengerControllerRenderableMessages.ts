// @ts-nocheck
// User attachments, agent/world renderable message lists, virtualization helpers, and plan state.
import type { MessengerControllerContext } from './messengerControllerContext';
import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, toRaw, watch } from 'vue';
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
import { buildMessageVirtualWindow, resolveVirtualOffsetTop } from '@/views/messenger/messageVirtualWindow';
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
  hasAssistantPendingQuestion,
  isAssistantMessageRunning,
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
import { buildMessageIdentityDebugList } from '@/utils/chatMessageDebug';
import {
  buildChatRuntimeRenderableMessages,
  hasChatRuntimeRenderSession,
  isChatRuntimeProjectionRenderEnabled,
  isChatRuntimeProjectionRenderShadowEnabled,
  resolveChatRuntimeRenderableSourceDecision,
  resolveChatRuntimeProjectionRenderMode,
  summarizeChatRuntimeRenderableMessages
} from '@/realtime/chat/chatRuntimeRenderAdapter';
import {
  compareChatRuntimeRenderShadow,
  summarizeChatRuntimeRenderShadowReport
} from '@/realtime/chat/chatRuntimeRenderShadow';
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

type MessageVirtualSpacer = {
  key: string;
  height: number;
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

export function installMessengerControllerRenderableMessages(ctx: MessengerControllerContext): void {
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
          const isWorkspaceImage = Boolean(publicPath) && (contentType.startsWith('image/') || isImagePath(publicPath));
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
              const isImage = contentType.startsWith('image/') || isImagePath(publicPath);
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
      const renderableMessages = (ctx.agentRenderableMessages?.value || [])
          .map((item) => item.message as Record<string, unknown>);
      if (ctx.shouldVirtualizeMessages?.value && ctx.agentVirtualWindow?.value?.enabled) {
          const renderable = [
              ...(ctx.visibleAgentRenderableMessages?.value || []),
              ...(ctx.pinnedAgentRenderableMessages?.value || [])
          ];
          return ctx.collectUserAttachmentWorkspacePaths(renderable.map((item) => item.message));
      }
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
      return ctx.hasMessageContent(message?.content) || ctx.hasWorkflowOrThinking(message);
  };

  const buildLegacyAgentRenderableMessages = (): AgentRenderableMessage[] => ctx.chatStore.messages.reduce<AgentRenderableMessage[]>((acc, rawMessage, sourceIndex) => {
      const message = (rawMessage || {}) as Record<string, unknown>;
      if (!ctx.shouldRenderAgentMessage(message)) {
          return acc;
      }
      acc.push({
          key: ctx.resolveAgentMessageKey(message, sourceIndex),
          sourceIndex,
          message
      });
      return acc;
  }, []);

  const mergeProjectionRenderableWithSyntheticUiMessages = (
      legacyRenderable: AgentRenderableMessage[],
      projectionRenderable: AgentRenderableMessage[]
  ): AgentRenderableMessage[] => {
      const synthetic = legacyRenderable.filter((item) => ctx.isGreetingMessage(item.message as Record<string, unknown>));
      if (!synthetic.length) {
          return projectionRenderable;
      }
      const projectedHasGreeting = projectionRenderable.some((item) => ctx.isGreetingMessage(item.message as Record<string, unknown>));
      if (projectedHasGreeting) {
          return projectionRenderable;
      }
      return [...synthetic, ...projectionRenderable];
  };

  let lastAgentRenderSourceSignature = '';
  const logAgentRenderSource = (
      event: string,
      payload: Record<string, unknown>,
      renderable: AgentRenderableMessage[] = []
  ) => {
      if (!isChatDebugEnabled())
          return;
      const signature = [
          event,
          String(payload.activeSessionId || ''),
          String(payload.count || payload.legacyCount || 0),
          Array.isArray(payload.keys) ? payload.keys.join('|') : ''
      ].join('::');
      if (signature === lastAgentRenderSourceSignature)
          return;
      lastAgentRenderSourceSignature = signature;
      chatDebugLog('chat.runtime.render', event, {
          ...payload,
          messages: buildMessageIdentityDebugList(renderable.map((item) => item.message as Record<string, unknown>))
      });
  };

  let lastAgentRenderShadowSignature = '';
  const inspectAgentRuntimeRenderShadow = (legacyRenderable: AgentRenderableMessage[], projectionRenderable: AgentRenderableMessage[]) => {
      if (!isChatDebugEnabled() && !isChatRuntimeProjectionRenderShadowEnabled())
          return;
      if (!projectionRenderable.length && !legacyRenderable.length)
          return;
      const report = compareChatRuntimeRenderShadow({
          sessionId: ctx.chatStore.activeSessionId,
          legacy: legacyRenderable,
          projection: projectionRenderable
      });
      if (report.ok)
          return;
      if (report.fingerprint === lastAgentRenderShadowSignature)
          return;
      lastAgentRenderShadowSignature = report.fingerprint;
      const summary = summarizeChatRuntimeRenderShadowReport(report);
      if (isChatDebugEnabled()) {
          chatDebugLog('chat.runtime.render', 'render-source-drift', {
              ...summary,
              legacyMessages: buildMessageIdentityDebugList(
                  legacyRenderable.map((item) => item.message as Record<string, unknown>)
              ),
              projectionMessages: buildMessageIdentityDebugList(
                  projectionRenderable.map((item) => item.message as Record<string, unknown>)
              )
          });
      } else if (typeof console !== 'undefined') {
          console.info('[wunder-chat-runtime-render] render-source-drift', summary);
      }
  };

  ctx.agentRenderableMessages = computed<AgentRenderableMessage[]>(() => {
      const _renderVersion = ctx.chatStore.messageMutationVersion;
      const _projectionRenderVersion = ctx.chatStore.runtimeProjectionVersion;
      const renderMode = resolveChatRuntimeProjectionRenderMode();
      const shadowEnabled = isChatRuntimeProjectionRenderShadowEnabled();
      const legacyRenderable = buildLegacyAgentRenderableMessages();
      if (renderMode !== 'legacy' || shadowEnabled) {
          const projection = toRaw(ctx.chatStore.runtimeProjection);
          const projectionRenderable = buildChatRuntimeRenderableMessages({
            projection,
            sessionId: ctx.chatStore.activeSessionId,
            shouldRenderMessage: ctx.shouldRenderAgentMessage
          }) as AgentRenderableMessage[];
          const displayProjectionRenderable = mergeProjectionRenderableWithSyntheticUiMessages(
              legacyRenderable,
              projectionRenderable
          );
          const hasProjectionSession = hasChatRuntimeRenderSession(projection, ctx.chatStore.activeSessionId);
          const decision = resolveChatRuntimeRenderableSourceDecision({
              renderMode,
              projectionCount: displayProjectionRenderable.length,
              projectionSessionKnown: hasProjectionSession,
              shadowEnabled
          });
          if (decision.inspectShadow) {
              inspectAgentRuntimeRenderShadow(legacyRenderable, displayProjectionRenderable);
          }
          if (decision.event === 'projection-source') {
              logAgentRenderSource('projection-source', {
                  activeSessionId: ctx.chatStore.activeSessionId,
                  projectionSessionKnown: hasProjectionSession,
                  renderMode,
                  ...summarizeChatRuntimeRenderableMessages(displayProjectionRenderable)
              }, displayProjectionRenderable);
              return displayProjectionRenderable;
          }
          if (decision.event === 'projection-empty-fallback') {
              logAgentRenderSource('projection-empty-fallback', {
                  activeSessionId: ctx.chatStore.activeSessionId,
                  legacyCount: legacyRenderable.length
              }, legacyRenderable);
          } else if (decision.event === 'projection-shadow') {
              logAgentRenderSource('projection-shadow', {
                  activeSessionId: ctx.chatStore.activeSessionId,
                  renderMode,
                  ...summarizeChatRuntimeRenderableMessages(displayProjectionRenderable)
              }, displayProjectionRenderable);
          }
      }
      return legacyRenderable;
  });

  ctx.resolveActiveAgentRenderableMessageRecords = (): Record<string, unknown>[] => {
      const renderable = ctx.agentRenderableMessages?.value;
      if (Array.isArray(renderable)) {
          return renderable
              .map((item) => (item?.message || {}) as Record<string, unknown>)
              .filter((item) => item && typeof item === 'object' && !Array.isArray(item));
      }
      return (Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [])
          .map((item) => (item || {}) as Record<string, unknown>)
          .filter((item) => item && typeof item === 'object' && !Array.isArray(item));
  };

  ctx.agentRenderableContextMessages = computed<Record<string, unknown>[]>(() => ctx.resolveActiveAgentRenderableMessageRecords());

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

  ctx.worldRenderableMessages = computed<WorldRenderableMessage[]>(() => (Array.isArray(ctx.userWorldStore.activeMessages) ? ctx.userWorldStore.activeMessages : []).map((rawMessage, sourceIndex) => {
      const message = (rawMessage || {}) as Record<string, unknown>;
      return {
          // Keep vnode keys strictly unique in a render pass to avoid component patch corruption.
          key: ctx.resolveWorldRenderKey(message, sourceIndex),
          sourceIndex,
          domId: ctx.resolveWorldMessageDomId(message),
          message
      };
  }));

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
      const renderVersion = ctx.chatStore.messageMutationVersion;
      const projectionRenderVersion = ctx.chatStore.runtimeProjectionVersion;
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
          renderVersion,
          projectionRenderVersion,
          String(message.id || message.localId || '').trim(),
          String(message.content || '').length,
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

  ctx.latestWorldRenderableMessageKey = computed(() => {
      const latest = ctx.worldRenderableMessages.value[ctx.worldRenderableMessages.value.length - 1];
      return String(latest?.key || '').trim();
  });

  ctx.MESSAGE_VIRTUAL_OVERSCAN = 10;

  ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT = 8;

  ctx.shouldVirtualizeMessages = computed(() => {
      if (ctx.sessionHub.activeSection !== 'messages') {
          return false;
      }
      if (ctx.showChatSettingsView.value) {
          return false;
      }
      if (ctx.isAgentConversationActive.value) {
          return ctx.agentRenderableMessages.value.length > 12;
      }
      if (ctx.isWorldConversationActive.value) {
          return ctx.worldRenderableMessages.value.length > 12;
      }
      return false;
  });

  ctx.resolveVirtualMessageHeight = (key: string): number => {
      const normalized = String(key || '').trim();
      if (!normalized) {
          return ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
      }
      return ctx.messageVirtualHeightCache.get(normalized) || ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
  };

  ctx.estimateVirtualOffsetTop = (keys: string[], index: number): number => resolveVirtualOffsetTop(
      Array.isArray(keys) ? keys : [],
      index,
      ctx.resolveVirtualMessageHeight
  );

  ctx.agentVirtualWindow = computed(() => buildMessageVirtualWindow({
      items: ctx.agentRenderableMessages.value,
      enabled: ctx.shouldVirtualizeMessages.value && ctx.isAgentConversationActive.value,
      scrollTop: ctx.messageVirtualScrollTop.value,
      viewportHeight: ctx.messageVirtualViewportHeight.value,
      overscan: ctx.MESSAGE_VIRTUAL_OVERSCAN,
      tailPinCount: ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT,
      estimatedHeight: ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT,
      resolveHeight: ctx.resolveVirtualMessageHeight
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

  ctx.visibleAgentRenderableMessages = computed<AgentRenderableMessage[]>(() => ctx.agentVirtualWindow.value.enabled
      ? ctx.agentVirtualWindow.value.visibleItems
      : ctx.agentRenderableMessages.value);

  ctx.pinnedAgentRenderableMessages = computed<AgentRenderableMessage[]>(() => ctx.agentVirtualWindow.value.enabled
      ? ctx.agentVirtualWindow.value.tailItems
      : []);

  ctx.agentVirtualGroups = computed<AgentRenderableMessage[][]>(() => ctx.agentVirtualWindow.value.enabled
      ? [ctx.visibleAgentRenderableMessages.value, ctx.pinnedAgentRenderableMessages.value]
      : [ctx.visibleAgentRenderableMessages.value]);

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
      enabled: ctx.shouldVirtualizeMessages.value && ctx.isWorldConversationActive.value,
      scrollTop: ctx.messageVirtualScrollTop.value,
      viewportHeight: ctx.messageVirtualViewportHeight.value,
      overscan: ctx.MESSAGE_VIRTUAL_OVERSCAN,
      tailPinCount: ctx.MESSAGE_VIRTUAL_TAIL_PIN_COUNT,
      estimatedHeight: ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT,
      resolveHeight: ctx.resolveVirtualMessageHeight
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

  ctx.isCompactionMarkerMessage = (message: Record<string, unknown>): boolean => {
      if (String(message?.role || '') !== 'assistant')
          return false;
      if (ctx.hasMessageContent(message?.content))
          return false;
      if (ctx.hasMessageContent(message?.reasoning))
          return false;
      if (ctx.hasPlanSteps(message?.plan))
          return false;
      const panelStatus = String(((message?.questionPanel as Record<string, unknown> | null)?.status || ''))
          .trim()
          .toLowerCase();
      if (panelStatus === 'pending')
          return false;
      if (message?.manual_compaction_marker === true || message?.manualCompactionMarker === true) {
          return true;
      }
      if (!isCompactionOnlyWorkflowItems(message?.workflowItems))
          return false;
      const isStreaming = Boolean(message?.workflowStreaming ||
          message?.reasoningStreaming ||
          message?.stream_incomplete);
      if (!isStreaming)
          return true;
      const snapshot = resolveLatestCompactionSnapshot(message?.workflowItems);
      const triggerMode = String(snapshot?.detail?.trigger_mode ?? snapshot?.detail?.triggerMode ?? '')
          .trim()
          .toLowerCase();
      return triggerMode === 'manual';
  };

  ctx.isGoalMarkerMessage = (message: Record<string, unknown>): boolean => Boolean(message &&
      String(message?.role || '') === 'assistant' &&
      (message?.manual_goal_marker === true || message?.manualGoalMarker === true));

  ctx.shouldShowCompactionDivider = (message: Record<string, unknown>): boolean => {
      if (!ctx.isCompactionMarkerMessage(message))
          return false;
      if ((message?.manual_compaction_marker === true || message?.manualCompactionMarker === true) &&
          Boolean(message?.workflowStreaming ||
              message?.reasoningStreaming ||
              message?.stream_incomplete)) {
          return true;
      }
      const snapshot = resolveLatestCompactionSnapshot(message?.workflowItems);
      if (!snapshot)
          return false;
      const detailStatus = String(snapshot.detail?.status || '').trim().toLowerCase();
      if (detailStatus === 'skipped')
          return false;
      return true;
  };

  ctx.isVisibleAgentAssistantMessage = (message: Record<string, unknown>): boolean => String(message?.role || '') === 'assistant' &&
      !ctx.isHiddenInternalMessage(message) &&
      (!ctx.isCompactionMarkerMessage(message) || ctx.shouldShowCompactionDivider(message));

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
      if (hasActiveSubagentItems(message.subagents))
          return 'running';
      if (hasAssistantWaitingForCurrentOutput(message))
          return 'running';
      const runtimeState = resolveAssistantMessageRuntimeState(message) as AgentRuntimeState;
      if (runtimeState === 'done' &&
          ctx.isAgentConversationActive.value &&
          ctx.activeMessengerSessionBusy.value &&
          ctx.latestVisibleAgentAssistantMessage.value === message) {
          return 'running';
      }
      return runtimeState;
  };

  ctx.shouldShowAgentMessageBubble = (message: Record<string, unknown>): boolean => ctx.hasMessageContent(buildAssistantDisplayContent(message, ctx.t));

  ctx.messageStatsNowTick = ref(Date.now());

  ctx.messageStatsTimer = null;

  ctx.hasLiveAssistantStats = computed(() => ctx.agentRenderableMessages.value.some((item) => {
      const message = (item?.message || {}) as Record<string, unknown>;
      if (String(message?.role || '') !== 'assistant' || message?.isGreeting) {
          return false;
      }
      return Boolean(
          message?.resume_available ||
          message?.slow_client ||
          message?.workflowStreaming ||
          message?.reasoningStreaming ||
          message?.stream_incomplete ||
          hasAssistantPendingQuestion(message) ||
          hasAssistantWaitingForCurrentOutput(message) ||
          isAssistantMessageRunning(message) ||
          hasActiveSubagentItems(message?.subagents) ||
          Number.isFinite(Number(message?.retry_started_at_ms ?? message?.retryStartedAtMs)) ||
          Number.isFinite(Number(message?.retry_next_attempt_at_ms ?? message?.retryNextAttemptAtMs)) ||
          Number.isFinite(Number(message?.retry_attempt ?? message?.retryAttempt)) ||
          Number.isFinite(Number(message?.retry_max_attempts ?? message?.retryMaxAttempts))
      );
  }));

  ctx.buildMessageStatsEntries = (message: Record<string, unknown>, index = 0) => {
      const nowTick = ctx.messageStatsNowTick.value;
      if (!message || String(message?.role || '') !== 'assistant' || message?.isGreeting) {
          return [];
      }
      const sourceIndex = Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0;
      const messageKey = ctx.resolveAgentMessageKey(message, sourceIndex);
      const scopedMessageKey = `${String(ctx.sessionHub.activeConversationKey || '')}:${messageKey}`;
      const activeSessionBusy = Boolean(ctx.isAgentConversationActive.value && ctx.activeMessengerSessionBusy.value);
      const latestVisibleAssistant = ctx.latestVisibleAgentAssistantMessage.value === message;
      const latestActiveAssistantBusy = Boolean(
          activeSessionBusy &&
          latestVisibleAssistant
      );
      const workflowItems = Array.isArray(message.workflowItems) ? (message.workflowItems as unknown[]) : [];
      const subagents = Array.isArray(message.subagents) ? (message.subagents as unknown[]) : [];
      const signature = [
          ctx.chatStore.activeSessionId,
          ctx.chatStore.messageMutationVersion,
          ctx.chatStore.runtimeProjectionVersion,
          nowTick,
          String(messageKey || '').trim(),
          latestActiveAssistantBusy,
          String(message.content || '').length,
          String(message.reasoning || '').length,
          String(message.state || '').trim(),
          Boolean(message.workflowStreaming),
          Boolean(message.reasoningStreaming),
          Boolean(message.stream_incomplete),
          Boolean(message.resume_available),
          Boolean(message.slow_client),
          workflowItems.length,
          subagents.length,
          String(message.retry_started_at_ms ?? message.retryStartedAtMs ?? ''),
          String(message.retry_next_attempt_at_ms ?? message.retryNextAttemptAtMs ?? ''),
          String(message.retry_attempt ?? message.retryAttempt ?? ''),
          String(message.retry_max_attempts ?? message.retryMaxAttempts ?? ''),
          JSON.stringify(message.stats || null)
      ].join('::');
      const cached = ctx.messageStatsEntryCache.get(scopedMessageKey);
      if (cached?.signature === signature) {
          return cached.entries;
      }
      const entries = buildAssistantMessageStatsEntries(
          message as Record<string, any>,
          ctx.t,
          ctx.agentRenderableMessages.value.map((item) => item.message as Record<string, any>),
          nowTick,
          {
              activeSessionBusy,
              latestVisibleAssistant
          }
      );
      ctx.messageStatsEntryCache.set(scopedMessageKey, { signature, entries });
      if (ctx.messageStatsEntryCache.size > 80) {
          const firstKey = ctx.messageStatsEntryCache.keys().next().value;
          if (firstKey) {
              ctx.messageStatsEntryCache.delete(firstKey);
          }
      }
      return entries;
  };

  ctx.shouldShowMessageStats = (message: Record<string, unknown>, index = 0): boolean => ctx.buildMessageStatsEntries(message, index).length > 0;

  const stopMessageStatsTimer = () => {
      if (typeof window !== 'undefined' && ctx.messageStatsTimer !== null) {
          window.clearInterval(ctx.messageStatsTimer);
          ctx.messageStatsTimer = null;
      }
  };

  const ensureMessageStatsTimer = () => {
      if (typeof window === 'undefined') {
          return;
      }
      if (!ctx.hasLiveAssistantStats.value) {
          stopMessageStatsTimer();
          return;
      }
      if (ctx.messageStatsTimer !== null) {
          return;
      }
      ctx.messageStatsTimer = window.setInterval(() => {
          ctx.messageStatsNowTick.value = Date.now();
      }, 1000);
  };

  watch(() => ctx.hasLiveAssistantStats.value, (enabled) => {
      if (enabled) {
          ensureMessageStatsTimer();
          return;
      }
      stopMessageStatsTimer();
  }, { immediate: true });

  onMounted(() => {
      ensureMessageStatsTimer();
  });

  onBeforeUnmount(() => {
      stopMessageStatsTimer();
  });

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
