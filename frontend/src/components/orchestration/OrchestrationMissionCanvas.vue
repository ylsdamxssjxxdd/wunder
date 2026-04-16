<template>
  <section ref="screenRef" class="beeroom-canvas-screen orchestration-canvas-screen">
    <div class="beeroom-canvas-layout">
      <div
        ref="boardRef"
        class="beeroom-canvas-board orchestration-canvas-board"
        :class="{ 'chat-collapsed': chatCollapsed, 'is-chat-resizing': isChatResizing }"
        :style="boardStyle"
      >
        <div class="orchestration-canvas-stage">
          <div class="orchestration-canvas-surface-shell">
            <BeeroomSwarmCanvasPane
              class="beeroom-canvas-pane"
              :group="group"
              :mission="null"
              :agents="agents"
              :dispatch-preview="null"
              :subagents-by-task="{}"
              :mother-workflow-items="[]"
              :workflow-items-by-task="{}"
              :workflow-preview-by-task="{}"
              :fullscreen="canvasFullscreen"
              :external-scope-key="canvasScopeKey"
              :external-projection="canvasProjection"
              :external-has-nodes="isReady"
              :show-minimap="false"
              @open-agent="emit('open-agent', $event)"
              @preview-node-output="handleAgentOutputPreview"
              @toggle-fullscreen="toggleCanvasFullscreen"
            />

            <div
              v-if="!isReady"
              class="beeroom-canvas-empty orchestration-canvas-empty"
            >
              <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
              <span>{{ t('orchestration.empty.createRun') }}</span>
              <small>{{ groupDescription }}</small>
            </div>
          </div>

          <div class="orchestration-timeline-shell" :class="{ collapsed: timelineCollapsed }">
            <div v-if="!timelineCollapsed" class="orchestration-timeline-dock">
              <div class="orchestration-timeline-track">
                <button
                  v-for="round in rounds"
                  :key="round.id"
                  class="orchestration-round-chip"
                  :class="{ active: round.id === activeRound?.id }"
                  type="button"
                  @click="emit('select-round', round.id)"
                >
                  <span>{{ t('orchestration.timeline.round', { round: round.index }) }}</span>
                  <small>{{ round.situation || t('orchestration.timeline.noSituation') }}</small>
                </button>
              </div>
            </div>
            <button
              class="orchestration-timeline-handle"
              type="button"
              :title="timelineCollapsed ? t('common.expand') : t('common.collapse')"
              :aria-label="timelineCollapsed ? t('common.expand') : t('common.collapse')"
              @click="toggleTimelineCollapsed"
            >
              <i class="fa-solid" :class="timelineCollapsed ? 'fa-chevron-up' : 'fa-chevron-down'" aria-hidden="true"></i>
            </button>
          </div>
        </div>

        <div
          v-if="showChatResizer"
          class="beeroom-canvas-chat-resizer"
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
          :messages="visibleChatMessages"
          :approvals="[]"
          :dispatch-can-stop="composerSending"
          :dispatch-approval-busy="false"
          :composer-text="composerText"
          :composer-target-agent-id="motherAgentId"
          :composer-target-options="composerTargetOptions"
          :composer-sending="composerSending"
          :composer-can-send="canSend"
          :composer-error="''"
          :title="group?.name || t('beeroom.canvas.chatTitle')"
          :artifacts-enabled="Boolean(activeArtifactWorkspace)"
          :show-artifacts-button="false"
          :resolve-message-avatar-image="resolveMessageAvatarImage"
          :avatar-label="avatarLabel"
          @update:collapsed="chatCollapsed = $event"
          @update:composer-text="emit('update:composer-text', $event)"
          @clear="emit('clear-chat')"
          @send="emit('send')"
          @open-agent="emit('open-agent', $event)"
        >
          <template #head-actions>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('orchestration.action.create')"
              :aria-label="t('orchestration.action.create')"
              :disabled="initializing"
              @click="emit('create-run')"
            >
              <i class="fa-solid fa-plus" aria-hidden="true"></i>
            </button>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('orchestration.action.situation')"
              :aria-label="t('orchestration.action.situation')"
              :disabled="!isReady"
              @click="emit('open-situation')"
            >
              <i class="fa-solid fa-wave-square" aria-hidden="true"></i>
            </button>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('common.refresh')"
              :aria-label="t('common.refresh')"
              :disabled="refreshing"
              @click="emit('refresh')"
            >
              <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            </button>
          </template>
        </BeeroomCanvasChatPanel>
      </div>
    </div>

    <el-dialog
      v-model="artifactWorkspaceVisible"
      :title="artifactWorkspaceDialogTitle"
      width="min(980px, calc(100vw - 28px))"
      top="clamp(10px, 4vh, 36px)"
      class="workspace-dialog beeroom-canvas-workspace-dialog"
      append-to-body
      destroy-on-close
    >
      <div class="beeroom-canvas-workspace-shell messenger-right-panel--sandbox messenger-workspace-scope chat-shell">
        <WorkspacePanel
          v-if="artifactWorkspaceVisible && activeArtifactWorkspace"
          :agent-id="activeArtifactWorkspace.agentId"
          :container-id="activeArtifactWorkspace.containerId"
          :title="artifactWorkspaceDialogTitle"
        />
      </div>
    </el-dialog>

    <BeeroomAgentOutputPreviewDialog
      v-model:visible="agentOutputPreviewVisible"
      :agent-name="agentOutputPreviewTitle"
      :role-label="agentOutputPreviewRoleLabel"
      :status-label="agentOutputPreviewStatusLabel"
      :outputs="agentOutputPreviewMessages"
      :resolve-message-avatar-image="resolveMessageAvatarImage"
      :avatar-label="avatarLabel"
    />
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import BeeroomAgentOutputPreviewDialog from '@/components/beeroom/BeeroomAgentOutputPreviewDialog.vue';
import BeeroomCanvasChatPanel from '@/components/beeroom/BeeroomCanvasChatPanel.vue';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import BeeroomSwarmCanvasPane from '@/components/beeroom/canvas/BeeroomSwarmCanvasPane.vue';
import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import type { OrchestrationArtifactCard, OrchestrationRound } from '@/components/orchestration/orchestrationRuntimeState';
import {
  buildOrchestrationCanvasProjection,
  buildOrchestrationCanvasScopeKey
} from '@/components/orchestration/orchestrationCanvasModel';
import { useI18n } from '@/i18n';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

const props = defineProps<{
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  rounds: OrchestrationRound[];
  activeRound: OrchestrationRound | null;
  activeRoundMissions: BeeroomMission[];
  artifactCards: OrchestrationArtifactCard[];
  visibleWorkers: BeeroomMember[];
  visibleChatMessages: MissionChatMessage[];
  motherAgentId: string;
  motherName: string;
  motherSessionId: string;
  runId: string;
  dispatchPreview: BeeroomSwarmDispatchPreview | null;
  composerText: string;
  composerSending: boolean;
  canSend: boolean;
  initializing: boolean;
  refreshing: boolean;
  isReady: boolean;
  groupDescription: string;
  resolveWorkerOutputs: (agentId: string) => MissionChatMessage[];
  resolveWorkerThreadSessionId: (agentId: string) => string;
  resolveMessageAvatarImage: (message: MissionChatMessage) => string;
  avatarLabel: (value: unknown) => string;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'update:composer-text', value: string): void;
  (event: 'clear-chat'): void;
  (event: 'send'): void;
  (event: 'create-run'): void;
  (event: 'open-situation'): void;
  (event: 'refresh'): void;
  (event: 'select-round', roundId: string): void;
}>();

const { t } = useI18n();
const screenRef = ref<HTMLElement | null>(null);
const boardRef = ref<HTMLElement | null>(null);
const canvasFullscreen = ref(false);
const boardWidth = ref(0);
const chatWidth = ref(352);
const isChatResizing = ref(false);
const chatCollapsed = ref(false);
const timelineCollapsed = ref(false);
const artifactWorkspaceVisible = ref(false);
const selectedArtifactAgentId = ref('');
const agentOutputPreviewVisible = ref(false);
const agentOutputPreviewAgentId = ref('');
const agentOutputPreviewTitle = ref('');
const agentOutputPreviewRoleLabel = ref('');
const agentOutputPreviewStatusLabel = ref('');

const DEFAULT_CHAT_WIDTH = 352;
const MIN_CHAT_WIDTH = 316;
const MAX_CHAT_WIDTH = 700;
const MOBILE_CHAT_BREAKPOINT = 960;
const AGENT_OUTPUT_PREVIEW_LIMIT = 6;

let boardResizeObserver: ResizeObserver | null = null;
let activeResizePointerId: number | null = null;
let dragStartClientX = 0;
let dragStartChatWidth = DEFAULT_CHAT_WIDTH;

const canvasScopeKey = computed(() =>
  buildOrchestrationCanvasScopeKey(props.runId, props.activeRound?.id || '')
);

const canvasProjection = computed(() =>
  props.isReady
    ? buildOrchestrationCanvasProjection({
        group: props.group,
        agents: props.agents,
        motherAgentId: props.motherAgentId,
        motherName: props.motherName,
        motherSessionId: props.motherSessionId,
        activeRound: props.activeRound,
        activeRoundMissions: props.activeRoundMissions,
        visibleWorkers: props.visibleWorkers,
        artifactCards: props.artifactCards,
        dispatchPreview: props.dispatchPreview,
        resolveWorkerOutputs: props.resolveWorkerOutputs,
        resolveWorkerThreadSessionId: props.resolveWorkerThreadSessionId,
        selectedNodeId: selectedArtifactAgentId.value ? `artifact:${selectedArtifactAgentId.value}` : '',
        nodePositionOverrides: getBeeroomMissionCanvasState(canvasScopeKey.value)?.nodePositionOverrides || {},
        t
      })
    : {
        nodes: [],
        edges: [],
        nodeMetaMap: new Map(),
        memberMap: new Map(),
        tasksByAgent: new Map(),
        motherNodeId: '',
        bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 }
      }
);

const composerTargetOptions = computed(() => [
  {
    agentId: props.motherAgentId,
    label: `${props.motherName} (${t('beeroom.summary.motherAgent')})`,
    role: 'mother' as const
  }
]);

const activeArtifactWorkspace = computed(() => {
  const agentId = String(selectedArtifactAgentId.value || '').trim();
  if (!agentId) return null;
  const card = props.artifactCards.find((item) => String(item.agentId || '').trim() === agentId) || null;
  const member = props.visibleWorkers.find((item) => String(item.agent_id || '').trim() === agentId) || null;
  if (!card || !member) return null;
  const containerId = Number.parseInt(String(member.sandbox_container_id ?? 1), 10) || 1;
  return {
    agentId,
    containerId,
    title: `${card.agentName} · ${card.path}`
  };
});

const artifactWorkspaceDialogTitle = computed(() => activeArtifactWorkspace.value?.title || t('beeroom.canvas.artifacts'));

const agentOutputPreviewMessages = computed(() => {
  const agentId = String(agentOutputPreviewAgentId.value || '').trim();
  if (!agentId) return [];
  return props.resolveWorkerOutputs(agentId).slice(0, AGENT_OUTPUT_PREVIEW_LIMIT);
});

const getChatWidthBounds = () => {
  const currentBoardWidth = Math.max(0, Math.round(boardWidth.value || boardRef.value?.clientWidth || 0));
  const maxWidth = Math.max(
    MIN_CHAT_WIDTH,
    Math.min(MAX_CHAT_WIDTH, currentBoardWidth > 0 ? currentBoardWidth - 320 : DEFAULT_CHAT_WIDTH)
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

const toggleTimelineCollapsed = () => {
  timelineCollapsed.value = !timelineCollapsed.value;
  mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
    timelineCollapsed: timelineCollapsed.value
  });
};

const syncBoardWidth = () => {
  const width = Math.round(boardRef.value?.getBoundingClientRect().width || boardRef.value?.clientWidth || 0);
  if (width > 0) {
    boardWidth.value = width;
  }
};

const persistChatWidth = () => {
  mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
    chatWidth: resolvedChatWidth.value,
    chatCollapsed: chatCollapsed.value
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

const handleAgentOutputPreview = (payload: {
  nodeId: string;
  agentId: string;
  agentName: string;
  roleLabel: string;
  statusLabel: string;
}) => {
  const nodeId = String(payload?.nodeId || '').trim();
  const artifactMatch = nodeId.match(/^artifact:(.+)$/);
  if (artifactMatch?.[1]) {
    selectedArtifactAgentId.value = artifactMatch[1];
    openActiveArtifactWorkspace();
    return;
  }
  const agentId = String(payload?.agentId || '').trim();
  if (!agentId) return;
  agentOutputPreviewAgentId.value = agentId;
  agentOutputPreviewTitle.value = String(payload?.agentName || agentId).trim() || agentId;
  agentOutputPreviewRoleLabel.value = String(payload?.roleLabel || '').trim();
  agentOutputPreviewStatusLabel.value = String(payload?.statusLabel || '').trim();
  agentOutputPreviewVisible.value = true;
};

const openActiveArtifactWorkspace = () => {
  if (!activeArtifactWorkspace.value) return;
  artifactWorkspaceVisible.value = true;
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
  canvasScopeKey,
  (scopeKey) => {
    const cached = getBeeroomMissionCanvasState(scopeKey);
    chatWidth.value = clampChatWidth(Number(cached?.chatWidth || DEFAULT_CHAT_WIDTH));
    chatCollapsed.value = Boolean(cached?.chatCollapsed);
    timelineCollapsed.value = Boolean(cached?.timelineCollapsed);
    artifactWorkspaceVisible.value = false;
    agentOutputPreviewVisible.value = false;
  },
  { immediate: true }
);

watch(
  () => [chatCollapsed.value, isCompactLayout.value] as const,
  ([collapsed, compact]) => {
    mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
      chatCollapsed: collapsed
    });
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

.beeroom-canvas-screen::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  border: 1px solid rgba(148, 163, 184, 0.08);
  pointer-events: none;
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
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
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

.orchestration-canvas-screen {
  min-height: 620px;
}

.orchestration-canvas-board {
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.04), transparent 28%),
    linear-gradient(180deg, rgba(8, 11, 17, 0.985), rgba(7, 10, 15, 0.985));
}

.orchestration-canvas-stage {
  position: relative;
  display: grid;
  grid-template-rows: minmax(0, 1fr) auto;
  width: 100%;
  height: 100%;
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.orchestration-canvas-surface-shell {
  position: relative;
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.orchestration-canvas-empty {
  position: absolute;
  inset: 0;
  gap: 10px;
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.08), transparent 32%),
    linear-gradient(180deg, rgba(8, 11, 17, 0.76), rgba(7, 10, 15, 0.8));
}

.orchestration-canvas-empty small {
  max-width: 360px;
  color: rgba(191, 219, 254, 0.64);
  line-height: 1.6;
}

.orchestration-timeline-shell {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 4;
  display: flex;
  justify-content: center;
  align-items: flex-end;
  pointer-events: none;
  overflow: visible;
}

.orchestration-timeline-shell.collapsed {
  height: 0;
  min-height: 0;
}

.orchestration-timeline-dock {
  position: relative;
  z-index: 1;
  display: grid;
  gap: 0;
  width: calc(100% - 28px);
  margin: 0 14px;
  padding: 18px 16px 16px;
  border-top: 1px solid rgba(148, 163, 184, 0.14);
  background:
    linear-gradient(180deg, rgba(11, 14, 21, 0.92), rgba(8, 10, 15, 0.96)),
    linear-gradient(90deg, rgba(56, 189, 248, 0.04), rgba(245, 158, 11, 0.03));
  box-shadow:
    0 -12px 28px rgba(2, 6, 23, 0.28),
    inset 0 1px 0 rgba(255, 255, 255, 0.03);
  border-radius: 18px 18px 0 0;
  pointer-events: auto;
}

.orchestration-timeline-handle {
  position: absolute;
  left: 50%;
  top: -12px;
  z-index: 3;
  width: 46px;
  height: 22px;
  padding: 0;
  transform: translateX(-50%);
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.36);
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.95), rgba(15, 23, 42, 0.94));
  color: #cbd5e1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  pointer-events: auto;
  opacity: 0.82;
  box-shadow: 0 12px 28px rgba(2, 6, 23, 0.3);
  transition:
    border-color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    background 160ms cubic-bezier(0.22, 1, 0.36, 1),
    color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-timeline-shell.collapsed .orchestration-timeline-handle {
  top: auto;
  bottom: -12px;
  transform: translateX(-50%);
  width: 22px;
  border-radius: 999px 999px 0 0;
}

.orchestration-timeline-handle:hover,
.orchestration-timeline-handle:focus-visible {
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.98), rgba(15, 23, 42, 0.98));
  border-color: rgba(148, 163, 184, 0.56);
  transform: translateX(-50%) translateY(-1px);
  outline: none;
}

.orchestration-timeline-track {
  display: flex;
  gap: 10px;
  overflow: auto;
  padding-bottom: 2px;
}

.orchestration-round-chip {
  display: inline-flex;
  min-width: 152px;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 16px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(15, 23, 42, 0.72);
  color: #dbeafe;
  cursor: pointer;
  transition:
    border-color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    background 160ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-round-chip:hover,
.orchestration-round-chip:focus-visible {
  border-color: rgba(96, 165, 250, 0.42);
  background: rgba(12, 74, 110, 0.26);
  transform: translateY(-1px);
  outline: none;
}

.orchestration-round-chip small {
  color: rgba(191, 219, 254, 0.64);
  font-size: 11px;
  line-height: 1.45;
}

.orchestration-round-chip.active {
  border-color: rgba(56, 189, 248, 0.42);
  background: rgba(12, 74, 110, 0.34);
}

.orchestration-panel-action {
  width: 28px;
  height: 28px;
  border-radius: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: rgba(19, 21, 29, 0.84);
  color: #d1d5db;
  cursor: pointer;
}

.orchestration-panel-action:hover:not(:disabled),
.orchestration-panel-action:focus-visible:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.42);
  background: rgba(30, 41, 59, 0.96);
  color: #e2e8f0;
  outline: none;
}

.orchestration-panel-action[disabled] {
  opacity: 0.55;
  cursor: not-allowed;
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
  transition: background-color 180ms cubic-bezier(0.22, 1, 0.36, 1);
}

.beeroom-canvas-chat-resizer:hover::before,
.beeroom-canvas-chat-resizer:focus-visible::before,
.beeroom-canvas-board.is-chat-resizing .beeroom-canvas-chat-resizer::before {
  background: linear-gradient(180deg, transparent, rgba(96, 165, 250, 0.74), transparent);
}

.beeroom-canvas-chat-resizer:focus-visible {
  outline: none;
}

.beeroom-canvas-workspace-shell {
  display: flex;
  width: 100%;
  min-height: clamp(460px, 68vh, 760px);
  height: clamp(460px, 68vh, 760px);
  overflow: hidden;
  border-radius: 18px;
  background: transparent;
  color: var(--chat-text);
}

.beeroom-canvas-workspace-dialog :deep(.el-dialog) {
  overflow: hidden;
  border-radius: 22px;
}

.beeroom-canvas-workspace-dialog :deep(.el-dialog__header) {
  padding: 18px 20px 0;
}

.beeroom-canvas-workspace-shell :deep(.workspace-panel) {
  flex: 1;
  height: 100%;
}

.beeroom-canvas-workspace-dialog :deep(.el-dialog__body) {
  padding: 14px 18px 18px;
  background: transparent;
}

@media (max-width: 1240px) {
  .orchestration-canvas-board {
    --beeroom-chat-width: 320px;
  }
}

@media (max-width: 960px) {
  .orchestration-canvas-screen {
    min-height: 680px;
  }

  .orchestration-canvas-board {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) auto;
  }

  .orchestration-canvas-board::after {
    display: none;
  }

  .beeroom-canvas-chat-resizer {
    display: none;
  }

  .orchestration-canvas-stage {
    min-height: 420px;
  }

  .orchestration-canvas-board.chat-collapsed {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) 0;
  }
}
</style>
