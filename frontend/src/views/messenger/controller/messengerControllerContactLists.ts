// @ts-nocheck
// Contact unit tree, contact filtering, group filtering, and virtual contact list projections.
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

export function installMessengerControllerContactLists(ctx: MessengerControllerContext): void {
  ctx.contactUnitLabelMap = computed(() => {
      const map = new Map<string, string>();
      (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : []).forEach((item) => {
          const source = item && typeof item === 'object' ? (item as Record<string, unknown>) : {};
          const key = resolveUnitIdKey(source.unit_id);
          if (!key || key === UNIT_UNGROUPED_ID || map.has(key))
              return;
          const label = normalizeUnitShortLabel(source.unit_name ||
              source.unitName ||
              source.unit_display_name ||
              source.unitDisplayName ||
              source.path_name ||
              source.pathName ||
              source.unit_path ||
              source.unitPath);
          if (label) {
              map.set(key, label);
          }
      });
      return map;
  });

  ctx.resolveUnitLabel = (unitId: unknown): string => {
      const cleaned = normalizeUnitText(unitId);
      if (!cleaned)
          return ctx.t('userWorld.unit.ungrouped');
      const mapped = normalizeUnitShortLabel(ctx.orgUnitPathMap.value[cleaned]);
      if (mapped)
          return mapped;
      const contactLabel = ctx.contactUnitLabelMap.value.get(cleaned);
      if (contactLabel)
          return contactLabel;
      return cleaned;
  };

  ctx.buildCurrentUserFallbackUnitTree = (): UnitTreeNode[] => {
      const user = ctx.authStore.user as Record<string, unknown> | null;
      const unitId = normalizeUnitText(user?.unit_id || user?.unitId);
      if (!unitId)
          return [];
      const label = normalizeUnitShortLabel(user?.unit_name ||
          user?.unitName ||
          user?.unit_display_name ||
          user?.unitDisplayName ||
          user?.path_name ||
          user?.pathName ||
          user?.unit_path ||
          user?.unitPath);
      return [
          {
              id: unitId,
              label: label || unitId,
              parentId: '',
              sortOrder: 0,
              children: []
          }
      ];
  };

  ctx.isContactUnitExpanded = (unitId: string): boolean => ctx.contactUnitExpandedIds.value.has(unitId);

  ctx.toggleContactUnitExpanded = (unitId: string) => {
      const cleaned = normalizeUnitText(unitId);
      if (!cleaned)
          return;
      const next = new Set(ctx.contactUnitExpandedIds.value);
      if (next.has(cleaned)) {
          next.delete(cleaned);
      }
      else {
          next.add(cleaned);
      }
      ctx.contactUnitExpandedIds.value = next;
  };

  ctx.contactTotalCount = computed(() => Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts.length : 0);

  ctx.contactUnitDirectCountMap = computed(() => {
      const map = new Map<string, number>();
      (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : []).forEach((item) => {
          const key = resolveUnitIdKey(item?.unit_id);
          map.set(key, (map.get(key) || 0) + 1);
      });
      return map;
  });

  ctx.contactUnitKnownIdSet = computed(() => {
      const set = new Set<string>();
      collectUnitNodeIds(ctx.orgUnitTree.value, set);
      return set;
  });

  ctx.contactUnitDescendantMap = computed(() => {
      const map = new Map<string, Set<string>>();
      const walk = (node: UnitTreeNode): Set<string> => {
          const ids = new Set<string>([node.id]);
          node.children.forEach((child) => {
              walk(child).forEach((value) => ids.add(value));
          });
          map.set(node.id, ids);
          return ids;
      };
      ctx.orgUnitTree.value.forEach((node) => {
          walk(node);
      });
      return map;
  });

  ctx.contactUnitTreeRows = computed<UnitTreeRow[]>(() => {
      const directCountMap = ctx.contactUnitDirectCountMap.value;
      const treeRows = buildUnitTreeRows(ctx.orgUnitTree.value, 0, directCountMap, ctx.isContactUnitExpanded).rows;
      const knownIds = ctx.contactUnitKnownIdSet.value;
      const extraRows: UnitTreeRow[] = [];
      directCountMap.forEach((count, unitId) => {
          if (!count || unitId === UNIT_UNGROUPED_ID || knownIds.has(unitId))
              return;
          extraRows.push({
              id: unitId,
              label: ctx.resolveUnitLabel(unitId),
              depth: 0,
              count,
              hasChildren: false,
              expanded: false
          });
      });
      extraRows.sort((left, right) => left.label.localeCompare(right.label, 'zh-CN'));
      const ungroupedCount = directCountMap.get(UNIT_UNGROUPED_ID) || 0;
      if (ungroupedCount > 0) {
          extraRows.push({
              id: UNIT_UNGROUPED_ID,
              label: ctx.t('userWorld.unit.ungrouped'),
              depth: 0,
              count: ungroupedCount,
              hasChildren: false,
              expanded: false
          });
      }
      return treeRows.concat(extraRows);
  });

  ctx.selectedContactUnitScope = computed<Set<string> | null>(() => {
      const selected = normalizeUnitText(ctx.selectedContactUnitId.value);
      if (!selected)
          return null;
      if (selected === UNIT_UNGROUPED_ID) {
          return new Set<string>([UNIT_UNGROUPED_ID]);
      }
      const descendants = ctx.contactUnitDescendantMap.value.get(selected);
      if (descendants?.size) {
          return descendants;
      }
      return new Set<string>([selected]);
  });

  ctx.filteredContacts = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      const selectedScope = ctx.selectedContactUnitScope.value;
      return (Array.isArray(ctx.userWorldStore.contacts) ? ctx.userWorldStore.contacts : []).filter((item) => {
          const username = String(item?.username || '').toLowerCase();
          const userId = String(item?.user_id || '').toLowerCase();
          const unitKey = resolveUnitIdKey(item?.unit_id);
          if (selectedScope && !selectedScope.has(unitKey)) {
              return false;
          }
          const unitLabel = ctx.resolveUnitLabel(item?.unit_id).toLowerCase();
          return !text || username.includes(text) || userId.includes(text) || unitLabel.includes(text);
      });
  });

  ctx.contactVirtualRange = computed(() => {
      const total = ctx.filteredContacts.value.length;
      if (!total) {
          return { start: 0, end: 0 };
      }
      const viewportHeight = ctx.contactVirtualViewportHeight.value ||
          ctx.contactVirtualListRef.value?.clientHeight ||
          ctx.CONTACT_VIRTUAL_ITEM_HEIGHT * 8;
      const start = Math.max(0, Math.floor(ctx.contactVirtualScrollTop.value / ctx.CONTACT_VIRTUAL_ITEM_HEIGHT) - ctx.CONTACT_VIRTUAL_OVERSCAN);
      const visibleCount = Math.ceil(viewportHeight / ctx.CONTACT_VIRTUAL_ITEM_HEIGHT) + ctx.CONTACT_VIRTUAL_OVERSCAN * 2;
      const end = Math.min(total, start + visibleCount);
      return { start, end };
  });

  ctx.visibleFilteredContacts = computed(() => ctx.filteredContacts.value.slice(ctx.contactVirtualRange.value.start, ctx.contactVirtualRange.value.end));

  ctx.contactVirtualTopPadding = computed(() => ctx.contactVirtualRange.value.start * ctx.CONTACT_VIRTUAL_ITEM_HEIGHT);

  ctx.contactVirtualBottomPadding = computed(() => {
      const remaining = ctx.filteredContacts.value.length - ctx.contactVirtualRange.value.end;
      return Math.max(0, remaining * ctx.CONTACT_VIRTUAL_ITEM_HEIGHT);
  });

  ctx.filteredGroups = computed(() => {
      const text = ctx.keyword.value.toLowerCase();
      return (Array.isArray(ctx.userWorldStore.groups) ? ctx.userWorldStore.groups : []).filter((item) => {
          const name = String(item?.group_name || '').toLowerCase();
          const groupId = String(item?.group_id || '').toLowerCase();
          return !text || name.includes(text) || groupId.includes(text);
      });
  });
}
