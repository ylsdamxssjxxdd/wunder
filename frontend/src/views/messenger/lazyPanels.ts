import { defineAsyncComponent } from 'vue';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineAsyncComponent({
    loader,
    suspensible: false
  });

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

const scheduleSecondaryAgentSettingsPanelsPreload = (): void => {
  if (secondaryAgentSettingsPanelsScheduled || typeof window === 'undefined') {
    return;
  }
  secondaryAgentSettingsPanelsScheduled = true;
  const run = () => {
    void Promise.allSettled([
      import('@/components/messenger/AgentCronPanel.vue'),
      import('@/components/channels/UserChannelSettingsPanel.vue'),
      import('@/components/messenger/AgentRuntimeRecordsPanel.vue'),
      import('@/components/messenger/memory/AgentMemoryPanel.vue'),
      import('@/components/messenger/ArchivedThreadManager.vue')
    ]);
  };
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(run, { timeout: 1200 });
    return;
  }
  window.setTimeout(run, 48);
};

export const preloadAgentSettingsPanels = () => {
  if (!agentSettingsPanelsPreloadPromise) {
    agentSettingsPanelsPreloadPromise = import('@/components/messenger/AgentSettingsPanel.vue');
    scheduleSecondaryAgentSettingsPanelsPreload();
  }
  return agentSettingsPanelsPreloadPromise;
};

