<template>
  <section ref="screenRef" class="beeroom-canvas-screen orchestration-canvas-screen">
    <div class="beeroom-canvas-layout">
      <div
        ref="boardRef"
        class="beeroom-canvas-board orchestration-canvas-board"
        :class="{
          'chat-collapsed': chatCollapsed,
          'is-chat-resizing': isChatResizing,
          'is-timeline-resizing': isTimelineResizing
        }"
        :style="boardStyle"
      >
        <div class="orchestration-canvas-stage">
          <div class="orchestration-canvas-surface-shell">
            <BeeroomSwarmCanvasPane
              class="beeroom-canvas-pane"
              :group="group"
              :mission="null"
              :agents="agents"
              :layout-mode="layoutMode"
              :dispatch-preview="dispatchPreview"
              :subagents-by-task="{}"
              :mother-workflow-items="motherWorkflowItems"
              :workflow-items-by-task="workflowItemsByTask"
              :workflow-preview-by-task="workflowPreviewByTask"
              :fullscreen="canvasFullscreen"
              :external-scope-key="canvasScopeKey"
              :external-projection="canvasProjection"
              :external-has-nodes="isReady"
              :show-minimap="false"
              :reveal-replay-key="revealReplayKey"
              @open-agent="emit('open-agent', $event)"
              @preview-node-output="handleAgentOutputPreview"
              @preview-artifact="handleArtifactFilePreview"
              @update:layout-mode="handleLayoutModeChange"
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
            <div
              v-if="!timelineCollapsed"
              class="orchestration-timeline-dock"
              :class="{ 'is-resizing': isTimelineResizing }"
              :style="timelineDockStyle"
            >
              <div
                class="orchestration-timeline-resizer"
                role="separator"
                aria-orientation="horizontal"
                :aria-label="t('orchestration.timeline.title')"
                tabindex="0"
                @pointerdown="handleTimelineResizePointerDown"
                @dblclick.prevent="resetTimelineHeight"
                @keydown.up.prevent="nudgeTimelineHeight(24)"
                @keydown.down.prevent="nudgeTimelineHeight(-24)"
              ></div>
              <div class="orchestration-timeline-rail">
                <div
                  class="orchestration-timeline-track orchestration-timeline-tree"
                  :aria-label="t('orchestration.timeline.title')"
                >
                  <div
                    v-if="!timelineLayout.items.length"
                    class="orchestration-timeline-empty"
                  >
                    {{ t('orchestration.timeline.empty') }}
                  </div>
                  <div
                    v-else
                    class="orchestration-timeline-tree-grid"
                    :style="{
                      '--timeline-lanes': String(timelineLayout.laneCount),
                      '--timeline-columns': String(timelineLayout.columnCount)
                    }"
                  >
                    <div
                      v-for="laneIndex in timelineLayout.laneCount"
                      :key="`timeline-lane-${laneIndex}`"
                      class="orchestration-timeline-lane-rail"
                      :style="{ '--lane': String(laneIndex) }"
                      aria-hidden="true"
                    ></div>
                    <div
                      v-for="connector in timelineLayout.connectors"
                      :key="connector.id"
                      :class="connector.className"
                      :style="connector.style"
                      aria-hidden="true"
                    ></div>
                    <template v-for="item in timelineLayout.items" :key="item.id">
                      <button
                        v-if="item.type === 'run'"
                        class="orchestration-run-chip"
                        :class="{
                          current: item.current,
                          active: item.active,
                          branched: item.branchFromRoundIndex > 0,
                          'is-disabled': runtimeLocked && !item.current
                        }"
                        type="button"
                        :disabled="runtimeLocked && !item.current"
                        :style="{
                          '--lane': String(item.lane + 1),
                          '--column': String(item.column)
                        }"
                        :title="item.title"
                        :aria-label="item.title"
                        @click="handleRunChipClick(item)"
                        @contextmenu.prevent.stop="openRunContextMenu($event, item)"
                      >
                        <span class="orchestration-run-chip-icon" aria-hidden="true">
                          <i class="fa-solid fa-code-branch" v-if="item.branchDepth > 0"></i>
                          <i class="fa-solid fa-diagram-project" v-else></i>
                        </span>
                      </button>
                      <button
                        v-else
                        class="orchestration-round-chip"
                        :class="{
                          selected: item.selected,
                          active: item.active,
                          'is-pending': item.pending,
                          'is-preview': item.preview,
                          'is-disabled': runtimeLocked
                        }"
                        type="button"
                        :disabled="runtimeLocked"
                        :style="{
                          '--lane': String(item.lane + 1),
                          '--column': String(item.column)
                        }"
                        :title="t('orchestration.timeline.round', { round: item.roundIndex })"
                        :aria-label="t('orchestration.timeline.round', { round: item.roundIndex })"
                        :aria-current="item.currentRun && (item.active || item.pending) ? 'step' : undefined"
                        @click="handleRoundChipClick(item)"
                        @contextmenu.prevent.stop="openRoundContextMenu($event, item)"
                      >
                        <span class="orchestration-round-chip-node" aria-hidden="true">
                          <span class="orchestration-round-chip-index">{{ item.roundIndex }}</span>
                        </span>
                      </button>
                    </template>
                  </div>
                </div>
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
            <div
              v-if="roundContextMenu.visible"
              class="orchestration-round-context-menu"
              :style="roundContextMenuStyle"
              @contextmenu.prevent
            >
              <button
                class="orchestration-round-context-menu__item"
                type="button"
                @click="handleDeleteRoundTail"
              >
                <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
                <span>
                  {{
                    t(
                      roundContextMenu.kind === 'run'
                        ? 'orchestration.timeline.deleteBranchAfter'
                        : 'orchestration.timeline.deleteAfter'
                    )
                  }}
                </span>
              </button>
            </div>
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
          :composer-text="''"
          :composer-target-agent-id="motherAgentId"
          :composer-target-options="composerTargetOptions"
          :composer-sending="composerSending"
          :composer-can-send="false"
        :composer-disabled="true"
        :composer-error="''"
        :title="group?.name || t('beeroom.canvas.chatTitle')"
          :artifacts-enabled="Boolean(activeArtifactWorkspace)"
          :show-artifacts-button="false"
          :show-clear-button="false"
          :show-composer="false"
          :resolve-message-avatar-image="resolveMessageAvatarImage"
          :avatar-label="avatarLabel"
          @update:collapsed="chatCollapsed = $event"
          @update:composer-text="emit('update:composer-text', $event)"
          @send="emit('send')"
          @open-agent="emit('open-agent', $event)"
        >
          <template #head-main>
            <span
              class="orchestration-panel-status-lamp"
              :class="{ 'is-active': isActive, 'is-busy': isBusy }"
              role="status"
              :title="statusLampLabel"
              :aria-label="statusLampLabel"
            ></span>
          </template>
          <template #empty>
            <div class="orchestration-chat-empty">
              <span>{{ t('orchestration.chat.empty') }}</span>
            </div>
          </template>
          <template #head-actions>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('orchestration.action.create')"
              :aria-label="t('orchestration.action.create')"
              :disabled="initializing || runtimeLocked"
              @click="emit('create-run')"
            >
              <i class="fa-solid fa-plus" aria-hidden="true"></i>
            </button>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('orchestration.action.exportLogs')"
              :aria-label="t('orchestration.action.exportLogs')"
              :disabled="!isReady"
              @click="emit('export-logs')"
            >
              <i class="fa-solid fa-download" aria-hidden="true"></i>
            </button>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="t('orchestration.action.history')"
              :aria-label="t('orchestration.action.history')"
              :disabled="historyLoading || runtimeLocked"
              @click="emit('open-history')"
            >
              <i class="fa-solid fa-clock-rotate-left" aria-hidden="true"></i>
            </button>
            <button
              class="beeroom-canvas-icon-btn orchestration-panel-action"
              type="button"
              :title="isActive ? t('common.close') : t('common.enable')"
              :aria-label="isActive ? t('common.close') : t('common.enable')"
              :disabled="!isReady || (isActive && isBusy)"
              @click="isActive ? emit('exit-run') : emit('start-run')"
            >
              <i class="fa-solid" :class="isActive ? 'fa-link-slash' : 'fa-plug-circle-check'" aria-hidden="true"></i>
            </button>
          </template>
          <template #footer>
            <section class="orchestration-side-control">
              <div class="orchestration-side-control-head">
                <div class="orchestration-side-control-head-main">
                  <span
                    v-if="currentPanelRoundIndex > 0"
                    class="orchestration-side-control-round"
                  >
                    {{ t('orchestration.panel.roundTag', { round: currentPanelRoundIndex }) }}
                  </span>
                  <span class="orchestration-side-control-title">{{ t('orchestration.panel.situation') }}</span>
                </div>
                <div class="orchestration-side-control-head-actions">
                  <button
                    class="beeroom-canvas-icon-btn orchestration-panel-action orchestration-side-control-icon-btn"
                    type="button"
                    :title="t('common.edit')"
                    :aria-label="t('common.edit')"
                    :disabled="!isReady || runtimeLocked"
                    @click="emit('open-situation')"
                  >
                    <i class="fa-solid fa-pen-to-square" aria-hidden="true"></i>
                  </button>
                  <button
                    v-if="showBranchAction"
                    class="beeroom-canvas-icon-btn orchestration-panel-action orchestration-side-control-icon-btn"
                    type="button"
                    :title="t('orchestration.action.branch')"
                    :aria-label="t('orchestration.action.branch')"
                    :disabled="branchActionDisabled"
                    @click="emit('branch-run')"
                  >
                    <i class="fa-solid fa-code-branch" aria-hidden="true"></i>
                  </button>
                  <button
                    class="beeroom-canvas-icon-btn orchestration-panel-action orchestration-side-control-icon-btn"
                    :class="{ 'is-stop': composerSending }"
                    type="button"
                    :title="
                      composerSending
                        ? t('common.stop')
                        : currentRoundShowsNextAction
                          ? t('orchestration.action.nextRound')
                          : t('orchestration.action.startRound')
                    "
                    :aria-label="
                      composerSending
                        ? t('common.stop')
                        : currentRoundShowsNextAction
                          ? t('orchestration.action.nextRound')
                          : t('orchestration.action.startRound')
                    "
                    :disabled="actionDisabled"
                    @click="emit('trigger-round')"
                  >
                    <i
                      class="fa-solid"
                      :class="
                        composerSending
                          ? 'fa-stop'
                          : currentRoundShowsNextAction
                            ? 'fa-forward-step'
                            : 'fa-play'
                      "
                      aria-hidden="true"
                    ></i>
                  </button>
                </div>
              </div>
              <textarea
                class="orchestration-side-control-textarea"
                :value="currentSituation"
                :disabled="!isReady || runtimeLocked"
                :placeholder="t('orchestration.canvas.noSituation')"
                rows="7"
                @input="emit('update:current-situation', ($event.target as HTMLTextAreaElement).value)"
                @change="emit('commit:current-situation')"
                @blur="emit('commit:current-situation')"
              ></textarea>
            </section>
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
          :initial-focus-path="activeArtifactWorkspace.path"
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

    <OrchestrationArtifactPreviewDialog
      v-model:visible="artifactPreviewVisible"
      :title="artifactPreviewTitle"
      :path="artifactPreviewPath"
      :content="artifactPreviewContent"
      :loading="artifactPreviewLoading"
      :error="artifactPreviewError"
      @download="handleArtifactPreviewDownload"
    />
  </section>
</template>

<script setup lang="ts">
import { computed, onActivated, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { downloadWunderWorkspaceFile, fetchWunderWorkspaceContent } from '@/api/workspace';
import BeeroomAgentOutputPreviewDialog from '@/components/beeroom/BeeroomAgentOutputPreviewDialog.vue';
import BeeroomCanvasChatPanel from '@/components/beeroom/BeeroomCanvasChatPanel.vue';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import BeeroomSwarmCanvasPane from '@/components/beeroom/canvas/BeeroomSwarmCanvasPane.vue';
import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import type { BeeroomTaskWorkflowPreview, BeeroomWorkflowItem } from '@/components/beeroom/beeroomTaskWorkflow';
import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import OrchestrationArtifactPreviewDialog from '@/components/orchestration/OrchestrationArtifactPreviewDialog.vue';
import type {
  OrchestrationArtifactCard,
  OrchestrationHistoryItem,
  OrchestrationRound
} from '@/components/orchestration/orchestrationRuntimeState';
import {
  buildOrchestrationCanvasProjection,
  buildOrchestrationCanvasScopeKey
} from '@/components/orchestration/orchestrationCanvasModel';
import {
  buildOrchestrationTimelineLayout,
  type TimelineLayout,
  type TimelineRunItem,
  type TimelineRoundItem
} from '@/components/orchestration/orchestrationTimelineLayout';
import { roundIsFinalized } from '@/components/orchestration/orchestrationRoundStateStability';
import { useI18n } from '@/i18n';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

const props = defineProps<{
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  rounds: OrchestrationRound[];
  activeRound: OrchestrationRound | null;
  activeRoundMissions: BeeroomMission[];
  artifactCards: OrchestrationArtifactCard[];
  historyItems: OrchestrationHistoryItem[];
  currentOrchestrationId: string;
  visibleWorkers: BeeroomMember[];
  visibleChatMessages: MissionChatMessage[];
  motherWorkflowItems: BeeroomWorkflowItem[];
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  motherAgentId: string;
  motherName: string;
  motherSessionId: string;
  runId: string;
  active?: boolean;
  dispatchPreview: BeeroomSwarmDispatchPreview | null;
  composerText: string;
  composerSending: boolean;
  canSend: boolean;
  composerDisabled: boolean;
  currentSituation: string;
  nextRoundReady: boolean;
  initializing: boolean;
  historyLoading: boolean;
  isActive: boolean;
  isBusy: boolean;
  isReady: boolean;
  runtimeLocked: boolean;
  groupDescription: string;
  resolveWorkerOutputs: (agentId: string) => MissionChatMessage[];
  resolveWorkerThreadSessionId: (agentId: string) => string;
  resolveMessageAvatarImage: (message: MissionChatMessage) => string;
  avatarLabel: (value: unknown) => string;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'update:composer-text', value: string): void;
  (event: 'update:current-situation', value: string): void;
  (event: 'commit:current-situation'): void;
  (event: 'send'): void;
  (event: 'trigger-round'): void;
  (event: 'branch-run'): void;
  (event: 'create-run'): void;
  (event: 'start-run'): void;
  (event: 'exit-run'): void;
  (event: 'open-history'): void;
  (event: 'open-situation'): void;
  (event: 'export-logs'): void;
  (event: 'select-round', roundId: string): void;
  (event: 'restore-run', payload: { orchestrationId: string; roundIndex?: number; preview?: boolean }): void;
  (event: 'delete-round-tail', payload: { orchestrationId: string; roundIndex: number }): void;
}>();

const { t } = useI18n();

const orchestrationTimelineDebug = (event: string, payload?: unknown) => {
  chatDebugLog('orchestration-timeline', event, payload);
};
const screenRef = ref<HTMLElement | null>(null);
const boardRef = ref<HTMLElement | null>(null);
const canvasFullscreen = ref(false);
const boardWidth = ref(0);
const boardHeight = ref(0);
const chatWidth = ref(352);
const isChatResizing = ref(false);
const chatCollapsed = ref(false);
const timelineCollapsed = ref(false);
const timelineHeight = ref(0);
const isTimelineResizing = ref(false);
const layoutMode = ref<'horizontal' | 'vertical'>('horizontal');
const revealReplayKey = ref(0);
const roundContextMenu = ref<{
  visible: boolean;
  x: number;
  y: number;
  orchestrationId: string;
  roundIndex: number;
  kind: 'round' | 'run';
}>({
  visible: false,
  x: 0,
  y: 0,
  orchestrationId: '',
  roundIndex: 0,
  kind: 'round'
});
const artifactWorkspaceVisible = ref(false);
const selectedArtifactAgentId = ref('');
const agentOutputPreviewVisible = ref(false);
const agentOutputPreviewAgentId = ref('');
const agentOutputPreviewTitle = ref('');
const agentOutputPreviewRoleLabel = ref('');
const agentOutputPreviewStatusLabel = ref('');
const artifactPreviewVisible = ref(false);
const artifactPreviewLoading = ref(false);
const artifactPreviewError = ref('');
const artifactPreviewTitle = ref('');
const artifactPreviewPath = ref('');
const artifactPreviewContent = ref('');
const artifactPreviewAgentId = ref('');
const artifactPreviewContainerId = ref(1);

const DEFAULT_CHAT_WIDTH = 352;
const MIN_CHAT_WIDTH = 316;
const MAX_CHAT_WIDTH = 700;
const DEFAULT_TIMELINE_HEIGHT = 152;
const MIN_TIMELINE_HEIGHT = 108;
const MAX_TIMELINE_HEIGHT = 420;
const MOBILE_CHAT_BREAKPOINT = 960;
const AGENT_OUTPUT_PREVIEW_LIMIT = 6;

let boardResizeObserver: ResizeObserver | null = null;
let activeResizePointerId: number | null = null;
let activeTimelineResizePointerId: number | null = null;
let dragStartClientX = 0;
let dragStartChatWidth = DEFAULT_CHAT_WIDTH;
let dragStartClientY = 0;
let dragStartTimelineHeight = DEFAULT_TIMELINE_HEIGHT;

const requestCanvasRevealReplay = () => {
  revealReplayKey.value += 1;
};

const canvasScopeKey = computed(() =>
  buildOrchestrationCanvasScopeKey(
    props.runId,
    props.activeRound?.id || '',
    String(props.currentOrchestrationId || '').trim()
  )
);

const canvasLayoutScopeKey = computed(() =>
  buildOrchestrationCanvasScopeKey(props.runId, '', String(props.currentOrchestrationId || '').trim())
);

const canvasProjection = computed(() =>
  props.isReady
    ? buildOrchestrationCanvasProjection({
        group: props.group,
        agents: props.agents,
        layoutMode: layoutMode.value,
        motherAgentId: props.motherAgentId,
        motherName: props.motherName,
        motherSessionId: props.motherSessionId,
        activeRound: props.activeRound,
        activeRoundMissions: props.activeRoundMissions,
        visibleWorkers: props.visibleWorkers,
        artifactCards: props.artifactCards,
        motherWorkflowItems: props.motherWorkflowItems,
        workflowItemsByTask: props.workflowItemsByTask,
        workflowPreviewByTask: props.workflowPreviewByTask,
        dispatchPreview: props.dispatchPreview,
        resolveWorkerOutputs: props.resolveWorkerOutputs,
        resolveWorkerThreadSessionId: props.resolveWorkerThreadSessionId,
        selectedNodeId: '',
        nodePositionOverrides: {},
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

const roundIsCompleted = (round: { finalizedAt?: unknown } | null | undefined) =>
  roundIsFinalized({ finalizedAt: Number(round?.finalizedAt || 0) });
const sortedRounds = computed(() =>
  [...(Array.isArray(props.rounds) ? props.rounds : [])].sort(
    (left, right) =>
      Number(left.index || 0) - Number(right.index || 0) ||
      String(left.id || '').localeCompare(String(right.id || ''))
  )
);
const formalRounds = computed(() => sortedRounds.value.filter((round) => roundIsCompleted(round)));
const latestFormalRound = computed(() => formalRounds.value[formalRounds.value.length - 1] || null);
const frontierPreparedRound = computed(() => {
  const targetIndex = latestFormalRound.value ? Math.max(1, Number(latestFormalRound.value.index || 0) + 1) : 1;
  return sortedRounds.value.find(
    (round) => Number(round.index || 0) === targetIndex && !roundIsCompleted(round)
  ) || null;
});
const latestRenderableRound = computed(
  () => frontierPreparedRound.value || latestFormalRound.value || sortedRounds.value[sortedRounds.value.length - 1] || null
);
const currentPanelRoundIndex = computed(() => {
  if (!props.isReady) return 0;
  const activeRoundIndex = Math.max(0, Number(props.activeRound?.index || 0));
  if (activeRoundIndex > 0) return activeRoundIndex;
  const fallbackRoundIndex = Math.max(0, Number(latestRenderableRound.value?.index || 0));
  return fallbackRoundIndex > 0 ? fallbackRoundIndex : 1;
});

const timelineLayout = computed<TimelineLayout>(() => {
  const layout = buildOrchestrationTimelineLayout({
    historyItems: Array.isArray(props.historyItems) ? props.historyItems : [],
    currentOrchestrationId: String(props.currentOrchestrationId || '').trim(),
    rounds: Array.isArray(props.rounds) ? props.rounds : [],
    activeRoundId: String(props.activeRound?.id || '').trim(),
    isActive: props.isActive,
    isBusy: props.isBusy,
      currentRunFallback: {
        runId: String(props.runId || props.currentOrchestrationId || '').trim(),
        status: props.isActive ? 'active' : 'closed',
        latestRoundIndex: Math.max(1, Number(latestFormalRound.value?.index || 0) || 1),
        groupId: String(props.group?.group_id || props.group?.hive_id || '').trim(),
        motherAgentId: String(props.motherAgentId || '').trim(),
        motherAgentName: String(props.motherName || '').trim(),
        motherSessionId: String(props.motherSessionId || '').trim()
      }
  });

  if (isChatDebugEnabled()) {
    orchestrationTimelineDebug('timeline-layout-input', {
      currentOrchestrationId: String(props.currentOrchestrationId || '').trim(),
      activeRoundId: String(props.activeRound?.id || '').trim(),
      rounds: (Array.isArray(props.rounds) ? props.rounds : []).map((round) => ({
        id: String(round.id || '').trim(),
        index: Number(round.index || 0),
        orchestrationId: String((round as { orchestrationId?: unknown }).orchestrationId || props.currentOrchestrationId || '').trim(),
        hasUserMessage: Boolean(String((round as { userMessage?: unknown }).userMessage || '').trim())
      })),
      historyItems: layout.debugRuns.map((item) => ({
        orchestrationId: String(item.orchestrationId || '').trim(),
        parentOrchestrationId: String(item.parentOrchestrationId || '').trim(),
        branchRootOrchestrationId: String(item.branchRootOrchestrationId || '').trim(),
        branchFromRoundIndex: Number(item.branchFromRoundIndex || 0),
        latestRoundIndex: Number(item.latestRoundIndex || 0),
        branchDepth: Number(item.branchDepth || 0),
        status: String(item.status || '').trim()
      }))
    });
  }

  return {
    items: layout.items,
    connectors: layout.connectors,
    laneCount: layout.laneCount,
    columnCount: layout.columnCount,
    debugRuns: layout.debugRuns
  };
});

const statusLampLabel = computed(() => {
  if (props.isBusy) {
    return t('orchestration.run.busy');
  }
  return props.isActive ? t('orchestration.run.active') : t('orchestration.run.idle');
});

const actionDisabled = computed(() => {
  if (props.composerSending) return false;
  if (!props.isReady || !props.isActive) return true;
  return false;
});

const currentRoundShowsNextAction = computed(
  () => props.nextRoundReady || roundIsCompleted(props.activeRound)
);

const showBranchAction = computed(() => {
  if (!props.isReady || !props.isActive) return false;
  return Boolean(roundIsCompleted(props.activeRound));
});

const branchActionDisabled = computed(() => {
  if (props.runtimeLocked) return true;
  if (!props.isReady || !props.isActive) return true;
  return !showBranchAction.value;
});

const activeArtifactWorkspace = computed(() => {
  const agentId = String(selectedArtifactAgentId.value || '').trim();
  if (!agentId) return null;
  const card = props.artifactCards.find((item) => String(item.agentId || '').trim() === agentId) || null;
  const member = resolveArtifactMember(agentId);
  if (!card || !member) return null;
  const containerId = Number.parseInt(String(member.sandbox_container_id ?? 1), 10) || 1;
  return {
    agentId,
    containerId,
    path: String(card.path || '').trim(),
    title: `${card.agentName} · ${card.path}`
  };
});

const resolveArtifactMember = (agentId: string) =>
  props.visibleWorkers.find((item) => String(item.agent_id || '').trim() === agentId) ||
  props.agents.find((item) => String(item.agent_id || '').trim() === agentId) ||
  null;

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

const getTimelineHeightBounds = () => {
  const currentBoardHeight = Math.max(
    0,
    Math.round(boardHeight.value || boardRef.value?.clientHeight || screenRef.value?.clientHeight || 0)
  );
  const maxHeight = Math.max(
    MIN_TIMELINE_HEIGHT,
    Math.min(
      MAX_TIMELINE_HEIGHT,
      currentBoardHeight > 0 ? currentBoardHeight - 136 : DEFAULT_TIMELINE_HEIGHT
    )
  );
  return {
    min: MIN_TIMELINE_HEIGHT,
    max: maxHeight
  };
};

const clampTimelineHeight = (value: number) => {
  const bounds = getTimelineHeightBounds();
  return Math.max(bounds.min, Math.min(bounds.max, Math.round(value || DEFAULT_TIMELINE_HEIGHT)));
};

const resolvedTimelineHeight = computed(() =>
  clampTimelineHeight(timelineHeight.value || DEFAULT_TIMELINE_HEIGHT)
);

const boardStyle = computed(() => ({
  '--beeroom-chat-width': `${resolvedChatWidth.value}px`
}));

const timelineDockStyle = computed(() => ({
  '--orchestration-timeline-height': `${resolvedTimelineHeight.value}px`
}));

const roundContextMenuStyle = computed(() => ({
  left: `${Math.max(8, Math.round(roundContextMenu.value.x || 0))}px`,
  top: `${Math.max(8, Math.round(roundContextMenu.value.y || 0))}px`
}));

const showChatResizer = computed(() => !chatCollapsed.value && !isCompactLayout.value);

const closeRoundContextMenu = () => {
  roundContextMenu.value = {
    visible: false,
    x: 0,
    y: 0,
    orchestrationId: '',
    roundIndex: 0,
    kind: 'round'
  };
};

const handleRunChipClick = (item: TimelineRunItem) => {
  if (props.runtimeLocked && !item.current) return;
  if (item.current) return;
  emit('restore-run', { orchestrationId: item.id.slice(4) });
};

const handleRoundChipClick = (item: TimelineRoundItem) => {
  if (props.runtimeLocked) return;
  if (item.currentRun) {
    if (item.preview) {
      emit('restore-run', {
        orchestrationId: item.orchestrationId,
        roundIndex: item.roundIndex,
        preview: true
      });
      return;
    }
    emit('select-round', item.roundId);
    return;
  }
  emit('restore-run', {
    orchestrationId: item.orchestrationId,
    roundIndex: item.roundIndex,
    preview: item.preview
  });
};

const openRoundContextMenu = (event: MouseEvent, item: TimelineRoundItem) => {
  if (props.runtimeLocked) {
    closeRoundContextMenu();
    return;
  }
  if (item.preview) {
    closeRoundContextMenu();
    return;
  }
  if (!item?.orchestrationId || item.orchestrationId !== props.currentOrchestrationId) {
    closeRoundContextMenu();
    return;
  }
  const shellRect = screenRef.value?.getBoundingClientRect();
  roundContextMenu.value = {
    visible: true,
    x: Math.round(event.clientX - (shellRect?.left || 0)),
    y: Math.round(event.clientY - (shellRect?.top || 0)),
    orchestrationId: item.orchestrationId,
    roundIndex: item.roundIndex,
    kind: 'round'
  };
};

const openRunContextMenu = (event: MouseEvent, item: TimelineRunItem) => {
  if (props.runtimeLocked) {
    closeRoundContextMenu();
    return;
  }
  const orchestrationId = String(item.id || '').replace(/^run:/, '').trim();
  const roundIndex = Math.max(0, Number(item.branchFromRoundIndex || 0));
  if (!orchestrationId || roundIndex <= 0) {
    closeRoundContextMenu();
    return;
  }
  const shellRect = screenRef.value?.getBoundingClientRect();
  roundContextMenu.value = {
    visible: true,
    x: Math.round(event.clientX - (shellRect?.left || 0)),
    y: Math.round(event.clientY - (shellRect?.top || 0)),
    orchestrationId,
    roundIndex,
    kind: 'run'
  };
};

const handleDeleteRoundTail = () => {
  const orchestrationId = String(roundContextMenu.value.orchestrationId || '').trim();
  const roundIndex = Math.max(1, Number(roundContextMenu.value.roundIndex || 1));
  closeRoundContextMenu();
  if (!orchestrationId) return;
  emit('delete-round-tail', { orchestrationId, roundIndex });
};

const toggleTimelineCollapsed = () => {
  timelineCollapsed.value = !timelineCollapsed.value;
  mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
    timelineCollapsed: timelineCollapsed.value,
    timelineHeight: resolvedTimelineHeight.value
  });
};

const syncBoardMetrics = () => {
  const rect = boardRef.value?.getBoundingClientRect();
  const width = Math.round(rect?.width || boardRef.value?.clientWidth || 0);
  const height = Math.round(rect?.height || boardRef.value?.clientHeight || 0);
  if (width > 0) {
    boardWidth.value = width;
  }
  if (height > 0) {
    boardHeight.value = height;
  }
};

const persistChatWidth = () => {
  mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
    chatWidth: resolvedChatWidth.value,
    chatCollapsed: chatCollapsed.value
  });
};

const persistTimelineState = () => {
  mergeBeeroomMissionCanvasState(canvasScopeKey.value, {
    timelineCollapsed: timelineCollapsed.value,
    timelineHeight: resolvedTimelineHeight.value
  });
};

const persistLayoutMode = () => {
  mergeBeeroomMissionCanvasState(canvasLayoutScopeKey.value, {
    layoutMode: layoutMode.value
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

const applyTimelineHeight = (value: number, options: { persist?: boolean } = {}) => {
  const nextHeight = clampTimelineHeight(value);
  if (nextHeight === timelineHeight.value) {
    if (options.persist) {
      persistTimelineState();
    }
    return;
  }
  timelineHeight.value = nextHeight;
  if (options.persist) {
    persistTimelineState();
  }
};

const resetChatWidth = () => {
  applyChatWidth(DEFAULT_CHAT_WIDTH, { persist: true });
};

const resetTimelineHeight = () => {
  applyTimelineHeight(DEFAULT_TIMELINE_HEIGHT, { persist: true });
};

const handleLayoutModeChange = (value: 'horizontal' | 'vertical') => {
  layoutMode.value = value === 'vertical' ? 'vertical' : 'horizontal';
  persistLayoutMode();
};

const nudgeChatWidth = (delta: number) => {
  applyChatWidth((chatWidth.value || resolvedChatWidth.value || DEFAULT_CHAT_WIDTH) + delta, {
    persist: true
  });
};

const nudgeTimelineHeight = (delta: number) => {
  applyTimelineHeight((timelineHeight.value || resolvedTimelineHeight.value || DEFAULT_TIMELINE_HEIGHT) + delta, {
    persist: true
  });
};

const stopChatResize = () => {
  activeResizePointerId = null;
  if (!isChatResizing.value) return;
  isChatResizing.value = false;
  persistChatWidth();
};

const stopTimelineResize = () => {
  activeTimelineResizePointerId = null;
  if (!isTimelineResizing.value) return;
  isTimelineResizing.value = false;
  persistTimelineState();
};

const handleGlobalPointerDown = (event: PointerEvent) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('.orchestration-round-context-menu')) {
    return;
  }
  closeRoundContextMenu();
};

const handleChatResizePointerMove = (event: PointerEvent) => {
  if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
  applyChatWidth(dragStartChatWidth + (dragStartClientX - event.clientX));
};

const handleTimelineResizePointerMove = (event: PointerEvent) => {
  if (activeTimelineResizePointerId === null || event.pointerId !== activeTimelineResizePointerId) return;
  applyTimelineHeight(dragStartTimelineHeight + (dragStartClientY - event.clientY));
};

const handleChatResizePointerUp = (event: PointerEvent) => {
  if (activeResizePointerId === null || event.pointerId !== activeResizePointerId) return;
  stopChatResize();
};

const handleTimelineResizePointerUp = (event: PointerEvent) => {
  if (activeTimelineResizePointerId === null || event.pointerId !== activeTimelineResizePointerId) return;
  stopTimelineResize();
};

const handleChatResizePointerDown = (event: PointerEvent) => {
  if (event.button !== 0 || !showChatResizer.value) return;
  syncBoardMetrics();
  activeResizePointerId = event.pointerId;
  dragStartClientX = event.clientX;
  dragStartChatWidth = resolvedChatWidth.value;
  isChatResizing.value = true;
  const target = event.currentTarget as HTMLElement | null;
  target?.setPointerCapture?.(event.pointerId);
  event.preventDefault();
};

const handleTimelineResizePointerDown = (event: PointerEvent) => {
  if (event.button !== 0 || timelineCollapsed.value) return;
  syncBoardMetrics();
  activeTimelineResizePointerId = event.pointerId;
  dragStartClientY = event.clientY;
  dragStartTimelineHeight = resolvedTimelineHeight.value;
  isTimelineResizing.value = true;
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

const handleArtifactFilePreview = async (payload: {
  nodeId: string;
  item: {
    path?: string;
    name?: string;
    previewable?: boolean;
  };
}) => {
  const itemPath = String(payload?.item?.path || '').trim();
  const itemName = String(payload?.item?.name || '').trim();
  if (!itemPath || payload?.item?.previewable !== true) {
    return;
  }
  const nodeId = String(payload?.nodeId || '').trim();
  const agentId = nodeId.replace(/^agent:/, '').trim();
  if (!agentId) return;
  const member = resolveArtifactMember(agentId);
  if (!member) return;
  const containerId = Number.parseInt(String(member.sandbox_container_id ?? 1), 10) || 1;

  artifactPreviewTitle.value = itemName || itemPath;
  artifactPreviewPath.value = itemPath;
  artifactPreviewContent.value = '';
  artifactPreviewError.value = '';
  artifactPreviewLoading.value = true;
  artifactPreviewVisible.value = true;
  artifactPreviewAgentId.value = agentId;
  artifactPreviewContainerId.value = containerId;

  try {
    const response = await fetchWunderWorkspaceContent({
      agent_id: agentId,
      container_id: containerId,
      path: itemPath,
      include_content: true,
      max_bytes: 1024 * 256
    });
    const payloadData = response?.data || {};
    artifactPreviewContent.value = typeof payloadData.content === 'string' ? payloadData.content : '';
    if (payloadData.truncated) {
      artifactPreviewError.value = t('workspace.preview.truncatedHint');
    }
  } catch (error: any) {
    artifactPreviewError.value = String(
      error?.response?.data?.detail || error?.message || t('workspace.preview.loadFailedHint')
    ).trim();
  } finally {
    artifactPreviewLoading.value = false;
  }
};

const saveBlobAsFile = (blob: Blob, filename: string) => {
  if (typeof window === 'undefined') return;
  const objectUrl = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = objectUrl;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
};

const handleArtifactPreviewDownload = async () => {
  const agentId = String(artifactPreviewAgentId.value || '').trim();
  const path = String(artifactPreviewPath.value || '').trim();
  const containerId = Number(artifactPreviewContainerId.value || 1);
  if (!agentId || !path) return;
  try {
    const response = await downloadWunderWorkspaceFile({
      agent_id: agentId,
      container_id: containerId,
      path
    });
    const fallbackName = path.split('/').pop() || path.split('\\').pop() || 'download';
    saveBlobAsFile(response.data as Blob, fallbackName);
  } catch (error: any) {
    artifactPreviewError.value = String(
      error?.response?.data?.detail || error?.message || t('workspace.preview.loadFailedHint')
    ).trim();
  }
};

onMounted(() => {
  requestCanvasRevealReplay();
  if (typeof document !== 'undefined') {
    document.addEventListener('fullscreenchange', refreshCanvasFullscreen);
    window.addEventListener('pointermove', handleChatResizePointerMove);
    window.addEventListener('pointerup', handleChatResizePointerUp);
    window.addEventListener('pointercancel', handleChatResizePointerUp);
    window.addEventListener('pointermove', handleTimelineResizePointerMove);
    window.addEventListener('pointerup', handleTimelineResizePointerUp);
    window.addEventListener('pointercancel', handleTimelineResizePointerUp);
    window.addEventListener('pointerdown', handleGlobalPointerDown);
    refreshCanvasFullscreen();
  }
  syncBoardMetrics();
  if (typeof ResizeObserver !== 'undefined' && boardRef.value) {
    boardResizeObserver = new ResizeObserver(() => {
      syncBoardMetrics();
      if (!isCompactLayout.value) {
        applyChatWidth(chatWidth.value || DEFAULT_CHAT_WIDTH);
      }
      applyTimelineHeight(timelineHeight.value || DEFAULT_TIMELINE_HEIGHT);
    });
    boardResizeObserver.observe(boardRef.value);
  }
});

onActivated(() => {
  requestCanvasRevealReplay();
});

onBeforeUnmount(() => {
  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', refreshCanvasFullscreen);
    window.removeEventListener('pointermove', handleChatResizePointerMove);
    window.removeEventListener('pointerup', handleChatResizePointerUp);
    window.removeEventListener('pointercancel', handleChatResizePointerUp);
    window.removeEventListener('pointermove', handleTimelineResizePointerMove);
    window.removeEventListener('pointerup', handleTimelineResizePointerUp);
    window.removeEventListener('pointercancel', handleTimelineResizePointerUp);
    window.removeEventListener('pointerdown', handleGlobalPointerDown);
  }
  stopChatResize();
  stopTimelineResize();
  boardResizeObserver?.disconnect();
  boardResizeObserver = null;
});

watch(
  canvasScopeKey,
  (scopeKey) => {
    const cached = getBeeroomMissionCanvasState(scopeKey);
    const sharedLayoutState = getBeeroomMissionCanvasState(canvasLayoutScopeKey.value);
    layoutMode.value =
      (sharedLayoutState?.layoutMode || cached?.layoutMode) === 'vertical' ? 'vertical' : 'horizontal';
    chatWidth.value = clampChatWidth(Number(cached?.chatWidth || DEFAULT_CHAT_WIDTH));
    chatCollapsed.value = Boolean(cached?.chatCollapsed);
    timelineCollapsed.value = Boolean(cached?.timelineCollapsed);
    timelineHeight.value = clampTimelineHeight(Number(cached?.timelineHeight || DEFAULT_TIMELINE_HEIGHT));
    artifactWorkspaceVisible.value = false;
    artifactPreviewVisible.value = false;
    agentOutputPreviewVisible.value = false;
  },
  { immediate: true }
);

watch(
  () => props.currentOrchestrationId,
  () => {
    closeRoundContextMenu();
    requestCanvasRevealReplay();
  }
);

watch(
  () => props.activeRound?.id || '',
  (roundId, previousRoundId) => {
    if (!roundId || roundId === previousRoundId) return;
    requestCanvasRevealReplay();
  }
);

watch(
  () => props.active,
  (active, previousActive) => {
    if (active !== true || previousActive === true) return;
    requestCanvasRevealReplay();
  }
);

watch(
  () => props.runtimeLocked,
  (locked) => {
    if (locked) {
      closeRoundContextMenu();
    }
  }
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

.beeroom-canvas-board.is-timeline-resizing {
  user-select: none;
  -webkit-user-select: none;
  cursor: ns-resize;
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
  display: flex;
  flex-direction: column;
  align-items: stretch;
  width: calc(100% - 28px);
  margin: 0 14px;
  height: var(--orchestration-timeline-height);
  min-height: 88px;
  padding: 0 16px 16px;
  border-top: 1px solid rgba(148, 163, 184, 0.14);
  background:
    linear-gradient(180deg, rgba(14, 18, 27, 0.86), rgba(9, 12, 18, 0.94)),
    linear-gradient(90deg, rgba(56, 189, 248, 0.03), rgba(251, 191, 36, 0.04));
  box-shadow:
    0 -12px 28px rgba(2, 6, 23, 0.28),
    inset 0 1px 0 rgba(255, 255, 255, 0.03);
  border-radius: 18px 18px 0 0;
  pointer-events: auto;
  transition: height var(--beeroom-motion-slow) var(--beeroom-ease-standard);
}

.orchestration-timeline-dock.is-resizing {
  transition: none;
}

.orchestration-timeline-resizer {
  position: relative;
  flex: 0 0 18px;
  height: 18px;
  margin: 0 -16px 8px;
  cursor: ns-resize;
  touch-action: none;
  display: flex;
  align-items: center;
  justify-content: center;
  outline: none;
}

.orchestration-timeline-resizer::before {
  content: '';
  position: absolute;
  top: -1px;
  left: 0;
  right: 0;
  height: 2px;
  border-radius: 999px;
  background: linear-gradient(90deg, transparent, rgba(148, 163, 184, 0.46), transparent);
  transition: background-color 180ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-timeline-resizer:hover::before,
.orchestration-timeline-resizer:focus-visible::before,
.orchestration-timeline-dock.is-resizing .orchestration-timeline-resizer::before {
  background: linear-gradient(90deg, transparent, rgba(96, 165, 250, 0.76), transparent);
}

.orchestration-timeline-rail {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
  padding: 6px 6px 6px;
  overflow: visible;
}

.orchestration-timeline-handle {
  position: absolute;
  left: 50%;
  top: 0;
  z-index: 3;
  width: 64px;
  height: 18px;
  padding: 0;
  transform: translateX(-50%);
  border-radius: 0 0 999px 999px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-top: 0;
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.9), rgba(15, 23, 42, 0.82));
  color: rgba(203, 213, 225, 0.78);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  pointer-events: auto;
  opacity: 0.72;
  box-shadow: 0 8px 18px rgba(2, 6, 23, 0.18);
  transition:
    opacity 160ms cubic-bezier(0.22, 1, 0.36, 1),
    border-color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    background 160ms cubic-bezier(0.22, 1, 0.36, 1),
    color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-timeline-handle i {
  font-size: 11px;
}

.orchestration-timeline-shell.collapsed .orchestration-timeline-handle {
  top: auto;
  bottom: 0;
  transform: translateX(-50%);
  width: 64px;
  border-top: 1px solid rgba(148, 163, 184, 0.22);
  border-bottom: 0;
  border-radius: 999px 999px 0 0;
}

.orchestration-timeline-handle:hover,
.orchestration-timeline-handle:focus-visible {
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.94), rgba(15, 23, 42, 0.88));
  border-color: rgba(148, 163, 184, 0.4);
  color: rgba(226, 232, 240, 0.9);
  opacity: 0.92;
  transform: translateX(-50%) translateY(-1px);
  outline: none;
}

.orchestration-timeline-shell.collapsed .orchestration-timeline-handle:hover,
.orchestration-timeline-shell.collapsed .orchestration-timeline-handle:focus-visible {
  transform: translateX(-50%) translateY(1px);
}

.orchestration-round-context-menu {
  position: absolute;
  z-index: 12;
  min-width: 168px;
  padding: 8px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  border-radius: 14px;
  background: linear-gradient(180deg, rgba(10, 14, 23, 0.98), rgba(7, 10, 18, 0.98));
  box-shadow: 0 18px 44px rgba(2, 6, 23, 0.42);
  pointer-events: auto;
}

.orchestration-round-context-menu__item {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  border: none;
  border-radius: 10px;
  background: transparent;
  color: rgba(248, 250, 252, 0.92);
  font-size: 12px;
  cursor: pointer;
  transition: background 160ms cubic-bezier(0.22, 1, 0.36, 1), color 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-round-context-menu__item:hover,
.orchestration-round-context-menu__item:focus-visible {
  background: rgba(239, 68, 68, 0.14);
  color: #fecaca;
  outline: none;
}

.orchestration-timeline-track {
  flex: 1;
  min-height: 0;
  max-height: 100%;
  overflow-x: auto;
  overflow-y: auto;
  padding: 2px 2px 8px;
  scrollbar-width: thin;
  scrollbar-color: rgba(var(--ui-accent-rgb), 0.45) transparent;
  overscroll-behavior: contain;
}

.orchestration-timeline-track::-webkit-scrollbar {
  width: var(--scrollbar-size);
  height: var(--scrollbar-size);
}

.orchestration-timeline-track::-webkit-scrollbar-track {
  background: transparent;
}

.orchestration-timeline-track::-webkit-scrollbar-thumb {
  background: rgba(var(--ui-accent-rgb), 0.45);
  border-radius: 999px;
}

.orchestration-timeline-track::-webkit-scrollbar-thumb:hover {
  background: rgba(var(--ui-accent-rgb), 0.62);
}

.orchestration-timeline-empty {
  display: flex;
  min-height: 72px;
  align-items: center;
  justify-content: center;
  color: rgba(148, 163, 184, 0.78);
  font-size: 13px;
}

.orchestration-timeline-tree-grid {
  --timeline-node-size: 36px;
  --timeline-track-width: 64px;
  --timeline-track-height: 48px;
  --timeline-row-gap: 12px;
  --timeline-column-gap: 10px;
  --timeline-grid-pad-top: 10px;
  --timeline-grid-pad-x: 10px;
  --timeline-grid-pad-bottom: 12px;
  --timeline-column-step: calc(var(--timeline-track-width) + var(--timeline-column-gap));
  --timeline-row-step: calc(var(--timeline-track-height) + var(--timeline-row-gap));
  --timeline-center-x: calc(var(--timeline-track-width) / 2);
  --timeline-center-y: calc(var(--timeline-track-height) / 2);
  position: relative;
  display: grid;
  grid-template-columns: repeat(var(--timeline-columns), var(--timeline-track-width));
  grid-template-rows: repeat(var(--timeline-lanes), var(--timeline-track-height));
  grid-auto-flow: row;
  gap: var(--timeline-row-gap) var(--timeline-column-gap);
  min-width: max-content;
  min-height: max-content;
  padding: var(--timeline-grid-pad-top) var(--timeline-grid-pad-x) var(--timeline-grid-pad-bottom);
}

.orchestration-timeline-lane-rail {
  position: absolute;
  left: var(--timeline-grid-pad-x);
  right: var(--timeline-grid-pad-x);
  top: calc(var(--timeline-grid-pad-top) + (var(--lane) - 1) * var(--timeline-row-step) + var(--timeline-center-y));
  height: 1px;
  background: linear-gradient(90deg, rgba(51, 65, 85, 0.12), rgba(59, 130, 246, 0.14), rgba(51, 65, 85, 0.12));
  pointer-events: none;
}

.orchestration-timeline-connector {
  position: absolute;
  pointer-events: none;
  z-index: 0;
}

.orchestration-timeline-connector.horizontal {
  left: calc(var(--timeline-grid-pad-x) + (var(--column-start) - 1) * var(--timeline-column-step) + var(--timeline-center-x));
  top: calc(var(--timeline-grid-pad-top) + (var(--lane) - 1) * var(--timeline-row-step) + var(--timeline-center-y) - 1px);
  width: max(10px, calc((var(--column-end) - var(--column-start)) * var(--timeline-column-step)));
  height: 2px;
  border-radius: 999px;
  background: linear-gradient(90deg, rgba(56, 189, 248, 0.34), rgba(125, 211, 252, 0.2));
  box-shadow: 0 0 0 1px rgba(15, 23, 42, 0.24);
}

.orchestration-timeline-connector.vertical {
  left: calc(var(--timeline-grid-pad-x) + (var(--column) - 1) * var(--timeline-column-step) + var(--timeline-center-x) - 1px);
  top: calc(var(--timeline-grid-pad-top) + (var(--lane-start) - 1) * var(--timeline-row-step) + var(--timeline-center-y));
  width: 2px;
  height: calc((var(--lane-end) - var(--lane-start)) * var(--timeline-row-step));
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(56, 189, 248, 0.34), rgba(125, 211, 252, 0.2));
  box-shadow: 0 0 0 1px rgba(15, 23, 42, 0.24);
}

.orchestration-timeline-connector--rounds {
  background: linear-gradient(90deg, rgba(251, 191, 36, 0.46), rgba(253, 224, 71, 0.28));
}

.orchestration-timeline-connector--branch {
  background: linear-gradient(180deg, rgba(251, 191, 36, 0.5), rgba(56, 189, 248, 0.26));
}

.orchestration-run-chip,
.orchestration-round-chip {
  position: relative;
  z-index: 1;
  display: inline-flex;
  width: var(--timeline-node-size);
  height: var(--timeline-node-size);
  align-items: center;
  justify-content: center;
  padding: 0;
  border: 0;
  background: transparent;
  color: #dbeafe;
  cursor: pointer;
  grid-column: var(--column);
  grid-row: var(--lane);
  place-self: center;
  transition:
    color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    opacity 160ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-run-chip {
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background:
    linear-gradient(180deg, rgba(15, 23, 42, 0.94), rgba(2, 6, 23, 0.98)),
    linear-gradient(120deg, rgba(56, 189, 248, 0.04), rgba(251, 191, 36, 0.04));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.04),
    0 8px 20px rgba(2, 6, 23, 0.16);
}

.orchestration-run-chip:hover,
.orchestration-run-chip:focus-visible,
.orchestration-round-chip:hover,
.orchestration-round-chip:focus-visible {
  color: #f8fafc;
  transform: translateY(-2px);
  outline: none;
}

.orchestration-run-chip.current {
  border-color: rgba(56, 189, 248, 0.34);
}

.orchestration-run-chip.is-disabled,
.orchestration-round-chip.is-disabled {
  cursor: not-allowed;
  opacity: 0.52;
}

.orchestration-run-chip.active {
  border-color: rgba(251, 191, 36, 0.36);
  box-shadow:
    inset 0 1px 0 rgba(255, 251, 235, 0.08),
    0 0 0 3px rgba(251, 191, 36, 0.1),
    0 14px 28px rgba(120, 53, 15, 0.18);
}

.orchestration-run-chip-icon {
  width: 100%;
  height: 100%;
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  color: rgba(191, 219, 254, 0.86);
  background: transparent;
  border: 0;
}

.orchestration-run-chip-icon i {
  font-size: 13px;
}

.orchestration-round-chip-node {
  position: relative;
  z-index: 1;
  width: var(--timeline-node-size);
  height: var(--timeline-node-size);
  border-radius: 999px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(148, 163, 184, 0.28);
  background:
    radial-gradient(circle at 32% 28%, rgba(255, 255, 255, 0.16), transparent 38%),
    linear-gradient(180deg, rgba(30, 41, 59, 0.88), rgba(15, 23, 42, 0.94));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.05),
    0 4px 14px rgba(2, 6, 23, 0.18);
  transition:
    border-color 160ms cubic-bezier(0.22, 1, 0.36, 1),
    background 160ms cubic-bezier(0.22, 1, 0.36, 1),
    box-shadow 160ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-round-chip-index {
  font-size: 13px;
  font-weight: 700;
  line-height: 1;
  color: rgba(226, 232, 240, 0.92);
}

.orchestration-round-chip.active {
  color: #fff8eb;
}

.orchestration-round-chip.selected:not(.active) .orchestration-round-chip-node {
  border-color: rgba(250, 204, 21, 0.38);
  box-shadow:
    inset 0 1px 0 rgba(255, 251, 235, 0.12),
    0 0 0 3px rgba(250, 204, 21, 0.08),
    0 8px 22px rgba(120, 53, 15, 0.14);
}

.orchestration-round-chip.active .orchestration-round-chip-node {
  border-color: rgba(251, 191, 36, 0.5);
  background:
    radial-gradient(circle at 32% 28%, rgba(255, 251, 235, 0.28), transparent 40%),
    linear-gradient(180deg, rgba(234, 179, 8, 0.86), rgba(180, 83, 9, 0.84));
  box-shadow:
    inset 0 1px 0 rgba(255, 251, 235, 0.24),
    0 0 0 3px rgba(251, 191, 36, 0.12),
    0 10px 24px rgba(120, 53, 15, 0.2);
  transform: scale(1.04);
}

.orchestration-round-chip.active .orchestration-round-chip-index {
  color: #3f2207;
}

.orchestration-round-chip.is-pending {
  color: #dbeafe;
}

.orchestration-round-chip.is-pending .orchestration-round-chip-node {
  border-color: rgba(96, 165, 250, 0.52);
  background:
    radial-gradient(circle at 32% 28%, rgba(239, 246, 255, 0.22), transparent 42%),
    linear-gradient(180deg, rgba(37, 99, 235, 0.3), rgba(15, 23, 42, 0.92));
  box-shadow:
    inset 0 1px 0 rgba(239, 246, 255, 0.16),
    0 0 0 3px rgba(59, 130, 246, 0.1),
    0 8px 22px rgba(30, 64, 175, 0.16);
  transform: scale(1.02);
}

.orchestration-round-chip.is-pending .orchestration-round-chip-index {
  color: #dbeafe;
}

.orchestration-round-chip.is-preview {
  cursor: default;
}

.orchestration-round-chip.is-preview .orchestration-round-chip-node {
  border-style: dashed;
  border-color: rgba(125, 211, 252, 0.44);
  background:
    radial-gradient(circle at 32% 28%, rgba(224, 242, 254, 0.16), transparent 44%),
    linear-gradient(180deg, rgba(14, 116, 144, 0.16), rgba(15, 23, 42, 0.88));
  box-shadow:
    inset 0 1px 0 rgba(224, 242, 254, 0.12),
    0 0 0 2px rgba(14, 165, 233, 0.08),
    0 6px 18px rgba(8, 47, 73, 0.14);
  transform: none;
}

.orchestration-round-chip.is-preview .orchestration-round-chip-index {
  color: rgba(186, 230, 253, 0.92);
}

.orchestration-run-chip.is-disabled:hover,
.orchestration-run-chip.is-disabled:focus-visible,
.orchestration-round-chip.is-disabled:hover,
.orchestration-round-chip.is-disabled:focus-visible {
  color: #dbeafe;
  transform: none;
}

.orchestration-round-chip.is-disabled:hover .orchestration-round-chip-node,
.orchestration-round-chip.is-disabled:focus-visible .orchestration-round-chip-node {
  border-color: rgba(148, 163, 184, 0.28);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.05),
    0 4px 14px rgba(2, 6, 23, 0.18);
}

.orchestration-round-chip:hover .orchestration-round-chip-node,
.orchestration-round-chip:focus-visible .orchestration-round-chip-node {
  border-color: rgba(125, 211, 252, 0.42);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    0 8px 18px rgba(8, 47, 73, 0.18);
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

.orchestration-panel-status-lamp {
  display: inline-flex;
  flex: 0 0 auto;
  width: 10px;
  height: 10px;
  margin-top: 1px;
  align-self: center;
  border-radius: 999px;
  background: #64748b;
  box-shadow: 0 0 0 4px rgba(100, 116, 139, 0.14);
  transition:
    background-color 180ms cubic-bezier(0.22, 1, 0.36, 1),
    box-shadow 180ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 180ms cubic-bezier(0.22, 1, 0.36, 1);
}

.orchestration-panel-status-lamp.is-active {
  background: #4ade80;
  box-shadow: 0 0 0 4px rgba(74, 222, 128, 0.14);
  transform: scale(1.02);
}

.orchestration-panel-status-lamp.is-busy {
  background: #38bdf8;
  box-shadow: 0 0 0 4px rgba(56, 189, 248, 0.18);
  transform: scale(1.04);
}

.orchestration-side-control {
  display: grid;
  gap: 10px;
  padding-top: 12px;
  border-top: 1px solid rgba(148, 163, 184, 0.16);
  background: linear-gradient(180deg, rgba(9, 10, 15, 0), rgba(9, 10, 15, 0.52));
}

.orchestration-side-control-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.orchestration-side-control-head-main {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.orchestration-side-control-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.orchestration-side-control-round {
  display: inline-flex;
  align-items: center;
  padding: 4px 9px;
  border-radius: 999px;
  border: 1px solid rgba(245, 158, 11, 0.28);
  background: rgba(245, 158, 11, 0.12);
  color: rgba(253, 230, 138, 0.92);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.02em;
  line-height: 1.2;
  white-space: nowrap;
}

.orchestration-side-control-title {
  font-size: 12px;
  font-weight: 700;
  color: rgba(226, 232, 240, 0.92);
  letter-spacing: 0.02em;
}

.orchestration-side-control-textarea {
  width: 100%;
  min-height: 148px;
  resize: vertical;
  padding: 12px 13px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: linear-gradient(180deg, rgba(19, 23, 32, 0.96), rgba(12, 16, 24, 0.92));
  color: #e5e7eb;
  line-height: 1.65;
  font-size: 12.5px;
  outline: none;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.orchestration-side-control-textarea:focus-visible {
  border-color: rgba(96, 165, 250, 0.4);
  box-shadow:
    0 0 0 2px rgba(96, 165, 250, 0.18),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.orchestration-side-control-textarea:disabled {
  opacity: 0.72;
  cursor: not-allowed;
}

.orchestration-side-control-icon-btn.is-stop {
  border-color: rgba(245, 158, 11, 0.4);
  background: linear-gradient(135deg, rgba(180, 83, 9, 0.92), rgba(146, 64, 14, 0.92));
  color: #fef3c7;
  box-shadow: 0 10px 24px rgba(120, 53, 15, 0.24);
}

.orchestration-chat-empty {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: min(100%, 220px);
  min-height: 40px;
  padding: 0 14px;
  border-radius: 999px;
  border: 1px dashed rgba(148, 163, 184, 0.16);
  background: rgba(15, 23, 42, 0.18);
  color: rgba(191, 219, 254, 0.68);
  font-size: 12px;
  line-height: 1.4;
  text-align: center;
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
