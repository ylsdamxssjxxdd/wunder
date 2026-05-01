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
import type { MessengerControllerContext } from './controller/messengerControllerContext';
import { installMessengerController } from './controller/installMessengerController';

export function useMessengerViewController(): Record<string, any> {
  const ctx: MessengerControllerContext = {};
  installMessengerController(ctx);
  return {
    AbilityTooltipListItem,
    AGENT_CONTAINER_IDS,
    AGENT_MAIN_READ_AT_STORAGE_PREFIX,
    AGENT_MAIN_UNREAD_STORAGE_PREFIX,
    AGENT_TOOL_OVERRIDE_NONE,
    AgentAvatar,
    AgentCronPanel,
    AgentMemoryPanel,
    AgentQuickCreateDialog,
    AgentRuntimeRecordsPanel,
    AgentSettingsPanel,
    ArchivedThreadManager,
    BeeroomWorkbench,
    buildAgentApprovalOptions,
    buildAssistantDisplayContent,
    buildAssistantMessageStatsEntries,
    buildDeclaredDependencyPayload,
    buildUnitTreeFromFlat,
    buildUnitTreeRows,
    buildWorkspacePublicPath,
    buildWorldVoicePayloadContent,
    ChatComposer,
    chatDebugLog,
    classifyWorldHistoryMessage,
    clearBeeroomMissionCanvasState,
    clearBeeroomMissionChatState,
    clearCachedDispatchPreview,
    clearWorkspaceLoadingLabelTimer,
    collectAbilityDetails,
    collectAbilityGroupDetails,
    collectAbilityNames,
    collectUnitNodeIds,
    computed,
    confirmWithFallback,
    copyText,
    createAgentApi,
    createBeeroomRealtimeSync,
    createMessageViewportRuntime,
    createMessengerRealtimePulse,
    DEFAULT_AGENT_KEY,
    defaultMessengerOrderPreferences,
    deleteAgentApi,
    DesktopContainerManagerPanel,
    DesktopSystemSettingsPanel,
    DISMISSED_AGENT_STORAGE_PREFIX,
    downloadUserWorldFile,
    downloadWorkerCardBundle,
    downloadWunderWorkspaceFile,
    ElLoading,
    ElMessage,
    ElMessageBox,
    emitUserToolsUpdated,
    emitWorkspaceRefresh,
    extractPromptToolingPreview,
    extractWorkspaceRefreshPaths,
    fetchCronJobs,
    fetchDesktopSettings,
    fetchExternalLinks,
    fetchOrgUnits,
    fetchRealtimeSystemPrompt,
    fetchSessionSystemPrompt,
    fetchUserSkillContent,
    fetchWunderWorkspaceContent,
    filterPlazaItemsByKindAndKeyword,
    flattenUnitNodes,
    formatWorldVoiceDuration,
    getChatSessionApi,
    getCurrentLanguage,
    getFilenameFromHeaders,
    getRuntimeConfig,
    GlobeAppPanel,
    hasActiveSubagentItems,
    hasActiveSubagentsAfterLatestUser,
    hasAssistantWaitingForCurrentOutput,
    hasRunningAssistantMessage,
    hasStreamingAssistantMessage,
    HoneycombWaitingOverlay,
    hydrateExternalMarkdownImages,
    InquiryPanel,
    invalidateAllUserToolsCaches,
    invalidateUserSkillsCache,
    invalidateUserToolsCatalogCache,
    invalidateUserToolsSummaryCache,
    isAudioRecordingSupported,
    isChatDebugEnabled,
    isCompactionOnlyWorkflowItems,
    isCompactionRunningFromWorkflowItems,
    isDesktopModeEnabled,
    isImagePath,
    isWorkspacePathAffected,
    isWorldVoiceContentType,
    listAgentUserRounds,
    listChannelBindings,
    listRunningAgents,
    loadMessengerOrderPreferences,
    loadUserAppearance,
    loadUserSkillsCache,
    loadUserToolsCatalogCache,
    loadUserToolsSummaryCache,
    MessageCompactionDivider,
    MessageFeedbackActions,
    MessageKnowledgeCitation,
    MessageSubagentPanel,
    MessageThinking,
    MessageToolWorkflow,
    MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY,
    MESSENGER_SEND_KEY_STORAGE_KEY,
    MESSENGER_UI_FONT_SIZE_STORAGE_KEY,
    MessengerDialogsHost,
    MessengerFileContainerMenu,
    MessengerGroupDock,
    MessengerHelpManualPanel,
    MessengerHivePlazaPanel,
    MessengerLocalFileSearchPanel,
    MessengerMiddlePane,
    MessengerRightDock,
    MessengerSettingsPanel,
    MessengerTimelineDialog,
    MessengerToolsSection,
    MessengerWorldComposer,
    nextTick,
    normalizeAgentApprovalMode,
    normalizeAgentPresetQuestions,
    normalizeAssistantMessageRuntimeState,
    normalizeAvatarColor,
    normalizeAvatarIcon,
    normalizePlazaBrowseKind,
    normalizeThemePalette,
    normalizeUnitNode,
    normalizeUnitShortLabel,
    normalizeUnitText,
    normalizeWorkspaceImageBlob,
    normalizeWorkspaceOwnerId,
    normalizeWorldHistoryText,
    onAgentRuntimeRefresh,
    onBeforeUnmount,
    onMounted,
    onUpdated,
    onUserToolsUpdated,
    onWorkspaceRefresh,
    OrchestrationWorkbench,
    parseWorkerCardText,
    parseWorkspaceResourceUrl,
    parseWorldVoicePayload,
    PlanPanel,
    preloadAgentSettingsPanels,
    preloadMessengerSettingsPanels,
    prepareMessageMarkdownContent,
    PROFILE_AVATAR_COLORS,
    PROFILE_AVATAR_IMAGE_KEYS,
    PROFILE_AVATAR_IMAGE_MAP,
    PROFILE_AVATAR_OPTION_KEYS,
    redirectToLoginAfterLogout,
    ref,
    renderMarkdown,
    renderSystemPromptHighlight,
    resetWorkspaceImageCardState,
    resolveAgentConfiguredAbilityNames,
    resolveAgentDependencyStatus,
    resolveAgentOverviewAbilityCounts,
    resolveAgentSelectionAfterRemoval,
    resolveAssistantFailureNotice,
    resolveAssistantMessageRuntimeState,
    resolveFileContainerLifecycleText,
    resolveFileWorkspaceEmptyText,
    resolveLatestCompactionSnapshot,
    resolveMarkdownWorkspacePath,
    resolveRetainedSelectedPlazaItemId,
    resolveSectionFromRoute,
    resolveUnitIdKey,
    resolveUnitTreeRowStyle,
    resolveWorldHistoryIcon,
    saveMessengerOrderPreferences,
    saveObjectUrlAsFile,
    saveUserAppearance,
    scheduleMessengerBootstrapBackgroundTasks,
    scheduleWorkspaceLoadingLabel,
    sectionRouteMap,
    setLanguage,
    settleAgentSessionBusyAfterRefresh,
    settleMessengerBootstrapTasks,
    showApiError,
    splitMessengerBootstrapTasks,
    startAudioRecording,
    ToolApprovalComposer,
    UNIT_UNGROUPED_ID,
    updateProfile,
    uploadUserSkillZip,
    uploadWunderWorkspace,
    useAgentStore,
    useAuthStore,
    useBeeroomStore,
    useChatStore,
    useComposerApprovalMode,
    useI18n,
    useMessengerHostWidth,
    useMessengerInteractionBlocker,
    useMessengerRightDockResize,
    useMiddlePaneOverlayPreview,
    usePersistentStableListOrder,
    usePlazaStore,
    USER_CONTAINER_ID,
    USER_WORLD_UPLOAD_BASE,
    UserChannelSettingsPanel,
    useRoute,
    useRouter,
    UserPromptSettingsPanel,
    useSessionHubStore,
    useStableMixedConversationOrder,
    useThemeStore,
    useUserWorldStore,
    watch,
    WorkerCardImportWaitingOverlay,
    workerCardToAgentPayload,
    WorkspacePanel,
    WORLD_COMPOSER_HEIGHT_STORAGE_KEY,
    WORLD_EMOJI_CATALOG,
    WORLD_QUICK_EMOJI_STORAGE_KEY,
    WORLD_UPLOAD_SIZE_LIMIT,
    ...ctx
  };
}
