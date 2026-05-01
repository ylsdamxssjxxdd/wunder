// @ts-nocheck
// Workspace path resolution, resource fetching, markdown resource cards, image preview, and resource downloads.
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

export function installMessengerControllerWorkspaceResourceHydration(ctx: MessengerControllerContext): void {
  ctx.resolveDesktopWorkspaceRoot = (): string => String(getRuntimeConfig().workspace_root || '').trim();

  ctx.resolveDesktopContainerRoot = (containerId?: number | null): string => {
      if (containerId !== null && Number.isFinite(Number(containerId))) {
          const mapped = String(ctx.desktopContainerRootMap.value[Number(containerId)] || '').trim();
          if (mapped)
              return mapped;
      }
      return ctx.resolveDesktopWorkspaceRoot();
  };

  ctx.resolveAgentMarkdownWorkspacePath = (rawPath: string): string => {
      const ownerId = normalizeWorkspaceOwnerId(ctx.authStore.user?.id);
      if (!ownerId)
          return '';
      return resolveMarkdownWorkspacePath({
          rawPath,
          ownerId,
          containerId: ctx.currentContainerId.value,
          desktopLocalMode: ctx.desktopLocalMode.value,
          workspaceRoot: ctx.resolveDesktopContainerRoot(ctx.currentContainerId.value)
      });
  };

  ctx.resolveWorldMarkdownWorkspacePath = (rawPath: string, senderUserId: string): string => {
      const ownerId = normalizeWorkspaceOwnerId(senderUserId);
      if (!ownerId)
          return '';
      return resolveMarkdownWorkspacePath({
          rawPath,
          ownerId,
          containerId: USER_CONTAINER_ID,
          desktopLocalMode: ctx.desktopLocalMode.value,
          workspaceRoot: ctx.resolveDesktopContainerRoot(USER_CONTAINER_ID)
      });
  };

  ctx.WORLD_AT_PATH_RE = /(^|[\s\n])@("([^"]+)"|'([^']+)'|[^\s]+)/g;

  ctx.WORLD_AT_PATH_SUFFIX_RE = /^(.*?)([)\]\}>,.;:!?\uFF0C\u3002\uFF1B\uFF1A\uFF01\uFF1F\u300B\u3011]+)?$/;

  ctx.decodeWorldAtPathToken = (value: string): string => {
      if (!/%[0-9a-fA-F]{2}/.test(value))
          return value;
      try {
          return decodeURIComponent(value);
      }
      catch {
          return value;
      }
  };

  ctx.replaceWorldAtPathTokens = (content: string, senderUserId: string): string => {
      if (!content)
          return '';
      const ownerId = normalizeWorkspaceOwnerId(senderUserId);
      if (!ownerId)
          return content;
      return content.replace(ctx.WORLD_AT_PATH_RE, (match, prefix, token, doubleQuoted, singleQuoted) => {
          const raw = doubleQuoted ?? singleQuoted ?? token ?? '';
          if (!raw)
              return match;
          let value = raw;
          let suffix = '';
          if (!doubleQuoted && !singleQuoted) {
              const split = ctx.WORLD_AT_PATH_SUFFIX_RE.exec(value);
              if (split) {
                  value = split[1] ?? value;
                  suffix = split[2] ?? '';
              }
          }
          const decoded = ctx.decodeWorldAtPathToken(String(value || '').trim());
          const normalized = ctx.normalizeUploadPath(decoded);
          if (!normalized)
              return match;
          const pathLike = decoded.startsWith('/') ||
              decoded.startsWith('./') ||
              decoded.startsWith('../') ||
              normalized.includes('/') ||
              normalized.includes('.');
          if (!pathLike)
              return match;
          const publicPath = buildWorkspacePublicPath(ownerId, normalized, USER_CONTAINER_ID);
          if (!publicPath)
              return match;
          const label = decoded;
          const replacement = isImagePath(normalized)
              ? `![${label}](${publicPath})`
              : `[${label}](${publicPath})`;
          return `${prefix}${replacement}${suffix}`;
      });
  };

  ctx.resolveWorkspaceResource = (publicPath: string): WorkspaceResolvedResource | null => {
      const parsed = parseWorkspaceResourceUrl(publicPath);
      if (!parsed)
          return null;
      const user = ctx.authStore.user as Record<string, unknown> | null;
      if (!user)
          return null;
      const currentId = normalizeWorkspaceOwnerId(user.id);
      const workspaceId = parsed.workspaceId || parsed.userId;
      const ownerId = parsed.ownerId || workspaceId;
      const agentId = parsed.agentId || '';
      const containerId = typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
          ? parsed.containerId
          : null;
      const isOwner = Boolean(currentId) &&
          (workspaceId === currentId ||
              workspaceId.startsWith(`${currentId}__agent__`) ||
              workspaceId.startsWith(`${currentId}__a__`) ||
              workspaceId.startsWith(`${currentId}__c__`));
      if (isOwner) {
          return {
              ...parsed,
              requestUserId: null,
              requestAgentId: agentId || null,
              requestContainerId: containerId,
              allowed: true
          };
      }
      if (ctx.isAdminUser(user)) {
          return {
              ...parsed,
              requestUserId: ownerId,
              requestAgentId: agentId || null,
              requestContainerId: containerId,
              allowed: true
          };
      }
      // Non-admin requests should prefer the current login context to avoid cross-display ID mismatches.
      return {
          ...parsed,
          requestUserId: null,
          requestAgentId: agentId || null,
          requestContainerId: containerId,
          allowed: true
      };
  };

  ctx.fetchWorkspaceResource = async (resource: WorkspaceResolvedResource) => {
      const cacheKey = resource.publicPath;
      const cached = ctx.workspaceResourceCache.get(cacheKey);
      if (cached?.objectUrl) {
          return {
              objectUrl: cached.objectUrl,
              filename: cached.filename || resource.filename || 'download'
          };
      }
      if (cached?.promise)
          return cached.promise;
      const promise = (async () => {
          const params: Record<string, string> = {
              path: String(resource.relativePath || '')
          };
          if (resource.requestUserId) {
              params.user_id = resource.requestUserId;
          }
          if (resource.requestAgentId) {
              params.agent_id = resource.requestAgentId;
          }
          if (resource.requestContainerId !== null && Number.isFinite(resource.requestContainerId)) {
              params.container_id = String(resource.requestContainerId);
          }
          const response = await downloadWunderWorkspaceFile(params);
          try {
              const filename = getFilenameFromHeaders(response?.headers as Record<string, unknown>, resource.filename || 'download');
              const contentType = String((response?.headers as Record<string, unknown>)?.['content-type'] ||
                  (response?.headers as Record<string, unknown>)?.['Content-Type'] ||
                  '');
              const normalizedBlob = normalizeWorkspaceImageBlob(response.data as Blob, filename, contentType);
              const objectUrl = URL.createObjectURL(normalizedBlob);
              const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
              ctx.workspaceResourceCache.set(cacheKey, entry);
              return entry;
          }
          catch (error) {
              ctx.workspaceResourceCache.delete(cacheKey);
              throw error;
          }
      })()
          .catch((error) => {
          ctx.workspaceResourceCache.delete(cacheKey);
          throw error;
      });
      ctx.workspaceResourceCache.set(cacheKey, { promise });
      return promise;
  };

  ctx.setUserAttachmentResourceState = (publicPath: string, state: AttachmentResourceState) => {
      const next = new Map(ctx.userAttachmentResourceCache.value);
      next.set(publicPath, state);
      ctx.userAttachmentResourceCache.value = next;
  };

  ctx.ensureUserAttachmentResource = async (publicPath: string) => {
      const normalized = String(publicPath || '').trim();
      if (!normalized)
          return;
      const existing = ctx.userAttachmentResourceCache.value.get(normalized);
      if (existing)
          return;
      const resource = ctx.resolveWorkspaceResource(normalized);
      if (!resource)
          return;
      if (!resource.allowed) {
          ctx.setUserAttachmentResourceState(normalized, { error: true });
          return;
      }
      ctx.setUserAttachmentResourceState(normalized, { loading: true });
      try {
          const entry = await ctx.fetchWorkspaceResource(resource);
          ctx.setUserAttachmentResourceState(normalized, {
              objectUrl: entry.objectUrl,
              filename: entry.filename
          });
      }
      catch (error) {
          ctx.setUserAttachmentResourceState(normalized, { error: true });
      }
  };

  ctx.isWorkspaceResourceMissing = (error: unknown): boolean => {
      const status = Number((error as {
          response?: {
              status?: unknown;
          };
      })?.response?.status || 0);
      if (status === 404 || status === 410)
          return true;
      const raw = (error as {
          response?: {
              data?: {
                  detail?: string;
                  message?: string;
              };
          };
      })?.response?.data?.detail ||
          (error as {
              response?: {
                  data?: {
                      message?: string;
                  };
              };
          })?.response?.data?.message ||
          (error as {
              message?: string;
          })?.message ||
          '';
      const message = typeof raw === 'string' ? raw : String(raw || '');
      return /not found|no such|娑撳秴鐡ㄩ崷鈻呴幍鍙ョ瑝閸掔殬瀹告彃鍨归梽顦㈠鑼╅梽顦emoved/i.test(message);
  };

  ctx.hydrateWorkspaceResourceCard = async (card: HTMLElement) => {
      if (!card || card.dataset.workspaceState)
          return;
      const kind = String(card.dataset.workspaceKind || 'image');
      if (kind !== 'image') {
          card.dataset.workspaceState = 'ready';
          return;
      }
      const publicPath = String(card.dataset.workspacePath || '').trim();
      const status = card.querySelector('.ai-resource-status') as HTMLElement | null;
      const preview = card.querySelector('.ai-resource-preview') as HTMLImageElement | null;
      if (!publicPath || !preview)
          return;
      const resource = ctx.resolveWorkspaceResource(publicPath);
      if (!resource || !resource.allowed) {
          if (status)
              status.textContent = ctx.t('chat.resourceUnavailable');
          card.dataset.workspaceState = 'error';
          card.classList.add('is-error');
          return;
      }
      card.dataset.workspaceState = 'loading';
      card.classList.remove('is-error');
      card.classList.remove('is-ready');
      const loadingTimerId = scheduleWorkspaceLoadingLabel(card, status, ctx.t('chat.resourceImageLoading'));
      try {
          const entry = await ctx.fetchWorkspaceResource(resource);
          preview.src = entry.objectUrl;
          card.dataset.workspaceState = 'ready';
          card.classList.add('is-ready');
          if (status)
              status.textContent = '';
      }
      catch (error) {
          if (status) {
              status.textContent = ctx.isWorkspaceResourceMissing(error)
                  ? ctx.t('chat.resourceMissing')
                  : ctx.t('chat.resourceImageFailed');
          }
          card.dataset.workspaceState = 'error';
          card.classList.add('is-error');
      }
      finally {
          clearWorkspaceLoadingLabelTimer(loadingTimerId);
      }
  };

  ctx.hydrateWorkspaceResources = () => {
      const container = ctx.messageListRef.value;
      if (!container)
          return;
      const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
      cards.forEach((card) => {
          void ctx.hydrateWorkspaceResourceCard(card as HTMLElement);
      });
      hydrateExternalMarkdownImages(container);
  };

  ctx.scheduleWorkspaceResourceHydration = () => {
      if (ctx.workspaceResourceHydrationFrame !== null || ctx.workspaceResourceHydrationPending)
          return;
      ctx.workspaceResourceHydrationPending = true;
      void nextTick(() => {
          ctx.workspaceResourceHydrationPending = false;
          if (ctx.workspaceResourceHydrationFrame !== null || typeof window === 'undefined')
              return;
          ctx.workspaceResourceHydrationFrame = window.requestAnimationFrame(() => {
              ctx.workspaceResourceHydrationFrame = null;
              ctx.hydrateWorkspaceResources();
          });
      });
  };

  ctx.resetWorkspaceResourceCards = (changedPaths: string[] = []) => {
      const container = ctx.messageListRef.value;
      if (!container)
          return;
      const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
      cards.forEach((card) => {
          const element = card as HTMLElement;
          const publicPath = String(element.dataset.workspacePath || '').trim();
          if (changedPaths.length) {
              const parsed = parseWorkspaceResourceUrl(publicPath);
              if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
                  return;
              }
          }
          resetWorkspaceImageCardState(element, { clearSrc: true, includeReady: true });
      });
  };

  ctx.clearWorkspaceResourceCache = () => {
      if (ctx.workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
          window.cancelAnimationFrame(ctx.workspaceResourceHydrationFrame);
          ctx.workspaceResourceHydrationFrame = null;
      }
      ctx.workspaceResourceHydrationPending = false;
      ctx.workspaceResourceCache.forEach((entry) => {
          if (entry?.objectUrl) {
              URL.revokeObjectURL(entry.objectUrl);
          }
      });
      ctx.workspaceResourceCache.clear();
      ctx.userAttachmentResourceCache.value = new Map();
  };

  ctx.clearWorkspaceResourceCacheByPaths = (changedPaths: string[] = []) => {
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          return;
      }
      if (ctx.workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
          window.cancelAnimationFrame(ctx.workspaceResourceHydrationFrame);
          ctx.workspaceResourceHydrationFrame = null;
      }
      ctx.workspaceResourceHydrationPending = false;
      Array.from(ctx.workspaceResourceCache.entries()).forEach(([cacheKey, entry]) => {
          const parsed = parseWorkspaceResourceUrl(cacheKey);
          if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
              return;
          }
          if (entry?.objectUrl) {
              URL.revokeObjectURL(entry.objectUrl);
          }
          ctx.workspaceResourceCache.delete(cacheKey);
      });
      if (ctx.userAttachmentResourceCache.value.size > 0) {
          const next = new Map(ctx.userAttachmentResourceCache.value);
          Array.from(next.keys()).forEach((publicPath) => {
              const parsed = parseWorkspaceResourceUrl(publicPath);
              if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
                  return;
              }
              const entry = next.get(publicPath);
              if (entry?.objectUrl) {
                  URL.revokeObjectURL(entry.objectUrl);
              }
              next.delete(publicPath);
          });
          ctx.userAttachmentResourceCache.value = next;
      }
  };

  ctx.parseWorkspaceRefreshContainerId = (value: unknown): number | null => {
      const parsed = Number.parseInt(String(value ?? ''), 10);
      return Number.isFinite(parsed) ? parsed : null;
  };

  ctx.shouldHandleWorkspaceResourceRefresh = (detail: Record<string, unknown>) => {
      const eventAgentId = ctx.normalizeAgentId(detail.agentId ?? detail.agent_id);
      const eventContainerId = ctx.parseWorkspaceRefreshContainerId(detail.containerId ?? detail.container_id);
      if (ctx.isWorldConversationActive.value) {
          if (eventAgentId)
              return false;
          return !Number.isFinite(eventContainerId) || eventContainerId === USER_CONTAINER_ID;
      }
      if (!ctx.isAgentConversationActive.value) {
          return false;
      }
      const currentAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      if (eventAgentId && eventAgentId !== currentAgentId) {
          return false;
      }
      return !Number.isFinite(eventContainerId) || eventContainerId === ctx.currentContainerId.value;
  };

  ctx.handleWorkspaceResourceRefresh = (event?: Event) => {
      const detail = (event as CustomEvent<Record<string, unknown>> | undefined)?.detail &&
          typeof (event as CustomEvent<Record<string, unknown>>).detail === 'object'
          ? ((event as CustomEvent<Record<string, unknown>>).detail as Record<string, unknown>)
          : {};
      if (!ctx.shouldHandleWorkspaceResourceRefresh(detail)) {
          return;
      }
      const changedPaths = extractWorkspaceRefreshPaths(detail);
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          ctx.resetWorkspaceResourceCards();
          ctx.scheduleWorkspaceResourceHydration();
          return;
      }
      ctx.clearWorkspaceResourceCacheByPaths(changedPaths);
      ctx.resetWorkspaceResourceCards(changedPaths);
      ctx.scheduleWorkspaceResourceHydration();
  };

  ctx.downloadWorkspaceResource = async (publicPath: string) => {
      const resource = ctx.resolveWorkspaceResource(publicPath);
      if (!resource || !resource.allowed)
          return;
      try {
          const entry = await ctx.fetchWorkspaceResource(resource);
          saveObjectUrlAsFile(entry.objectUrl, entry.filename || resource.filename || 'download');
      }
      catch (error) {
          ElMessage.error(ctx.isWorkspaceResourceMissing(error) ? ctx.t('chat.resourceMissing') : ctx.t('chat.resourceDownloadFailed'));
      }
  };

  ctx.downloadExternalImage = async (src: string) => {
      const url = String(src || '').trim();
      if (!url)
          return;
      try {
          const response = await fetch(url);
          if (!response.ok) {
              throw new Error(`HTTP ${response.status}`);
          }
          const blob = await response.blob();
          const objectUrl = URL.createObjectURL(blob);
          // Extract filename from URL
          let filename = 'image';
          try {
              const pathname = new URL(url).pathname;
              const basename = pathname.split('/').pop() || '';
              if (basename && basename.includes('.')) {
                  filename = basename;
              }
              else {
                  // Determine extension from MIME type
                  const ext = blob.type.split('/')[1] || 'png';
                  filename = `image.${ext}`;
              }
          }
          catch {
              const ext = blob.type.split('/')[1] || 'png';
              filename = `image.${ext}`;
          }
          saveObjectUrlAsFile(objectUrl, filename);
          URL.revokeObjectURL(objectUrl);
      }
      catch (error) {
          ElMessage.error(ctx.t('chat.resourceDownloadFailed'));
      }
  };

  ctx.openImagePreview = (src: string, title = '', workspacePath = '') => {
      const normalizedSrc = String(src || '').trim();
      if (!normalizedSrc)
          return;
      ctx.imagePreviewUrl.value = normalizedSrc;
      ctx.imagePreviewTitle.value = String(title || '').trim() || ctx.t('chat.imagePreview');
      ctx.imagePreviewWorkspacePath.value = String(workspacePath || '').trim();
      ctx.imagePreviewVisible.value = true;
  };

  ctx.handleImagePreviewDownload = async () => {
      const workspacePath = String(ctx.imagePreviewWorkspacePath.value || '').trim();
      if (workspacePath) {
          await ctx.downloadWorkspaceResource(workspacePath);
      }
      else {
          // External image: download directly from URL
          const url = String(ctx.imagePreviewUrl.value || '').trim();
          if (url) {
              await ctx.downloadExternalImage(url);
          }
      }
  };

  ctx.closeImagePreview = () => {
      ctx.imagePreviewVisible.value = false;
      ctx.imagePreviewUrl.value = '';
      ctx.imagePreviewTitle.value = '';
      ctx.imagePreviewWorkspacePath.value = '';
  };

  ctx.handleMessageContentClick = async (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target)
          return;
      // Handle external image preview (images from external URLs)
      const externalImage = target.closest('img.ai-external-image-preview') as HTMLImageElement | null;
      if (externalImage) {
          const card = externalImage.closest('.ai-external-image-card') as HTMLElement | null;
          const src = String(card?.dataset?.externalImageSrc || externalImage.getAttribute('src') || '').trim();
          if (!src)
              return;
          const title = String(card?.dataset?.externalImageAlt || externalImage.getAttribute('alt') || '').trim();
          ctx.openImagePreview(src, title, '');
          return;
      }
      // Handle workspace resource image preview
      const previewImage = target.closest('img.ai-resource-preview') as HTMLImageElement | null;
      if (previewImage) {
          const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
          if (card?.dataset?.workspaceState !== 'ready')
              return;
          const src = String(previewImage.getAttribute('src') || '').trim();
          if (!src)
              return;
          const title = String(card?.querySelector('.ai-resource-name')?.textContent || '').trim();
          const workspacePath = String(card?.dataset?.workspacePath || '').trim();
          ctx.openImagePreview(src, title, workspacePath);
          return;
      }
      // Handle external image download button
      const externalImageButton = target.closest('[data-external-image-action]') as HTMLElement | null;
      if (externalImageButton) {
          const card = externalImageButton.closest('.ai-external-image-card') as HTMLElement | null;
          const src = String(card?.dataset?.externalImageSrc || '').trim();
          if (!src)
              return;
          event.preventDefault();
          await ctx.downloadExternalImage(src);
          return;
      }
      // Handle workspace resource download button
      const resourceButton = target.closest('[data-workspace-action]') as HTMLElement | null;
      if (resourceButton) {
          const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
          const publicPath = String(container?.dataset?.workspacePath || '').trim();
          if (!publicPath)
              return;
          event.preventDefault();
          await ctx.downloadWorkspaceResource(publicPath);
          return;
      }
      const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
      if (resourceLink) {
          const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
          if (!publicPath)
              return;
          event.preventDefault();
          await ctx.downloadWorkspaceResource(publicPath);
          return;
      }
      const copyButton = target.closest('.ai-code-copy') as HTMLElement | null;
      if (!copyButton)
          return;
      event.preventDefault();
      const codeBlock = copyButton.closest('.ai-code-block');
      const codeText = String(codeBlock?.querySelector('code')?.textContent || '').trim();
      if (!codeText) {
          ElMessage.warning(ctx.t('chat.message.copyEmpty'));
          return;
      }
      const copied = await copyText(codeText);
      if (copied) {
          ElMessage.success(ctx.t('chat.message.copySuccess'));
      }
      else {
          ElMessage.warning(ctx.t('chat.message.copyFailed'));
      }
  };
}
