import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const MessengerGroupCreateDialog = lazy(
  () => import('@/components/messenger/MessengerGroupCreateDialog.vue')
);
export const MessengerImagePreviewDialog = lazy(
  () => import('@/components/messenger/MessengerImagePreviewDialog.vue')
);
export const MessengerPromptPreviewDialog = lazy(
  () => import('@/components/messenger/MessengerPromptPreviewDialog.vue')
);
export const MessengerTimelineDetailDialog = lazy(
  () => import('@/components/messenger/MessengerTimelineDetailDialog.vue')
);
export const MessengerWorldHistoryDialog = lazy(
  () => import('@/components/messenger/MessengerWorldHistoryDialog.vue')
);
