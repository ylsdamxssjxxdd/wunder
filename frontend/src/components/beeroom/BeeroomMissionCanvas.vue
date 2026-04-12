<template>
  <section ref="screenRef" class="beeroom-canvas-screen" :class="{ 'is-empty': !hasSwarmNodes }">
    <div v-if="!hasSwarmNodes" class="beeroom-canvas-empty">
      <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
      <span>{{ t('beeroom.canvas.empty') }}</span>
    </div>

    <div v-else class="beeroom-canvas-layout">
      <div
        ref="boardRef"
        class="beeroom-canvas-board"
        :class="{ 'chat-collapsed': chatCollapsed, 'is-chat-resizing': isChatResizing }"
        :style="boardStyle"
      >
        <BeeroomSwarmCanvasPane
          class="beeroom-canvas-pane"
          :group="group"
          :mission="mission"
          :agents="agents"
          :dispatch-preview="dispatchPreview"
          :subagents-by-task="subagentsByTask"
          :mother-workflow-items="motherWorkflowItems"
          :workflow-items-by-task="workflowItemsByTask"
          :workflow-preview-by-task="workflowPreviewByTask"
          :resolve-agent-avatar-image-by-agent-id="resolveAgentAvatarImageByAgentId"
          :fullscreen="canvasFullscreen"
          @open-agent="emit('open-agent', $event)"
          @toggle-fullscreen="toggleCanvasFullscreen"
        />

        <div
          v-if="showChatResizer"
          class="beeroom-canvas-chat-resizer"
          data-testid="beeroom-chat-resizer"
          role="separator"
          aria-orientation="vertical"
          :aria-label="t('beeroom.canvas.chatTitle')"
          tabindex="0"
          @pointerdown="handleChatResizePointerDown"
          @dblclick.prevent="resetChatWidth"
          @keydown.left.prevent="nudgeChatWidth(-24)"
          @keydown.right.prevent="nudgeChatWidth(24)"
        ></div>

        <BeeroomCanvasChatPanel
          :collapsed="chatCollapsed"
          :messages="displayChatMessages"
          :approvals="dispatchApprovals"
          :dispatch-can-stop="dispatchCanStop"
          :dispatch-approval-busy="dispatchApprovalBusy"
          :composer-text="composerText"
          :composer-target-agent-id="composerTargetAgentId"
          :composer-target-options="composerTargetOptions"
          :composer-sending="composerSending"
          :composer-can-send="composerCanSend"
          :composer-error="composerError"
          :resolve-message-avatar-image="resolveMessageAvatarImage"
          :avatar-label="avatarLabel"
          @update:collapsed="chatCollapsed = $event"
          @update:composer-text="composerText = $event"
          @update:composer-target-agent-id="composerTargetAgentId = $event"
          @clear="handleClearHistory"
          @send="handleComposerSend"
          @open-agent="emit('open-agent', $event)"
          @approval="handleDispatchApproval($event.decision, $event.approvalId)"
        />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, toRef, watch } from 'vue';

import BeeroomCanvasChatPanel from '@/components/beeroom/BeeroomCanvasChatPanel.vue';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import BeeroomSwarmCanvasPane from '@/components/beeroom/canvas/BeeroomSwarmCanvasPane.vue';
import { hasBeeroomSwarmNodes, resolveBeeroomSwarmScopeKey } from '@/components/beeroom/canvas/swarmCanvasModel';
import { useBeeroomMissionCanvasRuntime } from '@/components/beeroom/useBeeroomMissionCanvasRuntime';
import { useI18n } from '@/i18n';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  refreshing?: boolean;
  hideStandbyWhenMissionEmpty?: boolean;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'refresh'): void;
}>();

const { t } = useI18n();
const screenRef = ref<HTMLElement | null>(null);
const boardRef = ref<HTMLElement | null>(null);
const canvasFullscreen = ref(false);
const boardWidth = ref(0);
const chatWidth = ref(344);
const isChatResizing = ref(false);

const DEFAULT_CHAT_WIDTH = 344;
const MIN_CHAT_WIDTH = 308;
const MAX_CHAT_WIDTH = 680;
const MOBILE_CHAT_BREAKPOINT = 900;

let boardResizeObserver: ResizeObserver | null = null;
let activeResizePointerId: number | null = null;
let dragStartClientX = 0;
let dragStartChatWidth = DEFAULT_CHAT_WIDTH;

const groupRef = toRef(props, 'group');
const missionRef = toRef(props, 'mission');
const agentsRef = toRef(props, 'agents');

const {
  chatCollapsed,
  composerText,
  composerTargetAgentId,
  composerTargetOptions,
  composerSending,
  composerCanSend,
  composerError,
  dispatchApprovals,
  dispatchApprovalBusy,
  dispatchCanStop,
  dispatchPreview,
  displayChatMessages,
  motherWorkflowItems,
  subagentsByTask,
  workflowItemsByTask,
  workflowPreviewByTask,
  clearManualChatHistory,
  handleComposerSend,
  handleDispatchApproval,
  resolveAgentAvatarImageByAgentId,
  resolveMessageAvatarImage,
  avatarLabel
} = useBeeroomMissionCanvasRuntime({
  group: groupRef,
  mission: missionRef,
  agents: agentsRef,
  t,
  onRefresh: () => emit('refresh')
});

const hasSwarmNodes = computed(() => {
  if (props.hideStandbyWhenMissionEmpty && !props.mission) {
    return false;
  }
  return hasBeeroomSwarmNodes({
    group: props.group,
    mission: props.mission,
    agents: props.agents,
    dispatchPreview: dispatchPreview.value
  });
});

const missionCanvasScopeKey = computed(() =>
  resolveBeeroomSwarmScopeKey({
    missionId: props.mission?.mission_id,
    teamRunId: props.mission?.team_run_id,
    groupId: props.group?.group_id
  })
);

const getChatWidthBounds = () => {
  const currentBoardWidth = Math.max(0, Math.round(boardWidth.value || boardRef.value?.clientWidth || 0));
  const maxWidth = Math.max(
    MIN_CHAT_WIDTH,
    Math.min(MAX_CHAT_WIDTH, currentBoardWidth > 0 ? currentBoardWidth - 280 : DEFAULT_CHAT_WIDTH)
  );
  return {
    min: MIN_CHAT_WIDTH,
    max: Math.max(MIN_CHAT_WIDTH, maxWidth)
  };
};

const clampChatWidth = (value: number) => {
  const bounds = getChatWidthBounds();
  return Math.max(bounds.min, Math.min(bounds.max, Math.round(value || DEFAULT_CHAT_WIDTH)));
};

const isCompactLayout = computed(() => boardWidth.value > 0 && boardWidth.value <= MOBILE_CHAT_BREAKPOINT);

const resolvedChatWidth = computed(() =>
  isCompactLayout.value ? DEFAULT_CHAT_WIDTH : clampChatWidth(chatWidth.value || DEFAULT_CHAT_WIDTH)
);

const boardStyle = computed(() => ({
  '--beeroom-chat-width': `${resolvedChatWidth.value}px`
}));

const showChatResizer = computed(() => !chatCollapsed.value && !isCompactLayout.value);

const syncBoardWidth = () => {
  const width = Math.round(boardRef.value?.getBoundingClientRect().width || boardRef.value?.clientWidth || 0);
  if (width > 0) {
    boardWidth.value = width;
  }
};

const persistChatWidth = () => {
  mergeBeeroomMissionCanvasState(missionCanvasScopeKey.value, {
    chatWidth: resolvedChatWidth.value
  });
};

const applyChatWidth = (value: number, options: { persist?: boolean } = {}) => {
  const nextWidth = clampChatWidth(value);
  if (nextWidth === chatWidth.value) {
    if (options.persist) {
      persistChatWidth();
    }
    return;
  }
  chatWidth.value = nextWidth;
  if (options.persist) {
    persistChatWidth();
  }
};

const resetChatWidth = () => {
  applyChatWidth(DEFAULT_CHAT_WIDTH, { persist: true });
};

const nudgeChatWidth = (delta: number) => {
  applyChatWidth((chatWidth.value || resolvedChatWidth.value || DEFAULT_CHAT_WIDTH) + delta, {
    persist: true
  });
};

const stopChatResize = () => {
  activeResizePointerId = null;
  if (!isChatResizing.value) return;
  isChatResizing.value = false;
  persistChatWidth();
};

const handleChatResizePointerMove = (event: PointerEvent) => {
  if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
  applyChatWidth(dragStartChatWidth + (dragStartClientX - event.clientX));
};

const handleChatResizePointerUp = (event: PointerEvent) => {
  if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
  stopChatResize();
};

const handleChatResizePointerDown = (event: PointerEvent) => {
  if (event.button !== 0 || !showChatResizer.value) return;
  syncBoardWidth();
  activeResizePointerId = event.pointerId;
  dragStartClientX = event.clientX;
  dragStartChatWidth = resolvedChatWidth.value;
  isChatResizing.value = true;
  const target = event.currentTarget as HTMLElement | null;
  target?.setPointerCapture?.(event.pointerId);
  event.preventDefault();
};

const refreshCanvasFullscreen = () => {
  if (typeof document === 'undefined') {
    canvasFullscreen.value = false;
    return;
  }
  const fullEl = document.fullscreenElement;
  const screenEl = screenRef.value;
  canvasFullscreen.value = !!(fullEl && screenEl && (fullEl === screenEl || screenEl.contains(fullEl)));
};

const toggleCanvasFullscreen = async () => {
  const target = screenRef.value;
  if (!target || typeof document === 'undefined') return;
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else if (target.requestFullscreen) {
      await target.requestFullscreen();
    }
  } catch {
    // Ignore unsupported environment and browser permission errors.
  } finally {
    refreshCanvasFullscreen();
  }
};

const handleClearHistory = async () => {
  await clearManualChatHistory();
};

onMounted(() => {
  if (typeof document !== 'undefined') {
    document.addEventListener('fullscreenchange', refreshCanvasFullscreen);
    window.addEventListener('pointermove', handleChatResizePointerMove);
    window.addEventListener('pointerup', handleChatResizePointerUp);
    window.addEventListener('pointercancel', handleChatResizePointerUp);
    refreshCanvasFullscreen();
  }
  syncBoardWidth();
  if (typeof ResizeObserver !== 'undefined' && boardRef.value) {
    boardResizeObserver = new ResizeObserver(() => {
      syncBoardWidth();
      if (!isCompactLayout.value) {
        applyChatWidth(chatWidth.value || DEFAULT_CHAT_WIDTH);
      }
    });
    boardResizeObserver.observe(boardRef.value);
  }
});

onBeforeUnmount(() => {
  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', refreshCanvasFullscreen);
    window.removeEventListener('pointermove', handleChatResizePointerMove);
    window.removeEventListener('pointerup', handleChatResizePointerUp);
    window.removeEventListener('pointercancel', handleChatResizePointerUp);
  }
  stopChatResize();
  boardResizeObserver?.disconnect();
  boardResizeObserver = null;
});

watch(
  missionCanvasScopeKey,
  (scopeKey) => {
    const cached = getBeeroomMissionCanvasState(scopeKey);
    chatWidth.value = clampChatWidth(Number(cached?.chatWidth || DEFAULT_CHAT_WIDTH));
  },
  { immediate: true }
);

watch(
  () => boardWidth.value,
  () => {
    if (isCompactLayout.value) return;
    const clamped = clampChatWidth(chatWidth.value || DEFAULT_CHAT_WIDTH);
    if (clamped !== chatWidth.value) {
      chatWidth.value = clamped;
    }
  }
);

watch(
  () => [chatCollapsed.value, isCompactLayout.value] as const,
  ([collapsed, compact]) => {
    if (collapsed || compact) {
      stopChatResize();
    }
  }
);
</script>

<style scoped>
.beeroom-canvas-screen {
  --beeroom-motion-fast: 140ms;
  --beeroom-motion-normal: 180ms;
  --beeroom-motion-slow: 240ms;
  --beeroom-ease-standard: cubic-bezier(0.22, 1, 0.36, 1);
  --beeroom-focus-ring: 0 0 0 2px rgba(96, 165, 250, 0.52);
  position: relative;
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 560px;
  overflow: hidden;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 24px;
  color: #e5e7eb;
  background: linear-gradient(180deg, rgba(8, 11, 17, 0.995), rgba(7, 10, 15, 0.995));
  box-shadow: 0 16px 38px rgba(0, 0, 0, 0.24);
}

.beeroom-canvas-screen:fullscreen {
  width: 100vw;
  height: 100vh;
  min-height: 100vh;
  border-radius: 0;
  border: 0;
}

.beeroom-canvas-screen::before {
  display: none;
}

.beeroom-canvas-screen::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  border: 1px solid rgba(148, 163, 184, 0.08);
  pointer-events: none;
}

.beeroom-canvas-screen.is-empty::after {
  border-style: dashed;
}

.beeroom-canvas-layout {
  position: relative;
  z-index: 1;
  display: flex;
  flex: 1;
  min-height: 0;
}

.beeroom-canvas-board {
  --beeroom-chat-width: 344px;
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--beeroom-chat-width);
  flex: 1;
  width: 100%;
  min-width: 0;
  min-height: 0;
  border-radius: inherit;
  overflow: hidden;
  background: linear-gradient(180deg, rgba(8, 11, 17, 0.985), rgba(7, 10, 15, 0.985));
  transition: grid-template-columns var(--beeroom-motion-slow) var(--beeroom-ease-standard);
}

.beeroom-canvas-board.is-chat-resizing {
  user-select: none;
  -webkit-user-select: none;
  cursor: col-resize;
  transition: none;
}

.beeroom-canvas-board::before {
  display: none;
}

.beeroom-canvas-board::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  right: var(--beeroom-chat-width);
  width: 1px;
  background: linear-gradient(180deg, transparent, rgba(148, 163, 184, 0.32), transparent);
  pointer-events: none;
  transition: right var(--beeroom-motion-slow) var(--beeroom-ease-standard);
}

.beeroom-canvas-board.chat-collapsed {
  grid-template-columns: minmax(0, 1fr) 0;
}

.beeroom-canvas-board.chat-collapsed::after {
  right: 0;
  opacity: 0;
}

.beeroom-canvas-pane {
  min-width: 0;
  min-height: 0;
}

.beeroom-canvas-chat-resizer {
  position: absolute;
  top: 14px;
  bottom: 14px;
  right: calc(var(--beeroom-chat-width) - 7px);
  width: 14px;
  padding: 0;
  border: 0;
  background: transparent;
  cursor: col-resize;
  z-index: 3;
  touch-action: none;
  display: flex;
  align-items: center;
  justify-content: center;
}

.beeroom-canvas-chat-resizer::before {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: 50%;
  width: 1px;
  transform: translateX(-50%);
  background: linear-gradient(180deg, transparent, rgba(148, 163, 184, 0.46), transparent);
  transition: background-color var(--beeroom-motion-normal) var(--beeroom-ease-standard);
}

.beeroom-canvas-chat-resizer:hover::before,
.beeroom-canvas-chat-resizer:focus-visible::before,
.beeroom-canvas-board.is-chat-resizing .beeroom-canvas-chat-resizer::before {
  background: linear-gradient(180deg, transparent, rgba(96, 165, 250, 0.74), transparent);
}

.beeroom-canvas-chat-resizer:focus-visible {
  outline: none;
}

.beeroom-canvas-empty {
  position: relative;
  z-index: 1;
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  text-align: center;
  color: rgba(191, 219, 254, 0.82);
}

.beeroom-canvas-empty i {
  font-size: 30px;
  color: #38bdf8;
}

@media (prefers-reduced-motion: reduce) {
  .beeroom-canvas-screen *,
  .beeroom-canvas-screen *::before,
  .beeroom-canvas-screen *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}

@media (max-width: 1240px) {
  .beeroom-canvas-board {
    --beeroom-chat-width: 304px;
  }
}

@media (max-width: 900px) {
  .beeroom-canvas-screen {
    min-height: 640px;
  }

  .beeroom-canvas-board {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) auto;
  }

  .beeroom-canvas-board::after {
    display: none;
  }

  .beeroom-canvas-chat-resizer {
    display: none;
  }

  .beeroom-canvas-board.chat-collapsed {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) 0;
  }
}
</style>
