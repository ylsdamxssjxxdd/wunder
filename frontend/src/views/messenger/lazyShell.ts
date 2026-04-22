import { defineRecoverableAsyncComponent } from '@/utils/asyncComponentRecovery';

const lazy = <T extends object>(loader: () => Promise<T>) =>
  defineRecoverableAsyncComponent(loader);

export const MessengerFileContainerMenu = lazy(
  () => import('@/components/messenger/MessengerFileContainerMenu.vue')
);
export const MessengerGroupDock = lazy(() => import('@/components/messenger/MessengerGroupDock.vue'));
export const MessengerRightDock = lazy(() => import('@/components/messenger/MessengerRightDock.vue'));
export const MessengerTimelineDialog = lazy(
  () => import('@/components/messenger/MessengerTimelineDialog.vue')
);
