// @ts-nocheck
// User attachments, agent/world renderable message lists, virtualization helpers, and plan state.
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

  ctx.userAttachmentWorkspacePaths = computed(() => {
      const _ = ctx.currentUserId.value;
      const paths = new Set<string>();
      ctx.chatStore.messages.forEach((message) => {
          if (String((message as Record<string, unknown>)?.role || '') !== 'user')
              return;
          const attachments = Array.isArray((message as Record<string, unknown>)?.attachments)
              ? ((message as Record<string, unknown>).attachments as unknown[])
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
  });

  ctx.hasUserImageAttachments = (message: Record<string, unknown>): boolean => ctx.resolveUserImageAttachments(message).length > 0;

  ctx.hasUserAudioAttachments = (message: Record<string, unknown>): boolean => ctx.resolveUserAudioAttachments(message).length > 0;

  ctx.hasWorkflowOrThinking = (message: Record<string, unknown>): boolean => Boolean(message?.workflowStreaming) ||
      Boolean(message?.reasoningStreaming) ||
      Boolean((message?.workflowItems as unknown[])?.length) ||
      hasActiveSubagentItems(message?.subagents) ||
      Boolean((message?.subagents as unknown[])?.length) ||
      ctx.hasMessageContent(message?.reasoning);

  ctx.shouldRenderAgentMessage = (message: Record<string, unknown>): boolean => {
      if (String(message?.role || '') === 'user')
          return true;
      return ctx.hasMessageContent(message?.content) || ctx.hasWorkflowOrThinking(message);
  };

  ctx.agentRenderableMessages = computed<AgentRenderableMessage[]>(() => ctx.chatStore.messages.reduce<AgentRenderableMessage[]>((acc, rawMessage, sourceIndex) => {
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
  }, []));

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
      const workflowSignature = workflowItems
          .map((item, index) => {
          const record = (item || {}) as Record<string, unknown>;
          return [
              index,
              String(record.id || record.toolCallId || record.eventType || '').trim(),
              String(record.status || '').trim(),
              String(record.title || record.toolName || '').length,
              String(record.detail || '').length
          ].join(':');
      })
          .join('|');
      const subagents = Array.isArray(message.subagents) ? message.subagents : [];
      const subagentSignature = subagents
          .map((item, index) => {
          const record = (item || {}) as Record<string, unknown>;
          return [
              index,
              String(record.key || record.run_id || record.session_id || '').trim(),
              String(record.status || '').trim(),
              String(record.summary || '').length
          ].join(':');
      })
          .join('|');
      return [
          ctx.latestAgentRenderableMessageKey.value,
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

  ctx.shouldVirtualizeMessages = computed(
  // Messenger message virtualization is disabled because it repeatedly delayed live workflow rendering.
  () => false);

  ctx.resolveVirtualMessageHeight = (key: string): number => {
      const normalized = String(key || '').trim();
      if (!normalized) {
          return ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
      }
      return ctx.messageVirtualHeightCache.get(normalized) || ctx.MESSAGE_VIRTUAL_ESTIMATED_HEIGHT;
  };

  ctx.estimateVirtualOffsetTop = (_keys: string[], _index: number): number => 0;

  ctx.isGreetingMessage = (message: Record<string, unknown>): boolean => String(message?.role || '') === 'assistant' && Boolean(message?.isGreeting);

  ctx.isHiddenInternalMessage = (message: Record<string, unknown>): boolean => Boolean(message?.hiddenInternal);

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
      for (let index = ctx.chatStore.messages.length - 1; index >= 0; index -= 1) {
          const message = (ctx.chatStore.messages[index] || {}) as Record<string, unknown>;
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
      const runtimeState = resolveAssistantMessageRuntimeState(
          message,
          ctx.chatStore.messages as Record<string, unknown>[]
      ) as AgentRuntimeState;
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

  ctx.buildMessageStatsEntries = (message: Record<string, unknown>) => (void ctx.messageStatsNowTick.value,
      buildAssistantMessageStatsEntries(message as Record<string, any>, ctx.t, ctx.chatStore.messages as Record<string, any>[], ctx.messageStatsNowTick.value));

  ctx.shouldShowMessageStats = (message: Record<string, unknown>): boolean => ctx.buildMessageStatsEntries(message).length > 0;

  onMounted(() => {
      if (typeof window === 'undefined' || ctx.messageStatsTimer !== null)
          return;
      ctx.messageStatsTimer = window.setInterval(() => {
          ctx.messageStatsNowTick.value = Date.now();
      }, 1000);
  });

  onBeforeUnmount(() => {
      if (typeof window !== 'undefined' && ctx.messageStatsTimer !== null) {
          window.clearInterval(ctx.messageStatsTimer);
          ctx.messageStatsTimer = null;
      }
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
      for (let index = ctx.chatStore.messages.length - 1; index >= 0; index -= 1) {
          const message = ctx.chatStore.messages[index] as Record<string, unknown> | undefined;
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
