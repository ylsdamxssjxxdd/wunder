// @ts-nocheck
// Runtime busy state, prompt ability summaries, right-dock skills, file containers, and settings targets.
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

export function installMessengerControllerRuntimeToolLists(ctx: MessengerControllerContext): void {
  ctx.pendingApprovalAgentIdSet = computed(() => {
      const approvals = Array.isArray(ctx.chatStore.pendingApprovals) ? ctx.chatStore.pendingApprovals : [];
      const result = new Set<string>();
      if (!approvals.length) {
          return result;
      }
      const sessionAgentMap = ctx.buildSessionAgentMap();
      approvals.forEach((item) => {
          const sessionId = String((item as Record<string, unknown>)?.session_id || '').trim();
          if (!sessionId)
              return;
          const fromMap = sessionAgentMap.get(sessionId);
          if (fromMap) {
              result.add(fromMap);
              return;
          }
          if (sessionId === String(ctx.chatStore.activeSessionId || '').trim()) {
              result.add(ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || DEFAULT_AGENT_KEY));
          }
      });
      return result;
  });

  ctx.isSessionBusy = (sessionId: unknown): boolean => Boolean(ctx.chatStore.isSessionBusy?.(sessionId) || ctx.chatStore.isSessionLoading?.(sessionId));

  ctx.TERMINAL_RUNTIME_STATUS_SET = new Set(['idle', 'not_loaded', 'system_error']);

  ctx.resolveSessionRuntimeStatus = (sessionId: string): string => String(ctx.chatStore.sessionRuntimeStatus?.(sessionId) || '')
      .trim()
      .toLowerCase();

  ctx.resolveSessionLoadingFlag = (sessionId: string): boolean => {
      const loadingBySession = (ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession
          : {}) as Record<string, unknown>;
      return Boolean(loadingBySession[sessionId]);
  };

  ctx.resolveEffectiveSessionBusy = (sessionId: unknown, messagesOverride: unknown[] | null = null): boolean => {
      const normalizedSessionId = String(sessionId || '').trim();
      if (!normalizedSessionId)
          return false;
      const runtimeStatus = ctx.resolveSessionRuntimeStatus(normalizedSessionId);
      const loadingBySession = ctx.resolveSessionLoadingFlag(normalizedSessionId);
      const messages = Array.isArray(messagesOverride)
          ? messagesOverride
          : normalizedSessionId === String(ctx.chatStore.activeSessionId || '').trim()
              ? (Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [])
              : ctx.chatStore.getCachedSessionMessages(normalizedSessionId);
      const busyByStoreGetter = ctx.isSessionBusy(normalizedSessionId);
      if (!loadingBySession &&
          ctx.TERMINAL_RUNTIME_STATUS_SET.has(runtimeStatus) &&
          !hasActiveSubagentsAfterLatestUser(messages) &&
          hasStreamingAssistantMessage(messages)) {
          chatDebugLog('messenger.busy', 'force-idle-after-terminal-runtime', {
              sessionId: normalizedSessionId,
              runtimeStatus,
              loadingBySession,
              busyByStoreGetter,
              messageCount: messages.length
          });
          return false;
      }
      return busyByStoreGetter;
  };

  ctx.activeMessengerSessionBusy = computed(() => {
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (!sessionId)
          return false;
      return ctx.resolveEffectiveSessionBusy(sessionId);
  });

  ctx.streamingAgentIdSet = computed(() => {
      const sessionAgentMap = ctx.buildSessionAgentMap();
      const loadingBySession = (ctx.chatStore.loadingBySession && typeof ctx.chatStore.loadingBySession === 'object'
          ? ctx.chatStore.loadingBySession
          : {}) as Record<string, unknown>;
      const sessionIds = new Set<string>([
          ...Array.from(sessionAgentMap.keys()),
          ...Object.keys(loadingBySession).map((id) => String(id || '').trim())
      ]);
      const activeSessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (activeSessionId) {
          sessionIds.add(activeSessionId);
      }
      const result = new Set<string>();
      sessionIds.forEach((sessionId) => {
          if (!sessionId || !ctx.isSessionBusy(sessionId))
              return;
          const mappedAgentId = sessionAgentMap.get(sessionId);
          if (mappedAgentId) {
              result.add(mappedAgentId);
              return;
          }
          if (sessionId === activeSessionId) {
              const fallbackAgentId = ctx.normalizeAgentId(ctx.activeAgentId.value || ctx.selectedAgentId.value || ctx.chatStore.draftAgentId) ||
                  DEFAULT_AGENT_KEY;
              result.add(fallbackAgentId);
          }
      });
      return result;
  });

  ctx.resolveCurrentUserScope = (): string => String(ctx.currentUserId.value || '').trim() || 'guest';

  ctx.resolveCurrentUserScopeAliases = (): string[] => {
      const user = ctx.authStore.user as Record<string, unknown> | null;
      if (!user) {
          return ['guest'];
      }
      const rawIds = [user?.id, user?.user_id, user?.username];
      const aliases: string[] = [];
      rawIds.forEach((value) => {
          const normalized = String(value || '').trim();
          if (normalized && !aliases.includes(normalized)) {
              aliases.push(normalized);
          }
      });
      return aliases.length ? aliases : ['guest'];
  };

  ctx.createScopedStorageKeys = (prefix: string): string[] => ctx.resolveCurrentUserScopeAliases().map((scope) => `${prefix}:${scope}`);

  ctx.resolveAgentDraftIdentity = (): string => {
      const identity = ctx.activeConversation.value;
      if (identity?.kind === 'agent') {
          const conversationId = String(identity.id || '').trim();
          if (conversationId)
              return `conversation:${conversationId}`;
          const agentId = ctx.normalizeAgentId(identity.agentId || ctx.activeAgentId.value || ctx.selectedAgentId.value);
          return `draft:${agentId || DEFAULT_AGENT_KEY}`;
      }
      const sessionId = String(ctx.chatStore.activeSessionId || '').trim();
      if (sessionId)
          return `session:${sessionId}`;
      const draftAgentId = ctx.normalizeAgentId(ctx.chatStore.draftAgentId || ctx.activeAgentId.value || ctx.selectedAgentId.value);
      return `draft:${draftAgentId || DEFAULT_AGENT_KEY}`;
  };

  ctx.agentComposerDraftKey = computed(() => `messenger:agent:${ctx.resolveCurrentUserScope()}:${ctx.resolveAgentDraftIdentity()}`);

  ctx.normalizeAbilityItemName = (item: unknown): string => {
      if (!item)
          return '';
      if (typeof item === 'string')
          return item.trim();
      const source = item as Record<string, unknown>;
      return String(source.name || source.tool_name || source.toolName || source.id || '').trim();
  };

  ctx.buildAbilityAllowedNameSet = (summary: Record<string, unknown>): Set<string> => {
      const names = collectAbilityNames(summary);
      return new Set<string>([...(names.tools || []), ...(names.skills || [])]);
  };

  ctx.normalizeAbilityNameList = (values: unknown): string[] => {
      if (!Array.isArray(values))
          return [];
      const output: string[] = [];
      const seen = new Set<string>();
      values.forEach((item) => {
          const name = String(item || '').trim();
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push(name);
      });
      return output;
  };

  ctx.extractPromptPreviewSelectedAbilityNames = (payload: unknown): string[] => {
      const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      const tooling = source.tooling_preview && typeof source.tooling_preview === 'object'
          ? (source.tooling_preview as Record<string, unknown>)
          : {};
      return ctx.normalizeAbilityNameList(tooling.selected_tool_names);
  };

  ctx.filterAbilitySummaryByNames = (summary: Record<string, unknown>, selectedNames: Set<string>): Record<string, unknown> => {
      const filterList = (list: unknown) => Array.isArray(list)
          ? list.filter((item) => {
              const name = ctx.normalizeAbilityItemName(item);
              return Boolean(name) && selectedNames.has(name);
          })
          : [];
      const filterUnifiedItems = (list: unknown) => Array.isArray(list)
          ? list.filter((item) => {
              if (!item || typeof item !== 'object')
                  return false;
              const source = item as Record<string, unknown>;
              const name = String(source.runtime_name ||
                  source.runtimeName ||
                  source.name ||
                  source.tool_name ||
                  source.toolName ||
                  source.id ||
                  '').trim();
              return Boolean(name) && selectedNames.has(name);
          })
          : [];
      return {
          ...summary,
          builtin_tools: filterList(summary.builtin_tools),
          mcp_tools: filterList(summary.mcp_tools),
          a2a_tools: filterList(summary.a2a_tools),
          knowledge_tools: filterList(summary.knowledge_tools),
          user_tools: filterList(summary.user_tools),
          shared_tools: filterList(summary.shared_tools),
          skills: filterList(summary.skills),
          skill_list: filterList(summary.skill_list),
          skillList: filterList(summary.skillList),
          items: filterUnifiedItems(summary.items),
          itemList: filterUnifiedItems(summary.itemList)
      };
  };

  ctx.effectiveAgentToolSummary = computed<Record<string, unknown> | null>(() => {
      const summary = ctx.agentPromptToolSummary.value;
      if (!summary)
          return null;
      const allowedSet = ctx.buildAbilityAllowedNameSet(summary);
      if (!allowedSet.size)
          return summary;
      if (ctx.agentPromptPreviewSelectedNames.value !== null) {
          const selectedNames = new Set<string>();
          ctx.agentPromptPreviewSelectedNames.value.forEach((item) => {
              const name = String(item || '').trim();
              if (name && allowedSet.has(name)) {
                  selectedNames.add(name);
              }
          });
          return ctx.filterAbilitySummaryByNames(summary, selectedNames);
      }
      const activeAgentProfile = ctx.activeAgentId.value === DEFAULT_AGENT_KEY
          ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)
          : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
              (ctx.activeAgent.value as Record<string, unknown> | null));
      const agentDefaults = ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile));
      const sourceOverrides = agentDefaults;
      if (sourceOverrides.some((item) => String(item || '').trim() === AGENT_TOOL_OVERRIDE_NONE)) {
          return ctx.filterAbilitySummaryByNames(summary, new Set<string>());
      }
      const selectedNames = new Set<string>();
      sourceOverrides.forEach((item) => {
          const name = String(item || '').trim();
          if (name && allowedSet.has(name)) {
              selectedNames.add(name);
          }
      });
      return ctx.filterAbilitySummaryByNames(summary, selectedNames);
  });

  ctx.activeAgentPromptPreviewHtml = computed(() => renderSystemPromptHighlight(ctx.activeAgentPromptPreviewText.value, (ctx.effectiveAgentToolSummary.value || {}) as Record<string, unknown>));

  ctx.agentAbilitySections = computed(() => {
      const groups = collectAbilityGroupDetails((ctx.effectiveAgentToolSummary.value || {}) as Record<string, unknown>);
      return [
          {
              key: 'skills',
              kind: 'skill',
              title: ctx.t('toolManager.system.skills'),
              emptyText: ctx.t('chat.ability.emptySkills'),
              items: groups.skills
          },
          {
              key: 'mcp',
              kind: 'tool',
              title: ctx.t('toolManager.system.mcp'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.mcp
          },
          {
              key: 'knowledge',
              kind: 'tool',
              title: ctx.t('toolManager.system.knowledge'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.knowledge
          },
          {
              key: 'a2a',
              kind: 'tool',
              title: ctx.t('toolManager.system.a2a'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.a2a
          },
          {
              key: 'builtin',
              kind: 'tool',
              title: ctx.t('toolManager.system.builtin'),
              emptyText: ctx.t('chat.ability.emptyTools'),
              items: groups.builtin
          }
      ].filter((section) => section.items.length > 0);
  });

  ctx.hasAgentAbilitySummary = computed(() => ctx.agentAbilitySections.value.some((section) => section.items.length > 0));

  ctx.normalizeRightDockSkillCatalog = (list: unknown): RightDockSkillCatalogItem[] => {
      if (!Array.isArray(list))
          return [];
      const output: RightDockSkillCatalogItem[] = [];
      const seen = new Set<string>();
      list.forEach((item) => {
          if (!item || typeof item !== 'object')
              return;
          const source = item as Record<string, unknown>;
          const name = String(source.name || source.tool_name || source.toolName || source.id || '').trim();
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push({
              name,
              description: String(source.description || source.desc || source.summary || '').trim(),
              path: String(source.path || '').trim(),
              source: String(source.source || '').trim().toLowerCase(),
              builtin: Boolean(source.builtin),
              readonly: Boolean(source.readonly)
          });
      });
      return output;
  };

  ctx.normalizeRightDockSkillSummaryItems = (list: unknown): Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> => {
      if (!Array.isArray(list))
          return [];
      const output: Array<Pick<RightDockSkillCatalogItem, 'name' | 'description'>> = [];
      const seen = new Set<string>();
      list.forEach((item) => {
          if (!item || typeof item !== 'object')
              return;
          const source = item as Record<string, unknown>;
          const name = ctx.normalizeRightDockSkillRuntimeName(String(source.name || source.tool_name || source.toolName || source.id || ''));
          if (!name || seen.has(name))
              return;
          seen.add(name);
          output.push({
              name,
              description: String(source.description || source.desc || source.summary || '').trim()
          });
      });
      return output;
  };

  ctx.rightDockSkillEnabledNameSet = computed<Set<string>>(() => {
      const activeAgentProfile = ctx.activeAgentId.value === DEFAULT_AGENT_KEY
          ? (ctx.defaultAgentProfile.value as Record<string, unknown> | null)
          : ((ctx.activeAgentDetailProfile.value as Record<string, unknown> | null) ||
              (ctx.activeAgent.value as Record<string, unknown> | null));
      const selectedByProfile = ctx.normalizeRightDockSkillNameList(ctx.normalizeAbilityNameList(resolveAgentConfiguredAbilityNames(activeAgentProfile)));
      return new Set(selectedByProfile);
  });

  ctx.rightDockSkillItems = computed<RightDockSkillItem[]>(() => {
      const enabledSet = ctx.rightDockSkillEnabledNameSet.value;
      const merged = new Map<string, RightDockSkillItem>();
      ctx.rightDockSkillCatalog.value.forEach((item) => {
          merged.set(item.name, {
              name: item.name,
              description: item.description,
              enabled: enabledSet.has(item.name)
          });
      });
      const allSkills = collectAbilityDetails((ctx.agentPromptToolSummary.value || {}) as Record<string, unknown>);
      ctx.normalizeRightDockSkillSummaryItems(allSkills.skills).forEach((item) => {
          const existing = merged.get(item.name);
          if (existing) {
              if (!existing.description && item.description) {
                  existing.description = item.description;
              }
              return;
          }
          merged.set(item.name, {
              name: item.name,
              description: item.description,
              enabled: enabledSet.has(item.name)
          });
      });
      return Array.from(merged.values()).sort((left, right) => left.name.localeCompare(right.name, undefined, { numeric: true, sensitivity: 'base' }));
  });

  ctx.rightDockEnabledSkills = computed<RightDockSkillItem[]>(() => ctx.rightDockSkillItems.value.filter((item) => item.enabled));

  ctx.rightDockDisabledSkills = computed<RightDockSkillItem[]>(() => ctx.rightDockSkillItems.value.filter((item) => !item.enabled));

  ctx.rightDockSkillsLoading = computed(() => ctx.rightDockSkillCatalogLoading.value && ctx.rightDockSkillItems.value.length === 0);

  ctx.rightDockSelectedSkill = computed<RightDockSkillCatalogItem | null>(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      if (!name)
          return null;
      return ctx.rightDockSkillCatalog.value.find((item) => item.name === name) || null;
  });

  ctx.rightDockSkillDialogTitle = computed(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      return name ? `技能 skill · ${name}` : '技能 skill';
  });

  ctx.rightDockSkillDialogPath = computed(() => {
      const path = String(ctx.rightDockSkillContentPath.value || ctx.rightDockSelectedSkill.value?.path || '').trim();
      return path || 'SKILL.md';
  });

  ctx.rightDockSelectedSkillEnabled = computed(() => {
      const name = String(ctx.rightDockSelectedSkillName.value || '').trim();
      if (!name)
          return false;
      return ctx.rightDockSkillEnabledNameSet.value.has(name);
  });

  ctx.currentContainerId = computed(() => {
      const source = ctx.activeAgent.value as Record<string, unknown> | null;
      const parsed = Number.parseInt(String(source?.sandbox_container_id ?? 1), 10);
      if (!Number.isFinite(parsed))
          return 1;
      return Math.min(10, Math.max(1, parsed));
  });

  ctx.normalizeSandboxContainerId = (value: unknown): number => {
      const parsed = Number.parseInt(String(value ?? 1), 10);
      if (!Number.isFinite(parsed))
          return 1;
      return Math.min(10, Math.max(1, parsed));
  };

  ctx.agentFileContainers = computed<AgentFileContainer[]>(() => {
      const buckets = new Map<number, {
          agentIds: string[];
          agentNames: string[];
      }>();
      const seenAgentIds = new Set<string>();
      const collect = (agent: Record<string, unknown>) => {
          const normalizedId = ctx.normalizeAgentId(agent?.id);
          if (seenAgentIds.has(normalizedId))
              return;
          seenAgentIds.add(normalizedId);
          const containerId = ctx.normalizeSandboxContainerId(agent?.sandbox_container_id);
          const target = buckets.get(containerId) || { agentIds: [], agentNames: [] };
          target.agentIds.push(normalizedId);
          target.agentNames.push(String(agent?.name || normalizedId));
          buckets.set(containerId, target);
      };
      collect({
          id: DEFAULT_AGENT_KEY,
          name: ctx.t('messenger.defaultAgent'),
          sandbox_container_id: 1
      });
      ctx.ownedAgents.value.forEach((item) => collect(item as Record<string, unknown>));
      ctx.sharedAgents.value.forEach((item) => collect(item as Record<string, unknown>));
      return AGENT_CONTAINER_IDS.map((id) => {
          const bucket = buckets.get(id) || { agentIds: [], agentNames: [] };
          const names = bucket.agentNames.filter(Boolean);
          const preview = names.length === 0
              ? ctx.t('messenger.files.unboundAgentContainer')
              : names.length <= 2
                  ? names.join(' / ')
                  : `${names.slice(0, 2).join(' / ')} +${names.length - 2}`;
          const primaryAgentId = bucket.agentIds.find((agentId) => agentId !== DEFAULT_AGENT_KEY) || bucket.agentIds[0] || '';
          return {
              id,
              agentIds: bucket.agentIds,
              agentNames: names,
              preview,
              primaryAgentId
          };
      });
  });

  ctx.boundAgentFileContainers = computed(() => ctx.agentFileContainers.value.filter((item) => item.agentNames.length > 0));

  ctx.unboundAgentFileContainers = computed(() => ctx.agentFileContainers.value.filter((item) => item.agentNames.length === 0));

  ctx.selectedAgentFileContainer = computed(() => ctx.agentFileContainers.value.find((item) => item.id === ctx.selectedFileContainerId.value) || null);

  ctx.selectedFileAgentIdForApi = computed(() => {
      if (ctx.fileScope.value !== 'agent')
          return '';
      const target = ctx.selectedAgentFileContainer.value?.primaryAgentId || '';
      if (!target || target === DEFAULT_AGENT_KEY)
          return '';
      return target;
  });

  ctx.selectedFileContainerAgentLabel = computed(() => {
      if (ctx.fileScope.value !== 'agent')
          return ctx.currentUsername.value;
      const names = ctx.selectedAgentFileContainer.value?.agentNames || [];
      if (!names.length)
          return ctx.t('common.none');
      if (names.length <= 3)
          return names.join(' / ');
      return `${names.slice(0, 3).join(' / ')} +${names.length - 3}`;
  });

  ctx.resolveWorkspaceRootPrefix = (): {
      root: string;
      separator: '/' | '\\';
  } => {
      const runtimeRoot = String(getRuntimeConfig().workspace_root || '')
          .trim()
          .replace(/[\\/]+$/, '');
      const root = runtimeRoot || '/workspaces';
      return {
          root,
          separator: root.includes('\\') ? '\\' : '/'
      };
  };

  ctx.withTrailingSeparator = (path: string): string => {
      const trimmed = String(path || '').trim();
      if (!trimmed)
          return '';
      const separator = trimmed.includes('\\') ? '\\' : '/';
      if (trimmed.endsWith('/') || trimmed.endsWith('\\')) {
          return trimmed;
      }
      return `${trimmed}${separator}`;
  };

  ctx.resolveWorkspaceScopeSuffix = (): string => {
      const userId = String(ctx.currentUserId.value || '').trim() || 'anonymous';
      if (ctx.fileScope.value === 'user' || ctx.selectedFileContainerId.value === USER_CONTAINER_ID) {
          return userId;
      }
      return `${userId}__c__${ctx.selectedFileContainerId.value}`;
  };

  ctx.fileContainerCloudLocation = computed(() => {
      const { root } = ctx.resolveWorkspaceRootPrefix();
      const scope = ctx.resolveWorkspaceScopeSuffix();
      return `${root.replace(/\\/g, '/')}/${scope}/`;
  });

  ctx.fileContainerLocalLocation = computed(() => {
      if (!ctx.desktopMode.value) {
          return '';
      }
      const containerId = ctx.fileScope.value === 'user' || ctx.selectedFileContainerId.value === USER_CONTAINER_ID
          ? USER_CONTAINER_ID
          : ctx.selectedFileContainerId.value;
      const mapped = String(ctx.desktopContainerRootMap.value[containerId] || '').trim();
      if (mapped) {
          return ctx.withTrailingSeparator(mapped);
      }
      const { root, separator } = ctx.resolveWorkspaceRootPrefix();
      const scope = ctx.resolveWorkspaceScopeSuffix();
      return `${root}${separator}${scope}${separator}`;
  });

  ctx.workspacePanelKey = computed(() => `${ctx.fileScope.value}:${ctx.selectedFileContainerId.value}:${ctx.selectedFileAgentIdForApi.value || 'default'}`);

  ctx.fileContainerContextMenuStyle = computed(() => ({
      left: `${ctx.fileContainerContextMenu.value.x}px`,
      top: `${ctx.fileContainerContextMenu.value.y}px`
  }));

  ctx.showAgentSettingsPanel = computed(() => ctx.sessionHub.activeSection === 'agents' || ctx.isAgentConversationActive.value);

  ctx.settingsAgentId = computed(() => {
      if (ctx.sessionHub.activeSection === 'agents') {
          return ctx.normalizeAgentId(ctx.selectedAgentId.value);
      }
      if (ctx.isAgentConversationActive.value) {
          return ctx.normalizeAgentId(ctx.activeAgentId.value);
      }
      return '';
  });

  ctx.settingsAgentIdForPanel = computed(() => ctx.normalizeAgentId(ctx.settingsAgentId.value));

  ctx.isSettingsDefaultAgentReadonly = computed(() => false);

  ctx.canDeleteSettingsAgent = computed(() => {
      const value = ctx.normalizeAgentId(ctx.settingsAgentId.value);
      return Boolean(value) && value !== DEFAULT_AGENT_KEY;
  });

  ctx.settingsAgentIdForApi = computed(() => {
      const value = ctx.normalizeAgentId(ctx.settingsAgentId.value);
      return value === DEFAULT_AGENT_KEY ? '' : value;
  });

  ctx.settingsRuntimeAgentIdForApi = computed(() => {
      const value = ctx.normalizeAgentId(ctx.settingsAgentId.value);
      if (value === DEFAULT_AGENT_KEY) {
          return '__default__';
      }
      return value;
  });

  ctx.selectedContact = computed(() => (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : []).find((item) => String(item?.user_id || '') === ctx.selectedContactUserId.value) || null);

  ctx.selectedGroup = computed(() => (Array.isArray(ctx.userWorldStore.groups) ? ctx.userWorldStore.groups : []).find((item) => String(item?.group_id || '') === ctx.selectedGroupId.value) || null);

  ctx.selectedBeeroomGroup = computed<BeeroomGroup | null>(() => ctx.beeroomStore.activeGroup || ctx.beeroomStore.activeGroupSummary || null);

  ctx.showChatSettingsView = computed(() => ctx.sessionHub.activeSection !== 'messages');

  ctx.showMessengerChatHeader = computed(() => ctx.sessionHub.activeSection === 'messages' || ctx.sessionHub.activeSection === 'agents');

  ctx.showHelperAppsWorkspace = computed(() => ctx.sessionHub.activeSection === 'groups' && ctx.helperAppsWorkspaceMode.value);

  ctx.settingsPanelRenderKey = computed(() => ['settings', ctx.sessionHub.activeSection].join(':'));

  ctx.routeSectionIntent = computed<MessengerSection>(() => {
      if (ctx.desktopMode.value && ctx.desktopInitialSectionPinned.value) {
          return ctx.sessionHub.activeSection;
      }
      return resolveSectionFromRoute(ctx.route.path, ctx.route.query.section);
  });

  ctx.routeSettingsPanelModeIntent = computed<SettingsPanelMode>(() => ctx.resolveRouteSettingsPanelMode(ctx.route.path, ctx.route.query.panel, ctx.desktopMode.value));

  ctx.showHelpManualWaitingOverlay = computed(() => ctx.sessionHub.activeSection === 'more' &&
      ctx.settingsPanelMode.value === 'help-manual' &&
      ctx.helpManualLoading.value);

  ctx.suppressMessengerPageWaitingOverlay = computed(() => (ctx.routeSectionIntent.value === 'agents' &&
      ctx.agentSettingMode.value === 'agent' &&
      !ctx.showAgentGridOverview.value) ||
      (ctx.routeSectionIntent.value === 'more' &&
          ctx.routeSettingsPanelModeIntent.value === 'help-manual') ||
      ctx.showHelpManualWaitingOverlay.value);

  ctx.showChatComposerFooter = computed(() => {
      const routeSection = resolveSectionFromRoute(ctx.route.path, ctx.route.query.section);
      if (routeSection !== 'messages') {
          return false;
      }
      return !ctx.showChatSettingsView.value && (ctx.isAgentConversationActive.value || ctx.isWorldConversationActive.value);
  });

  ctx.filteredOwnedAgents = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return ctx.ownedAgents.value.filter((agent) => ctx.matchesAgentHiveSelection(agent) && ctx.matchesAgentKeyword(agent, text));
  });

  ctx.fullPrimaryAgentList = computed(() => {
      const items: Array<Record<string, unknown>> = [];
      if (ctx.showDefaultAgentEntry.value) {
          items.push({
              id: DEFAULT_AGENT_KEY,
              name: ctx.t('messenger.defaultAgent'),
              description: ctx.t('messenger.defaultAgentDesc'),
              icon: (ctx.defaultAgentProfile.value as Record<string, unknown> | null)?.icon
          });
      }
      return [...items, ...ctx.ownedAgents.value];
  });

  ctx.orderedOwnedAgentsState = usePersistentStableListOrder(ctx.fullPrimaryAgentList, {
      getKey: (agent) => ctx.normalizeAgentId(agent?.id),
      storageKey: computed(() => `messenger:agents:owned:${ctx.resolveCurrentUserScope()}`),
      storageFallbackKeys: computed(() => ctx.createScopedStorageKeys('messenger:agents:owned'))
  });

  watch(() => ctx.agentSettingMode.value, (mode) => {
      ctx.mountedAgentSettingModes.value[mode] = true;
  }, { immediate: true });

  ctx.handleHelpManualLoadingChange = (value: boolean) => {
      ctx.helpManualLoading.value = value === true;
  };

  ctx.filteredSharedAgents = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return ctx.sharedAgents.value.filter((agent) => ctx.matchesAgentHiveSelection(agent) && ctx.matchesAgentKeyword(agent, text));
  });

  ctx.orderedSharedAgentsState = usePersistentStableListOrder(ctx.sharedAgents, {
      getKey: (agent) => ctx.normalizeAgentId(agent?.id),
      storageKey: computed(() => `messenger:agents:shared:${ctx.resolveCurrentUserScope()}`),
      storageFallbackKeys: computed(() => ctx.createScopedStorageKeys('messenger:agents:shared'))
  });

  ctx.filteredOwnedAgentIdSet = computed(() => new Set(ctx.filteredOwnedAgents.value.map((agent) => ctx.normalizeAgentId(agent?.id)).filter(Boolean)));

  ctx.filteredSharedAgentIdSet = computed(() => new Set(ctx.filteredSharedAgents.value.map((agent) => ctx.normalizeAgentId(agent?.id)).filter(Boolean)));

  ctx.orderedPrimaryAgents = computed(() => ctx.orderedOwnedAgentsState.orderedItems.value.filter((agent) => {
      const agentId = ctx.normalizeAgentId(agent?.id);
      if (!agentId) {
          return false;
      }
      if (agentId === DEFAULT_AGENT_KEY) {
          return ctx.showDefaultAgentEntry.value;
      }
      return ctx.filteredOwnedAgentIdSet.value.has(agentId);
  }));

  ctx.filteredOwnedAgentsOrdered = computed(() => ctx.orderedPrimaryAgents.value.filter((agent) => ctx.normalizeAgentId(agent?.id) !== DEFAULT_AGENT_KEY));

  ctx.filteredSharedAgentsOrdered = computed(() => ctx.orderedSharedAgentsState.orderedItems.value.filter((agent) => ctx.filteredSharedAgentIdSet.value.has(ctx.normalizeAgentId(agent?.id))));

  ctx.visibleAgentIdsForSelection = computed(() => {
      const ids: string[] = [];
      ctx.orderedPrimaryAgents.value.forEach((agent) => {
          const agentId = ctx.normalizeAgentId(agent?.id);
          if (agentId && !ids.includes(agentId)) {
              ids.push(agentId);
          }
      });
      ctx.filteredSharedAgentsOrdered.value.forEach((agent) => {
          const agentId = ctx.normalizeAgentId(agent?.id);
          if (agentId && !ids.includes(agentId)) {
              ids.push(agentId);
          }
      });
      return ids;
  });

  ctx.showAgentGridOverview = computed(() => ctx.sessionHub.activeSection === 'agents' && ctx.agentOverviewMode.value === 'grid');

  watch(() => [ctx.sessionHub.activeSection, ctx.showAgentGridOverview.value] as const, ([section, showGrid]) => {
      if (section !== 'agents' || showGrid) {
          return;
      }
      void preloadAgentSettingsPanels();
      ctx.warmMessengerUserToolsData({
          catalog: true,
          summary: true
      });
  }, { immediate: true });

  watch(() => [ctx.sessionHub.activeSection, ctx.settingsPanelMode.value] as const, ([section, panelMode]) => {
      if (section === 'more' && panelMode === 'help-manual') {
          ctx.helpManualLoading.value = true;
          return;
      }
      ctx.helpManualLoading.value = false;
  }, { immediate: true });

  ctx.agentOverviewCards = computed<AgentOverviewCard[]>(() => {
      const cards: AgentOverviewCard[] = [];
      const seen = new Set<string>();
      const pushCard = (agent: Record<string, unknown>, options: {
          shared?: boolean;
          isDefault?: boolean;
      } = {}) => {
          const id = ctx.normalizeAgentId(agent?.id || DEFAULT_AGENT_KEY);
          if (!id || seen.has(id))
              return;
          seen.add(id);
          const containerId = ctx.normalizeSandboxContainerId(agent?.sandbox_container_id);
          const abilityCounts = resolveAgentOverviewAbilityCounts(agent);
          cards.push({
              id,
              name: String(agent?.name || id),
              icon: agent?.icon,
              description: String(agent?.description || ''),
              shared: options.shared === true,
              isDefault: options.isDefault === true,
              runtimeState: ctx.resolveAgentRuntimeState(id),
              hasCron: ctx.hasCronTask(id),
              hasChannelBinding: ctx.channelBoundAgentIds.value.has(id),
              containerId,
              userRounds: ctx.resolveAgentUserRounds(id),
              skillCount: abilityCounts.skillCount,
              mcpCount: abilityCounts.mcpCount
          });
      };
      ctx.orderedPrimaryAgents.value.forEach((item) => pushCard(item as Record<string, unknown>, {
          isDefault: ctx.normalizeAgentId((item as Record<string, unknown>)?.id) === DEFAULT_AGENT_KEY
      }));
      ctx.filteredSharedAgentsOrdered.value.forEach((item) => pushCard(item as Record<string, unknown>, { shared: true }));
      return cards;
  });

  ctx.normalizeUiFontSize = (value: unknown): number => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed))
          return 14;
      return Math.min(20, Math.max(12, Math.round(parsed)));
  };

  ctx.normalizeMessengerSendKey = (value: unknown): MessengerSendKeyMode => (() => {
      const text = String(value || '').trim().toLowerCase();
      if (text === 'enter')
          return 'enter';
      if (text === 'none' || text === 'off' || text === 'disabled')
          return 'none';
      return 'enter';
  })();

  ctx.applyUiFontSize = (value: number) => {
      if (typeof document === 'undefined')
          return;
      const normalized = ctx.normalizeUiFontSize(value);
      document.documentElement.style.setProperty('--messenger-font-size', `${normalized}px`);
      document.documentElement.style.setProperty('--messenger-font-scale', String(normalized / 14));
  };
}
