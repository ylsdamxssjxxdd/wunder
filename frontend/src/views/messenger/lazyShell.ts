import { defineAsyncComponent } from 'vue';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineAsyncComponent({
    loader,
    suspensible: false
  });

export const MessengerFileContainerMenu = lazy(
  () => import('@/components/messenger/MessengerFileContainerMenu.vue')
);
export const MessengerGroupDock = lazy(() => import('@/components/messenger/MessengerGroupDock.vue'));
export const MessengerRightDock = lazy(() => import('@/components/messenger/MessengerRightDock.vue'));
export const MessengerTimelineDialog = lazy(
  () => import('@/components/messenger/MessengerTimelineDialog.vue')
);
