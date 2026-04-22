import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const WorkspacePanel = lazy(() => import('@/components/chat/WorkspacePanel.vue'));
