import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const AgentCronPanel = lazy(() => import('@/components/messenger/AgentCronPanel.vue'));
export const AgentRuntimeRecordsPanel = lazy(
  () => import('@/components/messenger/AgentRuntimeRecordsPanel.vue')
);
export const ArchivedThreadManager = lazy(
  () => import('@/components/messenger/ArchivedThreadManager.vue')
);
export const AgentSettingsPanel = lazy(
  () => import('@/components/messenger/AgentSettingsPanel.vue')
);
export const DesktopContainerManagerPanel = lazy(
  () => import('@/components/messenger/DesktopContainerManagerPanel.vue')
);
export const DesktopSystemSettingsPanel = lazy(
  () => import('@/components/messenger/DesktopSystemSettingsPanel.vue')
);
export const GlobeAppPanel = lazy(() => import('@/components/globe/GlobeAppPanel.vue'));
export const MessengerLocalFileSearchPanel = lazy(
  () => import('@/components/messenger/MessengerLocalFileSearchPanel.vue')
);
export const MessengerHelpManualPanel = lazy(
  () => import('@/components/messenger/MessengerHelpManualPanel.vue')
);
export const MessengerSettingsPanel = lazy(
  () => import('@/components/messenger/MessengerSettingsPanel.vue')
);
export const MessengerWorldComposer = lazy(
  () => import('@/components/messenger/MessengerWorldComposer.vue')
);
export const UserKnowledgePane = lazy(() => import('@/components/user-tools/UserKnowledgePane.vue'));
export const UserMcpPane = lazy(() => import('@/components/user-tools/UserMcpPane.vue'));
export const UserChannelSettingsPanel = lazy(
  () => import('@/components/channels/UserChannelSettingsPanel.vue')
);
export const UserPromptSettingsPanel = lazy(
  () => import('@/components/messenger/UserPromptSettingsPanel.vue')
);
export const UserSharedToolsPanel = lazy(
  () => import('@/components/user-tools/UserSharedToolsPanel.vue')
);
export const UserSkillPane = lazy(() => import('@/components/user-tools/UserSkillPane.vue'));

export const AgentMemoryPanel = lazy(() => import('@/components/messenger/memory/AgentMemoryPanel.vue'));

let agentSettingsPanelsPreloadPromise: Promise<unknown> | null = null;
let secondaryAgentSettingsPanelsScheduled = false;
let messengerSettingsPanelPreloadPromise: Promise<unknown> | null = null;
let secondaryMorePanelsScheduled = false;

const scheduleIdleImport = (runner: () => void, timeout = 1200): void => {
  if (typeof window === 'undefined') {
    runner();
    return;
  }
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(runner, { timeout });
    return;
  }
  window.setTimeout(runner, 48);
};

const scheduleSecondaryAgentSettingsPanelsPreload = (): void => {
  if (secondaryAgentSettingsPanelsScheduled || typeof window === 'undefined') {
    return;
  }
  secondaryAgentSettingsPanelsScheduled = true;
  scheduleIdleImport(() => {
    void Promise.allSettled([
      import('@/components/messenger/AgentCronPanel.vue'),
      import('@/components/channels/UserChannelSettingsPanel.vue'),
      import('@/components/messenger/AgentRuntimeRecordsPanel.vue'),
      import('@/components/messenger/memory/AgentMemoryPanel.vue'),
      import('@/components/messenger/ArchivedThreadManager.vue')
    ]);
  });
};

export const preloadAgentSettingsPanels = () => {
  if (!agentSettingsPanelsPreloadPromise) {
    agentSettingsPanelsPreloadPromise = import('@/components/messenger/AgentSettingsPanel.vue');
    scheduleSecondaryAgentSettingsPanelsPreload();
  }
  return agentSettingsPanelsPreloadPromise;
};

const scheduleSecondaryMorePanelsPreload = (desktopMode: boolean): void => {
  if (secondaryMorePanelsScheduled || typeof window === 'undefined') {
    return;
  }
  secondaryMorePanelsScheduled = true;
  scheduleIdleImport(() => {
    const tasks: Promise<unknown>[] = [
      import('@/components/messenger/UserPromptSettingsPanel.vue'),
      import('@/components/messenger/MessengerHelpManualPanel.vue')
    ];
    if (desktopMode) {
      tasks.push(import('@/components/messenger/DesktopSystemSettingsPanel.vue'));
    }
    void Promise.allSettled(tasks);
  });
};

export const preloadMessengerSettingsPanels = (options: { desktopMode?: boolean } = {}) => {
  if (!messengerSettingsPanelPreloadPromise) {
    messengerSettingsPanelPreloadPromise = import('@/components/messenger/MessengerSettingsPanel.vue');
  }
  scheduleSecondaryMorePanelsPreload(options.desktopMode === true);
  return messengerSettingsPanelPreloadPromise;
};
