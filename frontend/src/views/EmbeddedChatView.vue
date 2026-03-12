<template>
  <div class="embedded-chat-view">
    <MessengerView />
  </div>
</template>

<script setup lang="ts">
import MessengerView from '@/views/MessengerView.vue';
</script>

<style scoped>
.embedded-chat-view {
  width: 100%;
  height: var(--app-viewport-height-dvh, 100dvh);
  min-height: 100vh;
}

.embedded-chat-view :deep(.messenger-left-rail),
.embedded-chat-view :deep(.messenger-middle-pane) {
  display: none !important;
}

.embedded-chat-view :deep(.messenger-view) {
  grid-template-columns: minmax(0, 1fr) var(--messenger-right-dock-width) !important;
}

.embedded-chat-view :deep(.messenger-view.messenger-view--without-right),
.embedded-chat-view :deep(.messenger-view.messenger-view--right-collapsed),
.embedded-chat-view :deep(.messenger-view.messenger-view--without-middle),
.embedded-chat-view :deep(.messenger-view.messenger-view--without-middle.messenger-view--without-right),
.embedded-chat-view :deep(.messenger-view.messenger-view--without-middle.messenger-view--right-collapsed) {
  grid-template-columns: minmax(0, 1fr) !important;
}

.embedded-chat-view :deep(.messenger-chat) {
  min-width: 0;
}
</style>
