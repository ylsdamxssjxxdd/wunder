import { computed, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus';
import { createAgent as createAgentApi, listAgentUserRounds, listRunningAgents } from '@/api/agents';
import { fetchOrgUnits, updateProfile } from '@/api/auth';
import { listChannelBindings } from '@/api/channels';
import {
  getSession as getChatSessionApi,
  fetchSessionSystemPrompt,
  fetchRealtimeSystemPrompt
} from '@/api/chat';
import { fetchCronJobs } from '@/api/cron';
import { fetchExternalLinks } from '@/api/externalLinks';
import {
  fetchUserSkillContent,
  uploadUserSkillZip
} from '@/api/userTools';
import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent, uploadWunderWorkspace } from '@/api/workspace';
import AbilityTooltipListItem from '@/components/common/AbilityTooltipListItem.vue';
import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import {
  scheduleMessengerBootstrapBackgroundTasks,
  settleMessengerBootstrapTasks,
  splitMessengerBootstrapTasks
} from '@/views/messenger/bootstrap';
import { resolveAgentSelectionAfterRemoval } from '@/views/messenger/agentSelection';
import { createMessageViewportRuntime, type MessageViewportRuntime } from '@/views/messenger/messageViewportRuntime';
import { useStableMixedConversationOrder } from '@/views/messenger/mixedConversationOrder';
import { usePersistentStableListOrder } from '@/views/messenger/stableListOrder';
import { createMessengerRealtimePulse } from '@/views/messenger/realtimePulse';
import { useMessengerHostWidth } from '@/views/messenger/hostWidth';
import { useMessengerInteractionBlocker } from '@/views/messenger/interactionBlocker';
import { resolveAgentConfiguredAbilityNames, resolveAgentOverviewAbilityCounts } from '@/views/messenger/agentOverviewAbilities';
import MessengerDialogsHost from '@/views/messenger/sections/MessengerDialogsHost.vue';
import ChatComposer from '@/components/chat/ChatComposer.vue';
import MessageToolWorkflow from '@/components/chat/MessageToolWorkflow.vue';
import {
  InquiryPanel,
  MessageFeedbackActions,
  MessageKnowledgeCitation,
  MessageSubagentPanel,
  MessageThinking,
  ToolApprovalComposer,
  WorkspacePanel
} from '@/views/messenger/lazyMessageBlocks';
import {
  AgentCronPanel,
  AgentMemoryPanel,
  AgentRuntimeRecordsPanel,
  AgentSettingsPanel,
  ArchivedThreadManager,
  MessengerHelpManualPanel,
  MessengerSettingsPanel,
  preloadAgentSettingsPanels,
  preloadMessengerSettingsPanels,
  UserChannelSettingsPanel,
  UserPromptSettingsPanel
} from '@/views/messenger/lazyPanels';
import {
  resolveFileContainerLifecycleText,
  resolveFileWorkspaceEmptyText
} from '@/views/messenger/fileWorkspacePresentation';
import { getRuntimeConfig } from '@/config/runtime';
import { useI18n, getCurrentLanguage, setLanguage } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { useThemeStore } from '@/stores/theme';
import {
  useSessionHubStore,
  resolveSectionFromRoute,
  type MessengerSection
} from '@/stores/sessionHub';
import { hydrateExternalMarkdownImages, renderMarkdown } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { showApiError } from '@/utils/apiError';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { buildDeclaredDependencyPayload, resolveAgentDependencyStatus } from '@/utils/agentDependencyStatus';

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
import { isCompactionRunningFromWorkflowItems } from '@/utils/chatCompactionWorkflow';
import {
  isAudioRecordingSupported,
  startAudioRecording,
  type AudioRecordingResult,
  type AudioRecordingSession
} from '@/utils/audioRecorder';
import { renderSystemPromptHighlight } from '@/utils/promptHighlight';
import { extractPromptToolingPreview, type PromptToolingPreviewItem } from '@/utils/promptToolingPreview';
import { collectAbilityDetails, collectAbilityGroupDetails, collectAbilityNames } from '@/utils/toolSummary';
import {
  buildWorkspacePublicPath,
  normalizeWorkspaceOwnerId,
  resolveMarkdownWorkspacePath
} from '@/utils/messageWorkspacePath';
import { isImagePath, isMetafileImagePath, parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
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
import { loadUserAppearance, saveUserAppearance } from '@/views/messenger/userAppearanceSync';
import {
  defaultMessengerOrderPreferences,
  loadMessengerOrderPreferences,
  saveMessengerOrderPreferences,
  type MessengerOrderPreferences
} from '@/views/messenger/messengerOrderSync';
import {
  buildAgentApprovalOptions,
  normalizeAgentApprovalMode,
  useComposerApprovalMode,
  type AgentApprovalMode
} from '@/views/messenger/composerApprovalMode';
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
  UNIT_UNGROUPED_ID,
  sectionRouteMap,
  type AgentFileContainer,
  type AgentLocalCommand,
  type AgentOverviewCard,
  type AgentRuntimeState,
  type FileContainerMenuTarget,
  type MessengerPerfTrace,
  type MessengerSendKeyMode,
  type MixedConversation,
  type ToolEntry,
  type UnitTreeNode,
  type UnitTreeRow
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
    AgentRuntimeRecordsPanel,
    AgentSettingsPanel,
    ArchivedThreadManager,
    buildAgentApprovalOptions,
    buildAssistantDisplayContent,
    buildAssistantMessageStatsEntries,
    buildDeclaredDependencyPayload,
    buildWorkspacePublicPath,
    ChatComposer,
    chatDebugLog,
    clearWorkspaceLoadingLabelTimer,
    collectAbilityDetails,
    collectAbilityGroupDetails,
    collectAbilityNames,
    computed,
    confirmWithFallback,
    copyText,
    createAgentApi,
    createMessageViewportRuntime,
    createMessengerRealtimePulse,
    DEFAULT_AGENT_KEY,
    defaultMessengerOrderPreferences,
    DISMISSED_AGENT_STORAGE_PREFIX,
    downloadWunderWorkspaceFile,
    ElLoading,
    ElMessage,
    ElMessageBox,
    emitUserToolsUpdated,
    emitWorkspaceRefresh,
    extractPromptToolingPreview,
    extractWorkspaceRefreshPaths,
    fetchCronJobs,
    fetchExternalLinks,
    fetchOrgUnits,
    fetchRealtimeSystemPrompt,
    fetchSessionSystemPrompt,
    fetchUserSkillContent,
    fetchWunderWorkspaceContent,
    getChatSessionApi,
    getCurrentLanguage,
    getFilenameFromHeaders,
    getRuntimeConfig,
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
    isCompactionRunningFromWorkflowItems,
    isImagePath,
    isMetafileImagePath,
    isWorkspacePathAffected,
    listAgentUserRounds,
    listChannelBindings,
    listRunningAgents,
    loadMessengerOrderPreferences,
    loadUserAppearance,
    loadUserSkillsCache,
    loadUserToolsCatalogCache,
    loadUserToolsSummaryCache,
    MessageFeedbackActions,
    MessageKnowledgeCitation,
    MessageSubagentPanel,
    MessageThinking,
    MessageToolWorkflow,
    MESSENGER_RIGHT_DOCK_WIDTH_STORAGE_KEY,
    MESSENGER_SEND_KEY_STORAGE_KEY,
    MESSENGER_UI_FONT_SIZE_STORAGE_KEY,
    MessengerDialogsHost,
    MessengerHelpManualPanel,
    MessengerSettingsPanel,
    nextTick,
    normalizeAgentApprovalMode,
    normalizeAgentPresetQuestions,
    normalizeAssistantMessageRuntimeState,
    normalizeAvatarColor,
    normalizeAvatarIcon,
    normalizeThemePalette,
    normalizeWorkspaceImageBlob,
    normalizeWorkspaceOwnerId,
    onAgentRuntimeRefresh,
    onBeforeUnmount,
    onMounted,
    onUpdated,
    onUserToolsUpdated,
    onWorkspaceRefresh,
    parseWorkspaceResourceUrl,
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
    resolveMarkdownWorkspacePath,
    resolveSectionFromRoute,
    saveMessengerOrderPreferences,
    saveObjectUrlAsFile,
    saveUserAppearance,
    scheduleMessengerBootstrapBackgroundTasks,
    scheduleWorkspaceLoadingLabel,
    sectionRouteMap,
    setLanguage,
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
    useChatStore,
    useComposerApprovalMode,
    useI18n,
    useMessengerHostWidth,
    useMessengerInteractionBlocker,
    usePersistentStableListOrder,
    USER_CONTAINER_ID,
    UserChannelSettingsPanel,
    useRoute,
    useRouter,
    UserPromptSettingsPanel,
    useSessionHubStore,
    useStableMixedConversationOrder,
    useThemeStore,
    watch,
    WorkspacePanel,
    ...ctx
  };
}
