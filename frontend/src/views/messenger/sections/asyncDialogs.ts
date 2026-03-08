import { defineAsyncComponent } from 'vue';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineAsyncComponent({
    loader,
    suspensible: false
  });

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

