// @ts-nocheck
// World upload paths, helper apps, screenshots, desktop model metadata, voice recording, and world sending.
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

export function installMessengerControllerWorldMessagingActions(ctx: MessengerControllerContext): void {
  ctx.normalizeUploadPath = (value: unknown): string => String(value || '')
      .replace(/\\/g, '/')
      .replace(/^\/+/, '')
      .trim();

  ctx.buildWorldAttachmentToken = (rawPath: unknown): string => {
      const normalized = ctx.normalizeUploadPath(rawPath);
      if (!normalized)
          return '';
      if (/\s/.test(normalized)) {
          if (!normalized.includes('"')) {
              return `@"${normalized}"`;
          }
          if (!normalized.includes("'")) {
              return `@'${normalized}'`;
          }
          return `@${encodeURIComponent(normalized)}`;
      }
      return `@${normalized}`;
  };

  ctx.appendWorldAttachmentTokens = (paths: string[]) => {
      const tokens = paths.map((path) => ctx.buildWorldAttachmentToken(path)).filter(Boolean);
      if (!tokens.length)
          return;
      const prefix = ctx.worldDraft.value.trim() ? '\n' : '';
      ctx.worldDraft.value = `${ctx.worldDraft.value}${prefix}${tokens.join(' ')}`;
  };

  ctx.normalizeHexColor = (value: unknown) => {
      const cleaned = String(value || '').trim();
      if (!cleaned)
          return '';
      const matched = cleaned.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
      if (!matched)
          return '';
      let hex = matched[1].toLowerCase();
      if (hex.length === 3) {
          hex = hex
              .split('')
              .map((part) => part + part)
              .join('');
      }
      return `#${hex}`;
  };

  ctx.resolveExternalIconConfig = (icon: unknown) => {
      const raw = String(icon || '').trim();
      if (!raw) {
          return { name: 'fa-globe', color: '' };
      }
      try {
          const parsed = JSON.parse(raw);
          if (parsed && typeof parsed === 'object') {
              const name = String((parsed as Record<string, unknown>)?.name || '').trim();
              const match = name.split(/\s+/).find((part) => part.startsWith('fa-'));
              return {
                  name: match || 'fa-globe',
                  color: ctx.normalizeHexColor((parsed as Record<string, unknown>)?.color)
              };
          }
      }
      catch {
      }
      const match = raw.split(/\s+/).find((part) => part.startsWith('fa-'));
      return {
          name: match || 'fa-globe',
          color: ''
      };
  };

  ctx.normalizeExternalLink = (item: Record<string, unknown>): HelperAppExternalItem => ({
      linkId: String(item?.link_id || '').trim(),
      title: String(item?.title || '').trim(),
      description: String(item?.description || '').trim(),
      url: String(item?.url || '').trim(),
      icon: String(item?.icon || '').trim(),
      sortOrder: Number.isFinite(Number(item?.sort_order)) ? Number(item.sort_order) : 0
  });

  ctx.resolveExternalIcon = (icon: unknown) => ctx.resolveExternalIconConfig(icon).name;

  ctx.resolveExternalIconStyle = (icon: unknown) => {
      const color = ctx.resolveExternalIconConfig(icon).color;
      return color ? { color } : {};
  };

  ctx.resolveExternalHost = (url: unknown) => {
      const value = String(url || '').trim();
      if (!value)
          return '-';
      try {
          const parsed = new URL(value);
          return parsed.host || value;
      }
      catch {
          return value;
      }
  };

  ctx.helperAppsOfflineItems = computed<HelperAppOfflineItem[]>(() => {
      const items: HelperAppOfflineItem[] = [
          {
              key: 'local-file-search',
              title: ctx.t('userWorld.helperApps.localFileSearch.cardTitle'),
              description: ctx.t('userWorld.helperApps.localFileSearch.cardDesc'),
              icon: 'fa-folder-tree'
          }
      ];
      if (!ctx.desktopMode.value) {
          items.push({
              key: 'globe',
              title: ctx.t('userWorld.helperApps.globe.cardTitle'),
              description: ctx.t('userWorld.helperApps.globe.cardDesc'),
              icon: 'fa-globe'
          });
      }
      return items;
  });

  ctx.helperAppsActiveOfflineItem = computed(() => {
      if (ctx.helperAppsActiveKind.value !== 'offline')
          return null;
      return ctx.helperAppsOfflineItems.value.find((item) => item.key === ctx.helperAppsActiveKey.value) || null;
  });

  ctx.helperAppsActiveExternalItem = computed(() => {
      if (ctx.helperAppsActiveKind.value !== 'online')
          return null;
      return ctx.helperAppsOnlineItems.value.find((item) => item.linkId === ctx.helperAppsActiveKey.value) || null;
  });

  ctx.helperAppsActiveTitle = computed(() => {
      if (ctx.helperAppsActiveKind.value === 'offline') {
          return ctx.helperAppsActiveOfflineItem.value?.title || '';
      }
      if (ctx.helperAppsActiveKind.value === 'online') {
          return ctx.helperAppsActiveExternalItem.value?.title || '';
      }
      return '';
  });

  ctx.helperAppsActiveDescription = computed(() => {
      if (ctx.helperAppsActiveKind.value === 'offline') {
          return ctx.helperAppsActiveOfflineItem.value?.description || '';
      }
      if (ctx.helperAppsActiveKind.value === 'online') {
          const item = ctx.helperAppsActiveExternalItem.value;
          if (!item)
              return '';
          return item.description || ctx.resolveExternalHost(item.url);
      }
      return '';
  });

  ctx.isHelperAppActive = (kind: 'offline' | 'online', key: string) => ctx.helperAppsActiveKind.value === kind && ctx.helperAppsActiveKey.value === key;

  ctx.selectHelperApp = (kind: 'offline' | 'online', key: string) => {
      ctx.helperAppsActiveKind.value = kind;
      ctx.helperAppsActiveKey.value = key;
      if (kind === 'online') {
          ctx.loadHelperExternalApps();
      }
  };

  ctx.ensureHelperAppsSelection = () => {
      if (ctx.helperAppsActiveKind.value === 'offline' &&
          ctx.helperAppsOfflineItems.value.some((item) => item.key === ctx.helperAppsActiveKey.value)) {
          return;
      }
      if (ctx.helperAppsActiveKind.value === 'online' &&
          ctx.helperAppsOnlineItems.value.some((item) => item.linkId === ctx.helperAppsActiveKey.value)) {
          return;
      }
      const fallback = ctx.helperAppsOfflineItems.value[0];
      if (fallback) {
          ctx.helperAppsActiveKind.value = 'offline';
          ctx.helperAppsActiveKey.value = fallback.key;
      }
  };

  ctx.loadHelperExternalApps = async () => {
      if (ctx.helperAppsOnlineLoading.value || ctx.helperAppsOnlineLoaded.value)
          return;
      ctx.helperAppsOnlineLoading.value = true;
      try {
          const { data } = await fetchExternalLinks();
          const items = Array.isArray(data?.data?.items) ? data.data.items : [];
          ctx.helperAppsOnlineItems.value = items
              .map((item) => ctx.normalizeExternalLink(item as Record<string, unknown>))
              .filter((item) => item.linkId && item.title && item.url)
              .sort((left, right) => left.sortOrder - right.sortOrder);
          if (ctx.helperAppsActiveKind.value === 'online') {
              const activeKey = ctx.helperAppsActiveKey.value;
              const hasActive = Boolean(activeKey) && ctx.helperAppsOnlineItems.value.some((item) => item.linkId === activeKey);
              if (!hasActive) {
                  ctx.helperAppsActiveKey.value = ctx.helperAppsOnlineItems.value[0]?.linkId || '';
              }
          }
      }
      catch {
          ctx.helperAppsOnlineItems.value = [];
      }
      finally {
          ctx.helperAppsOnlineLoading.value = false;
          ctx.helperAppsOnlineLoaded.value = true;
      }
  };

  ctx.openHelperAppsDialog = () => {
      ctx.clearMiddlePaneOverlayHide();
      ctx.middlePaneOverlayVisible.value = true;
      ctx.helperAppsWorkspaceMode.value = true;
      ctx.ensureHelperAppsSelection();
      ctx.loadHelperExternalApps();
      ctx.switchSection('groups', { preserveHelperWorkspace: true, helperWorkspace: true });
      ctx.selectedGroupId.value = '';
  };

  ctx.closeWorldAttachmentPanels = () => {
      ctx.worldQuickPanelMode.value = '';
      ctx.worldContainerPickerVisible.value = false;
  };

  ctx.findWorldOversizedFile = (files: File[]): File | undefined => files.find((file) => Number(file.size || 0) > WORLD_UPLOAD_SIZE_LIMIT);

  ctx.resolveUploadedWorldPath = (value: unknown): string => {
      if (typeof value === 'string') {
          return ctx.normalizeUploadPath(value);
      }
      if (value && typeof value === 'object') {
          const record = value as Record<string, unknown>;
          return ctx.normalizeUploadPath(record.path ?? record.relative_path ?? record.relativePath ?? '');
      }
      return '';
  };

  ctx.uploadWorldFilesToUserContainer = async (files: File[], options: {
      appendTokens?: boolean;
  } = {}): Promise<string[]> => {
      if (!files.length)
          return [];
      const formData = new FormData();
      formData.append('path', USER_WORLD_UPLOAD_BASE);
      formData.append('container_id', String(USER_CONTAINER_ID));
      files.forEach((file) => {
          formData.append('files', file as Blob);
      });
      const { data } = await uploadWunderWorkspace(formData);
      const uploaded = (Array.isArray(data?.files) ? data.files : [])
          .map((item) => ctx.resolveUploadedWorldPath(item))
          .filter(Boolean);
      if (uploaded.length && options.appendTokens !== false) {
          ctx.appendWorldAttachmentTokens(uploaded);
          emitWorkspaceRefresh({
              reason: 'messenger-world-upload',
              containerId: USER_CONTAINER_ID
          });
      }
      return uploaded;
  };

  ctx.screenshotDataUrlToFile = (dataUrl: string, fileName: string, mimeTypeHint = ''): File => {
      const normalizedDataUrl = String(dataUrl || '').trim();
      const commaIndex = normalizedDataUrl.indexOf(',');
      if (!normalizedDataUrl.startsWith('data:image/') || commaIndex <= 0) {
          throw new Error(ctx.t('chat.attachments.screenshotFailed'));
      }
      const metadata = normalizedDataUrl.slice(5, commaIndex);
      const payload = normalizedDataUrl.slice(commaIndex + 1);
      if (!/;base64$/i.test(metadata)) {
          throw new Error(ctx.t('chat.attachments.screenshotFailed'));
      }
      const binary = atob(payload);
      const bytes = new Uint8Array(binary.length);
      for (let index = 0; index < binary.length; index += 1) {
          bytes[index] = binary.charCodeAt(index);
      }
      const mimeType = String(mimeTypeHint || metadata.split(';')[0] || 'image/png').trim() || 'image/png';
      return new File([bytes], fileName, { type: mimeType });
  };

  ctx.appendScreenshotFileNameSuffix = (fileName: string, suffix: string): string => {
      const normalized = String(fileName || '').trim();
      if (!normalized)
          return `screenshot${suffix}.png`;
      const dotIndex = normalized.lastIndexOf('.');
      if (dotIndex <= 0)
          return `${normalized}${suffix}`;
      return `${normalized.slice(0, dotIndex)}${suffix}${normalized.slice(dotIndex)}`;
  };

  ctx.captureWorldScreenshotData = async (option: WorldScreenshotCaptureOption): Promise<{
      dataUrl: string;
      fileName: string;
      mimeType: string;
  }> => {
      const bridge = ctx.getDesktopBridge();
      if (!bridge || typeof bridge.captureScreenshot !== 'function') {
          throw new Error(ctx.t('chat.attachments.screenshotUnavailable'));
      }
      const result = (await bridge.captureScreenshot({
          hideWindow: option.hideWindow === true,
          region: option.region === true
      })) as DesktopScreenshotResult | null;
      if (result?.canceled) {
          throw new Error('__SCREENSHOT_CANCELED__');
      }
      if (!result || result.ok === false) {
          const reason = String(result?.message || ctx.t('chat.attachments.screenshotFailed')).trim();
          throw new Error(reason || ctx.t('chat.attachments.screenshotFailed'));
      }
      const fileName = String(result.name || '').trim() || `screenshot-${Date.now()}.png`;
      const mimeType = String(result.mimeType || '').trim() || 'image/png';
      const dataUrl = String(result.dataUrl || '').trim();
      if (!dataUrl.startsWith('data:image/')) {
          throw new Error(ctx.t('chat.attachments.screenshotFailed'));
      }
      return { dataUrl, fileName, mimeType };
  };

  ctx.resolveDesktopDefaultModelMeta = (settings: unknown): {
      hearingSupported: boolean;
      modelDisplayName: string;
      maxContext: number | null;
  } => {
      const normalizePositiveInteger = (value: unknown): number | null => {
          const parsed = Number(value);
          return Number.isFinite(parsed) && parsed > 0 ? Math.round(parsed) : null;
      };
      const root = ctx.asObjectRecord(settings);
      const llm = ctx.asObjectRecord(root.llm);
      const defaultModelKey = String(llm.default || '').trim();
      const models = ctx.asObjectRecord(llm.models);
      const currentModel = ctx.asObjectRecord(defaultModelKey ? models[defaultModelKey] : null);
      const configuredModelName = String(currentModel.model || currentModel.model_name || currentModel.name || '').trim();
      const maxContext = normalizePositiveInteger(
          currentModel.max_context ??
              currentModel.maxContext ??
              currentModel.context_max_tokens ??
              currentModel.contextMaxTokens ??
              currentModel.context_total_tokens ??
              currentModel.contextTotalTokens ??
              currentModel.context_window ??
              currentModel.contextWindow
      );
      const supportHearing = currentModel.support_hearing;
      return {
          hearingSupported: supportHearing === false ? false : true,
          modelDisplayName: configuredModelName || defaultModelKey,
          maxContext
      };
  };

  ctx.readDesktopDefaultModelMeta = async (force = false): Promise<{
      hearingSupported: boolean;
      modelDisplayName: string;
      maxContext: number | null;
  }> => {
      if (!ctx.desktopMode.value) {
          ctx.agentVoiceModelHearingSupported.value = true;
          ctx.desktopDefaultModelDisplayName.value = '';
          ctx.desktopDefaultModelMaxContext.value = null;
          return { hearingSupported: true, modelDisplayName: '', maxContext: null };
      }
      const now = Date.now();
      if (!force &&
          ctx.agentVoiceModelHearingSupported.value !== null &&
          now - ctx.agentVoiceModelSupportCheckedAt <= ctx.AGENT_VOICE_MODEL_SUPPORT_CACHE_MS) {
          return {
              hearingSupported: ctx.agentVoiceModelHearingSupported.value,
              modelDisplayName: String(ctx.desktopDefaultModelDisplayName.value || '').trim(),
              maxContext: ctx.desktopDefaultModelMaxContext.value
          };
      }
      if (ctx.desktopDefaultModelMetaFetchPromise) {
          return ctx.desktopDefaultModelMetaFetchPromise;
      }
      ctx.desktopDefaultModelMetaFetchPromise = (async () => {
          try {
              const response = await fetchDesktopSettings();
              const settings = (response?.data?.data || {}) as Record<string, unknown>;
              const meta = ctx.resolveDesktopDefaultModelMeta(settings);
              ctx.agentVoiceModelHearingSupported.value = meta.hearingSupported;
              ctx.desktopDefaultModelDisplayName.value = meta.modelDisplayName;
              ctx.desktopDefaultModelMaxContext.value = meta.maxContext;
              return meta;
          }
          catch {
              ctx.agentVoiceModelHearingSupported.value = null;
              ctx.desktopDefaultModelDisplayName.value = '';
              ctx.desktopDefaultModelMaxContext.value = null;
              return { hearingSupported: true, modelDisplayName: '', maxContext: null };
          }
          finally {
              ctx.agentVoiceModelSupportCheckedAt = Date.now();
              ctx.desktopDefaultModelMetaFetchPromise = null;
          }
      })();
      return ctx.desktopDefaultModelMetaFetchPromise;
  };

  ctx.readAgentVoiceModelSupport = async (force = false): Promise<boolean> => {
      const meta = await ctx.readDesktopDefaultModelMeta(force);
      return meta.hearingSupported;
  };

  ctx.WORLD_VOICE_RECORDING_TICK_MS = 120;

  ctx.clearAgentVoiceRecordingTimer = (runtime: AgentVoiceRecordingRuntime | null) => {
      if (!runtime)
          return;
      if (runtime.timerId !== null && typeof window !== 'undefined') {
          window.clearInterval(runtime.timerId);
      }
      runtime.timerId = null;
  };

  ctx.resetAgentVoiceRecordingState = () => {
      ctx.agentVoiceRecording.value = false;
      ctx.agentVoiceDurationMs.value = 0;
  };

  ctx.cancelAgentVoiceRecording = async () => {
      const runtime = ctx.agentVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.agentVoiceRecordingRuntime = null;
      ctx.clearAgentVoiceRecordingTimer(runtime);
      ctx.resetAgentVoiceRecordingState();
      await runtime.session.cancel().catch(() => undefined);
  };

  ctx.startAgentVoiceRecording = async () => {
      if (!ctx.isAgentConversationActive.value || ctx.agentSessionLoading.value)
          return;
      ctx.refreshAudioRecordingSupport();
      if (ctx.agentVoiceRecordingRuntime)
          return;
      const draftIdentity = ctx.resolveAgentDraftIdentity();
      if (!draftIdentity)
          return;
      try {
          const session = await startAudioRecording();
          const runtime: AgentVoiceRecordingRuntime = {
              session,
              startedAt: Date.now(),
              timerId: null,
              draftIdentity
          };
          ctx.agentVoiceRecordingRuntime = runtime;
          ctx.agentVoiceRecording.value = true;
          ctx.agentVoiceDurationMs.value = 0;
          if (typeof window !== 'undefined') {
              runtime.timerId = window.setInterval(() => {
                  ctx.agentVoiceDurationMs.value = Math.max(0, Date.now() - runtime.startedAt);
              }, ctx.WORLD_VOICE_RECORDING_TICK_MS);
          }
      }
      catch (error) {
          ctx.resetAgentVoiceRecordingState();
          const message = ctx.resolveVoiceRecordingErrorText(error);
          if (message) {
              ElMessage.warning(message);
              return;
          }
          showApiError(error, ctx.t('messenger.world.voice.startFailed'));
      }
  };

  ctx.buildAgentVoiceFileName = (): string => `agent-voice-${Date.now()}.wav`;

  ctx.stopAgentVoiceRecordingAndSend = async () => {
      const runtime = ctx.agentVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.agentVoiceRecordingRuntime = null;
      ctx.clearAgentVoiceRecordingTimer(runtime);
      ctx.resetAgentVoiceRecordingState();
      let recording: AudioRecordingResult;
      try {
          recording = await runtime.session.stop();
      }
      catch (error) {
          showApiError(error, ctx.t('messenger.world.voice.stopFailed'));
          return;
      }
      if (!(recording?.blob instanceof Blob) || !recording.blob.size) {
          ElMessage.warning(ctx.t('messenger.world.voice.empty'));
          return;
      }
      if (runtime.draftIdentity !== ctx.resolveAgentDraftIdentity()) {
          return;
      }
      try {
          const voiceFile = new File([recording.blob], ctx.buildAgentVoiceFileName(), { type: 'audio/wav' });
          const uploadedPaths = await ctx.uploadWorldFilesToUserContainer([voiceFile], { appendTokens: false });
          const uploadedPath = String(uploadedPaths[0] || '').trim();
          if (!uploadedPath) {
              throw new Error(ctx.t('workspace.upload.failed'));
          }
          const attachmentToken = ctx.buildWorldAttachmentToken(uploadedPath);
          await ctx.sendAgentMessage({
              content: attachmentToken || uploadedPath,
              attachments: [
                  {
                      type: 'file',
                      name: voiceFile.name,
                      content: uploadedPath,
                      mime_type: 'audio/wav'
                  }
              ]
          });
      }
      catch (error) {
          showApiError(error, ctx.t('chat.error.requestFailed'));
      }
  };

  ctx.toggleAgentVoiceRecord = async () => {
      if (ctx.agentVoiceRecordingRuntime) {
          await ctx.stopAgentVoiceRecordingAndSend();
          return;
      }
      await ctx.startAgentVoiceRecording();
  };

  ctx.clearWorldVoiceRecordingTimer = (runtime: WorldVoiceRecordingRuntime | null) => {
      if (!runtime)
          return;
      if (runtime.timerId !== null && typeof window !== 'undefined') {
          window.clearInterval(runtime.timerId);
      }
      runtime.timerId = null;
  };

  ctx.resetWorldVoiceRecordingState = () => {
      ctx.worldVoiceRecording.value = false;
      ctx.worldVoiceDurationMs.value = 0;
  };

  ctx.cancelWorldVoiceRecording = async () => {
      const runtime = ctx.worldVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.worldVoiceRecordingRuntime = null;
      ctx.clearWorldVoiceRecordingTimer(runtime);
      ctx.resetWorldVoiceRecordingState();
      await runtime.session.cancel().catch(() => undefined);
  };

  ctx.startWorldVoiceRecording = async () => {
      if (!ctx.isWorldConversationActive.value || ctx.worldUploading.value || ctx.userWorldStore.sending)
          return;
      ctx.refreshAudioRecordingSupport();
      if (ctx.worldVoiceRecordingRuntime)
          return;
      const conversationId = String(ctx.activeConversation.value?.id || '').trim();
      if (!conversationId)
          return;
      ctx.closeWorldAttachmentPanels();
      try {
          const session = await startAudioRecording();
          const runtime: WorldVoiceRecordingRuntime = {
              session,
              startedAt: Date.now(),
              timerId: null,
              conversationId
          };
          ctx.worldVoiceRecordingRuntime = runtime;
          ctx.worldVoiceRecording.value = true;
          ctx.worldVoiceDurationMs.value = 0;
          if (typeof window !== 'undefined') {
              runtime.timerId = window.setInterval(() => {
                  ctx.worldVoiceDurationMs.value = Math.max(0, Date.now() - runtime.startedAt);
              }, ctx.WORLD_VOICE_RECORDING_TICK_MS);
          }
      }
      catch (error) {
          ctx.resetWorldVoiceRecordingState();
          const message = ctx.resolveVoiceRecordingErrorText(error);
          if (message) {
              ElMessage.warning(message);
              return;
          }
          showApiError(error, ctx.t('messenger.world.voice.startFailed'));
      }
  };

  ctx.buildWorldVoiceFileName = (): string => `voice-${Date.now()}.wav`;

  ctx.stopWorldVoiceRecordingAndSend = async () => {
      const runtime = ctx.worldVoiceRecordingRuntime;
      if (!runtime)
          return;
      ctx.worldVoiceRecordingRuntime = null;
      ctx.clearWorldVoiceRecordingTimer(runtime);
      ctx.resetWorldVoiceRecordingState();
      let recording: AudioRecordingResult;
      try {
          recording = await runtime.session.stop();
      }
      catch (error) {
          showApiError(error, ctx.t('messenger.world.voice.stopFailed'));
          return;
      }
      if (!(recording?.blob instanceof Blob) || !recording.blob.size) {
          ElMessage.warning(ctx.t('messenger.world.voice.empty'));
          return;
      }
      if (runtime.conversationId !== String(ctx.activeConversation.value?.id || '').trim()) {
          return;
      }
      ctx.worldUploading.value = true;
      try {
          const voiceFile = new File([recording.blob], ctx.buildWorldVoiceFileName(), { type: 'audio/wav' });
          const uploadedPaths = await ctx.uploadWorldFilesToUserContainer([voiceFile], { appendTokens: false });
          const uploadedPath = String(uploadedPaths[0] || '').trim();
          if (!uploadedPath) {
              throw new Error(ctx.t('workspace.upload.failed'));
          }
          const senderUserId = String((ctx.authStore.user as Record<string, unknown> | null)?.id || '').trim();
          const payloadText = buildWorldVoicePayloadContent({
              path: uploadedPath,
              durationMs: recording.durationMs,
              mimeType: 'audio/wav',
              name: voiceFile.name,
              size: voiceFile.size,
              containerId: USER_CONTAINER_ID,
              ownerUserId: senderUserId
          });
          await ctx.userWorldStore.sendToActiveConversation(payloadText, { contentType: 'voice' });
          await ctx.scrollMessagesToBottom();
      }
      catch (error) {
          showApiError(error, ctx.t('userWorld.input.sendFailed'));
      }
      finally {
          ctx.worldUploading.value = false;
      }
  };

  ctx.toggleWorldVoiceRecord = async () => {
      if (ctx.worldVoiceRecordingRuntime) {
          await ctx.stopWorldVoiceRecordingAndSend();
          return;
      }
      await ctx.startWorldVoiceRecording();
  };

  ctx.triggerWorldUpload = () => {
      const uploadInput = ctx.worldComposerViewRef.value?.getUploadInputElement() || null;
      if (!ctx.isWorldConversationActive.value || ctx.worldUploading.value || ctx.worldVoiceRecording.value || !uploadInput)
          return;
      ctx.closeWorldAttachmentPanels();
      uploadInput.value = '';
      uploadInput.click();
  };

  ctx.triggerWorldScreenshot = async (option?: WorldScreenshotCaptureOption) => {
      if (!ctx.isWorldConversationActive.value || ctx.worldUploading.value || ctx.worldVoiceRecording.value)
          return;
      if (!ctx.worldDesktopScreenshotSupported.value) {
          ElMessage.warning(ctx.t('chat.attachments.screenshotUnavailable'));
          return;
      }
      ctx.closeWorldAttachmentPanels();
      const screenshotOption: WorldScreenshotCaptureOption = {
          hideWindow: option?.hideWindow === true,
          region: option?.region === true
      };
      ctx.worldUploading.value = true;
      try {
          const captured = await ctx.captureWorldScreenshotData(screenshotOption);
          let finalFileName = captured.fileName;
          if (screenshotOption.region && !/[-_]region(\.[^./]+)?$/i.test(finalFileName)) {
              finalFileName = ctx.appendScreenshotFileNameSuffix(finalFileName, '-region');
          }
          const screenshotFile = ctx.screenshotDataUrlToFile(captured.dataUrl, finalFileName, captured.mimeType);
          const uploaded = await ctx.uploadWorldFilesToUserContainer([screenshotFile]);
          if (!uploaded.length) {
              throw new Error(ctx.t('workspace.upload.failed'));
          }
          ElMessage.success(ctx.t('chat.attachments.screenshotAdded', { name: screenshotFile.name }));
          ctx.focusWorldTextareaToEnd();
      }
      catch (error) {
          if ((error as {
              message?: string;
          })?.message === '__SCREENSHOT_CANCELED__') {
              return;
          }
          showApiError(error, ctx.t('chat.attachments.screenshotFailed'));
      }
      finally {
          ctx.worldUploading.value = false;
      }
  };

  ctx.handleWorldUploadInput = async (event: Event) => {
      const target = event.target as HTMLInputElement | null;
      if (ctx.worldVoiceRecording.value) {
          if (target)
              target.value = '';
          return;
      }
      const files = target?.files ? Array.from(target.files) : [];
      if (!files.length)
          return;
      const oversized = ctx.findWorldOversizedFile(files);
      if (oversized) {
          ElMessage.warning(ctx.t('workspace.upload.tooLarge', { limit: '200 MB' }));
          if (target)
              target.value = '';
          return;
      }
      ctx.worldUploading.value = true;
      try {
          const uploaded = await ctx.uploadWorldFilesToUserContainer(files);
          ElMessage.success(ctx.t('userWorld.attachments.uploadSuccess', { count: uploaded.length || files.length }));
      }
      catch (error) {
          showApiError(error, ctx.t('workspace.upload.failed'));
      }
      finally {
          ctx.worldUploading.value = false;
          if (target) {
              target.value = '';
          }
      }
  };

  ctx.sendWorldMessage = async () => {
      if (ctx.isMessengerInteractionBlocked.value)
          return;
      if (ctx.worldVoiceRecording.value)
          return;
      if (!ctx.canSendWorldMessage.value)
          return;
      const text = ctx.worldDraft.value.trim();
      if (!text)
          return;
      const senderUserId = String((ctx.authStore.user as Record<string, unknown> | null)?.id || '').trim();
      const normalizedText = ctx.replaceWorldAtPathTokens(text, senderUserId);
      ctx.worldQuickPanelMode.value = '';
      ctx.worldDraft.value = '';
      try {
          await ctx.userWorldStore.sendToActiveConversation(normalizedText);
          await ctx.scrollMessagesToBottom();
      }
      catch (error) {
          ctx.worldDraft.value = text;
          showApiError(error, ctx.t('userWorld.input.sendFailed'));
      }
  };

  ctx.handleWorldComposerEnterKeydown = async (event: KeyboardEvent) => {
      if (event.isComposing) {
          return;
      }
      if (ctx.messengerSendKey.value === 'none') {
          return;
      }
      const hasPrimaryModifier = Boolean(event.ctrlKey ||
          event.metaKey ||
          event.getModifierState?.('Control') ||
          event.getModifierState?.('Meta'));
      const hasBackupModifier = Boolean(event.altKey && !hasPrimaryModifier);
      if (ctx.messengerSendKey.value === 'ctrl_enter') {
          if (hasPrimaryModifier || hasBackupModifier) {
              event.preventDefault();
              await ctx.sendWorldMessage();
          }
          return;
      }
      if (event.shiftKey) {
          return;
      }
      if (hasPrimaryModifier || hasBackupModifier) {
          event.preventDefault();
          await ctx.sendWorldMessage();
          return;
      }
      event.preventDefault();
      await ctx.sendWorldMessage();
  };
}
