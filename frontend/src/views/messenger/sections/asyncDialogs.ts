import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const MessengerResourcePreviewDialog = lazy(
  () => import('@/components/messenger/MessengerResourcePreviewDialog.vue')
);
export const MessengerPromptPreviewDialog = lazy(
  () => import('@/components/messenger/MessengerPromptPreviewDialog.vue')
);
export const MessengerTimelineDetailDialog = lazy(
  () => import('@/components/messenger/MessengerTimelineDetailDialog.vue')
);
