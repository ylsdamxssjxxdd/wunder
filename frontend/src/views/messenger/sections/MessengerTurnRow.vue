<template>
  <section class="messenger-turn" :data-root-turn-id="turn.rootTurnId"
    data-chat-measure :data-virtual-key="turn.key">
    <div class="messenger-turn-user" data-turn-slot="user">
      <MessengerAgentBubble :controller="controller" :item="turn.user" />
    </div>
    <div class="messenger-turn-assistant" data-turn-slot="assistant">
      <MessengerAgentBubble :controller="controller" :item="turn.assistant" />
    </div>
  </section>
</template>

<script setup lang="ts">
import MessengerAgentBubble from './MessengerAgentBubble.vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
// No messages array or fallback slot: the DOM has exactly two fixed regions.
defineProps<{
  controller: MessengerControllerContext;
  turn: {
    key: string;
    rootTurnId: string;
    user: { key: string; sourceIndex: number; message: Record<string, any> };
    assistant: { key: string; sourceIndex: number; message: Record<string, any> };
  };
}>();
</script>
