<template>
  <section class="messenger-split-workspace" data-testid="messenger-split-workspace">
    <div class="messenger-split-toolbar">
      <span class="messenger-split-toolbar-hint">{{ t('messenger.split.hint') }}</span>
      <div class="messenger-split-layout-actions" :aria-label="t('messenger.split.layout')">
        <button type="button" :title="t('messenger.split.single')" @click="setCount(1)">
          <i class="fa-regular fa-square" aria-hidden="true"></i>
        </button>
        <button type="button" :class="{ 'is-active': count === 2 }" :title="t('messenger.split.two')" @click="setCount(2)">
          <i class="fa-solid fa-table-columns" aria-hidden="true"></i>
        </button>
        <button type="button" :class="{ 'is-active': count === 4 }" :title="t('messenger.split.four')" @click="setCount(4)">
          <i class="fa-solid fa-table-cells-large" aria-hidden="true"></i>
        </button>
      </div>
    </div>

    <div
      ref="gridEl"
      class="messenger-split-grid"
      :class="`is-${count}`"
      :style="gridStyle"
    >
      <template v-for="(sessionId, index) in slots" :key="index">
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
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import MessengerChatSplitPane from './MessengerChatSplitPane.vue';
import {
  CHAT_THREAD_DRAG_TYPE,
  assignChatSplitSlot,
  clampChatSplitRatio,
  closeChatSplitSlot,
  createChatSplitSlots,
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
const gridStyle = computed(() => ({
  '--split-column': `${columnRatio.value * 100}%`,
  '--split-row': `${rowRatio.value * 100}%`
}));

const setCount = (next: ChatSplitCount) => {
  if (next === 1) {
    emit('update:count', 1);
    return;
  }
  const retained = slots.value.filter(Boolean);
  slots.value = createChatSplitSlots(next, retained[0] || activeSessionId.value);
  retained.slice(1, next).forEach((id, index) => { slots.value[index + 1] = id; });
  activeSlot.value = Math.min(activeSlot.value, next - 1);
  emit('update:count', next);
};
const dropThread = (index: number, event: DragEvent) => {
  const id = String(
    event.dataTransfer?.getData(CHAT_THREAD_DRAG_TYPE) || event.dataTransfer?.getData('text/plain') || ''
  ).trim();
  dropSlot.value = -1;
  if (!id) return;
  slots.value = assignChatSplitSlot(slots.value, index, id, activeSessionId.value);
  activeSlot.value = Math.max(0, slots.value.indexOf(id));
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

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onResizeMove);
  window.removeEventListener('pointerup', stopResize);
  window.removeEventListener('pointercancel', stopResize);
});
</script>
