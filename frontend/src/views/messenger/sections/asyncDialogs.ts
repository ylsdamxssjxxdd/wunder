import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const MessengerPromptPreviewDialog = lazy(
  () => import('@/components/messenger/MessengerPromptPreviewDialog.vue')
);

export const WorkspaceDocumentDialog = lazy(
  () => import('@/components/workspace/WorkspaceDocumentDialog.vue')
);
