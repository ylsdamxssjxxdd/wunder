<template>
  <section
    class="messenger-turn"
    :data-root-turn-id="turn.rootTurnId"
    data-chat-measure
    :data-virtual-key="turn.key"
  >
    <div class="messenger-turn-user" data-turn-slot="user">
      <MessengerAgentBubble
        :controller="controller"
        :item="turn.user"
      />
    </div>
    <div class="messenger-turn-assistant" data-turn-slot="assistant">
      <MessengerAgentBubble
        :controller="controller"
        :item="turn.assistant"
      />
    </div>
  </section>
</template>

<script setup lang="ts">
import MessengerAgentBubble from './MessengerAgentBubble.vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';

/**
 * 单个用户轮次：用户气泡 + 助手全宽时间线。
 *
 * 还在引擎队列里的轮次不会走到这里：`agentTurnSlots` 已经把整行让给排队条。
 *
 * 行级不再维护「最近 N 轮默认展开」的集合：一轮的中间内容已经整体折进
 * 「工作过程」（默认收起、收起时不渲染），长会话的高度由块渲染层收敛。
 */
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
