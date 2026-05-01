// @ts-nocheck
// Plaza, beeroom, agent ordering, mixed conversations, and drag-order projections.
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

export function installMessengerControllerHiveMixedLists(ctx: MessengerControllerContext): void {
  ctx.filteredPlazaItems = computed(() => filterPlazaItemsByKindAndKeyword(ctx.plazaStore.items, ctx.plazaBrowseKind.value, ''));

  ctx.filteredBeeroomGroups = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return (Array.isArray(ctx.beeroomStore.groups) ? ctx.beeroomStore.groups : []).filter((item) => {
          const name = String(item?.name || '').toLowerCase();
          const groupId = String(item?.group_id || item?.hive_id || '').toLowerCase();
          const description = String(item?.description || '').toLowerCase();
          return !text || name.includes(text) || groupId.includes(text) || description.includes(text);
      });
  });

  ctx.orderedBeeroomGroupsState = usePersistentStableListOrder(computed(() => (Array.isArray(ctx.beeroomStore.groups) ? ctx.beeroomStore.groups : [])), {
      getKey: (group) => String(group?.group_id || group?.hive_id || '').trim(),
      storageKey: computed(() => `messenger:swarms:${ctx.resolveCurrentUserScope()}`),
      storageFallbackKeys: computed(() => ctx.createScopedStorageKeys('messenger:swarms'))
  });

  ctx.filteredBeeroomGroupIdSet = computed(() => new Set(ctx.filteredBeeroomGroups.value
      .map((group) => String(group?.group_id || group?.hive_id || '').trim())
      .filter(Boolean)));

  ctx.filteredBeeroomGroupsOrdered = computed(() => ctx.orderedBeeroomGroupsState.orderedItems.value.filter((group) => ctx.filteredBeeroomGroupIdSet.value.has(String(group?.group_id || group?.hive_id || '').trim())));

  ctx.beeroomGroupOptions = computed(() => (Array.isArray(ctx.beeroomStore.groups) ? ctx.beeroomStore.groups : []).map((item) => {
      const groupId = String(item?.group_id || item?.hive_id || '').trim();
      return {
          group_id: groupId,
          name: String(item?.is_default ? ctx.t('messenger.agentGroup.defaultOption') : (item?.name || groupId)).trim()
      };
  }));

  ctx.preferredBeeroomGroupId = computed(() => {
      const activeHiveId = String((ctx.activeAgent.value as Record<string, unknown> | null)?.hive_id ||
          (ctx.activeAgent.value as Record<string, unknown> | null)?.hiveId ||
          '').trim();
      if (activeHiveId)
          return activeHiveId;
      const selectedAgent = ctx.ownedAgents.value.find((item) => ctx.normalizeAgentId(item?.id) === ctx.normalizeAgentId(ctx.selectedAgentId.value));
      const selectedHiveId = String(selectedAgent?.hive_id || selectedAgent?.hiveId || '').trim();
      if (selectedHiveId)
          return selectedHiveId;
      const defaultGroup = ctx.beeroomStore.groups.find((item) => item.is_default);
      return String(defaultGroup?.group_id || defaultGroup?.hive_id || '').trim();
  });

  ctx.beeroomCandidateAgents = computed(() => {
      const currentGroupId = String(ctx.beeroomStore.activeGroupId || '').trim();
      const memberIds = new Set(ctx.beeroomStore.activeAgents.map((item) => String(item?.agent_id || '').trim()).filter(Boolean));
      return ctx.ownedAgents.value
          .filter((item) => ctx.normalizeAgentId(item?.id) !== DEFAULT_AGENT_KEY)
          .filter((item) => {
          if (!currentGroupId)
              return true;
          const agentHiveId = String(item?.hive_id || item?.hiveId || '').trim();
          const agentId = String(item?.id || '').trim();
          return agentHiveId !== currentGroupId && !memberIds.has(agentId);
      })
          .map((item) => ({
          id: String(item?.id || '').trim(),
          name: String(item?.name || item?.id || '').trim()
      }))
          .filter((item) => item.id);
  });

  ctx.filteredGroupCreateContacts = computed(() => {
      const text = String(ctx.groupCreateKeyword.value || '')
          .trim()
          .toLowerCase();
      const currentUserId = String((ctx.authStore.user as Record<string, unknown> | null)?.id || '').trim();
      return (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : [])
          .filter((contact) => String(contact?.user_id || '').trim() !== currentUserId)
          .filter((contact) => {
          if (!text)
              return true;
          const username = String(contact?.username || '').toLowerCase();
          const userId = String(contact?.user_id || '').toLowerCase();
          const unit = ctx.resolveUnitLabel(contact?.unit_id).toLowerCase();
          return username.includes(text) || userId.includes(text) || unit.includes(text);
      });
  });

  ctx.selectedToolEntryKey = computed(() => {
      if (ctx.selectedToolCategory.value)
          return `category:${ctx.selectedToolCategory.value}`;
      return '';
  });

  ctx.adminToolGroups = computed(() => [
      { key: 'builtin', title: ctx.t('toolManager.system.builtin'), items: ctx.builtinTools.value },
      { key: 'mcp', title: ctx.t('toolManager.system.mcp'), items: ctx.mcpTools.value },
      { key: 'skills', title: ctx.t('toolManager.system.skills'), items: ctx.skillTools.value },
      { key: 'knowledge', title: ctx.t('toolManager.system.knowledge'), items: ctx.knowledgeTools.value }
  ]);

  ctx.resolveAdminToolDetail = (item: ToolEntry): string => {
      const name = String(item?.name || '').trim();
      const description = String(item?.description || '').trim();
      const detail = description || ctx.t('common.noDescription');
      return name ? `${name}\n${detail}` : detail;
  };

  ctx.sortedMixedConversations = computed<MixedConversation[]>(() => {
      const dismissedMap = ctx.dismissedAgentConversationMap.value;
      const sessionsByAgent = new Map<string, Array<{
          session: Record<string, unknown>;
          lastAt: number;
          isMain: boolean;
      }>>();
      (Array.isArray(ctx.chatStore.sessions) ? ctx.chatStore.sessions : []).forEach((sessionRaw) => {
          const session = (sessionRaw || {}) as Record<string, unknown>;
          const agentId = ctx.normalizeAgentId(session.agent_id || (session.is_default === true ? DEFAULT_AGENT_KEY : ''));
          if (!agentId) {
              return;
          }
          const list = sessionsByAgent.get(agentId) || [];
          list.push({
              session,
              lastAt: ctx.resolveSessionActivityTimestamp(session),
              isMain: Boolean(session.is_main)
          });
          sessionsByAgent.set(agentId, list);
      });
      const agentItems = Array.from(sessionsByAgent.entries())
          .map(([agentId, records]) => {
          const sorted = [...records].sort((left, right) => right.lastAt - left.lastAt);
          const latest = sorted[0];
          const main = sorted.find((item) => item.isMain) || latest;
          const agent = ctx.agentMap.value.get(agentId) || null;
          const title = String((agent as Record<string, unknown> | null)?.name ||
              (agentId === DEFAULT_AGENT_KEY ? ctx.t('messenger.defaultAgent') : agentId));
          const preview = ctx.resolveSessionTimelinePreview((latest?.session || main?.session || {}) as Record<string, unknown>);
          return {
              key: `agent:${agentId}`,
              kind: 'agent',
              sourceId: String(main?.session?.id || ''),
              agentId,
              icon: ctx.resolveAgentIconForDisplay(agentId, agent as Record<string, unknown> | null),
              title,
              preview,
              unread: Math.max(0, Math.floor(Number(ctx.agentMainUnreadCountMap.value[agentId] || 0))),
              lastAt: Number(latest?.lastAt || main?.lastAt || 0)
          } as MixedConversation;
      })
          .filter((item) => ctx.agentMap.value.has(item.agentId))
          .filter((item) => !ctx.isSilentAgent(item.agentId))
          .filter((item) => {
          const dismissedAt = Number(dismissedMap[item.agentId] || 0);
          if (!dismissedAt)
              return true;
          return item.lastAt > dismissedAt;
      });
      const draftIdentity = ctx.activeConversation.value;
      if (draftIdentity?.kind === 'agent' && draftIdentity.id.startsWith('draft:')) {
          const draftAgentId = ctx.normalizeAgentId(draftIdentity.agentId || draftIdentity.id.slice('draft:'.length));
          const draftDismissedAt = Number(dismissedMap[draftAgentId] || 0);
          if (ctx.agentMap.value.has(draftAgentId) &&
              !ctx.isSilentAgent(draftAgentId) &&
              !agentItems.some((item) => item.agentId === draftAgentId) &&
              !draftDismissedAt) {
              const agent = ctx.agentMap.value.get(draftAgentId) || null;
              agentItems.unshift({
                  key: `agent:${draftAgentId}`,
                  kind: 'agent',
                  sourceId: '',
                  agentId: draftAgentId,
                  icon: ctx.resolveAgentIconForDisplay(draftAgentId, agent as Record<string, unknown> | null),
                  title: String((agent as Record<string, unknown> | null)?.name ||
                      (draftAgentId === DEFAULT_AGENT_KEY ? ctx.t('messenger.defaultAgent') : draftAgentId)),
                  preview: '',
                  unread: 0,
                  lastAt: Date.now()
              });
          }
      }
      const worldItems = (Array.isArray(ctx.userWorldStore.conversations) ? ctx.userWorldStore.conversations : []).map((conversation) => {
          const conversationId = String(conversation?.conversation_id || '');
          const isGroup = String(conversation?.conversation_type || '').toLowerCase() === 'group';
          const title = ctx.userWorldStore.resolveConversationTitle(conversation) || conversationId;
          return {
              key: `${isGroup ? 'group' : 'direct'}:${conversationId}`,
              kind: isGroup ? 'group' : 'direct',
              sourceId: conversationId,
              agentId: '',
              title,
              preview: String(conversation?.last_message_preview || ''),
              unread: ctx.resolveUnread(ctx.userWorldStore.resolveConversationUnread(conversationId)),
              lastAt: ctx.normalizeTimestamp(conversation?.last_message_at || conversation?.updated_at)
          } as MixedConversation;
      });
      const entries = [...agentItems, ...worldItems];
      if (ctx.desktopShowFirstLaunchDefaultAgentHint.value && !entries.length) {
          const defaultAgent = ctx.agentMap.value.get(DEFAULT_AGENT_KEY) || null;
          entries.push({
              key: `agent:${DEFAULT_AGENT_KEY}`,
              kind: 'agent',
              sourceId: '',
              agentId: DEFAULT_AGENT_KEY,
              title: String((defaultAgent as Record<string, unknown> | null)?.name || ctx.t('messenger.defaultAgent')),
              preview: ctx.t('messenger.defaultAgentDesc'),
              unread: 0,
              lastAt: ctx.desktopFirstLaunchDefaultAgentHintAt.value || Date.now()
          } as MixedConversation);
      }
      return entries.sort((left, right) => right.lastAt - left.lastAt);
  });

  ctx.mixedConversations = useStableMixedConversationOrder(ctx.sortedMixedConversations);

  ctx.orderedMixedConversationsState = usePersistentStableListOrder(ctx.mixedConversations, {
      getKey: (item) => String(item?.key || '').trim(),
      storageKey: computed(() => `messenger:messages:${ctx.resolveCurrentUserScope()}`),
      storageFallbackKeys: computed(() => ctx.createScopedStorageKeys('messenger:messages'))
  });

  ctx.filteredMixedConversations = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return ctx.orderedMixedConversationsState.orderedItems.value.filter((item) => {
          if (!text)
              return true;
          return item.title.toLowerCase().includes(text) || item.preview.toLowerCase().includes(text);
      });
  });

  ctx.hasAnyMixedConversations = computed(() => ctx.orderedMixedConversationsState.orderedItems.value.length > 0);

  ctx.moveMixedConversationItem = (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => {
      ctx.orderedMixedConversationsState.moveItem(draggedKey, targetKey, position, visibleKeys);
  };

  ctx.moveAgentListItem = (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => {
      const normalizedDraggedKey = ctx.normalizeAgentId(draggedKey);
      const normalizedTargetKey = ctx.normalizeAgentId(targetKey);
      if (!normalizedDraggedKey || !normalizedTargetKey) {
          return;
      }
      const normalizedVisibleKeys = visibleKeys.map((key) => ctx.normalizeAgentId(key)).filter(Boolean);
      const draggedIsOwned = normalizedDraggedKey === DEFAULT_AGENT_KEY || ctx.filteredOwnedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === normalizedDraggedKey);
      const targetIsOwned = normalizedTargetKey === DEFAULT_AGENT_KEY || ctx.filteredOwnedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === normalizedTargetKey);
      if (draggedIsOwned && targetIsOwned) {
          const ownedVisibleKeys = normalizedVisibleKeys.filter((key) => {
              if (key === DEFAULT_AGENT_KEY) {
                  return ctx.showDefaultAgentEntry.value;
              }
              return ctx.filteredOwnedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === key);
          });
          ctx.orderedOwnedAgentsState.moveItem(normalizedDraggedKey, normalizedTargetKey, position, ownedVisibleKeys);
          return;
      }
      const draggedIsShared = ctx.filteredSharedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === normalizedDraggedKey);
      const targetIsShared = ctx.filteredSharedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === normalizedTargetKey);
      if (draggedIsShared && targetIsShared) {
          const sharedVisibleKeys = normalizedVisibleKeys.filter((key) => ctx.filteredSharedAgentsOrdered.value.some((agent) => ctx.normalizeAgentId(agent?.id) === key));
          ctx.orderedSharedAgentsState.moveItem(normalizedDraggedKey, normalizedTargetKey, position, sharedVisibleKeys);
      }
  };

  ctx.moveBeeroomGroupItem = (draggedKey: string, targetKey: string, position: 'before' | 'after', visibleKeys: string[]) => {
      ctx.orderedBeeroomGroupsState.moveItem(draggedKey, targetKey, position, visibleKeys);
  };
}
