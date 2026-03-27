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
  position: relative;
  min-height: 620px;
  height: 100%;
}

.beeroom-canvas-screen:fullscreen {
  padding: 18px;
  background:
    radial-gradient(circle at top left, rgba(59, 130, 246, 0.12), transparent 34%),
    radial-gradient(circle at bottom right, rgba(245, 158, 11, 0.1), transparent 36%),
    linear-gradient(180deg, rgba(248, 250, 252, 1), rgba(241, 245, 249, 1));
}

.beeroom-canvas-layout {
  height: 100%;
}

.beeroom-canvas-board {
  height: 100%;
  min-height: 620px;
  display: flex;
  overflow: hidden;
  border-radius: 34px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.96), rgba(248, 250, 252, 0.98));
  box-shadow:
    0 28px 58px rgba(15, 23, 42, 0.08),
    inset 0 1px 0 rgba(255, 255, 255, 0.7);
}

.beeroom-canvas-pane {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 620px;
}

.beeroom-canvas-empty {
  min-height: 620px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 14px;
  border-radius: 34px;
  border: 1px dashed rgba(148, 163, 184, 0.28);
  color: rgba(100, 116, 139, 0.92);
  background:
    radial-gradient(circle at top left, rgba(59, 130, 246, 0.08), transparent 34%),
    radial-gradient(circle at bottom right, rgba(245, 158, 11, 0.08), transparent 36%),
    linear-gradient(180deg, rgba(248, 250, 252, 0.98), rgba(241, 245, 249, 0.96));
}

.beeroom-canvas-empty i {
  font-size: 30px;
  color: rgba(96, 165, 250, 0.82);
}
</style>
