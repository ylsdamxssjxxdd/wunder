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
        :default-open="isOpenTurn(turn.key)"
        @activity="handleActivity(turn.key, $event)"
      />
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import MessengerAgentBubble from './MessengerAgentBubble.vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { MAX_OPEN_TURNS } from '@/components/chat/timelineGroupState';

/**
 * 单个用户轮次：用户气泡 + 助手全宽时间线。
 *
 * 展开上限（方案 §7.6）：`MAX_OPEN_TURNS = 8`。轮次是否默认展开由「最近位置
 * 的 8 个有内容轮次」决定；位置用虚拟行序号（`rowIndex`）表达，因此虚拟窗口
 * 前后滑动时即便行组件被卸载重建，展开集合依然稳定，不会因为重新挂载而漂移。
 */
const props = defineProps<{
  controller: MessengerControllerContext;
  turn: {
    key: string;
    rootTurnId: string;
    user: { key: string; sourceIndex: number; message: Record<string, any> };
    assistant: { key: string; sourceIndex: number; message: Record<string, any> };
  };
  /** 该轮次在当前虚拟行窗口内的序号（越大越新）。 */
  rowIndex?: number;
}>();

const activeRowIndexes = ref<Record<string, number>>({});
const openTurnKeys = ref<string[]>([]);
const pendingTurns = new Map<string, number>();
let flushScheduled = false;

const isOpenTurn = (key: string): boolean => openTurnKeys.value.includes(key);

const flushActiveTurns = (): void => {
  flushScheduled = false;
  const next: Record<string, number> = {};
  pendingTurns.forEach((rowIndex, key) => {
    next[key] = rowIndex;
  });
  const previousSignature = Object.keys(activeRowIndexes.value).sort().join('\u0001');
  const nextSignature = Object.keys(next).sort().join('\u0001');
  if (previousSignature === nextSignature) return;
  activeRowIndexes.value = next;
  // 只有最近的 MAX_OPEN_TURNS 个「有内容」轮次默认展开，更早的折回「已处理」。
  openTurnKeys.value = Object.entries(next)
    .sort((left, right) => left[1] - right[1])
    .slice(-MAX_OPEN_TURNS)
    .map(([key]) => key);
};

const handleActivity = (key: string, active: boolean): void => {
  const rowIndex = Number.isFinite(props.rowIndex) ? Number(props.rowIndex) : 0;
  if (active) {
    pendingTurns.set(key, rowIndex);
  } else {
    pendingTurns.delete(key);
  }
  if (flushScheduled) return;
  flushScheduled = true;
  if (typeof window === 'undefined') {
    flushActiveTurns();
    return;
  }
  // 与流式事件同一节奏：按帧合并，避免子组件挂载期同步回写父状态。
  window.setTimeout(flushActiveTurns, 0);
};
</script>
