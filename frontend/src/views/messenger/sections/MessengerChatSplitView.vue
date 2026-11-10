<template>
  <section class="messenger-split-workspace" data-testid="messenger-split-workspace">
    <div
      ref="gridEl"
      class="messenger-split-grid"
      :class="`is-${effectiveCount}`"
      :style="gridStyle"
      @dragover="handleGridDragOver"
    >
      <template v-for="(sessionId, index) in visibleSlots" :key="index">
        <div
          v-if="sessionId"
          class="messenger-split-cell"
          :class="{ 'is-drop-target': dropSlot === index }"
          @dragover.prevent="dropSlot = index"
          @dragleave="dropSlot = -1"
          @drop.prevent="dropThread(index, $event)"
        >
          <MessengerChatSplitPane
            :controller="controller"
            :session-id="sessionId"
            :slot-index="index"
            :active="activeSessionId === sessionId"
            @activate="activatePane"
            @close="closePane"
            @send="sendMessage"
            @stop="stopMessage"
          />
        </div>
        <div
          v-else
          class="messenger-split-cell messenger-split-drop-empty"
          :class="{ 'is-drop-target': dropSlot === index }"
          @dragover.prevent="dropSlot = index"
          @dragleave="dropSlot = -1"
          @drop.prevent="dropThread(index, $event)"
        >
          <i class="fa-solid fa-arrow-down-to-line" aria-hidden="true"></i>
          <span>{{ t('messenger.split.drop') }}</span>
        </div>
      </template>

      <div
        class="messenger-split-resizer is-vertical"
        :class="{ 'is-dragging': resizing === 'column' }"
        role="separator"
        :aria-label="t('messenger.split.resizeColumns')"
        aria-orientation="vertical"
        tabindex="0"
        @pointerdown.prevent="startResize('column', $event)"
        @dblclick.prevent="resetRatio('column')"
        @keydown.left.prevent="nudge('column', -0.03)"
        @keydown.right.prevent="nudge('column', 0.03)"
      ></div>
      <div
        v-if="count === 4"
        class="messenger-split-resizer is-horizontal"
        :class="{ 'is-dragging': resizing === 'row' }"
        role="separator"
        :aria-label="t('messenger.split.resizeRows')"
        aria-orientation="horizontal"
        tabindex="0"
        @pointerdown.prevent="startResize('row', $event)"
        @dblclick.prevent="resetRatio('row')"
        @keydown.up.prevent="nudge('row', -0.03)"
        @keydown.down.prevent="nudge('row', 0.03)"
      ></div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import MessengerChatSplitPane from './MessengerChatSplitPane.vue';
import {
  CHAT_THREAD_DRAG_TYPE,
  assignChatSplitSlot,
  clampChatSplitRatio,
  closeChatSplitSlot,
  createChatSplitSlots,
  resolveChatSplitPreviewCount,
  type ChatSplitCount
} from '../chatSplitLayout';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';

const props = defineProps<{
  controller: MessengerControllerContext;
  activeSessionId: string;
  count: ChatSplitCount;
  secondarySessionId?: string;
}>();
const emit = defineEmits<{ 'update:count': [count: ChatSplitCount] }>();
const t = props.controller.t;
const slots = ref<string[]>(createChatSplitSlots(props.count, props.activeSessionId));
if (props.secondarySessionId && slots.value.length > 1) {
  slots.value = assignChatSplitSlot(slots.value, 1, props.secondarySessionId, props.activeSessionId);
}
const activeSlot = ref(0);
const dropSlot = ref(-1);
const gridEl = ref<HTMLElement | null>(null);
const columnRatio = ref(readRatio('column'));
const rowRatio = ref(readRatio('row'));
const resizing = ref<'column' | 'row' | ''>('');
let resizePointerId = -1;

function readRatio(axis: 'column' | 'row'): number {
  try {
    const stored = localStorage.getItem(`messenger:chat-split:${axis}`);
    return stored === null ? 0.5 : clampChatSplitRatio(Number(stored));
  } catch {
    return 0.5;
  }
}
const persistRatio = (axis: 'column' | 'row') => {
  try {
    localStorage.setItem(`messenger:chat-split:${axis}`, String(axis === 'column' ? columnRatio.value : rowRatio.value));
  } catch {
    // Layout persistence is optional.
  }
};
const count = computed(() => props.count);
const activeSessionId = computed(() => props.activeSessionId);
const draggingThread = ref(false);
// 双栏已满时拖入线程，临时展开四格供放入新分区（与桌面版一致）。
const effectiveCount = computed<ChatSplitCount>(() =>
  resolveChatSplitPreviewCount(count.value, slots.value, draggingThread.value)
);
const visibleSlotCount = computed(() => (effectiveCount.value === 4 ? 4 : slots.value.length));
const visibleSlots = computed(() => {
  if (visibleSlotCount.value <= slots.value.length) return slots.value;
  return [...slots.value, ...Array.from({ length: visibleSlotCount.value - slots.value.length }, () => '')];
});
const gridStyle = computed(() => ({
  '--split-column': `${columnRatio.value * 100}%`,
  '--split-row': `${rowRatio.value * 100}%`
}));

const hasThreadDrag = (event: DragEvent): boolean =>
  Array.from(event.dataTransfer?.types || []).includes(CHAT_THREAD_DRAG_TYPE);
const handleGridDragOver = (event: DragEvent) => {
  if (!hasThreadDrag(event)) return;
  event.preventDefault();
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  draggingThread.value = true;
};
const endThreadDrag = () => {
  draggingThread.value = false;
};
const dropThread = (index: number, event: DragEvent) => {
  const id = String(
    event.dataTransfer?.getData(CHAT_THREAD_DRAG_TYPE) || event.dataTransfer?.getData('text/plain') || ''
  ).trim();
  draggingThread.value = false;
  dropSlot.value = -1;
  if (!id) return;
  slots.value = assignChatSplitSlot(slots.value, index, id, activeSessionId.value);
  activeSlot.value = Math.max(0, slots.value.indexOf(id));
  const filled = slots.value.filter(Boolean).length;
  const nextCount: ChatSplitCount = filled > 2 ? 4 : 2;
  if (nextCount !== count.value) emit('update:count', nextCount);
  if (id !== activeSessionId.value) void props.controller.openSessionById(id);
};
const closePane = (index: number) => {
  slots.value = closeChatSplitSlot(slots.value, index);
  const retained = slots.value.filter(Boolean).length;
  emit('update:count', retained <= 1 ? 1 : retained === 2 ? 2 : 4);
  if (!retained) return;
  if (activeSlot.value === index) activeSlot.value = slots.value.findIndex(Boolean);
  else if (activeSlot.value > index) activeSlot.value -= 1;
  const id = slots.value[activeSlot.value];
  if (id && id !== activeSessionId.value) void props.controller.openSessionById(id);
};
const activatePane = (sessionId: string) => {
  activeSlot.value = Math.max(0, slots.value.indexOf(sessionId));
  if (sessionId !== activeSessionId.value) void props.controller.openSessionById(sessionId);
};
const sendMessage = async ({ sessionId, payload }: { sessionId: string; payload: Record<string, unknown> }) => {
  await props.controller.openSessionById(sessionId);
  await props.controller.sendAgentMessage(payload);
};
const stopMessage = async (sessionId: string) => {
  await props.controller.chatStore.stopSessionActivity(sessionId, { terminateSubagents: true });
};

watch(activeSessionId, (id) => {
  if (!id || slots.value.includes(id)) return;
  const target = activeSlot.value >= 0 ? activeSlot.value : 0;
  slots.value = assignChatSplitSlot(slots.value, target, id, id);
});

const onResizeMove = (event: PointerEvent) => {
  if (event.pointerId !== resizePointerId) return;
  const rect = gridEl.value?.getBoundingClientRect();
  if (!rect) return;
  if (resizing.value === 'column') {
    columnRatio.value = clampChatSplitRatio((event.clientX - rect.left) / Math.max(1, rect.width));
  } else if (resizing.value === 'row') {
    rowRatio.value = clampChatSplitRatio((event.clientY - rect.top) / Math.max(1, rect.height));
  }
};
const stopResize = (event: PointerEvent) => {
  if (event.pointerId !== resizePointerId) return;
  const axis = resizing.value;
  resizePointerId = -1;
  resizing.value = '';
  window.removeEventListener('pointermove', onResizeMove);
  window.removeEventListener('pointerup', stopResize);
  window.removeEventListener('pointercancel', stopResize);
  if (axis) persistRatio(axis);
};
const startResize = (axis: 'column' | 'row', event: PointerEvent) => {
  resizing.value = axis;
  resizePointerId = event.pointerId;
  window.addEventListener('pointermove', onResizeMove);
  window.addEventListener('pointerup', stopResize);
  window.addEventListener('pointercancel', stopResize);
};
const resetRatio = (axis: 'column' | 'row') => {
  if (axis === 'column') columnRatio.value = 0.5;
  else rowRatio.value = 0.5;
  persistRatio(axis);
};
const nudge = (axis: 'column' | 'row', delta: number) => {
  if (axis === 'column') columnRatio.value = clampChatSplitRatio(columnRatio.value + delta);
  else rowRatio.value = clampChatSplitRatio(rowRatio.value + delta);
  persistRatio(axis);
};

onMounted(() => {
  window.addEventListener('dragend', endThreadDrag, true);
});

onBeforeUnmount(() => {
  window.removeEventListener('dragend', endThreadDrag, true);
  window.removeEventListener('pointermove', onResizeMove);
  window.removeEventListener('pointerup', stopResize);
  window.removeEventListener('pointercancel', stopResize);
});
</script>
