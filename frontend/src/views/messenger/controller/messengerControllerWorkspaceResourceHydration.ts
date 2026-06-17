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
import { fetchDesktopSettings, resolveDesktopWorkspacePath } from '@/api/desktop';
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
import { isDesktopModeEnabled, isDesktopSafeModeEnabled } from '@/config/desktop';
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
  isMetafileImagePath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import {
  bindWorkspaceImagePreviewState,
  getFilenameFromHeaders,
  hydrateWorkspaceResourceErrorDiagnostics,
  markWorkspaceImageCardError,
  normalizeWorkspaceImageResponseBlob,
  resetWorkspaceImageCardState,
  resolveWorkspaceResourceErrorDiagnostics,
  saveObjectUrlAsFile,
  scheduleWorkspaceLoadingLabel
} from '@/utils/workspaceResourceCards';
import { buildWorkspaceResourceRequestParams } from '@/utils/workspaceResourceRequest';
import {
  WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES,
  decodeWorkspaceResourceLabel,
  extractWorkspaceResourceExtension,
  normalizeWorkspacePreviewFilename,
  normalizeWorkspacePreviewBlob,
  resolveWorkspacePreviewTooLargeHint,
  resolveWorkspacePreviewUnsupportedHint,
  resolveWorkspaceResourcePreviewKind
} from '@/utils/workspaceResourcePreview';
import {
  extractWorkspaceRefreshPaths,
  isWorkspacePathAffected
} from '@/utils/workspaceRefresh';
import { emitWorkspaceRefresh, onAgentRuntimeRefresh, onWorkspaceRefresh } from '@/utils/workspaceEvents';
import { emitUserToolsUpdated, onUserToolsUpdated } from '@/utils/userToolsEvents';
import { chatDebugLog, isChatDebugVerboseEnabled } from '@/utils/chatDebug';
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

type WorkspaceResourceInvalidation = {
  epoch: number;
  paths: string[];
};

const WORKSPACE_RESOURCE_CACHE_BUST_PARAM = '_wunder_resource_version';

const buildWorkspaceResourceCacheKey = (publicPath: string, preview = '', version = 0): string => {
  const previewSuffix = preview ? `#preview=${preview}` : '';
  const versionSuffix = version > 0 ? `#version=${version}` : '';
  return `${publicPath}${previewSuffix}${versionSuffix}`;
};

const extractWorkspaceResourcePublicPathFromCacheKey = (cacheKey: string): string => {
  const text = String(cacheKey || '').trim();
  if (!text)
      return '';
  const previewIndex = text.indexOf('#preview=');
  const versionIndex = text.indexOf('#version=');
  const cutIndex = [previewIndex, versionIndex]
      .filter((index) => index >= 0)
      .reduce((min, index) => Math.min(min, index), text.length);
  return text.slice(0, cutIndex);
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
  let workspaceHydrationTimeout: number | null = null;
  let workspaceResourceCacheEpoch = 0;
  const workspaceResourceInvalidations: WorkspaceResourceInvalidation[] = [];

  const bumpWorkspaceResourceCacheEpoch = () => {
      workspaceResourceCacheEpoch += 1;
      if (!Number.isSafeInteger(workspaceResourceCacheEpoch)) {
          workspaceResourceCacheEpoch = 1;
      }
  };

  const buildCurrentWorkspaceResourceCacheKey = (publicPath: string, preview = ''): string =>
      buildWorkspaceResourceCacheKey(publicPath, preview, workspaceResourceCacheEpoch);

  const rememberWorkspaceResourceInvalidation = (paths: string[]) => {
      workspaceResourceInvalidations.push({
          epoch: workspaceResourceCacheEpoch,
          paths: Array.isArray(paths) ? paths.slice() : []
      });
      while (workspaceResourceInvalidations.length > 64) {
          workspaceResourceInvalidations.shift();
      }
  };

  const isWorkspaceResourceInvalidatedSince = (
      resource: WorkspaceResolvedResource,
      epoch: number
  ): boolean => {
      if (workspaceResourceCacheEpoch <= epoch) {
          return false;
      }
      const relativePath = String(resource?.relativePath || '').trim();
      return workspaceResourceInvalidations.some((entry) => {
          if (!entry || entry.epoch <= epoch) {
              return false;
          }
          return !entry.paths.length || isWorkspacePathAffected(relativePath, entry.paths);
      });
  };

  const buildVersionedWorkspaceResourceRequestParams = (
      resource: WorkspaceResolvedResource,
      extra: Record<string, unknown> = {}
  ) => {
      const scopedExtra = { ...extra };
      if (workspaceResourceCacheEpoch > 0) {
          scopedExtra[WORKSPACE_RESOURCE_CACHE_BUST_PARAM] = String(workspaceResourceCacheEpoch);
      }
      return buildWorkspaceResourceRequestParams(resource, scopedExtra);
  };

  const revokeUncachedWorkspaceObjectUrl = (objectUrl: string) => {
      const url = String(objectUrl || '').trim();
      if (!url.startsWith('blob:')) {
          return;
      }
      const cached = Array.from(ctx.workspaceResourceCache.values()).some((entry) => entry?.objectUrl === url);
      if (!cached) {
          URL.revokeObjectURL(url);
      }
  };

  ctx.resolveDesktopWorkspaceRoot = (): string => String(getRuntimeConfig().workspace_root || '').trim();

  ctx.resolveDesktopContainerRoot = (containerId?: number | null): string => {
      if (containerId !== null && Number.isFinite(Number(containerId))) {
          const mapped = String(ctx.desktopContainerRootMap.value[Number(containerId)] || '').trim();
          if (mapped)
              return mapped;
      }
      return ctx.resolveDesktopWorkspaceRoot();
  };

  ctx.resolveDesktopAbsoluteWorkspacePath = (
      relativePath: string,
      containerId?: number | null
  ): string => {
      const normalized = String(relativePath || '').replace(/\\/g, '/').replace(/^\/+/, '').trim();
      if (!normalized) {
          return '';
      }
      const root = String(ctx.resolveDesktopContainerRoot(containerId) || '').trim().replace(/[\\/]+$/, '');
      if (!root) {
          return normalized.replace(/\//g, '\\');
      }
      const looksLikeContainerScoped = /(?:^|[\\/])desktop_user(?:__c__\d+)?$/i.test(root);
      if (looksLikeContainerScoped) {
          return `${root.replace(/[\\/]+$/, '')}\\${normalized.replace(/\//g, '\\').replace(/^\\+/, '')}`;
      }
      const effectiveContainerId =
          containerId !== null && Number.isFinite(Number(containerId))
              ? Number(containerId)
              : ctx.currentContainerId.value;
      const scope = effectiveContainerId > 0 ? `desktop_user__c__${effectiveContainerId}` : 'desktop_user';
      return `${root}\\${scope}\\${normalized.replace(/\//g, '\\').replace(/^\\+/, '')}`;
  };

  ctx.resolveDesktopAbsoluteWorkspacePathAsync = async (
      relativePath: string,
      containerId?: number | null
  ): Promise<string> => {
      const normalized = String(relativePath || '').trim();
      if (!normalized) {
          return '';
      }
      try {
          const response = await resolveDesktopWorkspacePath(normalized, containerId ?? null);
          const payload = response?.data?.data || {};
          const absolutePath = String(payload.absolute_path || '').trim();
          if (absolutePath) {
              return absolutePath;
          }
      } catch {
          // Fall back to client-side path composition below.
      }
      return ctx.resolveDesktopAbsoluteWorkspacePath(normalized, containerId);
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
      const currentId = normalizeWorkspaceOwnerId(user?.id || user?.user_id || user?.username);
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
      if (user && ctx.isAdminUser(user)) {
          return {
              ...parsed,
              requestUserId: ownerId,
              requestAgentId: agentId || null,
              requestContainerId: containerId,
              allowed: true
          };
      }
      // Public workspace paths can be resolved by the backend from the bearer token even while the profile is loading.
      return {
          ...parsed,
          requestUserId: null,
          requestAgentId: agentId || null,
          requestContainerId: containerId,
          allowed: true
      };
  };

  ctx.fetchWorkspaceResource = async (
      resource: WorkspaceResolvedResource,
      options: { preview?: 'png' } = {}
  ) => {
      const preview = options.preview === 'png' ? 'png' : '';
      const requestEpoch = workspaceResourceCacheEpoch;
      const cacheKey = buildWorkspaceResourceCacheKey(resource.publicPath, preview, requestEpoch);
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
          const extra: Record<string, unknown> = preview ? { preview } : {};
          if (requestEpoch > 0) {
              extra[WORKSPACE_RESOURCE_CACHE_BUST_PARAM] = String(requestEpoch);
          }
          const params = buildWorkspaceResourceRequestParams(resource, extra);
          const response = await downloadWunderWorkspaceFile(params);
          try {
              const fallbackFilename = preview === 'png'
                  ? `${String(resource.filename || 'preview').replace(/\.[^.]+$/, '')}.png`
                  : resource.filename || 'download';
              const filename = getFilenameFromHeaders(response?.headers as Record<string, unknown>, fallbackFilename);
              const contentType = String((response?.headers as Record<string, unknown>)?.['content-type'] ||
                  (response?.headers as Record<string, unknown>)?.['Content-Type'] ||
                  '');
              const sourceBlob = response.data as Blob;
              const normalizedBlob = await normalizeWorkspaceImageResponseBlob(
                  sourceBlob,
                  filename,
                  contentType,
                  response
              );
              const objectUrl = URL.createObjectURL(normalizedBlob);
              const entry: WorkspaceResourceCachePayload = { objectUrl, filename };
              if (isWorkspaceResourceInvalidatedSince(resource, requestEpoch)) {
                  URL.revokeObjectURL(objectUrl);
                  ctx.workspaceResourceCache.delete(cacheKey);
                  return ctx.fetchWorkspaceResource(resource, options);
              }
              const activeCacheKey = buildCurrentWorkspaceResourceCacheKey(resource.publicPath, preview);
              ctx.workspaceResourceCache.set(activeCacheKey, entry);
              if (activeCacheKey !== cacheKey) {
                  ctx.workspaceResourceCache.delete(cacheKey);
              }
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
          const preview = isMetafileImagePath(resource.filename || resource.relativePath || resource.publicPath)
              ? 'png'
              : undefined;
          const entry = await ctx.fetchWorkspaceResource(resource, { preview });
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
      const hydrationEpoch = workspaceResourceCacheEpoch;
      card.dataset.workspaceHydrationEpoch = String(hydrationEpoch);
      const loadingTimerId = scheduleWorkspaceLoadingLabel(card, status, ctx.t('chat.resourceImageLoading'));
      try {
          const entry = await ctx.fetchWorkspaceResource(resource, {
              preview: isMetafileImagePath(resource.filename) ? 'png' : undefined
          });
          if (String(card.dataset.workspaceHydrationEpoch || '') !== String(hydrationEpoch)) {
              revokeUncachedWorkspaceObjectUrl(entry.objectUrl);
              return;
          }
          bindWorkspaceImagePreviewState(card, preview, entry.objectUrl, {
              status,
              loadingTimerId,
              failedLabel: ctx.t('chat.resourceImageFailed'),
              onDecodeError: () => {
                  ['', 'png'].forEach((previewKind) => {
                      const cacheEntry = ctx.workspaceResourceCache.get(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, previewKind));
                      if (cacheEntry?.objectUrl) {
                          URL.revokeObjectURL(cacheEntry.objectUrl);
                      }
                      ctx.workspaceResourceCache.delete(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, previewKind));
                  });
              }
          });
      }
      catch (error) {
          await hydrateWorkspaceResourceErrorDiagnostics(error);
          markWorkspaceImageCardError(card, status, loadingTimerId, ctx.isWorkspaceResourceMissing(error)
              ? ctx.t('chat.resourceMissing')
              : ctx.t('chat.resourceImageFailed'), resolveWorkspaceResourceErrorDiagnostics(error));
      }
  };

  ctx.hydrateWorkspaceResources = () => {
      if (isDesktopSafeModeEnabled()) {
          return;
      }
      const container = ctx.messageListRef.value;
      if (!container)
          return;
      const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
      const messageNodes = container.querySelectorAll('.messenger-message[data-virtual-key]');
      const cards = messageNodes.length
          ? Array.from(messageNodes).flatMap((node) => Array.from(node.querySelectorAll('.ai-resource-card[data-workspace-path]')))
          : Array.from(container.querySelectorAll('.ai-resource-card[data-workspace-path]'));
      cards.forEach((card) => {
          void ctx.hydrateWorkspaceResourceCard(card as HTMLElement);
      });
      if (messageNodes.length) {
          messageNodes.forEach((node) => hydrateExternalMarkdownImages(node));
      }
      else {
          hydrateExternalMarkdownImages(container);
      }
      if (isChatDebugVerboseEnabled()) {
          const durationMs = Number(((typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt).toFixed(1));
          chatDebugLog('messenger.hydration', 'workspace-scan', {
              activeSection: ctx.sessionHub.activeSection,
              activeConversationKey: ctx.sessionHub.activeConversationKey,
              virtualized: Boolean(ctx.shouldVirtualizeMessages?.value),
              messageNodeCount: messageNodes.length,
              resourceCardCount: cards.length,
              durationMs
          });
      }
  };

  ctx.scheduleWorkspaceResourceHydration = (reason = '') => {
      if (isDesktopSafeModeEnabled()) {
          return;
      }
      if (ctx.sessionHub.activeSection !== 'messages') {
          return;
      }
      if (ctx.workspaceResourceHydrationFrame !== null || ctx.workspaceResourceHydrationPending)
          return;
      if (typeof window !== 'undefined' && workspaceHydrationTimeout !== null) {
          window.clearTimeout(workspaceHydrationTimeout);
          workspaceHydrationTimeout = null;
      }
      ctx.workspaceResourceHydrationPending = true;
      void nextTick(() => {
          ctx.workspaceResourceHydrationPending = false;
          if (ctx.workspaceResourceHydrationFrame !== null || typeof window === 'undefined')
              return;
          workspaceHydrationTimeout = window.setTimeout(() => {
              workspaceHydrationTimeout = null;
              if (ctx.sessionHub.activeSection !== 'messages') {
                  return;
              }
              ctx.workspaceResourceHydrationFrame = window.requestAnimationFrame(() => {
                  ctx.workspaceResourceHydrationFrame = null;
                  if (isChatDebugVerboseEnabled()) {
                      chatDebugLog('messenger.hydration', 'workspace-run', {
                          reason,
                          activeSection: ctx.sessionHub.activeSection,
                          activeConversationKey: ctx.sessionHub.activeConversationKey
                      });
                  }
                  ctx.hydrateWorkspaceResources();
              });
          }, 90);
      });
  };

  ctx.resetWorkspaceResourceCards = (changedPaths: string[] = []) => {
      const container = ctx.messageListRef.value;
      if (!container)
          return 0;
      let resetCount = 0;
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
          if (resetWorkspaceImageCardState(element, { clearSrc: true, includeReady: true })) {
              delete element.dataset.workspaceHydrationEpoch;
              resetCount += 1;
          }
      });
      return resetCount;
  };

  ctx.clearWorkspaceResourceCache = () => {
      ctx.resetWorkspaceResourceCards();
      if (typeof window !== 'undefined' && workspaceHydrationTimeout !== null) {
          window.clearTimeout(workspaceHydrationTimeout);
          workspaceHydrationTimeout = null;
      }
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
      return 0;
  };

  ctx.clearWorkspaceResourceCacheByPaths = (changedPaths: string[] = []) => {
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          return 0;
      }
      if (typeof window !== 'undefined' && workspaceHydrationTimeout !== null) {
          window.clearTimeout(workspaceHydrationTimeout);
          workspaceHydrationTimeout = null;
      }
      if (ctx.workspaceResourceHydrationFrame !== null && typeof window !== 'undefined') {
          window.cancelAnimationFrame(ctx.workspaceResourceHydrationFrame);
          ctx.workspaceResourceHydrationFrame = null;
      }
      ctx.workspaceResourceHydrationPending = false;
      let clearedCount = 0;
      Array.from(ctx.workspaceResourceCache.entries()).forEach(([cacheKey, entry]) => {
          const parsed = parseWorkspaceResourceUrl(extractWorkspaceResourcePublicPathFromCacheKey(cacheKey));
          if (!isWorkspacePathAffected(parsed?.relativePath || '', changedPaths)) {
              return;
          }
          if (entry?.objectUrl) {
              URL.revokeObjectURL(entry.objectUrl);
          }
          ctx.workspaceResourceCache.delete(cacheKey);
          clearedCount += 1;
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
              clearedCount += 1;
          });
          ctx.userAttachmentResourceCache.value = next;
      }
      return clearedCount;
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
      bumpWorkspaceResourceCacheEpoch();
      const changedPaths = extractWorkspaceRefreshPaths(detail);
      rememberWorkspaceResourceInvalidation(changedPaths);
      if (!changedPaths.length) {
          ctx.clearWorkspaceResourceCache();
          ctx.resetWorkspaceResourceCards();
          ctx.scheduleWorkspaceResourceHydration('workspace-refresh');
          return;
      }
      const clearedCount = ctx.clearWorkspaceResourceCacheByPaths(changedPaths);
      const resetCount = ctx.resetWorkspaceResourceCards(changedPaths);
      if (clearedCount === 0 && resetCount === 0) {
          rememberWorkspaceResourceInvalidation([]);
          ctx.clearWorkspaceResourceCache();
      }
      ctx.scheduleWorkspaceResourceHydration('workspace-refresh');
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

  ctx.openWorkspaceResourceWithDefaultApp = async (resourcePath: string) => {
      const normalized = String(resourcePath || '').trim();
      if (!normalized || !ctx.desktopMode.value) {
          return false;
      }
      const bridge = ctx.getDesktopBridge();
      if (!bridge || typeof bridge.openPathWithDefaultApp !== 'function') {
          return false;
      }
      const resolved = ctx.resolveWorkspaceResource(normalized);
      const localPath = await ctx.resolveDesktopAbsoluteWorkspacePathAsync(
          String(resolved?.relativePath || normalized).trim(),
          resolved?.requestContainerId ?? null
      );
      if (!localPath) {
          return false;
      }
      try {
          return Boolean(await bridge.openPathWithDefaultApp(localPath));
      }
      catch {
          return false;
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

  ctx.openResourcePreview = async (options: {
      src?: string;
      title?: string;
      workspacePath?: string;
      userId?: string;
      content?: string;
      hint?: string;
      meta?: string;
      kind?: string;
  } = {}) => {
      const workspacePath = String(options.workspacePath || '').trim();
      const title = String(options.title || '').trim() || ctx.t('workspace.preview.dialogTitle');
      const meta = String(options.meta || workspacePath || title).trim();
      const userId = String(options.userId || '').trim();
      const initialKind = String(options.kind || '').trim();
      const fileName = normalizeWorkspacePreviewFilename(title, workspacePath.split('/').pop() || '');
      const previewKind = initialKind || resolveWorkspaceResourcePreviewKind(fileName);
      ctx.resourcePreviewVisible.value = true;
      ctx.resourcePreviewLoading.value = false;
      ctx.resourcePreviewTitle.value = title;
      ctx.resourcePreviewMeta.value = meta;
      ctx.resourcePreviewHint.value = String(options.hint || '').trim();
      ctx.resourcePreviewContent.value = String(options.content || '').trim();
      ctx.resourcePreviewWorkspacePath.value = workspacePath;
      ctx.resourcePreviewUrl.value = String(options.src || '').trim();
      ctx.resourcePreviewKind.value = previewKind || 'image';
      ctx.resourcePreviewUserId.value = userId;
      if (!workspacePath) {
          return;
      }
      if (previewKind === 'drawio') {
          const resource = ctx.resolveWorkspaceResource(workspacePath);
          const relativePath = String(resource?.relativePath || workspacePath).trim();
          ctx.resourcePreviewVisible.value = false;
          ctx.drawioVisible.value = true;
          ctx.drawioPath.value = relativePath;
          ctx.drawioUserId.value = String(resource?.requestUserId || userId).trim();
          ctx.drawioAgentId.value = String(resource?.requestAgentId || '').trim();
          ctx.drawioContainerId.value =
              resource?.requestContainerId !== null && Number.isFinite(resource?.requestContainerId)
                  ? resource.requestContainerId
                  : null;
          return;
      }
      if (previewKind === 'onlyoffice') {
          const resource = ctx.resolveWorkspaceResource(workspacePath);
          const relativePath = String(resource?.relativePath || workspacePath).trim();
          if (ctx.desktopMode.value) {
              const opened = await ctx.openWorkspaceResourceWithDefaultApp(workspacePath);
              if (opened) {
                  return;
              }
          }
          ctx.resourcePreviewVisible.value = false;
          ctx.onlyOfficeVisible.value = true;
          ctx.onlyOfficePath.value = relativePath;
          ctx.onlyOfficeUserId.value = String(resource?.requestUserId || userId).trim();
          ctx.onlyOfficeAgentId.value = String(resource?.requestAgentId || '').trim();
          ctx.onlyOfficeContainerId.value =
              resource?.requestContainerId !== null && Number.isFinite(resource?.requestContainerId)
                  ? resource.requestContainerId
                  : null;
          return;
      }
      const resource = ctx.resolveWorkspaceResource(workspacePath);
      if (!resource || !resource.allowed) {
          ctx.resourcePreviewHint.value = ctx.t('chat.resourceUnavailable');
          ctx.resourcePreviewKind.value = 'unsupported';
          return;
      }
      if (previewKind === 'unsupported') {
          ctx.resourcePreviewHint.value = resolveWorkspacePreviewUnsupportedHint();
          return;
      }
      ctx.resourcePreviewLoading.value = true;
      try {
          if (previewKind === 'text') {
              const response = await fetchWunderWorkspaceContent(buildVersionedWorkspaceResourceRequestParams(resource, {
                  include_content: true,
                  max_bytes: WORKSPACE_RESOURCE_PREVIEW_TEXT_MAX_BYTES
              }));
              const payload = response.data || {};
              if (payload.truncated) {
                  ctx.resourcePreviewHint.value = ctx.t('workspace.preview.truncatedHint');
              }
              ctx.resourcePreviewContent.value = typeof payload.content === 'string'
                  ? payload.content || ctx.t('workspace.preview.emptyContent')
                  : ctx.t('workspace.preview.emptyContent');
              return;
          }
          const preview = isMetafileImagePath(resource.filename || resource.relativePath || resource.publicPath)
              ? 'png'
              : undefined;
          const entry = await ctx.fetchWorkspaceResource(resource, { preview });
          const extension = extractWorkspaceResourceExtension(fileName);
          const cacheEntry = ctx.workspaceResourceCache.get(buildCurrentWorkspaceResourceCacheKey(resource.publicPath, preview || ''));
          if (cacheEntry?.objectUrl && previewKind !== 'pdf') {
              ctx.resourcePreviewUrl.value = cacheEntry.objectUrl;
              return;
          }
          const response = await downloadWunderWorkspaceFile(buildVersionedWorkspaceResourceRequestParams(resource));
          const blob = normalizeWorkspacePreviewBlob(response.data as Blob, previewKind as never, extension);
          ctx.resourcePreviewUrl.value = URL.createObjectURL(blob);
          ctx.resourcePreviewContent.value = '';
          ctx.resourcePreviewHint.value = '';
          void entry;
      }
      catch (error) {
          const missing = ctx.isWorkspaceResourceMissing(error);
          ctx.resourcePreviewHint.value = missing
              ? ctx.t('chat.resourceMissing')
              : previewKind === 'text'
                  ? ctx.t('workspace.preview.loadFailedHint')
                  : resolveWorkspacePreviewTooLargeHint();
          if (previewKind === 'text') {
              ctx.resourcePreviewContent.value = ctx.t('workspace.preview.empty');
          }
      }
      finally {
          ctx.resourcePreviewLoading.value = false;
      }
  };

  ctx.handleResourcePreviewDownload = async () => {
      const workspacePath = String(ctx.resourcePreviewWorkspacePath.value || '').trim();
      if (workspacePath) {
          await ctx.downloadWorkspaceResource(workspacePath);
          return;
      }
      const url = String(ctx.resourcePreviewUrl.value || '').trim();
      if (url) {
          await ctx.downloadExternalImage(url);
      }
  };

  ctx.closeResourcePreview = () => {
      const currentUrl = String(ctx.resourcePreviewUrl.value || '').trim();
      const currentWorkspacePath = String(ctx.resourcePreviewWorkspacePath.value || '').trim();
      const isCachedObjectUrl =
          currentUrl.startsWith('blob:') &&
          Array.from(ctx.workspaceResourceCache.values()).some((entry) => entry?.objectUrl === currentUrl);
      if (
          currentUrl &&
          currentWorkspacePath &&
          currentUrl.startsWith('blob:') &&
          !isCachedObjectUrl
      ) {
          URL.revokeObjectURL(currentUrl);
      }
      ctx.resourcePreviewVisible.value = false;
      ctx.resourcePreviewLoading.value = false;
      ctx.resourcePreviewUrl.value = '';
      ctx.resourcePreviewTitle.value = '';
      ctx.resourcePreviewMeta.value = '';
      ctx.resourcePreviewHint.value = '';
      ctx.resourcePreviewContent.value = '';
      ctx.resourcePreviewWorkspacePath.value = '';
      ctx.resourcePreviewKind.value = 'image';
      ctx.resourcePreviewUserId.value = '';
  };

  ctx.handleWorkspaceEditorSaved = async (payload: { path?: string } = {}) => {
      const changedPath = String(payload.path || ctx.onlyOfficePath.value || ctx.drawioPath.value || '').trim();
      if (!changedPath)
          return;
      const editorAgentId = ctx.onlyOfficeVisible.value
          ? ctx.onlyOfficeAgentId.value
          : ctx.drawioVisible.value
              ? ctx.drawioAgentId.value
              : ctx.activeAgentId.value;
      const explicitEditorContainerId = ctx.onlyOfficeVisible.value
          ? ctx.onlyOfficeContainerId.value
          : ctx.drawioVisible.value
              ? ctx.drawioContainerId.value
              : null;
      const editorContainerId =
          explicitEditorContainerId !== null && Number.isFinite(explicitEditorContainerId)
              ? explicitEditorContainerId
              : ctx.currentContainerId.value;
      emitWorkspaceRefresh({
          path: changedPath,
          changed_paths: [changedPath],
          agent_id: editorAgentId || '',
          container_id: editorContainerId
      });
      if (ctx.resourcePreviewVisible.value) {
          await ctx.openResourcePreview({
              title: ctx.resourcePreviewTitle.value,
              workspacePath: ctx.resourcePreviewWorkspacePath.value,
              userId: ctx.resourcePreviewUserId.value,
              meta: ctx.resourcePreviewMeta.value,
              kind: ctx.resourcePreviewKind.value
          });
      }
  };

  ctx.handleWorkspaceEditorFallback = async (payload: { path?: string; message?: string } = {}) => {
      const path = String(payload.path || '').trim() || ctx.onlyOfficePath.value || ctx.drawioPath.value;
      const fallbackContainerId =
          ctx.onlyOfficeVisible.value
              ? ctx.onlyOfficeContainerId.value
              : ctx.drawioVisible.value
                  ? ctx.drawioContainerId.value
                  : null;
      ctx.onlyOfficeVisible.value = false;
      ctx.drawioVisible.value = false;
      ctx.onlyOfficePath.value = '';
      ctx.drawioPath.value = '';
      ctx.onlyOfficeUserId.value = '';
      ctx.drawioUserId.value = '';
      ctx.onlyOfficeAgentId.value = '';
      ctx.drawioAgentId.value = '';
      ctx.onlyOfficeContainerId.value = null;
      ctx.drawioContainerId.value = null;
      if (!path)
          return;
      if (ctx.desktopMode.value) {
          const bridge = ctx.getDesktopBridge();
          if (bridge && typeof bridge.openPathWithDefaultApp === 'function') {
              try {
                  const localPath = await ctx.resolveDesktopAbsoluteWorkspacePathAsync(
                      path,
                      fallbackContainerId
                  );
                  await bridge.openPathWithDefaultApp(localPath || ctx.resolveDesktopAbsoluteWorkspacePath(path, fallbackContainerId) || path);
                  return;
              }
              catch {
                  // Fall back to in-app preview if the local app cannot open the file.
              }
          }
      }
      await ctx.openResourcePreview({
          title: ctx.resourcePreviewTitle.value || path.split('/').pop() || '',
          workspacePath: path,
          userId: ctx.resourcePreviewUserId.value,
          meta: path,
          hint: String(payload.message || '').trim(),
          kind: resolveWorkspaceResourcePreviewKind(path)
      });
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
          await ctx.openResourcePreview({
              src,
              title,
              meta: title,
              kind: 'image'
          });
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
          await ctx.openResourcePreview({
              src,
              title,
              workspacePath,
              meta: workspacePath,
              kind: 'image'
          });
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
          const action = String(resourceButton.dataset?.workspaceAction || container?.dataset?.workspaceAction || '').trim().toLowerCase();
          if (action === 'download') {
              await ctx.downloadWorkspaceResource(publicPath);
              return;
          }
          const title = String(container?.querySelector('.ai-resource-name')?.textContent || '').trim();
          await ctx.openResourcePreview({
              title: decodeWorkspaceResourceLabel(title),
              workspacePath: publicPath,
              meta: publicPath,
              kind: resolveWorkspaceResourcePreviewKind(publicPath || title)
          });
          return;
      }
      const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
      if (resourceLink) {
          const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
          if (!publicPath)
              return;
          event.preventDefault();
          const title = String(resourceLink.textContent || '').trim();
          await ctx.openResourcePreview({
              title: decodeWorkspaceResourceLabel(title),
              workspacePath: publicPath,
              meta: publicPath,
              kind: resolveWorkspaceResourcePreviewKind(publicPath || title)
          });
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
