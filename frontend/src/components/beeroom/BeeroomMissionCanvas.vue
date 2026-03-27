<template>
  <section ref="screenRef" class="beeroom-canvas-screen" :class="{ 'is-empty': !hasSwarmNodes }">
    <div v-if="!hasSwarmNodes" class="beeroom-canvas-empty">
      <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
      <span>{{ t('beeroom.canvas.empty') }}</span>
    </div>

    <div v-else class="beeroom-canvas-layout">
      <div class="beeroom-canvas-board" :class="{ 'chat-collapsed': chatCollapsed }">
        <BeeroomSwarmCanvasPane
          class="beeroom-canvas-pane"
          :group="group"
          :mission="mission"
          :agents="agents"
          :workflow-items-by-task="workflowItemsByTask"
          :workflow-preview-by-task="workflowPreviewByTask"
          :fullscreen="canvasFullscreen"
          @open-agent="emit('open-agent', $event)"
          @toggle-fullscreen="toggleCanvasFullscreen"
        />

        <BeeroomCanvasChatPanel
          :collapsed="chatCollapsed"
          :messages="displayChatMessages"
          :approvals="dispatchApprovals"
          :dispatch-runtime-status="dispatchRuntimeStatus"
          :dispatch-runtime-tone="dispatchRuntimeTone"
          :dispatch-runtime-label="dispatchRuntimeLabel"
          :dispatch-session-id="dispatchSessionId"
          :dispatch-can-stop="dispatchCanStop"
          :dispatch-can-resume="dispatchCanResume"
          :dispatch-approval-busy="dispatchApprovalBusy"
          :composer-text="composerText"
          :composer-target-agent-id="composerTargetAgentId"
          :composer-target-options="composerTargetOptions"
          :composer-sending="composerSending"
          :composer-can-send="composerCanSend"
          :composer-error="composerError"
          :demo-error="demoError"
          :demo-action-disabled="demoActionDisabled"
          :demo-action-label="demoActionLabel"
          :demo-can-cancel="demoCanCancel"
          :resolve-agent-avatar-image-by-agent-id="resolveAgentAvatarImageByAgentId"
          :avatar-label="avatarLabel"
          @update:collapsed="chatCollapsed = $event"
          @update:composer-text="composerText = $event"
          @update:composer-target-agent-id="composerTargetAgentId = $event"
          @clear="clearManualChatHistory"
          @stop="handleDispatchStop"
          @resume="handleDispatchResume"
          @send="handleComposerSend"
          @demo="handleDemoAction"
          @open-agent="emit('open-agent', $event)"
          @approval="handleDispatchApproval($event.decision, $event.approvalId)"
        />
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, toRef } from 'vue';

import BeeroomCanvasChatPanel from '@/components/beeroom/BeeroomCanvasChatPanel.vue';
import BeeroomSwarmCanvasPane from '@/components/beeroom/canvas/BeeroomSwarmCanvasPane.vue';
import { hasBeeroomSwarmNodes } from '@/components/beeroom/canvas/swarmCanvasModel';
import { useBeeroomMissionCanvasRuntime } from '@/components/beeroom/useBeeroomMissionCanvasRuntime';
import { useI18n } from '@/i18n';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  refreshing?: boolean;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'refresh'): void;
}>();

const { t } = useI18n();
const screenRef = ref<HTMLElement | null>(null);
const canvasFullscreen = ref(false);

const groupRef = toRef(props, 'group');
const missionRef = toRef(props, 'mission');
const agentsRef = toRef(props, 'agents');

const hasSwarmNodes = computed(() =>
  hasBeeroomSwarmNodes({
    group: props.group,
    mission: props.mission,
    agents: props.agents
  })
);

const {
  chatCollapsed,
  composerText,
  composerTargetAgentId,
  composerTargetOptions,
  composerSending,
  composerCanSend,
  composerError,
  demoError,
  demoActionDisabled,
  demoActionLabel,
  demoCanCancel,
  dispatchApprovals,
  dispatchApprovalBusy,
  dispatchCanResume,
  dispatchCanStop,
  dispatchRuntimeLabel,
  dispatchRuntimeStatus,
  dispatchRuntimeTone,
  dispatchSessionId,
  displayChatMessages,
  workflowItemsByTask,
  workflowPreviewByTask,
  clearManualChatHistory,
  handleComposerSend,
  handleDispatchApproval,
  handleDispatchResume,
  handleDispatchStop,
  handleDemoAction,
  resolveAgentAvatarImageByAgentId,
  avatarLabel
} = useBeeroomMissionCanvasRuntime({
  group: groupRef,
  mission: missionRef,
  agents: agentsRef,
  t,
  onRefresh: () => emit('refresh')
});

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

onMounted(() => {
  if (typeof document !== 'undefined') {
    document.addEventListener('fullscreenchange', refreshCanvasFullscreen);
    refreshCanvasFullscreen();
  }
});

onBeforeUnmount(() => {
  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', refreshCanvasFullscreen);
  }
});
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
  background:
    radial-gradient(circle at top left, rgba(99, 102, 241, 0.08), transparent 42%),
    radial-gradient(circle at bottom right, rgba(56, 189, 248, 0.06), transparent 48%),
    linear-gradient(180deg, rgba(6, 8, 12, 0.995), rgba(7, 9, 14, 0.992));
  box-shadow:
    0 22px 54px rgba(0, 0, 0, 0.36),
    inset 0 0 0 1px rgba(255, 255, 255, 0.03),
    inset 0 1px 0 rgba(255, 255, 255, 0.05);
}

.beeroom-canvas-screen:fullscreen {
  width: 100vw;
  height: 100vh;
  min-height: 100vh;
  border-radius: 0;
  border: 0;
}

.beeroom-canvas-screen::before {
  content: '';
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(148, 163, 184, 0.08) 1px, transparent 1px),
    linear-gradient(90deg, rgba(148, 163, 184, 0.08) 1px, transparent 1px),
    linear-gradient(rgba(148, 163, 184, 0.13) 1px, transparent 1px),
    linear-gradient(90deg, rgba(148, 163, 184, 0.13) 1px, transparent 1px);
  background-size: 40px 40px, 40px 40px, 200px 200px, 200px 200px;
  background-position: 0 0, 0 0, -1px -1px, -1px -1px;
  opacity: 0.36;
  pointer-events: none;
}

.beeroom-canvas-screen::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  border: 1px solid rgba(255, 255, 255, 0.04);
  box-shadow:
    inset 0 0 0 1px rgba(255, 255, 255, 0.02),
    inset 0 0 36px rgba(15, 23, 42, 0.34);
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
  background: linear-gradient(180deg, rgba(8, 11, 17, 0.98), rgba(7, 9, 14, 0.975));
  box-shadow:
    inset 0 0 0 1px rgba(255, 255, 255, 0.04),
    inset 0 1px 0 rgba(255, 255, 255, 0.05),
    0 20px 38px rgba(0, 0, 0, 0.26);
  transition: grid-template-columns var(--beeroom-motion-slow) var(--beeroom-ease-standard);
}

.beeroom-canvas-board::before {
  content: '';
  position: absolute;
  inset: 0;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.04), transparent 70px);
  pointer-events: none;
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

  .beeroom-canvas-board.chat-collapsed {
    grid-template-columns: 1fr;
    grid-template-rows: minmax(0, 1fr) 0;
  }
}
</style>
