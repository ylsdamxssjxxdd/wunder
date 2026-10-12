<template>
  <section
    class="messenger-turn"
    :data-root-turn-id="turn.rootTurnId"
    data-chat-measure
    :data-virtual-key="turn.key"
  >
    <div
      v-if="!isUserTurnOwnedByQueueStrip(turn.user.message)"
      class="messenger-turn-user"
      data-turn-slot="user"
    >
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
import { chatQueueState } from '@/stores/chatQueueState';

// 排队中的用户轮次由输入区上方的排队条承载；只有排队条确实拿到这一条时才收起时间线里的
// 气泡，避免排队接口不可用时两头都不显示。
const PROJECTED_QUEUE_USER_ID = /^queue:(.+):user$/;
const isUserTurnOwnedByQueueStrip = (message: Record<string, any> | undefined): boolean => {
  if (String(message?.status || '').trim().toLowerCase() !== 'queued') return false;
  const messageId = String(message?.message_id ?? message?.messageId ?? '').trim();
  const queueId = PROJECTED_QUEUE_USER_ID.exec(messageId)?.[1] || '';
  if (!queueId) return false;
  return chatQueueState.items.some((item) => item.queueId === queueId);
};

/**
 * 单个用户轮次：用户气泡 + 助手全宽时间线。
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
