// @ts-nocheck
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

export function installMessengerControllerSharedHelpers(ctx: MessengerControllerContext): void {
  ctx.resolveRouteSettingsPanelMode = function resolveRouteSettingsPanelMode(routePath: string, panelValue: unknown, desktopEnabled: boolean): SettingsPanelMode {
      const path = String(routePath || '').trim().toLowerCase();
      const panelHint = String(panelValue || '').trim().toLowerCase();
      if (path.includes('/profile')) {
          return 'profile';
      }
      if (panelHint === 'profile') {
          return 'profile';
      }
      if (panelHint === 'prompts' || panelHint === 'prompt' || panelHint === 'system-prompt') {
          return 'prompts';
      }
      if (panelHint === 'help-manual' ||
          panelHint === 'manual' ||
          panelHint === 'help' ||
          panelHint === 'docs' ||
          panelHint === 'docs-site') {
          return 'help-manual';
      }
      if (desktopEnabled && panelHint === 'desktop-models') {
          return 'desktop-models';
      }
      if (desktopEnabled && panelHint === 'desktop-lan') {
          return 'desktop-lan';
      }
      return 'general';
  };

  ctx.resolveRouteHelperWorkspaceEnabled = function resolveRouteHelperWorkspaceEnabled(sectionValue: unknown, helperValue: unknown): boolean {
      const sectionHint = String(sectionValue || '').trim().toLowerCase();
      const helperHint = String(helperValue || '').trim().toLowerCase();
      return (sectionHint === 'groups' &&
          (helperHint === '1' || helperHint === 'true' || helperHint === 'yes'));
  };

  ctx.setNavigationPaneCollapsed = function setNavigationPaneCollapsed(collapsed: boolean): void {
      if (!ctx.allowNavigationCollapse.value) {
          ctx.standardNavigationCollapsed.value = false;
          return;
      }
      ctx.standardNavigationCollapsed.value = collapsed;
      if (collapsed) {
          ctx.leftRailMoreExpanded.value = false;
          ctx.clearMiddlePaneOverlayHide();
          ctx.clearMiddlePaneOverlayPreview();
          ctx.middlePaneOverlayVisible.value = false;
          return;
      }
      if (ctx.isMiddlePaneOverlay.value) {
          ctx.openMiddlePaneOverlay();
      }
  };

  ctx.toggleNavigationPaneCollapsed = function toggleNavigationPaneCollapsed(): void {
      ctx.setNavigationPaneCollapsed(!ctx.navigationPaneCollapsed.value);
  };

  ctx.resolveChatShellPath = function resolveChatShellPath(): string {
      return ctx.isEmbeddedChatRoute.value ? String(ctx.route.path || '').trim() : `${ctx.basePrefix.value}/chat`;
  };

  ctx.readServerDefaultModelName = async function readServerDefaultModelName(force = false): Promise<string> {
      if (ctx.desktopMode.value) {
          ctx.serverDefaultModelDisplayName.value = '';
          return '';
      }
      const now = Date.now();
      if (!force &&
          String(ctx.serverDefaultModelDisplayName.value || '').trim() &&
          now - ctx.serverDefaultModelCheckedAt <= ctx.SERVER_DEFAULT_MODEL_CACHE_MS) {
          return String(ctx.serverDefaultModelDisplayName.value || '').trim();
      }
      if (ctx.serverDefaultModelFetchPromise) {
          return ctx.serverDefaultModelFetchPromise;
      }
      ctx.serverDefaultModelFetchPromise = (async () => {
          try {
              const profile = ((await ctx.agentStore.getAgent(DEFAULT_AGENT_KEY, { force }).catch(() => null)) as Record<string, unknown> | null) || null;
              if (profile) {
                  ctx.defaultAgentProfile.value = profile;
              }
              const resolved = String(ctx.resolveModelNameFromRecord(profile) || '').trim();
              ctx.serverDefaultModelDisplayName.value = resolved;
              return resolved;
          }
          finally {
              ctx.serverDefaultModelCheckedAt = Date.now();
              ctx.serverDefaultModelFetchPromise = null;
          }
      })();
      return ctx.serverDefaultModelFetchPromise;
  };

  ctx.normalizeRightDockSkillRuntimeName = function normalizeRightDockSkillRuntimeName(value: unknown): string {
      const normalized = String(value || '').trim();
      if (!normalized)
          return '';
      if (ctx.rightDockSkillCatalog.value.some((item) => item.name === normalized)) {
          return normalized;
      }
      const separatorIndex = normalized.indexOf('@');
      if (separatorIndex <= 0 || separatorIndex >= normalized.length - 1) {
          return normalized;
      }
      const legacyName = normalized.slice(separatorIndex + 1).trim();
      if (!legacyName) {
          return normalized;
      }
      return ctx.rightDockSkillCatalog.value.some((item) => item.name === legacyName)
          ? legacyName
          : normalized;
  };

  ctx.normalizeRightDockSkillNameList = function normalizeRightDockSkillNameList(values: string[]): string[] {
      const output: string[] = [];
      const seen = new Set<string>();
      values.forEach((value) => {
          const normalized = ctx.normalizeRightDockSkillRuntimeName(value);
          if (!normalized || seen.has(normalized)) {
              return;
          }
          seen.add(normalized);
          output.push(normalized);
      });
      return output;
  };

  ctx.resolveSessionActivityTimestamp = function resolveSessionActivityTimestamp(session: Record<string, unknown>): number {
      // Keep conversation ordering aligned to real message activity to avoid list jumps on UI-only updates.
      return ctx.normalizeTimestamp(session.last_message_at || session.updated_at || session.created_at);
  };

  ctx.resolveMessengerRootElement = function resolveMessengerRootElement(): HTMLElement | null {
      const root = ctx.messengerRootRef.value as unknown;
      if (!root)
          return null;
      if (root instanceof HTMLElement)
          return root;
      const candidate = (root as {
          $el?: unknown;
      }).$el;
      return candidate instanceof HTMLElement ? candidate : null;
  };

  ctx.measureMessengerLayoutElement = function measureMessengerLayoutElement(element: Element | null): {
      width: number;
      left: number;
      right: number;
  } | null {
      if (!(element instanceof HTMLElement))
          return null;
      const rect = element.getBoundingClientRect();
      const width = Number.isFinite(rect.width) ? Math.round(rect.width) : 0;
      const left = Number.isFinite(rect.left) ? Math.round(rect.left) : 0;
      const right = Number.isFinite(rect.right) ? Math.round(rect.right) : 0;
      return { width, left, right };
  };

  ctx.reportMessengerLayoutAnomaly = function reportMessengerLayoutAnomaly(reason: string): void {
      if (typeof window === 'undefined')
          return;
      const root = ctx.resolveMessengerRootElement();
      if (!root)
          return;
      const rootRect = ctx.measureMessengerLayoutElement(root);
      const parentRect = ctx.measureMessengerLayoutElement(root.parentElement);
      const leftRailRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-left-rail'));
      const middlePaneRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-middle-pane'));
      const chatRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
      const chatBodyRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-chat-body'));
      const footerRect = ctx.measureMessengerLayoutElement(ctx.chatFooterRef.value);
      const composerRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
      const rightDockRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
      const sandboxPanelRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--sandbox'));
      const skillsPanelRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-right-panel--skills'));
      const snapshot = {
          reason,
          route: ctx.route.fullPath,
          section: ctx.sessionHub.activeSection,
          windowWidth: Math.round(window.innerWidth || 0),
          viewportWidth: Math.round(ctx.viewportWidth.value || 0),
          root: rootRect,
          parent: parentRect,
          leftRail: leftRailRect,
          middlePane: middlePaneRect,
          chat: chatRect,
          chatBody: chatBodyRect,
          footer: footerRect,
          composer: composerRect,
          rightDock: rightDockRect,
          sandboxPanel: sandboxPanelRect,
          skillsPanel: skillsPanelRect,
          showMiddlePane: ctx.showMiddlePane.value,
          showRightDock: ctx.showRightDock.value,
          rightDockCollapsed: ctx.rightDockCollapsed.value,
          navigationPaneCollapsed: ctx.navigationPaneCollapsed.value,
          isMiddlePaneOverlay: ctx.isMiddlePaneOverlay.value,
          isRightDockOverlay: ctx.isRightDockOverlay.value,
          rootClasses: Array.from(root.classList.values()),
          gridTemplateColumns: window.getComputedStyle(root).gridTemplateColumns
      };
      const signature = JSON.stringify({
          reason,
          windowWidth: snapshot.windowWidth,
          viewportWidth: snapshot.viewportWidth,
          root: rootRect,
          chat: chatRect,
          footer: footerRect,
          composer: composerRect,
          rightDock: rightDockRect,
          gridTemplateColumns: snapshot.gridTemplateColumns,
          section: snapshot.section,
          showMiddlePane: snapshot.showMiddlePane,
          showRightDock: snapshot.showRightDock,
          rightDockCollapsed: snapshot.rightDockCollapsed,
          navigationPaneCollapsed: snapshot.navigationPaneCollapsed,
          isMiddlePaneOverlay: snapshot.isMiddlePaneOverlay,
          isRightDockOverlay: snapshot.isRightDockOverlay
      });
      if (signature === ctx.lastMessengerLayoutDebugSignature)
          return;
      ctx.lastMessengerLayoutDebugSignature = signature;
      if (isChatDebugEnabled()) {
          chatDebugLog('messenger.layout', 'anomaly', snapshot);
          console.warn('[messenger-layout-anomaly]', snapshot);
      }
  };

  ctx.detectMessengerLayoutAnomaly = function detectMessengerLayoutAnomaly(): void {
      if (typeof window === 'undefined')
          return;
      const root = ctx.resolveMessengerRootElement();
      if (!root)
          return;
      const rootRect = ctx.measureMessengerLayoutElement(root);
      const chatRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-chat'));
      const footerRect = ctx.measureMessengerLayoutElement(ctx.chatFooterRef.value);
      const composerRect = ctx.measureMessengerLayoutElement(root.querySelector('.messenger-composer-scope.chat-shell'));
      const rightDockRect = ctx.measureMessengerLayoutElement(root.querySelector(':scope > .messenger-right-dock'));
      const windowWidth = Math.round(window.innerWidth || 0);
      if (windowWidth <= 0 || !rootRect)
          return;
      if (rootRect.width > 0 && rootRect.width < windowWidth - 240) {
          ctx.reportMessengerLayoutAnomaly('root-too-narrow');
          return;
      }
      if (windowWidth >= 900 &&
          ((chatRect && chatRect.width > 0 && chatRect.width < 220) ||
              (footerRect && footerRect.width > 0 && footerRect.width < 220) ||
              (composerRect && composerRect.width > 0 && composerRect.width < 220))) {
          ctx.reportMessengerLayoutAnomaly('chat-too-narrow');
          return;
      }
      if (ctx.isRightDockOverlay.value &&
          rightDockRect &&
          rightDockRect.width > 0 &&
          !ctx.rightDockCollapsed.value &&
          windowWidth >= ctx.MESSENGER_RIGHT_DOCK_OVERLAY_BREAKPOINT &&
          rootRect.width >= windowWidth - 80 &&
          rightDockRect.left < Math.round(windowWidth * 0.72) &&
          rightDockRect.right < windowWidth - 12) {
          ctx.reportMessengerLayoutAnomaly('overlay-dock-shifted-left');
      }
  };

  ctx.refreshMessengerRootBounds = function refreshMessengerRootBounds(): void {
      const root = ctx.resolveMessengerRootElement();
      if (!root) {
          ctx.cachedMessengerRootRight = 0;
          ctx.cachedMessengerRootWidth = 0;
          return;
      }
      const rect = root.getBoundingClientRect();
      ctx.cachedMessengerRootRight = Number.isFinite(rect.right) ? rect.right : 0;
      ctx.cachedMessengerRootWidth = Number.isFinite(rect.width) ? rect.width : 0;
      ctx.detectMessengerLayoutAnomaly();
  };

  ctx.setRightDockEdgeHover = function setRightDockEdgeHover(next: boolean): void {
      if (ctx.rightDockEdgeHover.value === next)
          return;
      ctx.rightDockEdgeHover.value = next;
  };

  ctx.handleMessengerRootPointerMove = function handleMessengerRootPointerMove(event: PointerEvent | MouseEvent): void {
      if (!ctx.showRightDock.value) {
          ctx.setRightDockEdgeHover(false);
          return;
      }
      const pointerX = Number(event.clientX);
      if (!Number.isFinite(pointerX)) {
          ctx.setRightDockEdgeHover(false);
          return;
      }
      ctx.pendingRightDockPointerX = pointerX;
      if (typeof window === 'undefined') {
          ctx.refreshMessengerRootBounds();
          if (!Number.isFinite(ctx.cachedMessengerRootRight) || ctx.cachedMessengerRootWidth <= 0) {
              ctx.setRightDockEdgeHover(false);
              return;
          }
          ctx.setRightDockEdgeHover(pointerX >= ctx.cachedMessengerRootRight - ctx.RIGHT_DOCK_EDGE_HOVER_THRESHOLD);
          return;
      }
      if (ctx.rightDockEdgeHoverFrame !== null) {
          return;
      }
      ctx.rightDockEdgeHoverFrame = window.requestAnimationFrame(() => {
          ctx.rightDockEdgeHoverFrame = null;
          if (!ctx.showRightDock.value) {
              ctx.setRightDockEdgeHover(false);
              return;
          }
          ctx.refreshMessengerRootBounds();
          if (!Number.isFinite(ctx.cachedMessengerRootRight) || ctx.cachedMessengerRootWidth <= 0) {
              ctx.setRightDockEdgeHover(false);
              return;
          }
          const nextPointerX = ctx.pendingRightDockPointerX;
          if (!Number.isFinite(nextPointerX)) {
              ctx.setRightDockEdgeHover(false);
              return;
          }
          ctx.setRightDockEdgeHover(nextPointerX >= ctx.cachedMessengerRootRight - ctx.RIGHT_DOCK_EDGE_HOVER_THRESHOLD);
      });
  };

  ctx.handleMessengerRootPointerLeave = function handleMessengerRootPointerLeave(): void {
      ctx.pendingRightDockPointerX = null;
      ctx.setRightDockEdgeHover(false);
  };

  ctx.loadAgentToolSummary = async function loadAgentToolSummary(options: {
      force?: boolean;
  } = {}) {
      const force = options.force === true;
      if (ctx.agentToolSummaryPromise) {
          return ctx.agentToolSummaryPromise;
      }
      if (!force && ctx.agentPromptToolSummary.value) {
          return ctx.agentPromptToolSummary.value;
      }
      ctx.agentToolSummaryLoading.value = true;
      ctx.agentToolSummaryError.value = '';
      ctx.agentToolSummaryPromise = (async () => {
          try {
              const summary = (await loadUserToolsSummaryCache({ force })) as Record<string, unknown> | null;
              ctx.agentPromptToolSummary.value = summary;
              return summary;
          }
          catch (error) {
              ctx.agentToolSummaryError.value =
                  (error as {
                      response?: {
                          data?: {
                              detail?: string;
                          };
                      };
                      message?: string;
                  })?.response?.data?.detail ||
                      ctx.t('chat.toolSummaryFailed');
              return null;
          }
          finally {
              ctx.agentToolSummaryLoading.value = false;
              ctx.agentToolSummaryPromise = null;
              if (ctx.agentAbilityTooltipVisible.value) {
                  await ctx.updateAgentAbilityTooltip();
              }
          }
      })();
      return ctx.agentToolSummaryPromise;
  };

  ctx.loadRightDockSkills = async function loadRightDockSkills(options: {
      force?: boolean;
      silent?: boolean;
  } = {}) {
      const force = options.force === true;
      const silent = options.silent !== false;
      if (force) {
          ctx.clearRightDockSkillAutoRetry();
      }
      if (ctx.rightDockSkillCatalogLoading.value && !force) {
          return false;
      }
      const currentVersion = ++ctx.rightDockSkillCatalogLoadVersion;
      ctx.rightDockSkillCatalogLoading.value = true;
      try {
          const skills = await loadUserSkillsCache({ force });
          if (currentVersion !== ctx.rightDockSkillCatalogLoadVersion)
              return;
          ctx.rightDockSkillCatalog.value = ctx.normalizeRightDockSkillCatalog(skills);
          if (!force && ctx.rightDockSkillCatalog.value.length === 0) {
              // First pass may race with startup auth/cache warmup and return empty transiently.
              ctx.scheduleRightDockSkillAutoRetry();
          }
          return true;
      }
      catch (error) {
          if (currentVersion !== ctx.rightDockSkillCatalogLoadVersion)
              return;
          if (!silent) {
              showApiError(error, ctx.t('userTools.skills.loadFailed'));
          }
          if (!force) {
              ctx.scheduleRightDockSkillAutoRetry();
          }
          return false;
      }
      finally {
          if (currentVersion === ctx.rightDockSkillCatalogLoadVersion) {
              ctx.rightDockSkillCatalogLoading.value = false;
          }
      }
  };

  ctx.warmMessengerUserToolsData = function warmMessengerUserToolsData(options: {
      catalog?: boolean;
      skills?: boolean;
      summary?: boolean;
  } = {}) {
      if (options.catalog === true) {
          void loadUserToolsCatalogCache();
      }
      if (options.summary === true) {
          void ctx.loadAgentToolSummary();
      }
      if (options.skills === true) {
          void ctx.loadRightDockSkills({ silent: true });
      }
  };

  ctx.handleDesktopModelMetaChanged = function handleDesktopModelMetaChanged(): void {
      if (!ctx.desktopMode.value)
          return;
      ctx.agentVoiceModelSupportCheckedAt = 0;
      ctx.desktopDefaultModelMetaFetchPromise = null;
      void ctx.readDesktopDefaultModelMeta(true);
  };

  ctx.resolveReusableFreshAgentSessionId = function resolveReusableFreshAgentSessionId(targetAgentId: string, options: {
      activeOnly?: boolean;
  } = {}): string {
      return ctx.chatStore.resolveReusableFreshSessionId(targetAgentId, options);
  };

  ctx.openOrReuseFreshAgentSession = async function openOrReuseFreshAgentSession(targetAgentId: string, options: {
      reuseScope?: 'any' | 'active_only' | 'none';
  } = {}): Promise<string> {
      const reuseScope = options.reuseScope || 'any';
      const reusableSessionId = reuseScope === 'none'
          ? ''
          : ctx.resolveReusableFreshAgentSessionId(targetAgentId, {
              activeOnly: reuseScope === 'active_only'
          });
      if (reusableSessionId) {
          void ctx.chatStore.setMainSession(reusableSessionId).catch(() => null);
          return reusableSessionId;
      }
      const payloadAgentId = targetAgentId === DEFAULT_AGENT_KEY ? '' : targetAgentId;
      const session = await ctx.chatStore.createSession(payloadAgentId ? { agent_id: payloadAgentId } : {});
      const sessionId = String((session as Record<string, unknown> | null)?.id || '').trim();
      if (!sessionId)
          return '';
      return sessionId;
  };

  ctx.runStartNewSession = async function runStartNewSession(options: {
      notify?: boolean;
  } = {}): Promise<StartNewSessionOutcome> {
      if (!ctx.isAgentConversationActive.value || ctx.creatingAgentSession.value || ctx.isMessengerInteractionBlocked.value) {
          return 'noop';
      }
      if (ctx.activeSessionOrchestrationLocked.value) {
          ElMessage.warning(ctx.t('orchestration.chat.lockedInMessenger'));
          return 'noop';
      }
      if (ctx.activeMessengerSessionBusy.value) {
          void ctx.refreshActiveAgentConversation();
          ElMessage.info(ctx.t('chat.session.running'));
          return 'noop';
      }
      const targetAgent = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      const reusableSessionId = ctx.resolveReusableFreshAgentSessionId(targetAgent, {
          activeOnly: true
      });
      if (activeSessionId && reusableSessionId && activeSessionId === reusableSessionId) {
          if (options.notify === true) {
              ElMessage.info(ctx.t('chat.newSessionAlreadyCurrent'));
          }
          return 'already_current';
      }
      const runResult = await ctx.runWithMessengerInteractionBlock('new_session', async () => {
          ctx.creatingAgentSession.value = true;
          try {
              const sessionId = await ctx.openOrReuseFreshAgentSession(targetAgent, {
                  reuseScope: 'active_only'
              });
              if (!sessionId)
                  return 'noop';
              if (options.notify === true) {
                  ElMessage.success(ctx.t('chat.newSessionOpened'));
              }
              // Keep "new thread" action responsive; detail hydration continues in background.
              void ctx.openAgentSession(sessionId, targetAgent);
              return 'opened';
          }
          finally {
              ctx.creatingAgentSession.value = false;
          }
      });
      return runResult || 'noop';
  };

  ctx.startNewSession = async function startNewSession() {
      try {
          await ctx.runStartNewSession({ notify: true });
      }
      catch (error) {
          showApiError(error, ctx.t('common.requestFailed'));
      }
  };

  ctx.normalizeAgentId = function normalizeAgentId(value: unknown): string {
      const text = String(value || '').trim();
      return text || DEFAULT_AGENT_KEY;
  };
}
