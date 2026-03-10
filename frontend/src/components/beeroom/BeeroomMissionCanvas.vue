<template>
  <section ref="screenRef" class="beeroom-canvas-screen" :class="{ 'is-empty': !projection.nodes.length }">
    <div v-if="!projection.nodes.length" class="beeroom-canvas-empty">
      <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
      <span>{{ t('beeroom.canvas.empty') }}</span>
    </div>

    <div v-else class="beeroom-canvas-layout">
      <div class="beeroom-canvas-board" :class="{ 'chat-collapsed': chatCollapsed }">
        <div ref="boardRef" class="beeroom-canvas-graph-shell">
          <div ref="canvasRef" class="beeroom-canvas-surface"></div>
          <div class="beeroom-canvas-legend">
            <span class="beeroom-canvas-legend-item is-running">
              <i aria-hidden="true"></i>
              <span>{{ t('beeroom.status.running') }} {{ canvasStatusSummary.running }}</span>
            </span>
            <span class="beeroom-canvas-legend-item is-danger">
              <i aria-hidden="true"></i>
              <span>{{ t('beeroom.status.failed') }} {{ canvasStatusSummary.failed }}</span>
            </span>
            <span class="beeroom-canvas-legend-item is-idle">
              <i aria-hidden="true"></i>
              <span>{{ t('beeroom.members.idle') }} {{ canvasStatusSummary.idle }}</span>
            </span>
          </div>
          <div class="beeroom-canvas-tools" role="toolbar" aria-label="画布控制区">
            <button class="beeroom-canvas-tool-btn" type="button" title="放大画布" aria-label="放大画布" @click="zoomCanvasIn">
              <i class="fa-solid fa-magnifying-glass-plus" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">放大画布</span>
            </button>
            <button class="beeroom-canvas-tool-btn" type="button" title="缩小画布" aria-label="缩小画布" @click="zoomCanvasOut">
              <i class="fa-solid fa-magnifying-glass-minus" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">缩小画布</span>
            </button>
            <button class="beeroom-canvas-tool-btn" type="button" title="重置缩放 100%" aria-label="重置缩放 100%" @click="resetCanvasZoom">
              <i class="fa-solid fa-arrows-rotate" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">重置缩放 100%</span>
            </button>
            <button class="beeroom-canvas-tool-btn" type="button" title="适配视图" aria-label="适配视图" @click="fitCanvasView">
              <i class="fa-solid fa-expand" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">适配视图</span>
            </button>
            <button class="beeroom-canvas-tool-btn" type="button" title="自动整理" aria-label="自动整理" @click="autoArrangeCanvas">
              <i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">自动整理</span>
            </button>
            <button
              class="beeroom-canvas-tool-btn"
              :class="{ 'is-active': canvasFullscreen }"
              type="button"
              :title="canvasFullscreen ? '退出全屏' : '全屏'"
              :aria-label="canvasFullscreen ? '退出全屏' : '全屏'"
              :aria-pressed="canvasFullscreen"
              @click="toggleCanvasFullscreen"
            >
              <i class="fa-solid" :class="canvasFullscreen ? 'fa-minimize' : 'fa-maximize'" aria-hidden="true"></i>
              <span class="beeroom-visually-hidden">{{ canvasFullscreen ? '退出全屏' : '全屏' }}</span>
            </button>
          </div>
          <div class="beeroom-canvas-minimap-shell">
            <div class="beeroom-canvas-minimap-label">{{ t('beeroom.canvas.minimap') }}</div>
            <div ref="minimapRef" class="beeroom-canvas-minimap"></div>
          </div>

          <div
            v-if="showNodeTooltip && hoveredNodeMeta"
            class="beeroom-canvas-tooltip"
            :style="hoveredTooltipStyle"
          >
            <div class="beeroom-canvas-tooltip-head">
              <div class="beeroom-canvas-tooltip-title">{{ hoveredNodeMeta.agent_name }}</div>
              <span class="beeroom-canvas-role-chip">{{ hoveredNodeMeta.role_label }}</span>
            </div>
            <div class="beeroom-canvas-tooltip-meta">
              <span class="beeroom-canvas-status-chip" :class="resolveToneClass(hoveredNodeMeta.status)">
                {{ resolveStatusLabel(hoveredNodeMeta.status) }}
              </span>
              <span v-if="hoveredNodeMeta.entry_agent" class="beeroom-canvas-entry-flag">
                {{ t('beeroom.canvas.entryAgent') }}
              </span>
            </div>
            <div class="beeroom-canvas-tooltip-grid">
              <div class="beeroom-canvas-tooltip-item">
                <span>{{ t('beeroom.canvas.currentTaskTotal', { count: hoveredNodeMeta.task_total || 0 }) }}</span>
                <strong>{{ hoveredNodeMeta.task_total || 0 }}</strong>
              </div>
              <div class="beeroom-canvas-tooltip-item">
                <span>{{ t('beeroom.canvas.activeSessions') }}</span>
                <strong>{{ hoveredNodeMeta.active_session_total || 0 }}</strong>
              </div>
            </div>
            <p class="beeroom-canvas-tooltip-desc">{{ hoveredNodeDescription }}</p>
          </div>
        </div>

        <aside class="beeroom-canvas-chat" :class="{ collapsed: chatCollapsed }">
        <button
          class="beeroom-canvas-chat-handle"
          type="button"
          :title="chatCollapsed ? t('common.expand') : t('common.collapse')"
          :aria-label="chatCollapsed ? t('common.expand') : t('common.collapse')"
          @click="chatCollapsed = !chatCollapsed"
        >
          <i class="fa-solid" :class="chatCollapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
        </button>
        <template v-if="!chatCollapsed">
          <div class="beeroom-canvas-chat-head">
            <div>
              <div class="beeroom-canvas-chat-title">{{ t('beeroom.canvas.chatTitle') }}</div>
              <div class="beeroom-canvas-chat-subtitle">{{ chatPanelSubtitle }}</div>
              <div class="beeroom-canvas-chat-runtime">
                <span class="beeroom-canvas-runtime-chip" :class="`is-${dispatchRuntimeTone}`">
                  {{ dispatchRuntimeLabel }}
                </span>
                <span v-if="dispatchSessionId" class="beeroom-canvas-runtime-session">
                  #{{ shortIdentity(dispatchSessionId, 6, 4) }}
                </span>
              </div>
            </div>
            <div class="beeroom-canvas-chat-head-actions">
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="t('common.clear')"
                @click="clearManualChatHistory"
              >
                <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
              </button>
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="refreshing ? t('common.loading') : t('common.refresh')"
                :disabled="refreshing"
                @click="emit('refresh')"
              >
                <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
              </button>
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="t('common.stop')"
                :disabled="!dispatchCanStop"
                @click="handleDispatchStop"
              >
                <i class="fa-solid fa-stop" aria-hidden="true"></i>
              </button>
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="t('chat.message.resume')"
                :disabled="!dispatchCanResume"
                @click="handleDispatchResume"
              >
                <i class="fa-solid fa-play" aria-hidden="true"></i>
              </button>
              <span class="beeroom-canvas-chat-count">{{ displayChatMessages.length }}</span>
            </div>
          </div>

          <section ref="chatStreamRef" class="beeroom-canvas-chat-stream">
            <article
              v-for="message in displayChatMessages"
              :key="message.key"
              class="beeroom-canvas-chat-message"
              :class="[`is-${message.tone}`]"
            >
              <button
                v-if="message.senderAgentId"
                class="beeroom-canvas-chat-avatar"
                type="button"
                @click="emit('open-agent', message.senderAgentId)"
              >
                {{ avatarLabel(message.senderName) }}
              </button>
              <div v-else class="beeroom-canvas-chat-avatar" :class="message.tone === 'user' ? 'is-user' : 'is-system'">
                <i
                  class="fa-solid"
                  :class="message.tone === 'user' ? 'fa-user' : 'fa-wave-square'"
                  aria-hidden="true"
                ></i>
              </div>
              <div class="beeroom-canvas-chat-main">
                <div class="beeroom-canvas-chat-meta-row">
                  <button
                    v-if="message.senderAgentId"
                    class="beeroom-canvas-chat-sender"
                    type="button"
                    @click="emit('open-agent', message.senderAgentId)"
                  >
                    {{ message.senderName }}
                  </button>
                  <span
                    v-else
                    class="beeroom-canvas-chat-sender"
                    :class="message.tone === 'user' ? 'is-user' : 'is-system'"
                  >
                    {{ message.senderName }}
                  </span>
                  <span class="beeroom-canvas-chat-time">{{ message.timeLabel }}</span>
                </div>
                <div class="beeroom-canvas-chat-bubble">
                  <span v-if="message.mention" class="beeroom-canvas-chat-mention">@{{ message.mention }}</span>
                  <span>{{ message.body }}</span>
                </div>
                <div v-if="message.meta" class="beeroom-canvas-chat-extra">{{ message.meta }}</div>
              </div>
            </article>
          </section>

          <section v-if="dispatchApprovals.length" class="beeroom-canvas-chat-approvals">
            <div class="beeroom-canvas-chat-approvals-head">
              <span>{{ t('chat.approval.title') }}</span>
              <span class="beeroom-canvas-chat-approvals-count">{{ dispatchApprovals.length }}</span>
            </div>
            <article
              v-for="approval in dispatchApprovals"
              :key="approval.approval_id"
              class="beeroom-canvas-chat-approval-item"
            >
              <div class="beeroom-canvas-chat-approval-summary">
                {{ approval.summary || approval.tool || approval.approval_id }}
              </div>
              <div class="beeroom-canvas-chat-approval-meta">
                {{ t('chat.approval.tool') }}: {{ approval.tool || '-' }}
              </div>
              <div class="beeroom-canvas-chat-approval-actions">
                <button
                  class="beeroom-canvas-chat-approval-btn"
                  type="button"
                  :disabled="dispatchApprovalBusy"
                  @click="handleDispatchApproval('approve_once', approval.approval_id)"
                >
                  {{ t('chat.approval.once') }}
                </button>
                <button
                  class="beeroom-canvas-chat-approval-btn"
                  type="button"
                  :disabled="dispatchApprovalBusy"
                  @click="handleDispatchApproval('approve_session', approval.approval_id)"
                >
                  {{ t('chat.approval.session') }}
                </button>
                <button
                  class="beeroom-canvas-chat-approval-btn is-danger"
                  type="button"
                  :disabled="dispatchApprovalBusy"
                  @click="handleDispatchApproval('deny', approval.approval_id)"
                >
                  {{ t('chat.approval.deny') }}
                </button>
              </div>
            </article>
          </section>

          <section class="beeroom-canvas-chat-composer">
            <textarea
              v-model="composerText"
              class="beeroom-canvas-chat-textarea"
              :placeholder="t('beeroom.canvas.chatInputPlaceholder')"
              :disabled="composerSending"
              rows="3"
              @keydown.enter.exact.prevent="handleComposerSend"
            ></textarea>
            <div class="beeroom-canvas-chat-compose-foot">
              <el-select
                id="beeroom-chat-target"
                v-model="composerTargetAgentId"
                class="beeroom-canvas-chat-select"
                popper-class="beeroom-canvas-chat-select-popper"
                :placeholder="t('beeroom.canvas.chatTarget')"
                :disabled="composerSending"
              >
                <el-option
                  v-for="option in composerTargetOptions"
                  :key="option.agentId"
                  :label="option.label"
                  :value="option.agentId"
                />
              </el-select>
              <button
                class="beeroom-canvas-chat-send"
                type="button"
                :disabled="composerSending || !composerCanSend"
                @click="handleComposerSend"
              >
                {{ composerSending ? t('common.loading') : t('chat.input.send') }}
              </button>
            </div>
            <div v-if="composerError" class="beeroom-canvas-chat-compose-status is-error">{{ composerError }}</div>
          </section>
        </template>
        </aside>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import type { Graph as G6Graph } from '@antv/g6';
import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import {
  appendBeeroomChatMessage,
  clearBeeroomChatMessages,
  listBeeroomChatMessages,
  openBeeroomChatStream,
  openBeeroomSocket
} from '@/api/beeroom';
import {
  cancelMessageStream,
  createSession,
  listSessions,
  resumeMessageStream,
  sendMessageStream
} from '@/api/chat';
import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import { consumeSseStream } from '@/utils/sse';
import { createWsMultiplexer } from '@/utils/ws';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';
import type { BeeroomGroup, BeeroomMember, BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import {
  getBeeroomMissionCanvasState,
  setBeeroomMissionCanvasState,
  type BeeroomCanvasViewportState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';

type CanvasNodeMeta = {
  id: string;
  agent_id: string;
  agent_name: string;
  role: 'mother' | 'worker';
  role_label: string;
  status: string;
  task_total: number;
  active_session_total: number;
  updated_time: number;
  summary: string;
  entry_agent: boolean;
};

type CanvasLinkItem = {
  key: string;
  title: string;
  subtitle: string;
  meta: string;
};

type MissionChatMessage = {
  key: string;
  senderName: string;
  senderAgentId: string;
  mention: string;
  body: string;
  meta: string;
  time: number;
  timeLabel: string;
  tone: 'mother' | 'worker' | 'system' | 'user';
};

type ComposerTargetOption = {
  agentId: string;
  label: string;
  role: 'mother' | 'worker';
};

type DispatchSessionTarget = {
  sessionId: string;
  sessionSummary: Record<string, unknown> | null;
};

type DispatchRuntimeStatus =
  | 'idle'
  | 'queued'
  | 'running'
  | 'awaiting_approval'
  | 'resuming'
  | 'stopped'
  | 'completed'
  | 'failed';

type DispatchApprovalItem = {
  approval_id: string;
  session_id: string;
  tool: string;
  summary: string;
};

type CanvasPositionOverride = {
  x: number;
  y: number;
};

type HoneycombSlot = {
  q: number;
  r: number;
};

type PendingCanvasViewportRestore = {
  scopeKey: string;
  viewport: BeeroomCanvasViewportState;
};

const HEX_DIRECTIONS: HoneycombSlot[] = [
  { q: 1, r: 0 },
  { q: 1, r: -1 },
  { q: 0, r: -1 },
  { q: -1, r: 0 },
  { q: -1, r: 1 },
  { q: 0, r: 1 }
];
const HONEYCOMB_RADIUS = 164;
const HONEYCOMB_VERTICAL_RATIO = 1.18;
const NODE_WIDTH = 248;
const MOTHER_NODE_WIDTH = NODE_WIDTH;
const NODE_HEIGHT = 94;
const MOTHER_NODE_HEIGHT = NODE_HEIGHT;
const GRID_PLUGIN_KEY = 'beeroom-grid-line';
const MANUAL_CHAT_HISTORY_LIMIT = 120;
const CHAT_POLL_INTERVAL_MS = 2000;
const CHAT_WS_RETRY_DELAY_MS = 1400;
const CHAT_SSE_RETRY_DELAY_MS = 2200;
const CARD_ACCENT_PALETTE = ['#3b82f6', '#8b5cf6', '#22c55e', '#06b6d4', '#eab308', '#f97316', '#ef4444'];

const beeroomWsClient = createWsMultiplexer(() => openBeeroomSocket({ allowQueryToken: true }), {
  idleTimeoutMs: 20000,
  connectTimeoutMs: 10000,
  pingIntervalMs: 20000
});
const CANVAS_ZOOM_MIN = 0.5;
const CANVAS_ZOOM_MAX = 1.8;
const CANVAS_ZOOM_STEP = 0.12;
const showNodeTooltip = false;
const ACTIVE_DISPATCH_STATUSES = new Set(['queued', 'running', 'awaiting_idle']);

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
const chatStore = useChatStore();
const screenRef = ref<HTMLElement | null>(null);
const boardRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLDivElement | null>(null);
const minimapRef = ref<HTMLDivElement | null>(null);
const chatStreamRef = ref<HTMLElement | null>(null);
const activeNodeId = ref('');
const hoveredNodeId = ref('');
const hoveredTooltipPosition = ref({ x: 0, y: 0 });
const chatCollapsed = ref(false);
const manualChatMessages = ref<MissionChatMessage[]>([]);
const composerText = ref('');
const composerTargetAgentId = ref('');
const composerSending = ref(false);
const composerError = ref('');
const dispatchSessionId = ref('');
const dispatchRequestId = ref('');
const dispatchLastEventId = ref(0);
const dispatchRuntimeStatus = ref<DispatchRuntimeStatus>('idle');
const dispatchTargetAgentId = ref('');
const dispatchTargetName = ref('');
const dispatchTargetTone = ref<MissionChatMessage['tone']>('worker');
const dispatchRespondingApprovalId = ref('');
const chatRealtimeTransport = ref<'none' | 'ws' | 'sse'>('none');
const nodePositionOverrides = ref<Record<string, CanvasPositionOverride>>({});
const canvasFullscreen = ref(false);

let manualMessageSerial = 0;

type G6Module = typeof import('@antv/g6');
type GraphCtor = G6Module['Graph'];

let graph: G6Graph | null = null;
let graphCtor: GraphCtor | null = null;
let resizeObserver: ResizeObserver | null = null;
let resizeFrame = 0;
let graphInitPromise: Promise<void> | null = null;
let canvasDisposed = false;
let renderSequence = 0;
let renderTask: Promise<void> | null = null;
let renderRequested = false;
let renderRequestedForceFit = false;
let chatPollTimer: number | null = null;
let chatWatchController: AbortController | null = null;
let chatWatchRequestId = '';
let chatWatchRetryTimer: number | null = null;
let chatSseRetryTimer: number | null = null;
let chatSseSource: EventSource | null = null;
let chatSseGroupId = '';
let dispatchStreamController: AbortController | null = null;
let dispatchStopRequested = false;
let dispatchFlowTimer: number | null = null;
let dispatchFlowOffset = 0;
let canvasWheelSaveTimer: number | null = null;
const activeDispatchEdgeIds = ref<string[]>([]);

const loadGraphCtor = async (): Promise<GraphCtor> => {
  if (graphCtor) {
    return graphCtor;
  }
  // Lazy-load G6 so desktop startup does not eagerly execute heavy graph runtime on non-beeroom pages.
  const g6 = await import('@antv/g6');
  graphCtor = g6.Graph;
  return graphCtor;
};

const formatDateTime = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) return '-';
  return new Intl.DateTimeFormat(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  }).format(new Date(numeric * 1000));
};

const formatCount = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric < 0) return '-';
  return new Intl.NumberFormat().format(Math.round(numeric));
};

const normalizeManualChatTone = (value: unknown): MissionChatMessage['tone'] => {
  const tone = String(value || '').trim().toLowerCase();
  if (tone === 'mother' || tone === 'worker' || tone === 'system' || tone === 'user') {
    return tone;
  }
  return 'system';
};

const mapApiChatMessage = (value: unknown): MissionChatMessage | null => {
  if (!value || typeof value !== 'object') return null;
  const payload = value as Record<string, unknown>;
  const messageId = Number(payload.message_id || 0);
  const time = Number(payload.created_at || payload.time || 0);
  if (!Number.isFinite(time) || time <= 0) return null;
  const body = String(payload.body || '').trim();
  if (!body) return null;
  const key = String(payload.key || '').trim() || `history:${messageId || time}`;
  return {
    key,
    senderName: String(payload.sender_name || payload.senderName || '').trim() || t('messenger.section.swarms'),
    senderAgentId: String(payload.sender_agent_id || payload.senderAgentId || '').trim(),
    mention: String(payload.mention_name || payload.mention || payload.mentionName || '').trim(),
    body,
    meta: String(payload.meta || '').trim(),
    time,
    timeLabel: formatDateTime(time),
    tone: normalizeManualChatTone(payload.tone)
  };
};

const shortIdentity = (value: unknown, head = 8, tail = 6) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= head + tail + 3) return text;
  return `${text.slice(0, head)}...${text.slice(-tail)}`;
};

const trimNodeTitle = (value: unknown, max = 12) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= max) return text;
  return `${text.slice(0, max)}...`;
};

const trimEdgeTaskLabel = (value: unknown, max = 26) => {
  const text = String(value || '').replace(/\s+/g, ' ').trim();
  if (!text) return '';
  if (text.length <= max) return text;
  return `${text.slice(0, max)}...`;
};

const hashText = (value: string) => {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(index);
    hash |= 0;
  }
  return Math.abs(hash);
};

const resolveNodeAccent = (agentId: string, isMother: boolean) => {
  if (isMother) return '#f59e0b';
  return CARD_ACCENT_PALETTE[hashText(agentId) % CARD_ACCENT_PALETTE.length] || '#3b82f6';
};

const avatarLabel = (value: unknown) => String(value || '?').trim().slice(0, 1).toUpperCase() || '?';

const resolveStatusLabel = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  const keyMap: Record<string, string> = {
    queued: 'beeroom.status.queued',
    running: 'beeroom.status.running',
    awaiting_idle: 'beeroom.status.awaitingIdle',
    completed: 'beeroom.status.completed',
    success: 'beeroom.status.completed',
    failed: 'beeroom.status.failed',
    error: 'beeroom.status.failed',
    timeout: 'beeroom.status.timeout',
    cancelled: 'beeroom.status.cancelled',
    idle: 'beeroom.members.idle'
  };
  return t(keyMap[normalized] || 'beeroom.status.unknown');
};

const resolveToneClass = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'running' || normalized === 'queued') return 'tone-running';
  if (normalized === 'completed' || normalized === 'success') return 'tone-success';
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout') return 'tone-danger';
  if (normalized === 'awaiting_idle') return 'tone-warn';
  return 'tone-muted';
};

const resolveDispatchTaskLabel = (mission: BeeroomMission | null, task: BeeroomMissionTask | null) => {
  const missionText = trimEdgeTaskLabel(mission?.summary || mission?.strategy || '');
  if (missionText) return missionText;
  const taskText = trimEdgeTaskLabel(task?.result_summary || task?.error || '');
  if (taskText) return taskText;
  const taskId = String(task?.task_id || '').trim();
  return taskId ? `#${taskId.slice(0, 8)}` : '';
};

const projectionMemberBusy = (member: BeeroomMember | undefined) => member?.idle === false;

const resolveMinimapNodeFill = (status: unknown) => {
  const normalized = String(status || '').trim().toLowerCase();
  if (normalized === 'running' || normalized === 'queued') return 'rgba(34, 197, 94, 0.9)';
  if (normalized === 'completed' || normalized === 'success') return 'rgba(59, 130, 246, 0.9)';
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout' || normalized === 'cancelled') {
    return 'rgba(239, 68, 68, 0.9)';
  }
  if (normalized === 'awaiting_idle') return 'rgba(245, 158, 11, 0.9)';
  return 'rgba(148, 163, 184, 0.88)';
};

const escapeHtml = (value: unknown) =>
  String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

const resolveCanvasNodeStatusClass = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'running' || normalized === 'queued') return 'is-running';
  if (normalized === 'completed' || normalized === 'success') return 'is-success';
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout' || normalized === 'cancelled') return 'is-danger';
  if (normalized === 'awaiting_idle') return 'is-warn';
  return 'is-muted';
};

const buildCanvasNodeCardHtml = (options: {
  isMother: boolean;
  selected: boolean;
  accentColor: string;
  avatar: string;
  fullName: string;
  displayName: string;
  roleLabel: string;
  status: string;
  statusLabel: string;
  taskTotal: number;
  sessionTotal: number;
}) => {
  const statusClass = resolveCanvasNodeStatusClass(options.status);
  const title = escapeHtml(options.displayName);
  const fullName = escapeHtml(options.fullName);
  const roleLabel = escapeHtml(trimNodeTitle(options.roleLabel, 6));
  const statusLabel = escapeHtml(trimNodeTitle(options.statusLabel, 7));
  const avatar = escapeHtml(options.avatar);
  const tools = escapeHtml(formatCount(options.taskTotal));
  const reports = escapeHtml(formatCount(options.sessionTotal));
  const ariaLabel = escapeHtml(`${options.fullName} ${options.roleLabel} ${options.statusLabel}`);
  const accentColor = String(options.accentColor || '#64748b').replace(/[^#a-zA-Z0-9(),.\s-]/g, '');

  // Render a compact, fully-clipped card to avoid overflow under any zoom level.
  return `
    <div class="beeroom-node-card ${statusClass} ${options.isMother ? 'is-mother' : ''} ${options.selected ? 'is-selected' : ''}" role="group" aria-label="${ariaLabel}" style="--node-accent:${accentColor};">
      <div class="beeroom-node-card-head">
        <span class="beeroom-node-avatar">${avatar}</span>
        <div class="beeroom-node-title-group">
          <div class="beeroom-node-title" title="${fullName}">${title}</div>
          <div class="beeroom-node-role-chip">${roleLabel}</div>
        </div>
        <span class="beeroom-node-status"><i class="beeroom-node-status-dot"></i><span>${statusLabel}</span></span>
      </div>
      <div class="beeroom-node-metrics">
        <span class="beeroom-node-metric" title="任务总数"><i class="fa-solid fa-list-check" aria-hidden="true"></i><b>${tools}</b></span>
        <span class="beeroom-node-metric" title="会话总数"><i class="fa-solid fa-comments" aria-hidden="true"></i><b>${reports}</b></span>
      </div>
    </div>
  `;
};

const resolveNodeStatus = (
  tasks: BeeroomMissionTask[],
  member: BeeroomMember | undefined,
  missionStatus: string
) => {
  if (member?.idle === false) return 'running';
  if (!tasks.length) return missionStatus || 'idle';
  const statuses = tasks.map((task) => String(task.status || '').trim().toLowerCase());
  if (statuses.some((status) => status === 'running' || status === 'queued')) return 'running';
  if (statuses.some((status) => status === 'failed' || status === 'error' || status === 'timeout')) return 'failed';
  if (statuses.some((status) => status === 'cancelled')) return 'cancelled';
  if (statuses.every((status) => status === 'success' || status === 'completed')) return 'completed';
  return missionStatus || 'idle';
};

const buildHoneycombSlots = (count: number): HoneycombSlot[] => {
  if (count <= 0) return [];
  const slots: HoneycombSlot[] = [{ q: 0, r: 0 }];
  let ring = 1;
  while (slots.length < count) {
    let current: HoneycombSlot = { q: -ring, r: ring };
    for (const direction of HEX_DIRECTIONS) {
      for (let step = 0; step < ring; step += 1) {
        if (slots.length >= count) break;
        slots.push({ ...current });
        current = {
          q: current.q + direction.q,
          r: current.r + direction.r
        };
      }
    }
    ring += 1;
  }
  return slots.slice(0, count);
};

const resolveHoneycombPosition = (slot: HoneycombSlot) => {
  const x = HONEYCOMB_RADIUS * Math.sqrt(3) * (slot.q + slot.r / 2);
  const y = HONEYCOMB_RADIUS * HONEYCOMB_VERTICAL_RATIO * slot.r;
  return {
    x: Math.round(x),
    y: Math.round(y)
  };
};

const resolveStatusRank = (status: string) => {
  const normalized = String(status || '').trim().toLowerCase();
  const rankMap: Record<string, number> = {
    running: 0,
    queued: 1,
    awaiting_idle: 2,
    failed: 3,
    cancelled: 4,
    completed: 5,
    success: 5,
    idle: 6
  };
  return rankMap[normalized] ?? 7;
};

const projection = computed(() => {
  const mission = props.mission;
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const members = Array.isArray(props.agents) ? props.agents : [];
  const memberMap = new Map(members.map((agent) => [String(agent.agent_id || '').trim(), agent]));
  const motherAgentId = String(
    mission?.mother_agent_id ||
      props.group?.mother_agent_id ||
      mission?.entry_agent_id ||
      members[0]?.agent_id ||
      ''
  ).trim();
  const entryAgentId = String(mission?.entry_agent_id || '').trim();
  const missionStatus = String(mission?.completion_status || mission?.status || '').trim().toLowerCase() || 'idle';
  const involvedAgentIds = new Set<string>();

  members.forEach((member) => {
    const agentId = String(member.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  if (motherAgentId) involvedAgentIds.add(motherAgentId);
  if (entryAgentId) involvedAgentIds.add(entryAgentId);

  if (!involvedAgentIds.size) {
    return {
      nodes: [] as any[],
      edges: [] as any[],
      nodeMetaMap: new Map<string, CanvasNodeMeta>(),
      memberMap,
      tasksByAgent: new Map<string, BeeroomMissionTask[]>(),
      motherNodeId: '',
      extent: { width: 0, height: 0 }
    };
  }

  const tasksByAgent = new Map<string, BeeroomMissionTask[]>();
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (!agentId) return;
    const bucket = tasksByAgent.get(agentId) || [];
    bucket.push(task);
    tasksByAgent.set(agentId, bucket);
  });

  const orderedAgentIds = Array.from(involvedAgentIds).sort((left, right) => {
    if (left === motherAgentId) return -1;
    if (right === motherAgentId) return 1;
    if (left === entryAgentId) return -1;
    if (right === entryAgentId) return 1;
    const leftTasks = tasksByAgent.get(left) || [];
    const rightTasks = tasksByAgent.get(right) || [];
    const leftStatus = resolveNodeStatus(leftTasks, memberMap.get(left), missionStatus);
    const rightStatus = resolveNodeStatus(rightTasks, memberMap.get(right), missionStatus);
    const rankDiff = resolveStatusRank(leftStatus) - resolveStatusRank(rightStatus);
    if (rankDiff !== 0) return rankDiff;
    const taskDiff = rightTasks.length - leftTasks.length;
    if (taskDiff !== 0) return taskDiff;
    return String(memberMap.get(left)?.name || left).localeCompare(
      String(memberMap.get(right)?.name || right),
      'zh-Hans-CN'
    );
  });

  const slots = buildHoneycombSlots(orderedAgentIds.length);
  const nodeMetaMap = new Map<string, CanvasNodeMeta>();
  let minX = 0;
  let maxX = 0;
  let minY = 0;
  let maxY = 0;

  const selectedNodeId = String(activeNodeId.value || '').trim();
  const nodes = orderedAgentIds.map((agentId, index) => {
    const member = memberMap.get(agentId);
    const agentTasks = tasksByAgent.get(agentId) || [];
    const isMother = agentId === motherAgentId || (!motherAgentId && index === 0);
    const status = resolveNodeStatus(agentTasks, member, missionStatus);
    const statusLabel = resolveStatusLabel(status);
    const nodeId = `agent:${agentId}`;
    const selected = nodeId === selectedNodeId;
    const roleLabel = isMother ? t('beeroom.canvas.legendMother') : t('beeroom.canvas.legendWorker');
    const name = String(member?.name || (isMother ? props.group?.mother_agent_name : '') || agentId).trim();
    const displayName = trimNodeTitle(name, 12);
    const accentColor = resolveNodeAccent(agentId, isMother);
    const summary = String(
      agentTasks
        .map((task) => task.result_summary || task.error || '')
        .find((item) => String(item || '').trim()) || member?.description || ''
    ).trim();
    const sessionTotal = Number(member?.active_session_total || 0);
    const avatar = avatarLabel(name);
    const slot = slots[index] || { q: 0, r: 0 };
    const position = nodePositionOverrides.value[nodeId] || resolveHoneycombPosition(slot);
    const width = isMother ? MOTHER_NODE_WIDTH : NODE_WIDTH;
    const height = isMother ? MOTHER_NODE_HEIGHT : NODE_HEIGHT;
    const innerHTML = buildCanvasNodeCardHtml({
      isMother,
      selected,
      accentColor,
      avatar,
      fullName: name,
      displayName,
      roleLabel,
      status,
      statusLabel,
      taskTotal: agentTasks.length,
      sessionTotal
    });

    minX = Math.min(minX, position.x - width / 2);
    maxX = Math.max(maxX, position.x + width / 2);
    minY = Math.min(minY, position.y - height / 2);
    maxY = Math.max(maxY, position.y + height / 2);

    const meta: CanvasNodeMeta = {
      id: nodeId,
      agent_id: agentId,
      agent_name: name,
      role: isMother ? 'mother' : 'worker',
      role_label: roleLabel,
      status,
      task_total: agentTasks.length,
      active_session_total: sessionTotal,
      updated_time: Math.max(
        ...agentTasks.map((task) => Number(task.updated_time || task.finished_time || task.started_time || 0)),
        Number(mission?.updated_time || 0)
      ),
      summary,
      entry_agent: agentId === entryAgentId
    };
    nodeMetaMap.set(nodeId, meta);

    return {
      id: nodeId,
      data: meta,
      style: {
        x: position.x,
        y: position.y,
        size: [width, height],
        dx: -Math.round(width / 2),
        dy: -Math.round(height / 2),
        innerHTML,
        cursor: 'pointer',
        label: false,
        badge: false,
        port: false
      }
    };
  });

  const edges: any[] = [];
  const effectiveMotherAgentId = motherAgentId || orderedAgentIds[0] || '';
  const motherNodeId = effectiveMotherAgentId ? `agent:${effectiveMotherAgentId}` : '';
  if (effectiveMotherAgentId) {
    orderedAgentIds.forEach((agentId) => {
      if (!agentId || agentId === effectiveMotherAgentId) return;
      const agentTasks = tasksByAgent.get(agentId) || [];
      const latestTask = [...agentTasks].sort(
        (left, right) => Number(right.started_time || right.updated_time || 0) - Number(left.started_time || left.updated_time || 0)
      )[0] || null;
      const latestStatus = String(latestTask?.status || '').trim().toLowerCase();
      const memberBusy = projectionMemberBusy(memberMap.get(agentId));
      const dispatchActive = ACTIVE_DISPATCH_STATUSES.has(latestStatus) || (memberBusy && agentTasks.length > 0);
      const dispatchTaskText = dispatchActive ? resolveDispatchTaskLabel(mission, latestTask) : '';
      const targetNodeId = `agent:${agentId}`;
      const edgeSelected = Boolean(selectedNodeId) && (selectedNodeId === motherNodeId || selectedNodeId === targetNodeId);
      const activeStroke = 'rgba(59, 130, 246, 0.92)';
      const idleStroke = 'rgba(148, 163, 184, 0.42)';
      edges.push({
        id: `dispatch:${effectiveMotherAgentId}:${agentId}`,
        source: motherNodeId,
        target: targetNodeId,
        style: {
          stroke: dispatchActive ? activeStroke : edgeSelected ? 'rgba(96, 165, 250, 0.72)' : idleStroke,
          lineWidth: dispatchActive ? 1.32 : edgeSelected ? 1.14 : 0.9,
          lineDash: dispatchActive ? [10, 8] : [5, 9],
          lineDashOffset: 0,
          radius: 14,
          opacity: dispatchActive ? 0.95 : edgeSelected ? 0.92 : 0.78,
          shadowColor: dispatchActive ? 'rgba(59, 130, 246, 0.28)' : 'rgba(0, 0, 0, 0)',
          shadowBlur: dispatchActive ? 6 : 0,
          endArrow: false,
          label: Boolean(dispatchTaskText),
          labelText: dispatchTaskText,
          labelPlacement: 0.54,
          labelOffsetY: -7,
          labelAutoRotate: true,
          labelFill: dispatchActive ? 'rgba(219, 234, 254, 0.96)' : 'rgba(203, 213, 225, 0.92)',
          labelFontSize: 10,
          labelFontWeight: 560,
          labelBackground: Boolean(dispatchTaskText),
          labelBackgroundFill: dispatchActive ? 'rgba(30, 64, 175, 0.62)' : 'rgba(30, 41, 59, 0.58)',
          labelBackgroundStroke: dispatchActive ? 'rgba(96, 165, 250, 0.38)' : 'rgba(148, 163, 184, 0.3)',
          labelBackgroundLineWidth: 1,
          labelBackgroundRadius: 999,
          labelPadding: [2, 8]
        },
        data: {
          kind: 'dispatch',
          task_id: latestTask?.task_id || null,
          active: dispatchActive,
          selected: edgeSelected
        }
      });
    });
  }

  return {
    nodes,
    edges,
    nodeMetaMap,
    memberMap,
    tasksByAgent,
    motherNodeId: motherNodeId || nodes[0]?.id || '',
    extent: {
      width: Math.max(0, maxX - minX),
      height: Math.max(0, maxY - minY)
    }
  };
});

const activeNodeMeta = computed(() => projection.value.nodeMetaMap.get(activeNodeId.value) || null);
const activeNodeMember = computed(() => {
  const meta = activeNodeMeta.value;
  return meta ? projection.value.memberMap.get(meta.agent_id) || null : null;
});
const activeNodeTasks = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as BeeroomMissionTask[];
  return [...(projection.value.tasksByAgent.get(meta.agent_id) || [])]
    .sort(
      (left, right) =>
        Number(right.updated_time || right.finished_time || right.started_time || 0) -
        Number(left.updated_time || left.finished_time || left.started_time || 0)
    )
    .slice(0, 3);
});
const activeNodeDescription = computed(() => {
  const member = activeNodeMember.value;
  const meta = activeNodeMeta.value;
  return String(member?.description || meta?.summary || t('beeroom.members.noDescription'));
});
const activeNodeStats = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as Array<{ key: string; label: string; value: string }>;
  return [
    {
      key: 'status',
      label: t('beeroom.canvas.currentStatus'),
      value: resolveStatusLabel(meta.status)
    },
    {
      key: 'sessions',
      label: t('beeroom.canvas.activeSessions'),
      value: String(meta.active_session_total || 0)
    },
    {
      key: 'tasks',
      label: t('beeroom.canvas.currentTaskTotal', { count: meta.task_total || 0 }),
      value: String(meta.task_total || 0)
    },
    {
      key: 'updated',
      label: t('beeroom.canvas.lastUpdate'),
      value: formatDateTime(meta.updated_time)
    }
  ];
});

const activeNodeContextRows = computed(() => {
  const mission = props.mission;
  return [
    {
      key: 'total',
      label: t('beeroom.canvas.contextTotal'),
      value: mission ? formatCount(mission.context_tokens_total) : '-'
    },
    {
      key: 'peak',
      label: t('beeroom.canvas.contextPeak'),
      value: mission ? formatCount(mission.context_tokens_peak) : '-'
    },
    {
      key: 'rounds',
      label: t('beeroom.canvas.modelRounds'),
      value: mission ? formatCount(mission.model_round_total) : '-'
    }
  ];
});

const activeNodeToolNames = computed(() => {
  const member = activeNodeMember.value;
  const values = Array.isArray(member?.tool_names) ? member.tool_names : [];
  return Array.from(new Set(values.map((item) => String(item || '').trim()).filter(Boolean))).slice(0, 8);
});

const resolveCollaborationMeta = (task: BeeroomMissionTask | null | undefined) => {
  if (!task) return '';
  if (String(task.session_run_id || '').trim()) {
    return `${t('beeroom.task.runId')} ${shortIdentity(task.session_run_id)}`;
  }
  const sessionId = task.spawned_session_id || task.target_session_id || '';
  if (String(sessionId || '').trim()) {
    return `${t('beeroom.task.sessionId')} ${shortIdentity(sessionId)}`;
  }
  return formatDateTime(task.updated_time || task.finished_time || task.started_time || 0);
};

const activeNodeCollaborationLinks = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as CanvasLinkItem[];

  const mission = props.mission;
  const motherMeta = projection.value.nodeMetaMap.get(projection.value.motherNodeId) || null;
  const latestTask = activeNodeTasks.value[0] || null;
  const links: CanvasLinkItem[] = [];

  if (meta.role === 'mother') {
    Array.from(projection.value.tasksByAgent.entries())
      .filter(([agentId, tasks]) => agentId !== meta.agent_id && tasks.length > 0)
      .map(([agentId, tasks]) => {
        const latest = [...tasks].sort(
          (left, right) =>
            Number(right.updated_time || right.finished_time || right.started_time || 0) -
            Number(left.updated_time || left.finished_time || left.started_time || 0)
        )[0];
        const workerMeta = projection.value.nodeMetaMap.get(`agent:${agentId}`);
        return {
          key: `mother:${agentId}:${latest?.task_id || ''}`,
          title: `${meta.agent_name} -> ${workerMeta?.agent_name || agentId}`,
          subtitle: t('beeroom.canvas.legendDispatch'),
          meta: resolveCollaborationMeta(latest),
          updated: Number(latest?.updated_time || latest?.finished_time || latest?.started_time || 0)
        };
      })
      .sort((left, right) => right.updated - left.updated)
      .slice(0, 3)
      .forEach((item) => {
        links.push({ key: item.key, title: item.title, subtitle: item.subtitle, meta: item.meta });
      });
    return links;
  }

  if (motherMeta?.agent_name) {
    links.push({
      key: `dispatch:${meta.agent_id}`,
      title: `${motherMeta.agent_name} -> ${meta.agent_name}`,
      subtitle: t('beeroom.canvas.legendDispatch'),
      meta: resolveCollaborationMeta(latestTask)
    });
  }

  if (latestTask) {
    const terminal = ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(
      String(latestTask.status || '').trim().toLowerCase()
    );
    if (terminal && motherMeta?.agent_name) {
      links.push({
        key: `report:${latestTask.task_id}`,
        title: `${meta.agent_name} -> ${motherMeta.agent_name}`,
        subtitle: t('beeroom.canvas.legendReport'),
        meta: String(latestTask.result_summary || latestTask.error || resolveStatusLabel(latestTask.status)).trim()
      });
    }
    const runOrSession = String(
      latestTask.session_run_id || latestTask.spawned_session_id || latestTask.target_session_id || ''
    ).trim();
    if (runOrSession) {
      links.push({
        key: `run:${runOrSession}`,
        title: meta.agent_name,
        subtitle: latestTask.session_run_id ? t('beeroom.task.runId') : t('beeroom.task.sessionId'),
        meta: shortIdentity(runOrSession)
      });
    }
  }

  if (!links.length && mission) {
    links.push({
      key: `mission:${mission.mission_id || mission.team_run_id}`,
      title: meta.agent_name,
      subtitle: t('beeroom.canvas.currentStatus'),
      meta: resolveStatusLabel(meta.status)
    });
  }
  return links.slice(0, 3);
});

const activeNodeToolSummaryText = computed(() => {
  const toolNames = activeNodeToolNames.value;
  if (!toolNames.length) {
    return t('beeroom.canvas.toolSummaryEmpty');
  }
  return t('beeroom.canvas.toolSummaryFallback', { count: toolNames.length });
});

const canvasStatusSummary = computed(() => {
  const summary = { running: 0, failed: 0, idle: 0 };
  projection.value.nodeMetaMap.forEach((meta) => {
    const status = String(meta.status || '').trim().toLowerCase();
    if (status === 'running' || status === 'queued' || status === 'awaiting_idle') {
      summary.running += 1;
      return;
    }
    if (status === 'failed' || status === 'error' || status === 'timeout' || status === 'cancelled') {
      summary.failed += 1;
      return;
    }
    summary.idle += 1;
  });
  return summary;
});

const hoveredNodeMeta = computed(() => projection.value.nodeMetaMap.get(hoveredNodeId.value) || null);
const hoveredNodeMember = computed(() => {
  const meta = hoveredNodeMeta.value;
  return meta ? projection.value.memberMap.get(meta.agent_id) || null : null;
});
const hoveredNodeDescription = computed(() => {
  const member = hoveredNodeMember.value;
  const meta = hoveredNodeMeta.value;
  return String(member?.description || meta?.summary || t('beeroom.members.noDescription'));
});
const hoveredTooltipStyle = computed(() => {
  const boardRect = boardRef.value?.getBoundingClientRect();
  const boardWidth = Math.max(0, Number(boardRect?.width || boardRef.value?.clientWidth || 0));
  const boardHeight = Math.max(0, Number(boardRect?.height || boardRef.value?.clientHeight || 0));
  const tooltipWidth = 260;
  const tooltipHeight = 168;
  const x = Math.max(12, Math.min(boardWidth - tooltipWidth - 12, hoveredTooltipPosition.value.x + 18));
  const y = Math.max(12, Math.min(boardHeight - tooltipHeight - 12, hoveredTooltipPosition.value.y + 18));
  return {
    left: `${Math.round(x)}px`,
    top: `${Math.round(y)}px`
  };
});

const resolveAgentNameById = (agentId: unknown) => {
  const normalized = String(agentId || '').trim();
  if (!normalized) return '-';
  if (normalized === DEFAULT_AGENT_KEY) {
    return t('messenger.defaultAgent');
  }
  const member = projection.value.memberMap.get(normalized);
  if (member?.name) {
    return String(member.name).trim();
  }
  const meta = projection.value.nodeMetaMap.get(`agent:${normalized}`);
  return String(meta?.agent_name || normalized).trim();
};

const normalizeComparableName = (value: unknown) =>
  String(value || '')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '');

const resolveMemberAgentKey = (member: BeeroomMember) => {
  const raw = String(member.agent_id || '').trim();
  if (raw) return raw;
  return member.hive_id ? DEFAULT_AGENT_KEY : '';
};

const motherAgentId = computed(() =>
  String(
    props.mission?.mother_agent_id ||
      props.group?.mother_agent_id ||
      props.mission?.entry_agent_id ||
      props.agents[0]?.agent_id ||
      ''
  ).trim()
);

const composerTargetOptions = computed<ComposerTargetOption[]>(() => {
  const seen = new Set<string>();
  const motherId = motherAgentId.value;
  const options: ComposerTargetOption[] = [];

  const pushOption = (agentId: string, role: 'mother' | 'worker') => {
    const normalized = String(agentId || '').trim();
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    const label = role === 'mother'
      ? `${resolveAgentNameById(normalized)} (${t('beeroom.summary.motherAgent')})`
      : resolveAgentNameById(normalized);
    options.push({ agentId: normalized, label, role });
  };

  if (motherId) {
    pushOption(motherId, 'mother');
  }
  props.agents.forEach((member) => {
    const agentId = resolveMemberAgentKey(member);
    if (!agentId || agentId === motherId) return;
    pushOption(agentId, 'worker');
  });
  return options;
});

const composerCanSend = computed(
  () => String(composerText.value || '').trim().length > 0 && !!composerTargetOptions.value.length
);

const activeGroupId = computed(() => String(props.group?.group_id || '').trim());
const dispatchApprovals = computed<DispatchApprovalItem[]>(() => {
  const sessionId = String(dispatchSessionId.value || '').trim();
  if (!sessionId) return [];
  const source = Array.isArray(chatStore.pendingApprovals)
    ? (chatStore.pendingApprovals as Record<string, unknown>[])
    : [];
  return source
    .map((item) => {
      if (!item || typeof item !== 'object') return null;
      const currentSessionId = String(item.session_id || '').trim();
      const approvalId = String(item.approval_id || '').trim();
      if (!approvalId || currentSessionId !== sessionId) return null;
      return {
        approval_id: approvalId,
        session_id: currentSessionId,
        tool: String(item.tool || '').trim(),
        summary: String(item.summary || '').trim()
      } satisfies DispatchApprovalItem;
    })
    .filter((item: DispatchApprovalItem | null): item is DispatchApprovalItem => Boolean(item));
});
const dispatchApprovalBusy = computed(() => dispatchRespondingApprovalId.value !== '');
const dispatchCanStop = computed(() => Boolean(dispatchSessionId.value) && composerSending.value);
const dispatchCanResume = computed(
  () =>
    Boolean(dispatchSessionId.value) &&
    !composerSending.value &&
    dispatchLastEventId.value > 0 &&
    (dispatchRuntimeStatus.value === 'stopped' || dispatchRuntimeStatus.value === 'failed')
);
const dispatchRuntimeLabel = computed(() => {
  const keyMap: Record<DispatchRuntimeStatus, string> = {
    idle: 'beeroom.canvas.chatStandby',
    queued: 'beeroom.status.queued',
    running: 'beeroom.status.running',
    awaiting_approval: 'chat.approval.title',
    resuming: 'chat.message.resume',
    stopped: 'chat.workflow.aborted',
    completed: 'beeroom.status.completed',
    failed: 'beeroom.status.failed'
  };
  return t(keyMap[dispatchRuntimeStatus.value] || 'beeroom.status.unknown');
});
const dispatchRuntimeTone = computed(() => {
  if (dispatchRuntimeStatus.value === 'failed') return 'danger';
  if (dispatchRuntimeStatus.value === 'completed') return 'success';
  if (dispatchRuntimeStatus.value === 'awaiting_approval') return 'warn';
  if (dispatchRuntimeStatus.value === 'queued' || dispatchRuntimeStatus.value === 'running' || dispatchRuntimeStatus.value === 'resuming') {
    return 'running';
  }
  return 'idle';
});

const normalizeStreamEventId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value || '').trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

const updateDispatchLastEventId = (value: unknown) => {
  const normalized = normalizeStreamEventId(value);
  if (normalized > dispatchLastEventId.value) {
    dispatchLastEventId.value = normalized;
  }
};

const nextManualMessageKey = (prefix: string) => {
  manualMessageSerial += 1;
  return `${prefix}:${Date.now()}:${manualMessageSerial}`;
};

const appendManualChatMessage = (message: MissionChatMessage) => {
  const current = Array.isArray(manualChatMessages.value) ? manualChatMessages.value : [];
  const existingIndex = current.findIndex((item) => item.key === message.key);
  const merged =
    existingIndex >= 0
      ? current.map((item, index) => (index === existingIndex ? message : item))
      : [...current, message];
  manualChatMessages.value = merged
    .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
    .slice(-MANUAL_CHAT_HISTORY_LIMIT);
};

const replaceManualChatMessages = (messages: MissionChatMessage[]) => {
  manualChatMessages.value = [...messages]
    .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
    .slice(-MANUAL_CHAT_HISTORY_LIMIT);
};

const sameManualChatMessages = (left: MissionChatMessage[], right: MissionChatMessage[]) => {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    const leftItem = left[index];
    const rightItem = right[index];
    if (!leftItem || !rightItem) return false;
    if (
      leftItem.key !== rightItem.key ||
      leftItem.time !== rightItem.time ||
      leftItem.tone !== rightItem.tone ||
      leftItem.senderName !== rightItem.senderName ||
      leftItem.senderAgentId !== rightItem.senderAgentId ||
      leftItem.mention !== rightItem.mention ||
      leftItem.body !== rightItem.body ||
      leftItem.meta !== rightItem.meta
    ) {
      return false;
    }
  }
  return true;
};

const loadManualChatHistory = async () => {
  const groupId = activeGroupId.value;
  if (!groupId) {
    manualChatMessages.value = [];
    return;
  }
  try {
    const response = await listBeeroomChatMessages(groupId, { limit: MANUAL_CHAT_HISTORY_LIMIT });
    const items = Array.isArray(response?.data?.data?.items)
      ? response.data.data.items
          .map((item: unknown) => mapApiChatMessage(item))
          .filter((item: MissionChatMessage | null): item is MissionChatMessage => !!item)
      : [];
    const next = [...items]
      .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    if (!sameManualChatMessages(manualChatMessages.value, next)) {
      replaceManualChatMessages(next);
    }
  } catch {
    // Keep the last successful snapshot to avoid empty flashes on transient network errors.
  }
};

const persistManualChatMessage = async (payload: {
  senderKind: string;
  senderName: string;
  senderAgentId?: string;
  mention?: string;
  mentionAgentId?: string;
  body: string;
  meta?: string;
  tone: MissionChatMessage['tone'];
  createdAt?: number;
  clientMsgId?: string;
}) => {
  const groupId = activeGroupId.value;
  if (!groupId) {
    throw new Error(t('common.requestFailed'));
  }
  const response = await appendBeeroomChatMessage(groupId, {
    senderKind: payload.senderKind,
    senderName: payload.senderName,
    senderAgentId: payload.senderAgentId,
    mentionName: payload.mention,
    mentionAgentId: payload.mentionAgentId,
    body: payload.body,
    meta: payload.meta,
    tone: payload.tone,
    createdAt: payload.createdAt,
    clientMsgId: payload.clientMsgId
  });
  const message = mapApiChatMessage(response?.data?.data);
  if (!message) {
    throw new Error(t('common.requestFailed'));
  }
  appendManualChatMessage(message);
  return message;
};

const clearManualChatHistory = async () => {
  composerError.value = '';
  try {
    const groupId = activeGroupId.value;
    if (groupId) {
      await clearBeeroomChatMessages(groupId);
    }
    manualChatMessages.value = [];
  } catch {
    ElMessage.error(t('common.requestFailed'));
  }
};

const clipMessageBody = (value: unknown, limit = 240) => {
  const text = String(value || '').trim();
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, limit)}...`;
};

const safeJsonParse = (value: unknown) => {
  try {
    return JSON.parse(String(value || '')) as Record<string, any>;
  } catch {
    return null;
  }
};

const extractReplyText = (payload: Record<string, any> | null) => {
  const candidates = [
    payload?.content,
    payload?.reply,
    payload?.message,
    payload?.text,
    payload?.data?.content,
    payload?.data?.reply,
    payload?.data?.message,
    payload?.data?.text,
    payload?.data?.final_reply,
    payload?.final_reply
  ];
  return clipMessageBody(candidates.find((item) => String(item || '').trim()) || '');
};

const extractErrorText = (payload: Record<string, any> | null) => {
  const candidates = [
    payload?.detail,
    payload?.error,
    payload?.message,
    payload?.data?.detail,
    payload?.data?.error,
    payload?.data?.message
  ];
  return String(candidates.find((item) => String(item || '').trim()) || '').trim();
};

const resolveDispatchTarget = (rawContent: string) => {
  const text = String(rawContent || '').trim();
  const explicitMatch = text.match(/^@([^\s]+)\s*(.*)$/);
  const normalizedExplicit = normalizeComparableName(explicitMatch?.[1] || '');
  const explicitTarget = normalizedExplicit
    ? composerTargetOptions.value.find((item) => {
        const optionName = normalizeComparableName(resolveAgentNameById(item.agentId));
        const optionLabel = normalizeComparableName(item.label.replace(/\(.+?\)/g, ''));
        return optionName === normalizedExplicit || optionLabel === normalizedExplicit;
      }) || null
    : null;
  const target = explicitTarget ||
    composerTargetOptions.value.find((item) => item.agentId === composerTargetAgentId.value) ||
    composerTargetOptions.value[0] ||
    null;
  return {
    target,
    body: String(explicitMatch?.[2] || text).trim() || text
  };
};

const normalizeDispatchAgentId = (agentId: string): string =>
  agentId === DEFAULT_AGENT_KEY ? '' : String(agentId || '').trim();

const toSessionTimestampMs = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const parsed = new Date(text).getTime();
  return Number.isNaN(parsed) ? 0 : parsed;
};

const sortChatSessionsByActivity = (sessions: Record<string, unknown>[]): Record<string, unknown>[] =>
  [...sessions]
    .map((session, index) => ({ session, index }))
    .sort((left, right) => {
      const leftAt = toSessionTimestampMs(
        left.session.updated_at ?? left.session.last_message_at ?? left.session.created_at
      );
      const rightAt = toSessionTimestampMs(
        right.session.updated_at ?? right.session.last_message_at ?? right.session.created_at
      );
      if (leftAt !== rightAt) {
        return rightAt - leftAt;
      }
      return left.index - right.index;
    })
    .map((item) => item.session);

const syncDispatchSessionToChatStore = (payload: {
  sessionId: string;
  agentId: string;
  sessionSummary: Record<string, unknown> | null;
  userPreview: string;
}) => {
  const targetSessionId = String(payload.sessionId || '').trim();
  if (!targetSessionId) return;
  const nowIso = new Date().toISOString();
  const preview = clipMessageBody(payload.userPreview, 120);
  const fallbackAgentId = normalizeDispatchAgentId(payload.agentId);
  const summary = payload.sessionSummary || null;
  const summaryAgentId = String(summary?.agent_id || '').trim();
  const nextSession: Record<string, unknown> = {
    ...(summary || {}),
    id: targetSessionId,
    agent_id: summaryAgentId || fallbackAgentId,
    updated_at: nowIso,
    last_message_at: nowIso
  };
  if (preview) {
    nextSession.last_message_preview = preview;
    nextSession.last_user_message_preview = preview;
  }
  if (!String(nextSession.title || '').trim()) {
    nextSession.title = preview || t('chat.newSession');
  }
  const currentSessions = Array.isArray(chatStore.sessions)
    ? (chatStore.sessions as Record<string, unknown>[])
    : [];
  const targetIndex = currentSessions.findIndex(
    (item) => String(item?.id || '').trim() === targetSessionId
  );
  const mergedSessions = [...currentSessions];
  if (targetIndex >= 0) {
    mergedSessions[targetIndex] = {
      ...mergedSessions[targetIndex],
      ...nextSession
    };
  } else {
    mergedSessions.unshift(nextSession);
  }
  chatStore.sessions = sortChatSessionsByActivity(mergedSessions);
};

const ensureDispatchSession = async (agentId: string): Promise<DispatchSessionTarget> => {
  const apiAgentId = agentId === DEFAULT_AGENT_KEY ? '' : agentId;
  const { data } = await listSessions({ agent_id: apiAgentId });
  const source = Array.isArray(data?.data?.items) ? data.data.items : [];
  const matched = source
    .filter((item) => {
      const sessionAgentId = String(item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')).trim();
      return sessionAgentId === agentId;
    })
    .sort((left, right) => {
      const leftTime = toSessionTimestampMs(
        left?.updated_at ?? left?.last_message_at ?? left?.created_at
      );
      const rightTime = toSessionTimestampMs(
        right?.updated_at ?? right?.last_message_at ?? right?.created_at
      );
      return rightTime - leftTime;
    });
  const primary = matched.find((item) => item?.is_main === true) || matched[0];
  if (primary?.id) {
    return {
      sessionId: String(primary.id),
      sessionSummary: primary && typeof primary === 'object' ? (primary as Record<string, unknown>) : null
    };
  }
  const created = await createSession(agentId === DEFAULT_AGENT_KEY ? {} : { agent_id: agentId });
  const createdSummary = created?.data?.data;
  return {
    sessionId: String(createdSummary?.id || ''),
    sessionSummary:
      createdSummary && typeof createdSummary === 'object'
        ? (createdSummary as Record<string, unknown>)
        : null
  };
};

const scrollChatToBottom = async () => {
  await nextTick();
  const element = chatStreamRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const resetDispatchRuntime = (options: { keepSession?: boolean } = {}) => {
  if (dispatchStreamController) {
    dispatchStreamController.abort();
    dispatchStreamController = null;
  }
  composerSending.value = false;
  dispatchStopRequested = false;
  dispatchRespondingApprovalId.value = '';
  dispatchRequestId.value = '';
  dispatchRuntimeStatus.value = 'idle';
  if (!options.keepSession) {
    dispatchSessionId.value = '';
    dispatchLastEventId.value = 0;
    dispatchTargetAgentId.value = '';
    dispatchTargetName.value = '';
    dispatchTargetTone.value = 'worker';
  }
};

const consumeDispatchStream = async (response: Response) => {
  let finalPayload: Record<string, any> | null = null;
  let streamError = '';
  await consumeSseStream(response, (eventType, dataText, eventId) => {
    updateDispatchLastEventId(eventId);
    const payload = safeJsonParse(dataText);
    const data = payload?.data ?? payload;
    updateDispatchLastEventId(data?.event_id ?? data?.eventId ?? payload?.event_id ?? payload?.eventId);
    const eventRequestId = String(
      payload?.request_id ??
      payload?.requestId ??
      data?.request_id ??
      data?.requestId ??
      ''
    ).trim();
    if (eventRequestId) {
      dispatchRequestId.value = eventRequestId;
    }

    if (eventType === 'heartbeat' || eventType === 'ping') {
      return;
    }
    if (eventType === 'approval_request') {
      chatStore.enqueueApprovalRequest(dispatchRequestId.value, dispatchSessionId.value, data);
      dispatchRuntimeStatus.value = 'awaiting_approval';
      return;
    }
    if (eventType === 'approval_result') {
      chatStore.resolveApprovalResult(data);
      const status = String(data?.status || payload?.status || '').trim().toLowerCase();
      dispatchRuntimeStatus.value = status === 'approved' ? 'running' : 'failed';
      return;
    }
    if (eventType === 'queued') {
      dispatchRuntimeStatus.value = 'queued';
      return;
    }
    if (eventType === 'slow_client') {
      dispatchRuntimeStatus.value = 'stopped';
      return;
    }
    if (eventType === 'error') {
      streamError = extractErrorText(payload) || t('common.requestFailed');
      dispatchRuntimeStatus.value = 'failed';
      return;
    }
    if (eventType === 'final') {
      finalPayload = payload;
      dispatchRuntimeStatus.value = 'completed';
      return;
    }
    dispatchRuntimeStatus.value = 'running';
  });

  if (streamError) {
    throw new Error(streamError);
  }
  return finalPayload;
};

const startDispatchStream = async (
  mode: 'send' | 'resume',
  sessionId: string,
  payload: { content?: string; afterEventId?: number } = {}
) => {
  if (dispatchStreamController) {
    dispatchStreamController.abort();
  }
  dispatchStopRequested = false;
  composerSending.value = true;
  dispatchRuntimeStatus.value = mode === 'resume' ? 'resuming' : 'running';
  const controller = new AbortController();
  dispatchStreamController = controller;

  const response =
    mode === 'resume'
      ? await resumeMessageStream(sessionId, {
          signal: controller.signal,
          afterEventId:
            Number.isFinite(payload.afterEventId) && Number(payload.afterEventId) > 0
              ? Number(payload.afterEventId)
              : undefined
        })
      : await sendMessageStream(
          sessionId,
          { content: String(payload.content || ''), stream: true },
          { signal: controller.signal }
        );

  if (!response.ok) {
    const errorText = String(await response.text()).trim();
    throw new Error(
      errorText || (mode === 'resume' ? t('chat.error.resumeFailed') : t('common.requestFailed'))
    );
  }

  return consumeDispatchStream(response);
};

const persistDispatchReply = async (finalPayload: Record<string, any> | null) => {
  await persistManualChatMessage({
    senderKind: 'agent',
    senderName: dispatchTargetName.value || t('messenger.section.swarms'),
    senderAgentId: dispatchTargetAgentId.value,
    body: extractReplyText(finalPayload) || t('beeroom.canvas.chatDispatchAccepted'),
    meta: t('beeroom.canvas.chatResultMeta'),
    tone: dispatchTargetTone.value,
    createdAt: Math.floor(Date.now() / 1000),
    clientMsgId: nextManualMessageKey('reply')
  });
  await scrollChatToBottom();
};

const handleComposerSend = async () => {
  if (composerSending.value) return;
  const content = String(composerText.value || '').trim();
  if (!content) return;

  const { target, body } = resolveDispatchTarget(content);
  if (!target?.agentId) {
    const message = t('beeroom.canvas.chatTargetRequired');
    composerError.value = message;
    ElMessage.warning(message);
    return;
  }

  const targetName = resolveAgentNameById(target.agentId);
  const now = Math.floor(Date.now() / 1000);
  const visibleBody = String(body || content).trim();
  const targetTone = target.role === 'mother' ? 'mother' : 'worker';

  composerError.value = '';
  composerText.value = '';
  dispatchTargetAgentId.value = target.agentId;
  dispatchTargetName.value = targetName;
  dispatchTargetTone.value = targetTone;
  try {
    await persistManualChatMessage({
      senderKind: 'user',
      senderName: t('chat.message.user'),
      mention: targetName,
      mentionAgentId: target.agentId,
      body: visibleBody,
      meta: props.group?.name || props.group?.group_id || '',
      tone: 'user',
      createdAt: now,
      clientMsgId: nextManualMessageKey('user')
    });
    await persistManualChatMessage({
      senderKind: 'system',
      senderName: t('messenger.section.swarms'),
      mention: targetName,
      mentionAgentId: target.agentId,
      body: t('beeroom.canvas.chatDispatchPending'),
      meta: props.group?.group_id || '',
      tone: 'system',
      createdAt: now,
      clientMsgId: nextManualMessageKey('dispatch')
    });
    await scrollChatToBottom();

    const dispatchSession = await ensureDispatchSession(target.agentId);
    const sessionId = String(dispatchSession.sessionId || '').trim();
    if (!sessionId) {
      throw new Error(t('common.requestFailed'));
    }
    dispatchSessionId.value = sessionId;
    dispatchRequestId.value = nextManualMessageKey('dispatch-request');
    dispatchLastEventId.value = 0;
    dispatchRuntimeStatus.value = 'queued';
    syncDispatchSessionToChatStore({
      sessionId,
      agentId: target.agentId,
      sessionSummary: dispatchSession.sessionSummary,
      userPreview: visibleBody
    });
    const finalPayload = await startDispatchStream('send', sessionId, { content: visibleBody });
    await persistDispatchReply(finalPayload);
  } catch (error: any) {
    if (error?.name === 'AbortError' || dispatchStopRequested) {
      dispatchRuntimeStatus.value = 'stopped';
      return;
    }
    const message = String(error?.message || '').trim() || t('common.requestFailed');
    dispatchRuntimeStatus.value = 'failed';
    composerError.value = message;
    try {
      await persistManualChatMessage({
        senderKind: 'system',
        senderName: t('messenger.section.swarms'),
        mention: targetName,
        mentionAgentId: target.agentId,
        body: t('beeroom.canvas.chatDispatchFailed'),
        meta: message,
        tone: 'system',
        createdAt: Math.floor(Date.now() / 1000),
        clientMsgId: nextManualMessageKey('error')
      });
    } catch {
      // Keep the original dispatch error visible even if chat persistence fails.
    }
    await scrollChatToBottom();
    ElMessage.error(message);
  } finally {
    if (dispatchSessionId.value) {
      void chatStore.preloadSessionDetail(dispatchSessionId.value).catch(() => undefined);
    }
    dispatchStreamController = null;
    composerSending.value = false;
  }
};

const handleDispatchStop = async () => {
  if (!dispatchCanStop.value) return;
  const sessionId = String(dispatchSessionId.value || '').trim();
  if (!sessionId) return;
  dispatchStopRequested = true;
  dispatchRuntimeStatus.value = 'stopped';
  if (dispatchStreamController) {
    dispatchStreamController.abort();
    dispatchStreamController = null;
  }
  try {
    await cancelMessageStream(sessionId);
  } catch {
    // Keep local interrupt behavior even if cancel API fails.
  } finally {
    composerSending.value = false;
  }
  try {
    await persistManualChatMessage({
      senderKind: 'system',
      senderName: t('messenger.section.swarms'),
      mention: dispatchTargetName.value,
      mentionAgentId: dispatchTargetAgentId.value,
      body: t('chat.workflow.aborted'),
      meta: t('chat.workflow.abortedDetail'),
      tone: 'system',
      createdAt: Math.floor(Date.now() / 1000),
      clientMsgId: nextManualMessageKey('abort')
    });
    await scrollChatToBottom();
  } catch {
    // Ignore local command-log persistence failures.
  }
};

const handleDispatchResume = async () => {
  if (!dispatchCanResume.value) return;
  const sessionId = String(dispatchSessionId.value || '').trim();
  if (!sessionId) return;
  composerError.value = '';
  try {
    const finalPayload = await startDispatchStream('resume', sessionId, {
      afterEventId: dispatchLastEventId.value
    });
    await persistDispatchReply(finalPayload);
  } catch (error: any) {
    if (error?.name === 'AbortError' || dispatchStopRequested) {
      dispatchRuntimeStatus.value = 'stopped';
      return;
    }
    const message = String(error?.message || '').trim() || t('chat.error.resumeFailed');
    dispatchRuntimeStatus.value = 'failed';
    composerError.value = message;
    ElMessage.error(message);
  } finally {
    if (dispatchSessionId.value) {
      void chatStore.preloadSessionDetail(dispatchSessionId.value).catch(() => undefined);
    }
    dispatchStreamController = null;
    composerSending.value = false;
  }
};

const handleDispatchApproval = async (
  decision: 'approve_once' | 'approve_session' | 'deny',
  approvalId: string
) => {
  const normalizedApprovalId = String(approvalId || '').trim();
  if (!normalizedApprovalId || dispatchRespondingApprovalId.value) return;
  dispatchRespondingApprovalId.value = normalizedApprovalId;
  try {
    await chatStore.respondApproval(decision, normalizedApprovalId);
    if (decision !== 'deny') {
      ElMessage.success(t('chat.approval.sent'));
      dispatchRuntimeStatus.value = 'running';
    }
  } catch {
    ElMessage.error(t('chat.approval.sendFailed'));
  } finally {
    dispatchRespondingApprovalId.value = '';
  }
};

const chatPanelSubtitle = computed(() => {
  const mission = props.mission;
  const groupName = props.group?.name || props.group?.group_id || t('messenger.section.swarms');
  if (!mission) {
    return `${groupName} / ${t('beeroom.canvas.chatStandby')}`;
  }
  return `${groupName} / ${resolveStatusLabel(mission.completion_status || mission.status)} / ${t('beeroom.missions.taskCount', {
    count: mission.task_total || 0
  })}`;
});

const missionChatMessages = computed(() => {
  const mission = props.mission;
  const messages: MissionChatMessage[] = [];

  if (!mission) {
    messages.push({
      key: 'standby',
      senderName: t('messenger.section.swarms'),
      senderAgentId: '',
      mention: '',
      body: t('beeroom.canvas.chatStandbyBody', { count: projection.value.nodes.length || props.agents.length || 0 }),
      meta: props.group?.mother_agent_name || props.group?.mother_agent_id
        ? `${t('beeroom.summary.motherAgent')}: ${props.group?.mother_agent_name || props.group?.mother_agent_id}`
        : '',
      time: Number(props.group?.updated_time || props.group?.created_time || 0),
      timeLabel: formatDateTime(props.group?.updated_time || props.group?.created_time || 0),
      tone: 'system'
    });
    return messages;
  }

  const motherAgentId = String(
    mission.mother_agent_id || props.group?.mother_agent_id || mission.entry_agent_id || ''
  ).trim();
  const motherName = resolveAgentNameById(motherAgentId || props.group?.mother_agent_name || '');
  const entryName = resolveAgentNameById(mission.entry_agent_id || motherAgentId || '');
  const kickoffBody = String(mission.summary || mission.strategy || t('beeroom.missions.noSummary')).trim();

  messages.push({
    key: `kickoff:${mission.mission_id || mission.team_run_id}`,
    senderName: entryName,
    senderAgentId: String(mission.entry_agent_id || motherAgentId || '').trim(),
    mention: motherAgentId && String(mission.entry_agent_id || '').trim() !== motherAgentId ? motherName : '',
    body: kickoffBody,
    meta: `${t('beeroom.canvas.currentStatus')}: ${resolveStatusLabel(mission.completion_status || mission.status)}`,
    time: Number(mission.started_time || mission.updated_time || 0),
    timeLabel: formatDateTime(mission.started_time || mission.updated_time || 0),
    tone: 'mother'
  });

  (Array.isArray(mission.tasks) ? mission.tasks : []).forEach((task) => {
    const workerName = resolveAgentNameById(task.agent_id);
    const startedTime = Number(task.started_time || task.updated_time || 0);
    const finishedTime = Number(task.finished_time || task.updated_time || task.started_time || 0);
    const priority = Number(task.priority || 0);
    const taskId = shortTaskId(task.task_id);
    const runMeta = resolveCollaborationMeta(task);

    messages.push({
      key: `dispatch:${task.task_id}`,
      senderName: motherName,
      senderAgentId: motherAgentId,
      mention: workerName,
      body: t('beeroom.canvas.chatDispatchBody', { taskId, priority }),
      meta: runMeta,
      time: startedTime,
      timeLabel: formatDateTime(startedTime),
      tone: 'mother'
    });

    const normalizedStatus = String(task.status || '').trim().toLowerCase();
    if (['queued', 'running', 'awaiting_idle'].includes(normalizedStatus)) {
      messages.push({
        key: `accept:${task.task_id}`,
        senderName: workerName,
        senderAgentId: String(task.agent_id || '').trim(),
        mention: motherName,
        body: t('beeroom.canvas.chatAcceptBody', { taskId }),
        meta: resolveStatusLabel(task.status),
        time: Number(task.updated_time || task.started_time || 0),
        timeLabel: formatDateTime(task.updated_time || task.started_time || 0),
        tone: 'worker'
      });
    }

    if (task.result_summary || task.error || ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(normalizedStatus)) {
      messages.push({
        key: `result:${task.task_id}`,
        senderName: workerName,
        senderAgentId: String(task.agent_id || '').trim(),
        mention: motherName,
        body: String(task.result_summary || task.error || resolveStatusLabel(task.status)).trim(),
        meta: runMeta,
        time: finishedTime,
        timeLabel: formatDateTime(finishedTime),
        tone: 'worker'
      });
    }
  });

  return messages.sort((left, right) => left.time - right.time).slice(-24);
});

const displayChatMessages = computed(() =>
  [...missionChatMessages.value, ...manualChatMessages.value].sort(
    (left, right) => left.time - right.time || left.key.localeCompare(right.key)
  )
);

watch(
  composerTargetOptions,
  (options) => {
    if (!options.length) {
      composerTargetAgentId.value = '';
      return;
    }
    if (!options.some((item) => item.agentId === composerTargetAgentId.value)) {
      composerTargetAgentId.value = options[0]?.agentId || '';
    }
  },
  { immediate: true }
);

watch(
  activeGroupId,
  (groupId) => {
    resetDispatchRuntime();
    composerText.value = '';
    composerError.value = '';
    void loadManualChatHistory();
    stopChatRealtimeWatch();
    if (String(groupId || '').trim()) {
      startChatRealtimeWatch(String(groupId || '').trim());
    }
    restartChatPolling();
  },
  { immediate: true }
);

watch(
  () => [displayChatMessages.value.length, chatCollapsed.value] as const,
  async ([, collapsed]) => {
    if (collapsed) return;
    await scrollChatToBottom();
  },
  { immediate: true }
);

const shortTaskId = (value: unknown) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= 12) return text;
  return `${text.slice(0, 6)}...${text.slice(-4)}`;
};

const renderSignature = computed(() => {
  const mission = props.mission;
  const agentsSignature = (Array.isArray(props.agents) ? props.agents : [])
    .map((agent) => `${agent.agent_id}:${agent.idle === false ? 'busy' : 'idle'}:${agent.active_session_total || 0}`)
    .join('|');
  if (!mission) {
    return ['standby', props.group?.group_id || '', props.group?.mother_agent_id || '', agentsSignature].join('||');
  }
  const tasksSignature = (Array.isArray(mission.tasks) ? mission.tasks : [])
    .map((task) => `${task.task_id}:${task.agent_id}:${task.status || ''}:${task.updated_time || 0}:${task.finished_time || 0}`)
    .join('|');
  return [
    mission.mission_id || mission.team_run_id || '',
    mission.status || '',
    mission.completion_status || '',
    mission.updated_time || 0,
    mission.finished_time || 0,
    tasksSignature,
    agentsSignature
  ].join('||');
});

const stableMissionScopeKey = ref('');

watch(
  () => props.group?.group_id,
  () => {
    stableMissionScopeKey.value = '';
  },
  { immediate: true }
);

watch(
  () => [props.mission?.mission_id, props.mission?.team_run_id] as const,
  ([missionId, teamRunId]) => {
    const key = String(missionId || teamRunId || '').trim();
    if (key) {
      stableMissionScopeKey.value = key;
    }
  },
  { immediate: true }
);

const nodePositionScopeKey = computed(() =>
  String(props.mission?.mission_id || props.mission?.team_run_id || stableMissionScopeKey.value || props.group?.group_id || 'standby').trim()
);

const layoutSignature = computed(() => {
  const identity = props.mission?.mission_id || props.mission?.team_run_id || props.group?.group_id || 'standby';
  return [identity, projection.value.nodes.length, projection.value.edges.length, projection.value.extent.width, projection.value.extent.height].join(':');
});

let lastLayoutSignature = '';
let lastMissionIdentity = '';
let currentCanvasScopeKey = '';
let pendingCanvasViewportRestore: PendingCanvasViewportRestore | null = null;

const resolveCanvasScopeKey = (value?: unknown) => {
  const base = value ?? nodePositionScopeKey.value;
  const key = String(base || '').trim();
  return key || 'standby';
};

const clonePositionOverrides = (source: Record<string, CanvasPositionOverride>) => {
  const cloned: Record<string, CanvasPositionOverride> = {};
  Object.entries(source || {}).forEach(([nodeId, override]) => {
    const id = String(nodeId || '').trim();
    if (!id) return;
    const x = Number(override?.x);
    const y = Number(override?.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    cloned[id] = { x, y };
  });
  return cloned;
};

const hasPositionOverrides = (source?: Record<string, CanvasPositionOverride> | null) =>
  !!source && Object.keys(source).length > 0;

const normalizeGraphPosition = (value: unknown): [number, number] | null => {
  if (!value || typeof value !== 'object') return null;
  const array = Array.from(value as ArrayLike<number>);
  const x = Number(array[0]);
  const y = Number(array[1]);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return null;
  }
  return [x, y];
};

const clearCanvasContainers = () => {
  if (canvasRef.value) {
    canvasRef.value.innerHTML = '';
  }
  if (minimapRef.value) {
    minimapRef.value.innerHTML = '';
  }
};

const getCanvasViewport = () => {
  const rect = canvasRef.value?.getBoundingClientRect();
  return {
    width: Math.floor(rect?.width || canvasRef.value?.clientWidth || 0),
    height: Math.floor(rect?.height || canvasRef.value?.clientHeight || 0)
  };
};

const getCanvasViewportCenter = (): [number, number] => {
  const viewport = getCanvasViewport();
  return [Math.max(1, viewport.width / 2), Math.max(1, viewport.height / 2)];
};

const clampCanvasZoom = (zoom: number) => Math.min(CANVAS_ZOOM_MAX, Math.max(CANVAS_ZOOM_MIN, zoom));

const clearCanvasWheelSaveTimer = () => {
  if (canvasWheelSaveTimer !== null) {
    window.clearTimeout(canvasWheelSaveTimer);
    canvasWheelSaveTimer = null;
  }
};

const scheduleCanvasStateSave = (delayMs = 120) => {
  if (typeof window === 'undefined') {
    saveCanvasState();
    return;
  }
  clearCanvasWheelSaveTimer();
  canvasWheelSaveTimer = window.setTimeout(() => {
    canvasWheelSaveTimer = null;
    saveCanvasState();
  }, delayMs);
};

const readGraphViewportState = (): BeeroomCanvasViewportState | null => {
  if (!graph) return null;
  try {
    const zoom = clampCanvasZoom(Number(graph.getZoom()));
    const position = normalizeGraphPosition(graph.getPosition());
    if (!position || !Number.isFinite(zoom)) return null;
    const center = getCanvasViewportCenter();
    // Persist pan as "offset from viewport center" so remounting with a different
    // container size can still restore to the same visual focus.
    return {
      zoom,
      position,
      centerOffset: [position[0] - center[0], position[1] - center[1]]
    };
  } catch {
    return null;
  }
};

const saveCanvasState = (scopeKey?: unknown) => {
  const key = resolveCanvasScopeKey(scopeKey);
  const cached = getBeeroomMissionCanvasState(key);
  const overrides = clonePositionOverrides(nodePositionOverrides.value);
  const hasProjection = projection.value.nodes.length > 0;
  const viewport = readGraphViewportState() || cached?.viewport || null;
  setBeeroomMissionCanvasState(key, {
    nodePositionOverrides: hasProjection ? overrides : (cached?.nodePositionOverrides ?? overrides),
    activeNodeId: hasProjection
      ? String(activeNodeId.value || '').trim()
      : String(activeNodeId.value || cached?.activeNodeId || '').trim(),
    chatCollapsed: chatCollapsed.value,
    viewport
  });
};

const hydrateCanvasState = (scopeKey?: unknown) => {
  const key = resolveCanvasScopeKey(scopeKey);
  const cached = getBeeroomMissionCanvasState(key);
  if (!cached) {
    nodePositionOverrides.value = {};
    pendingCanvasViewportRestore = null;
    return false;
  }
  nodePositionOverrides.value = clonePositionOverrides(cached.nodePositionOverrides);
  chatCollapsed.value = !!cached.chatCollapsed;
  activeNodeId.value = String(cached.activeNodeId || '').trim();
  pendingCanvasViewportRestore = cached.viewport
    ? {
        scopeKey: key,
        viewport: {
          zoom: cached.viewport.zoom,
          position: [...cached.viewport.position] as [number, number],
          centerOffset: [
            Number(cached.viewport.centerOffset?.[0] || 0),
            Number(cached.viewport.centerOffset?.[1] || 0)
          ] as [number, number]
        }
      }
    : null;
  return true;
};

const restoreGraphViewportState = async (viewport: BeeroomCanvasViewportState | null) => {
  if (!graph || !viewport) return false;
  const targetZoom = clampCanvasZoom(Number(viewport.zoom));
  const savedX = Number(viewport.position?.[0]);
  const savedY = Number(viewport.position?.[1]);
  const offsetX = Number(viewport.centerOffset?.[0]);
  const offsetY = Number(viewport.centerOffset?.[1]);
  const center = getCanvasViewportCenter();
  const targetX = Number.isFinite(offsetX) ? center[0] + offsetX : savedX;
  const targetY = Number.isFinite(offsetY) ? center[1] + offsetY : savedY;
  if (!Number.isFinite(targetZoom) || !Number.isFinite(targetX) || !Number.isFinite(targetY)) {
    return false;
  }
  await graph.zoomTo(targetZoom, false, getCanvasViewportCenter());
  const targetPosition: [number, number] = [targetX, targetY];
  const currentPosition = normalizeGraphPosition(graph.getPosition());
  if (!currentPosition) return false;
  await graph.translateBy([targetPosition[0] - currentPosition[0], targetPosition[1] - currentPosition[1]], false);
  const appliedPosition = normalizeGraphPosition(graph.getPosition());
  if (!appliedPosition) return false;
  const drift = Math.hypot(appliedPosition[0] - targetPosition[0], appliedPosition[1] - targetPosition[1]);
  if (drift > 1.5) {
    await graph.translateBy([targetPosition[0] - appliedPosition[0], targetPosition[1] - appliedPosition[1]], false);
  }
  const finalPosition = normalizeGraphPosition(graph.getPosition());
  if (!finalPosition) return false;
  return Math.hypot(finalPosition[0] - targetPosition[0], finalPosition[1] - targetPosition[1]) <= 2.5;
};

const persistDraggedNodePositions = (nodeIds?: string[]) => {
  if (!graph) return;
  const ids = (nodeIds || [])
    .map((item) => String(item || '').trim())
    .filter(Boolean);
  if (!ids.length) return;

  const nextOverrides = { ...nodePositionOverrides.value };
  ids.forEach((nodeId) => {
    try {
      const position = graph?.getElementPosition(nodeId) as { x?: number; y?: number } | undefined;
      const x = Number(position?.x);
      const y = Number(position?.y);
      if (Number.isFinite(x) && Number.isFinite(y)) {
        nextOverrides[nodeId] = { x, y };
      }
    } catch {
      return;
    }
  });
  nodePositionOverrides.value = nextOverrides;
  saveCanvasState();
};

const zoomCanvasTo = async (zoom: number) => {
  if (!graph) return;
  await graph.zoomTo(clampCanvasZoom(zoom), false, getCanvasViewportCenter());
  saveCanvasState();
};

const zoomCanvasIn = async () => {
  if (!graph) return;
  await zoomCanvasTo(graph.getZoom() + CANVAS_ZOOM_STEP);
};

const zoomCanvasOut = async () => {
  if (!graph) return;
  await zoomCanvasTo(graph.getZoom() - CANVAS_ZOOM_STEP);
};

const resetCanvasZoom = async () => {
  await zoomCanvasTo(1);
};

const fitCanvasView = async () => {
  if (!graph) return;
  await graph.fitView({ when: 'always', direction: 'both' });
  saveCanvasState();
};

const autoArrangeCanvas = async () => {
  nodePositionOverrides.value = {};
  hoveredNodeId.value = '';
  lastLayoutSignature = '';
  await enqueueRenderGraph(true);
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
  const target = screenRef.value || boardRef.value;
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

const normalizeCanvasViewport = (viewport: { width: number; height: number }) => ({
  width: Math.max(360, Number(viewport?.width || 0) || 0),
  height: Math.max(520, Number(viewport?.height || 0) || 0)
});

const waitForCanvasFrame = () =>
  new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });

const stopDispatchEdgeFlow = () => {
  if (dispatchFlowTimer !== null) {
    window.clearInterval(dispatchFlowTimer);
    dispatchFlowTimer = null;
  }
  dispatchFlowOffset = 0;
};

const syncDispatchEdgeFlow = () => {
  if (!graph) {
    stopDispatchEdgeFlow();
    activeDispatchEdgeIds.value = [];
    return;
  }
  activeDispatchEdgeIds.value = projection.value.edges
    .filter((edge) => edge?.data?.kind === 'dispatch' && edge?.data?.active)
    .map((edge) => String(edge?.id || '').trim())
    .filter(Boolean);
  if (!activeDispatchEdgeIds.value.length) {
    stopDispatchEdgeFlow();
    return;
  }
  if (dispatchFlowTimer !== null) return;
  // Keep updating dash offset to simulate flowing dispatch lines.
  dispatchFlowTimer = window.setInterval(() => {
    if (!graph || !activeDispatchEdgeIds.value.length) return;
    dispatchFlowOffset = (dispatchFlowOffset - 1.4) % 120;
    graph.updateEdgeData(
      activeDispatchEdgeIds.value.map((id) => ({
        id,
        style: { lineDashOffset: dispatchFlowOffset }
      }))
    );
    void graph.draw();
  }, 110);
};

function stopChatPolling() {
  if (chatPollTimer !== null) {
    window.clearInterval(chatPollTimer);
    chatPollTimer = null;
  }
}

function syncChatPollingState(options: { immediate?: boolean } = {}) {
  stopChatPolling();
  if (typeof window === 'undefined') return;
  const groupId = String(activeGroupId.value || '').trim();
  if (!groupId) return;
  if (chatRealtimeTransport.value !== 'none') return;
  if (options.immediate) {
    void loadManualChatHistory();
  }
  chatPollTimer = window.setInterval(() => {
    void loadManualChatHistory();
  }, CHAT_POLL_INTERVAL_MS);
}

function clearChatWatchRetry() {
  if (chatWatchRetryTimer !== null) {
    window.clearTimeout(chatWatchRetryTimer);
    chatWatchRetryTimer = null;
  }
}

function clearChatSseRetry() {
  if (chatSseRetryTimer !== null) {
    window.clearTimeout(chatSseRetryTimer);
    chatSseRetryTimer = null;
  }
}

function stopChatSseWatch() {
  clearChatSseRetry();
  if (chatSseSource) {
    chatSseSource.close();
    chatSseSource = null;
  }
  chatSseGroupId = '';
  if (chatRealtimeTransport.value === 'sse') {
    chatRealtimeTransport.value = 'none';
    syncChatPollingState();
  }
}

function scheduleChatSseRetry(groupId: string) {
  if (typeof window === 'undefined') return;
  clearChatSseRetry();
  chatSseRetryTimer = window.setTimeout(() => {
    chatSseRetryTimer = null;
    if (groupId !== activeGroupId.value) return;
    startChatSseWatch(groupId);
  }, CHAT_SSE_RETRY_DELAY_MS);
}

function startChatSseWatch(groupId: string) {
  const normalizedGroupId = String(groupId || '').trim();
  if (!normalizedGroupId || typeof window === 'undefined' || typeof EventSource === 'undefined') {
    return;
  }
  if (chatSseSource && chatSseGroupId === normalizedGroupId) {
    return;
  }
  stopChatSseWatch();
  let source: EventSource;
  try {
    source = openBeeroomChatStream(normalizedGroupId, {
      allowQueryToken: true,
      params: { after_event_id: 0 }
    });
  } catch {
    scheduleChatSseRetry(normalizedGroupId);
    return;
  }
  chatSseSource = source;
  chatSseGroupId = normalizedGroupId;

  const bindEvent = (eventType: string) => {
    source.addEventListener(eventType, (event: Event) => {
      if (chatSseSource !== source) return;
      const messageEvent = event as MessageEvent;
      const dataText =
        typeof messageEvent.data === 'string'
          ? messageEvent.data
          : JSON.stringify(messageEvent.data ?? null);
      handleChatRealtimeEvent(
        normalizedGroupId,
        eventType,
        dataText,
        String(messageEvent.lastEventId || ''),
        'sse'
      );
    });
  };

  bindEvent('watching');
  bindEvent('sync_required');
  bindEvent('chat_cleared');
  bindEvent('chat_message');

  source.onerror = () => {
    if (chatSseSource !== source) return;
    source.close();
    chatSseSource = null;
    chatSseGroupId = '';
    if (chatRealtimeTransport.value === 'sse') {
      chatRealtimeTransport.value = 'none';
      syncChatPollingState();
    }
    if (normalizedGroupId !== activeGroupId.value) return;
    scheduleChatSseRetry(normalizedGroupId);
  };
}

function stopChatRealtimeWatch() {
  clearChatWatchRetry();
  stopChatSseWatch();
  if (chatWatchController) {
    chatWatchController.abort();
    chatWatchController = null;
  }
  if (chatWatchRequestId) {
    beeroomWsClient.sendCancel(chatWatchRequestId, activeGroupId.value);
    chatWatchRequestId = '';
  }
  if (chatRealtimeTransport.value === 'ws') {
    chatRealtimeTransport.value = 'none';
  }
  syncChatPollingState();
}

function handleChatRealtimeEvent(
  groupId: string,
  eventType: string,
  dataText: string,
  _eventId: string,
  transport: 'ws' | 'sse' = 'ws'
) {
  if (!groupId || groupId !== activeGroupId.value) return;
  const payload = safeJsonParse(dataText);
  const normalizedType = String(eventType || '').trim().toLowerCase();
  if (
    (normalizedType === 'chat_message' ||
      normalizedType === 'chat_cleared' ||
      normalizedType === 'sync_required') &&
    chatRealtimeTransport.value !== transport
  ) {
    chatRealtimeTransport.value = transport;
    syncChatPollingState();
  }
  if (normalizedType === 'watching') {
    if (transport === 'ws') {
      stopChatSseWatch();
      chatRealtimeTransport.value = 'ws';
    } else if (transport === 'sse') {
      chatRealtimeTransport.value = 'sse';
    }
    syncChatPollingState();
    return;
  }
  if (normalizedType === 'sync_required') {
    void loadManualChatHistory();
    return;
  }
  if (normalizedType === 'chat_cleared') {
    manualChatMessages.value = [];
    return;
  }
  if (normalizedType === 'chat_message') {
    const message = mapApiChatMessage(payload);
    if (message) {
      appendManualChatMessage(message);
    }
    return;
  }
}

function scheduleChatRealtimeRetry(groupId: string) {
  if (typeof window === 'undefined') return;
  clearChatWatchRetry();
  chatWatchRetryTimer = window.setTimeout(() => {
    chatWatchRetryTimer = null;
    if (groupId !== activeGroupId.value) return;
    startChatRealtimeWatch(groupId);
  }, CHAT_WS_RETRY_DELAY_MS);
}

function startChatRealtimeWatch(groupId: string) {
  const normalizedGroupId = String(groupId || '').trim();
  if (!normalizedGroupId) return;
  clearChatWatchRetry();
  if (chatWatchController) {
    chatWatchController.abort();
    chatWatchController = null;
  }
  if (chatWatchRequestId) {
    beeroomWsClient.sendCancel(chatWatchRequestId, activeGroupId.value);
    chatWatchRequestId = '';
  }
  chatWatchController = new AbortController();
  const controller = chatWatchController;
  const requestId = nextManualMessageKey('chat-watch');
  chatWatchRequestId = requestId;
  beeroomWsClient
    .request({
      requestId,
      sessionId: normalizedGroupId,
      message: {
        type: 'watch',
        request_id: requestId,
        payload: {
          group_id: normalizedGroupId,
          after_event_id: 0
        }
      },
      closeOnFinal: false,
      signal: controller.signal,
      onEvent: (eventType, dataText, eventId) =>
        handleChatRealtimeEvent(normalizedGroupId, eventType, dataText, eventId, 'ws')
    })
    .catch(() => {
      if (controller.signal.aborted) return;
      if (chatRealtimeTransport.value === 'ws') {
        chatRealtimeTransport.value = 'none';
        syncChatPollingState();
      }
      startChatSseWatch(normalizedGroupId);
      scheduleChatRealtimeRetry(normalizedGroupId);
    })
    .finally(() => {
      if (chatWatchController === controller) {
        chatWatchController = null;
      }
      if (chatWatchRequestId === requestId) {
        chatWatchRequestId = '';
      }
    });
}

function restartChatPolling() {
  syncChatPollingState({ immediate: true });
}

const waitForCanvasViewport = async (attempts = 10) => {
  let viewport = getCanvasViewport();
  for (let index = 0; index < attempts; index += 1) {
    if (viewport.width > 48 && viewport.height > 48) {
      return normalizeCanvasViewport(viewport);
    }
    await waitForCanvasFrame();
    viewport = getCanvasViewport();
  }
  return normalizeCanvasViewport(viewport);
};

const ensureGraph = async () => {
  if (!canvasRef.value || graph || canvasDisposed) return;
  if (graphInitPromise) {
    await graphInitPromise;
    return;
  }
  graphInitPromise = (async () => {
    if (!canvasRef.value || graph || canvasDisposed) return;
    clearCanvasContainers();
    const Graph = await loadGraphCtor();
    if (!canvasRef.value || graph || canvasDisposed) return;
    const viewport = await waitForCanvasViewport();
    if (!canvasRef.value || graph || canvasDisposed) return;
    const plugins: any[] = [
      {
        key: GRID_PLUGIN_KEY,
        type: 'grid-line',
        size: 32,
        lineWidth: 1,
        stroke: 'rgba(148, 163, 184, 0.12)',
        border: false,
        follow: true
      }
    ];
    if (minimapRef.value) {
      plugins.push({
        key: 'beeroom-minimap',
        type: 'minimap',
        container: minimapRef.value,
        size: [132, 80],
        padding: 10,
        delay: 64,
        shape: (id: string, elementType: string, element: any) => {
          const keyShape = element?.getShape?.('key');
          if (elementType === 'node') {
            const keyContainer = element?.getShape?.('key-container') || keyShape;
            const cloned = keyContainer?.cloneNode?.();
            if (cloned) {
              const status = projection.value.nodeMetaMap.get(String(id || '').trim())?.status || '';
              Object.assign(cloned.style, {
                fill: resolveMinimapNodeFill(status),
                stroke: 'rgba(148, 163, 184, 0.48)',
                lineWidth: 0.8,
                opacity: 0.96,
                radius: 3
              });
              return cloned;
            }
          }
          if (elementType === 'edge' && keyShape?.cloneNode) {
            const cloned = keyShape.cloneNode();
            Object.assign(cloned.style, {
              stroke: 'rgba(148, 163, 184, 0.42)',
              lineWidth: 0.8,
              opacity: 0.9
            });
            return cloned;
          }
          return keyShape?.cloneNode?.() || element;
        },
        maskStyle: {
          border: '1px solid rgba(148, 163, 184, 0.56)',
          background: 'rgba(148, 163, 184, 0.12)',
          borderRadius: '8px'
        }
      } as any);
    }
    const nextGraph = new Graph({
      container: canvasRef.value,
      width: viewport.width,
      height: viewport.height,
      devicePixelRatio: Math.max(2, globalThis.devicePixelRatio || 1),
      zoomRange: [CANVAS_ZOOM_MIN, CANVAS_ZOOM_MAX],
      data: { nodes: [], edges: [] },
      plugins,
      node: {
        type: 'html',
        style: {
          pointerEvents: 'auto',
          label: false,
          badge: false
        }
      },
      edge: {
        type: 'polyline'
      },
      behaviors: [
        {
          type: 'drag-element',
          key: 'drag-node',
          dropEffect: 'none',
          hideEdge: 'none',
          shadow: false,
          enable: (event: any) => event?.targetType === 'node',
          onFinish: (ids: string[]) => persistDraggedNodePositions(ids)
        },
        'drag-canvas',
        'zoom-canvas',
        'hover-activate'
      ],
      transforms: ['process-parallel-edges'],
      animation: false,
      theme: 'light'
    });
    if (canvasDisposed) {
      nextGraph.destroy();
      return;
    }
    graph = nextGraph;

    graph.on('node:pointerenter', (event: any) => {
      if (!showNodeTooltip) return;
      const nodeId = String(event?.target?.id || '').trim();
      const boardRect = boardRef.value?.getBoundingClientRect();
      if (!nodeId) return;
      hoveredNodeId.value = nodeId;
      if (boardRect) {
        hoveredTooltipPosition.value = {
          x: Number(event?.client?.x || 0) - boardRect.left,
          y: Number(event?.client?.y || 0) - boardRect.top
        };
      }
    });

    graph.on('node:pointermove', (event: any) => {
      if (!showNodeTooltip) return;
      const nodeId = String(event?.target?.id || '').trim();
      const boardRect = boardRef.value?.getBoundingClientRect();
      if (!nodeId || !boardRect) return;
      hoveredNodeId.value = nodeId;
      hoveredTooltipPosition.value = {
        x: Number(event?.client?.x || 0) - boardRect.left,
        y: Number(event?.client?.y || 0) - boardRect.top
      };
    });

    graph.on('node:pointerleave', () => {
      hoveredNodeId.value = '';
    });

    graph.on('node:dragstart', () => {
      hoveredNodeId.value = '';
    });

    graph.on('node:dragend', (event: any) => {
      const nodeId = String(event?.target?.id || '').trim();
      if (!nodeId) return;
      persistDraggedNodePositions([nodeId]);
    });

    graph.on('canvas:dragend', () => {
      saveCanvasState();
    });

    graph.on('wheel', () => {
      scheduleCanvasStateSave(120);
    });

    graph.on('canvas:pointerleave', () => {
      hoveredNodeId.value = '';
    });

    graph.on('node:click', (event: any) => {
      if (event?.targetType !== 'node') return;
      const nodeId = String(event?.target?.id || '').trim();
      if (!nodeId) return;
      activeNodeId.value = nodeId;
    });

    graph.on('node:dblclick', (event: any) => {
      if (event?.targetType !== 'node') return;
      const nodeId = String(event?.target?.id || '').trim();
      if (!nodeId) return;
      const meta = projection.value.nodeMetaMap.get(nodeId);
      if (meta?.agent_id) {
        emit('open-agent', meta.agent_id);
      }
    });

    resizeObserver = new ResizeObserver(() => {
      if (!graph || !canvasRef.value) return;
      if (resizeFrame) {
        cancelAnimationFrame(resizeFrame);
      }
      resizeFrame = requestAnimationFrame(async () => {
        const viewport = await waitForCanvasViewport(3);
        const currentGraph = graph;
        if (!currentGraph) {
          resizeFrame = 0;
          return;
        }
        const size = currentGraph.getSize?.() || [viewport.width, viewport.height];
        const previousCenter: [number, number] = [
          Math.max(1, Number(size?.[0] || 0) / 2),
          Math.max(1, Number(size?.[1] || 0) / 2)
        ];
        const previousPosition = normalizeGraphPosition(currentGraph.getPosition());
        currentGraph.resize(viewport.width, viewport.height);
        if (previousPosition) {
          // Keep the same pan offset relative to viewport center after resize.
          const nextCenter: [number, number] = [Math.max(1, viewport.width / 2), Math.max(1, viewport.height / 2)];
          const targetPosition: [number, number] = [
            nextCenter[0] + (previousPosition[0] - previousCenter[0]),
            nextCenter[1] + (previousPosition[1] - previousCenter[1])
          ];
          const resizedPosition = normalizeGraphPosition(currentGraph.getPosition());
          if (resizedPosition) {
            await currentGraph.translateBy(
              [targetPosition[0] - resizedPosition[0], targetPosition[1] - resizedPosition[1]],
              false
            );
          }
        }
        scheduleCanvasStateSave(80);
        // Keep current pan/zoom when chat panel width changes.
        resizeFrame = 0;
      });
    });
    resizeObserver.observe(canvasRef.value);
  })();
  try {
    await graphInitPromise;
  } finally {
    graphInitPromise = null;
  }
};

const clearGraph = async () => {
  if (!graph) return;
  stopDispatchEdgeFlow();
  activeDispatchEdgeIds.value = [];
  graph.setData({ nodes: [], edges: [] });
  await graph.render();
};

const renderGraph = async (forceFit = false) => {
  const sequence = ++renderSequence;
  if (!projection.value.nodes.length) {
    activeNodeId.value = '';
    hoveredNodeId.value = '';
    stopDispatchEdgeFlow();
    activeDispatchEdgeIds.value = [];
    await clearGraph();
    lastLayoutSignature = '';
    return;
  }
  await nextTick();
  if (sequence !== renderSequence || canvasDisposed) return;
  const viewport = await waitForCanvasViewport();
  if (sequence !== renderSequence || canvasDisposed) return;
  await ensureGraph();
  if (sequence !== renderSequence || canvasDisposed) return;
  const currentGraph = graph;
  if (!currentGraph) return;
  currentGraph.resize(viewport.width, viewport.height);
  currentGraph.setData({ nodes: projection.value.nodes, edges: projection.value.edges });
  await currentGraph.render();
  if (sequence !== renderSequence || canvasDisposed || currentGraph !== graph) return;
  const scopeKey = resolveCanvasScopeKey();
  const cachedScopeState = getBeeroomMissionCanvasState(scopeKey);
  const hasManualLayoutOverrides =
    hasPositionOverrides(nodePositionOverrides.value) || hasPositionOverrides(cachedScopeState?.nodePositionOverrides);
  const viewportToRestore =
    pendingCanvasViewportRestore && pendingCanvasViewportRestore.scopeKey === scopeKey
      ? pendingCanvasViewportRestore.viewport
      : (cachedScopeState?.viewport ?? null);
  let restoredViewport = false;
  if (viewportToRestore) {
    restoredViewport = await restoreGraphViewportState(viewportToRestore);
    if (!restoredViewport) {
      // One more attempt on next frame after viewport settles.
      await waitForCanvasFrame();
      restoredViewport = await restoreGraphViewportState(viewportToRestore);
    }
  }
  if (pendingCanvasViewportRestore && pendingCanvasViewportRestore.scopeKey === scopeKey) {
    pendingCanvasViewportRestore = null;
  }
  if (sequence !== renderSequence || canvasDisposed || currentGraph !== graph) return;
  if (
    !restoredViewport &&
    !viewportToRestore &&
    !hasManualLayoutOverrides &&
    (forceFit || lastLayoutSignature !== layoutSignature.value)
  ) {
    await currentGraph.fitView({ when: 'always', direction: 'both' });
    if (sequence !== renderSequence || canvasDisposed || currentGraph !== graph) return;
  }
  lastLayoutSignature = layoutSignature.value;
  syncDispatchEdgeFlow();
  saveCanvasState(scopeKey);
};

const enqueueRenderGraph = async (forceFit = false) => {
  renderRequested = true;
  renderRequestedForceFit = renderRequestedForceFit || forceFit;
  if (renderTask) {
    await renderTask;
    return;
  }
  renderTask = (async () => {
    while (renderRequested) {
      const nextForceFit = renderRequestedForceFit;
      renderRequested = false;
      renderRequestedForceFit = false;
      await renderGraph(nextForceFit);
    }
  })();
  try {
    await renderTask;
  } finally {
    renderTask = null;
  }
};

watch(
  nodePositionScopeKey,
  (current, previous) => {
    const previousScopeKey = String(previous || '').trim() || 'standby';
    const currentScopeKey = resolveCanvasScopeKey(current);
    if (previousScopeKey && previousScopeKey !== currentScopeKey) {
      saveCanvasState(previousScopeKey);
    }
    currentCanvasScopeKey = currentScopeKey;
    hydrateCanvasState(currentScopeKey);
    // New scope should re-evaluate layout/viewport once.
    lastLayoutSignature = '';
  },
  { immediate: true }
);

watch(
  activeNodeId,
  async (current, previous) => {
    if (current === previous || !graph || !projection.value.nodes.length) return;
    await enqueueRenderGraph(false);
    saveCanvasState();
  }
);

watch(chatCollapsed, () => {
  saveCanvasState();
});

watch(
  renderSignature,
  async () => {
    const missionIdentity = String(props.mission?.mission_id || props.mission?.team_run_id || '').trim();
    const missionChanged = missionIdentity !== lastMissionIdentity;
    lastMissionIdentity = missionIdentity;
    if (!activeNodeId.value || !projection.value.nodeMetaMap.has(activeNodeId.value)) {
      activeNodeId.value = projection.value.motherNodeId || projection.value.nodes[0]?.id || '';
    }
    const scopeKey = resolveCanvasScopeKey();
    const cachedScopeState = getBeeroomMissionCanvasState(scopeKey);
    const hasManualLayoutOverrides =
      hasPositionOverrides(nodePositionOverrides.value) || hasPositionOverrides(cachedScopeState?.nodePositionOverrides);
    const shouldForceFit =
      missionChanged && !(pendingCanvasViewportRestore?.scopeKey === scopeKey) && !hasManualLayoutOverrides;
    await enqueueRenderGraph(shouldForceFit);
  },
  { immediate: true }
);

onMounted(async () => {
  canvasDisposed = false;
  if (typeof document !== 'undefined') {
    document.addEventListener('fullscreenchange', refreshCanvasFullscreen);
    refreshCanvasFullscreen();
  }
  restartChatPolling();
});

onBeforeUnmount(() => {
  canvasDisposed = true;
  renderSequence += 1;
  renderRequested = false;
  renderRequestedForceFit = false;
  renderTask = null;
  graphInitPromise = null;
  saveCanvasState(currentCanvasScopeKey || resolveCanvasScopeKey());
  clearCanvasWheelSaveTimer();
  resetDispatchRuntime();
  stopChatPolling();
  stopDispatchEdgeFlow();
  stopChatRealtimeWatch();
  beeroomWsClient.close(1000, 'beeroom-canvas-unmount');
  if (resizeFrame) {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = 0;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (typeof document !== 'undefined') {
    document.removeEventListener('fullscreenchange', refreshCanvasFullscreen);
  }
  graph?.destroy();
  graph = null;
  clearCanvasContainers();
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
  min-height: 0;
  overflow: hidden;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 20px;
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
  padding: 0;
}

.beeroom-canvas-board {
  --beeroom-chat-width: 344px;
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--beeroom-chat-width);
  flex: 1;
  width: 100%;
  height: 100%;
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
  grid-template-columns: minmax(0, 1fr) 0px;
}

.beeroom-canvas-board.chat-collapsed::after {
  right: 0;
  opacity: 0;
}

.beeroom-canvas-graph-shell {
  position: relative;
  display: flex;
  min-width: 0;
  min-height: 0;
}

.beeroom-canvas-graph-shell::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 0;
  border: 1px solid rgba(255, 255, 255, 0.04);
  box-shadow: inset 0 0 24px rgba(15, 23, 42, 0.2);
  pointer-events: none;
}

.beeroom-canvas-graph-shell::after {
  content: '';
  position: absolute;
  inset: 0;
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.03), transparent 96px),
    linear-gradient(135deg, transparent 0%, rgba(239, 68, 68, 0.02) 48%, transparent 100%);
  opacity: 0.56;
  pointer-events: none;
}

.beeroom-canvas-surface :deep(.beeroom-node-card) {
  position: relative;
  box-sizing: border-box;
  width: 100%;
  height: 100%;
  padding: 8px 8px 7px;
  border-radius: 11px;
  border: 1px solid rgba(148, 163, 184, 0.25);
  background: linear-gradient(180deg, rgba(23, 26, 35, 0.96), rgba(14, 16, 22, 0.95));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.04),
    0 6px 16px rgba(0, 0, 0, 0.28);
  color: #e5e7eb;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  text-rendering: geometricPrecision;
  backface-visibility: hidden;
  transform: translateZ(0);
  transition:
    border-color var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    box-shadow var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    transform var(--beeroom-motion-normal) var(--beeroom-ease-standard);
}

.beeroom-canvas-surface :deep(.beeroom-node-card:hover) {
  transform: translate3d(0, -1px, 0);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.06),
    0 10px 20px rgba(0, 0, 0, 0.3);
}

.beeroom-canvas-surface :deep(.beeroom-node-card::before) {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--node-accent, #64748b);
  opacity: 0.9;
}

.beeroom-canvas-surface :deep(.beeroom-node-card-head) {
  min-width: 0;
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) max-content;
  gap: 6px;
  align-items: center;
}

.beeroom-canvas-surface :deep(.beeroom-node-avatar) {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 700;
  color: #f8fafc;
  border: 1px solid rgba(255, 255, 255, 0.18);
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.13), rgba(255, 255, 255, 0.04)),
    var(--node-accent, #64748b);
  flex-shrink: 0;
}

.beeroom-canvas-surface :deep(.beeroom-node-title-group) {
  min-width: 0;
  display: grid;
  gap: 2px;
}

.beeroom-canvas-surface :deep(.beeroom-node-title) {
  color: #f8fafc;
  font-size: 11px;
  font-weight: 640;
  line-height: 1.18;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-canvas-surface :deep(.beeroom-node-role-chip) {
  justify-self: flex-start;
  max-width: 88px;
  min-height: 14px;
  padding: 0 5px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(30, 41, 59, 0.38);
  color: rgba(203, 213, 225, 0.92);
  font-size: 8px;
  line-height: 12px;
  letter-spacing: 0.03em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-canvas-surface :deep(.beeroom-node-status) {
  max-width: 74px;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.3);
  background: rgba(51, 65, 85, 0.35);
  color: #cbd5e1;
  font-size: 9px;
  font-weight: 600;
  line-height: 16px;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  text-align: center;
  flex-shrink: 0;
}

.beeroom-canvas-surface :deep(.beeroom-node-status-dot) {
  width: 5px;
  height: 5px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.96);
  flex-shrink: 0;
}

.beeroom-canvas-surface :deep(.beeroom-node-metrics) {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 5px;
}

.beeroom-canvas-surface :deep(.beeroom-node-metric) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  min-width: 0;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: rgba(30, 41, 59, 0.42);
  color: rgba(226, 232, 240, 0.9);
  font-size: 9px;
  line-height: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-canvas-surface :deep(.beeroom-node-metric i) {
  font-size: 8px;
  opacity: 0.9;
  flex-shrink: 0;
}

.beeroom-canvas-surface :deep(.beeroom-node-metric b) {
  font-size: 9px;
  font-weight: 700;
  color: rgba(248, 250, 252, 0.94);
  line-height: 1;
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-mother) {
  border-color: rgba(245, 158, 11, 0.42);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-selected) {
  border-color: rgba(96, 165, 250, 0.62);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    0 0 0 1px rgba(96, 165, 250, 0.36),
    0 12px 24px rgba(8, 47, 73, 0.3);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-selected::before) {
  width: 3px;
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-running .beeroom-node-status) {
  border-color: rgba(34, 197, 94, 0.44);
  background: rgba(34, 197, 94, 0.16);
  color: rgba(134, 239, 172, 0.97);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-running .beeroom-node-status-dot) {
  background: rgba(34, 197, 94, 0.98);
  animation: beeroom-node-status-pulse 1.3s ease-in-out infinite;
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-success .beeroom-node-status) {
  border-color: rgba(59, 130, 246, 0.44);
  background: rgba(59, 130, 246, 0.16);
  color: rgba(191, 219, 254, 0.98);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-success .beeroom-node-status-dot) {
  background: rgba(59, 130, 246, 0.98);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-danger .beeroom-node-status) {
  border-color: rgba(239, 68, 68, 0.44);
  background: rgba(239, 68, 68, 0.18);
  color: rgba(252, 165, 165, 0.98);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-danger .beeroom-node-status-dot) {
  background: rgba(248, 113, 113, 0.98);
  animation: beeroom-node-status-pulse 1.45s ease-in-out infinite;
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-warn .beeroom-node-status) {
  border-color: rgba(245, 158, 11, 0.44);
  background: rgba(245, 158, 11, 0.16);
  color: rgba(253, 230, 138, 0.98);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-warn .beeroom-node-status-dot) {
  background: rgba(245, 158, 11, 0.98);
}

.beeroom-canvas-surface :deep(.beeroom-node-card.is-muted .beeroom-node-status-dot) {
  background: rgba(148, 163, 184, 0.92);
}

@keyframes beeroom-node-status-pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.55;
    transform: scale(0.78);
  }
}

.beeroom-canvas-legend {
  position: absolute;
  top: 12px;
  right: 14px;
  z-index: 5;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(12, 13, 18, 0.88);
  color: rgba(229, 231, 235, 0.86);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.04),
    0 12px 24px rgba(0, 0, 0, 0.2);
}

.beeroom-canvas-tools {
  position: absolute;
  left: 14px;
  top: 12px;
  z-index: 5;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(12, 13, 18, 0.72);
  opacity: 0.72;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.03),
    0 8px 16px rgba(0, 0, 0, 0.16);
  transition:
    opacity var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    border-color var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    background var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    box-shadow var(--beeroom-motion-normal) var(--beeroom-ease-standard);
}

.beeroom-canvas-board:hover .beeroom-canvas-tools,
.beeroom-canvas-tools:focus-within {
  opacity: 1;
  border-color: rgba(148, 163, 184, 0.28);
  background: rgba(12, 13, 18, 0.86);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.04),
    0 10px 20px rgba(0, 0, 0, 0.22);
}

.beeroom-canvas-tool-btn {
  width: 30px;
  height: 30px;
  padding: 0;
  border-radius: 8px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: rgba(30, 41, 59, 0.28);
  color: #e2e8f0;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition:
    border-color var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    background var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    color var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    transform var(--beeroom-motion-fast) var(--beeroom-ease-standard);
}

.beeroom-canvas-tool-btn:hover,
.beeroom-canvas-tool-btn:focus-visible,
.beeroom-canvas-tool-btn.is-active {
  border-color: rgba(96, 165, 250, 0.48);
  background: rgba(30, 64, 175, 0.32);
  color: #dbeafe;
  transform: translateY(-1px);
}

.beeroom-canvas-tool-btn:focus-visible {
  outline: none;
  box-shadow: var(--beeroom-focus-ring);
}

.beeroom-canvas-tool-btn:disabled {
  opacity: 0.46;
  cursor: not-allowed;
  transform: none;
}

.beeroom-visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.beeroom-canvas-legend-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  white-space: nowrap;
}

.beeroom-canvas-legend-item i {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.9);
  box-shadow: 0 0 0 3px rgba(148, 163, 184, 0.16);
}

.beeroom-canvas-legend-item.is-running i {
  background: rgba(239, 68, 68, 0.95);
  box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.18);
}

.beeroom-canvas-legend-item.is-danger i {
  background: rgba(248, 113, 113, 0.95);
  box-shadow: 0 0 0 3px rgba(248, 113, 113, 0.2);
}

.beeroom-canvas-legend-item.is-idle i {
  background: rgba(148, 163, 184, 0.9);
  box-shadow: 0 0 0 3px rgba(148, 163, 184, 0.16);
}

.beeroom-canvas-tooltip {
  position: absolute;
  z-index: 6;
  width: 260px;
  display: grid;
  gap: 8px;
  padding: 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: rgba(12, 13, 18, 0.94);
  box-shadow: 0 18px 36px rgba(0, 0, 0, 0.42);
  pointer-events: none;
}

.beeroom-canvas-tooltip-head,
.beeroom-canvas-tooltip-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.beeroom-canvas-tooltip-title {
  color: #f3f4f6;
  font-size: 14px;
  font-weight: 700;
}

.beeroom-canvas-tooltip-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.beeroom-canvas-tooltip-item {
  display: grid;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 10px;
  background: rgba(22, 24, 31, 0.82);
  border: 1px solid rgba(148, 163, 184, 0.12);
}

.beeroom-canvas-tooltip-item span,
.beeroom-canvas-tooltip-desc {
  color: rgba(156, 163, 175, 0.92);
  font-size: 11px;
  line-height: 1.55;
}

.beeroom-canvas-tooltip-item strong {
  color: #f3f4f6;
  font-size: 13px;
}

.beeroom-canvas-chat {
  position: relative;
  z-index: 1;
  display: flex;
  width: var(--beeroom-chat-width);
  min-width: 0;
  flex-direction: column;
  gap: 12px;
  padding: 14px 14px 14px 18px;
  border-left: 1px solid rgba(148, 163, 184, 0.2);
  background:
    linear-gradient(180deg, rgba(13, 14, 20, 0.95), rgba(9, 10, 15, 0.97)),
    linear-gradient(180deg, rgba(239, 68, 68, 0.03), rgba(148, 163, 184, 0.02));
  color: #e5e7eb;
  box-shadow:
    inset 1px 0 0 rgba(255, 255, 255, 0.03),
    inset 0 1px 0 rgba(255, 255, 255, 0.02);
  overflow: hidden;
  transition:
    width var(--beeroom-motion-slow) var(--beeroom-ease-standard),
    padding var(--beeroom-motion-slow) var(--beeroom-ease-standard),
    background var(--beeroom-motion-normal) var(--beeroom-ease-standard),
    opacity var(--beeroom-motion-normal) var(--beeroom-ease-standard);
}

.beeroom-canvas-chat::before {
  content: '';
  position: absolute;
  inset: 0 0 auto 0;
  height: 56px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.04), transparent);
  pointer-events: none;
}

.beeroom-canvas-chat.collapsed {
  width: 0;
  padding: 0;
  border-left: 0;
  box-shadow: none;
  background: transparent;
  gap: 0;
  overflow: visible;
}

.beeroom-canvas-chat-handle {
  position: absolute;
  left: -12px;
  top: 50%;
  transform: translateY(-50%);
  width: 22px;
  height: 78px;
  border: 1px solid rgba(148, 163, 184, 0.36);
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.95), rgba(15, 23, 42, 0.94));
  color: #cbd5e1;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  z-index: 2;
  opacity: 0;
  pointer-events: none;
  box-shadow: none;
  transition:
    opacity var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    border-color var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    background var(--beeroom-motion-fast) var(--beeroom-ease-standard);
}

.beeroom-canvas-board:hover .beeroom-canvas-chat-handle,
.beeroom-canvas-board:focus-within .beeroom-canvas-chat-handle {
  opacity: 1;
  pointer-events: auto;
}

.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
  left: -14px;
  opacity: 0.72;
  pointer-events: auto;
}

.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle:focus-visible,
.beeroom-canvas-board:hover .beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
  opacity: 1;
}

.beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat-handle:focus-visible {
  background: linear-gradient(180deg, rgba(30, 41, 59, 0.98), rgba(15, 23, 42, 0.98));
  border-color: rgba(148, 163, 184, 0.56);
  transform: translateY(-50%);
}

.beeroom-canvas-chat-handle:focus-visible {
  outline: none;
  box-shadow: var(--beeroom-focus-ring);
}

.beeroom-canvas-chat-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 4px;
  border-bottom: 1px solid rgba(148, 163, 184, 0.14);
}

.beeroom-canvas-chat-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.beeroom-canvas-icon-btn {
  width: 28px;
  height: 28px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: rgba(19, 21, 29, 0.84);
  color: #d1d5db;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition:
    border-color var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    background var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    color var(--beeroom-motion-fast) var(--beeroom-ease-standard);
}

.beeroom-canvas-icon-btn:hover:not(:disabled),
.beeroom-canvas-icon-btn:focus-visible:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.42);
  background: rgba(30, 41, 59, 0.96);
  color: #e2e8f0;
}

.beeroom-canvas-icon-btn:focus-visible {
  outline: none;
  box-shadow: var(--beeroom-focus-ring);
}

.beeroom-canvas-icon-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.beeroom-canvas-chat-title {
  color: #f3f4f6;
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.beeroom-canvas-chat-subtitle,
.beeroom-canvas-chat-time,
.beeroom-canvas-chat-extra,
.beeroom-canvas-chat-overview-label {
  color: rgba(156, 163, 175, 0.92);
  font-size: 11px;
}

.beeroom-canvas-chat-runtime {
  margin-top: 6px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.beeroom-canvas-runtime-chip {
  display: inline-flex;
  align-items: center;
  min-height: 20px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(30, 41, 59, 0.48);
  color: rgba(226, 232, 240, 0.92);
  font-size: 11px;
  line-height: 1;
}

.beeroom-canvas-runtime-chip.is-running {
  border-color: rgba(59, 130, 246, 0.36);
  background: rgba(30, 64, 175, 0.28);
  color: rgba(191, 219, 254, 0.96);
}

.beeroom-canvas-runtime-chip.is-success {
  border-color: rgba(34, 197, 94, 0.36);
  background: rgba(21, 128, 61, 0.28);
  color: rgba(187, 247, 208, 0.96);
}

.beeroom-canvas-runtime-chip.is-danger {
  border-color: rgba(239, 68, 68, 0.4);
  background: rgba(127, 29, 29, 0.3);
  color: rgba(254, 202, 202, 0.96);
}

.beeroom-canvas-runtime-chip.is-warn {
  border-color: rgba(245, 158, 11, 0.42);
  background: rgba(146, 64, 14, 0.28);
  color: rgba(254, 240, 138, 0.98);
}

.beeroom-canvas-runtime-session {
  font-size: 11px;
  color: rgba(148, 163, 184, 0.92);
}

.beeroom-canvas-chat-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 24px;
  padding: 0 8px;
  border-radius: 999px;
  background: rgba(127, 29, 29, 0.22);
  border: 1px solid rgba(239, 68, 68, 0.28);
  color: rgba(248, 113, 113, 0.98);
  font-size: 11px;
}

.beeroom-canvas-chat-stream {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
  padding-right: 2px;
}

.beeroom-canvas-chat-approvals {
  display: grid;
  gap: 8px;
  max-height: 178px;
  overflow: auto;
  padding: 8px 0 4px;
  border-top: 1px solid rgba(148, 163, 184, 0.14);
  border-bottom: 1px solid rgba(148, 163, 184, 0.14);
}

.beeroom-canvas-chat-approvals-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: rgba(226, 232, 240, 0.94);
  font-size: 12px;
  font-weight: 600;
}

.beeroom-canvas-chat-approvals-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 20px;
  padding: 0 6px;
  border-radius: 999px;
  border: 1px solid rgba(245, 158, 11, 0.32);
  background: rgba(120, 53, 15, 0.28);
  color: rgba(254, 240, 138, 0.96);
  font-size: 11px;
}

.beeroom-canvas-chat-approval-item {
  display: grid;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: rgba(15, 23, 42, 0.5);
}

.beeroom-canvas-chat-approval-summary {
  color: rgba(243, 244, 246, 0.94);
  font-size: 12px;
  line-height: 1.5;
}

.beeroom-canvas-chat-approval-meta {
  color: rgba(148, 163, 184, 0.94);
  font-size: 11px;
}

.beeroom-canvas-chat-approval-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}

.beeroom-canvas-chat-approval-btn {
  min-height: 26px;
  padding: 0 8px;
  border-radius: 8px;
  border: 1px solid rgba(148, 163, 184, 0.28);
  background: rgba(30, 41, 59, 0.65);
  color: rgba(226, 232, 240, 0.96);
  cursor: pointer;
  font-size: 11px;
}

.beeroom-canvas-chat-approval-btn.is-danger {
  border-color: rgba(239, 68, 68, 0.34);
  background: rgba(127, 29, 29, 0.44);
  color: rgba(254, 202, 202, 0.98);
}

.beeroom-canvas-chat-approval-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.beeroom-canvas-chat-message {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.beeroom-canvas-chat-avatar {
  width: 34px;
  height: 34px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 12px;
  background: rgba(23, 25, 34, 0.9);
  color: #e5e7eb;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
  cursor: pointer;
  transition:
    border-color var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    background var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    box-shadow var(--beeroom-motion-fast) var(--beeroom-ease-standard);
}

.beeroom-canvas-chat-avatar.is-system {
  cursor: default;
  background: rgba(23, 25, 34, 0.76);
  color: #9ca3af;
}

.beeroom-canvas-chat-main {
  display: grid;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-meta-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.beeroom-canvas-chat-sender {
  padding: 0;
  border: none;
  background: transparent;
  color: #f3f4f6;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
  border-radius: 8px;
}

.beeroom-canvas-chat-avatar:focus-visible,
.beeroom-canvas-chat-sender:focus-visible {
  outline: none;
  box-shadow: var(--beeroom-focus-ring);
}

.beeroom-canvas-chat-sender.is-system {
  color: #9ca3af;
  cursor: default;
}

.beeroom-canvas-chat-bubble {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.12);
  background: linear-gradient(180deg, rgba(24, 26, 34, 0.86), rgba(16, 18, 24, 0.82));
  color: #e5e7eb;
  font-size: 12.5px;
  line-height: 1.65;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-message.is-mother .beeroom-canvas-chat-bubble {
  border-color: rgba(239, 68, 68, 0.24);
  background: rgba(69, 10, 10, 0.24);
}

.beeroom-canvas-chat-message.is-worker .beeroom-canvas-chat-bubble {
  border-color: rgba(148, 163, 184, 0.2);
  background: rgba(31, 41, 55, 0.32);
}

.beeroom-canvas-chat-message.is-system .beeroom-canvas-chat-bubble {
  border-style: dashed;
  background: rgba(17, 24, 39, 0.56);
}

.beeroom-canvas-chat-message.is-user .beeroom-canvas-chat-bubble {
  border-color: rgba(239, 68, 68, 0.32);
  background: rgba(127, 29, 29, 0.3);
}

.beeroom-canvas-chat-mention {
  color: #fca5a5;
  font-weight: 700;
}

.beeroom-canvas-chat-avatar.is-user {
  cursor: default;
  background: rgba(127, 29, 29, 0.52);
  color: #fee2e2;
}

.beeroom-canvas-chat-sender.is-user {
  color: #fee2e2;
}

.beeroom-canvas-chat-composer {
  display: grid;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid rgba(148, 163, 184, 0.16);
  background: linear-gradient(180deg, rgba(9, 10, 15, 0), rgba(9, 10, 15, 0.52));
}

.beeroom-canvas-chat-compose-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-chat-compose-status {
  color: rgba(156, 163, 175, 0.9);
  font-size: 11px;
}

.beeroom-canvas-chat-compose-status.is-error {
  color: #f87171;
}

.beeroom-canvas-chat-textarea {
  width: 100%;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.92), rgba(15, 17, 23, 0.88));
  color: #f3f4f6;
  outline: none;
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-select {
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-select :deep(.el-select__wrapper) {
  min-height: 38px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid rgba(148, 163, 184, 0.2);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.92), rgba(15, 17, 23, 0.88));
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.03);
}

.beeroom-canvas-chat-select :deep(.el-select__selected-item),
.beeroom-canvas-chat-select :deep(.el-select__placeholder),
.beeroom-canvas-chat-select :deep(.el-select__input) {
  color: #f3f4f6;
}

.beeroom-canvas-chat-select :deep(.el-select__caret) {
  color: rgba(209, 213, 219, 0.78);
}

.beeroom-canvas-chat-select :deep(.is-focused .el-select__wrapper),
.beeroom-canvas-chat-select :deep(.el-select__wrapper.is-focused) {
  box-shadow:
    0 0 0 2px rgba(96, 165, 250, 0.46),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.beeroom-canvas-chat-textarea {
  resize: none;
  min-height: 84px;
  padding: 10px 12px;
  line-height: 1.6;
}

.beeroom-canvas-chat-textarea:focus-visible {
  box-shadow:
    var(--beeroom-focus-ring),
    inset 0 1px 0 rgba(255, 255, 255, 0.05);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper) {
  border: 1px solid rgba(148, 163, 184, 0.28);
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.98), rgba(14, 16, 22, 0.98));
  box-shadow: 0 18px 40px rgba(0, 0, 0, 0.42);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper .el-popper__arrow::before) {
  border-color: rgba(148, 163, 184, 0.28);
  background: rgba(14, 16, 22, 0.98);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item) {
  color: #e5e7eb;
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-hovering),
:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item:hover) {
  background: rgba(31, 41, 55, 0.78);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-selected) {
  color: #fca5a5;
  background: rgba(127, 29, 29, 0.6);
}

.beeroom-canvas-chat-send {
  min-width: 74px;
  min-height: 34px;
  padding: 0 12px;
  border-radius: 12px;
  border: 1px solid rgba(239, 68, 68, 0.34);
  background: linear-gradient(135deg, rgba(220, 38, 38, 0.92), rgba(185, 28, 28, 0.92));
  color: #fee2e2;
  cursor: pointer;
  box-shadow: 0 10px 24px rgba(127, 29, 29, 0.24);
  transition:
    transform var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    filter var(--beeroom-motion-fast) var(--beeroom-ease-standard),
    opacity var(--beeroom-motion-fast) var(--beeroom-ease-standard);
}

.beeroom-canvas-chat-send:hover:not(:disabled) {
  transform: translateY(-1px);
  filter: brightness(1.04);
}

.beeroom-canvas-chat-send:focus-visible {
  outline: none;
  box-shadow:
    var(--beeroom-focus-ring),
    0 10px 24px rgba(127, 29, 29, 0.24);
}

.beeroom-canvas-chat-send:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.beeroom-canvas-surface {
  position: relative;
  z-index: 1;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.beeroom-canvas-minimap-shell {
  position: absolute;
  left: 12px;
  bottom: 12px;
  z-index: 4;
  display: flex;
  flex-direction: column;
  gap: 6px;
  pointer-events: none;
}

.beeroom-canvas-minimap-label {
  align-self: flex-start;
  padding: 2px 6px;
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.94), rgba(15, 17, 23, 0.9));
  border: 1px solid rgba(148, 163, 184, 0.24);
  color: rgba(209, 213, 219, 0.86);
  font-size: 9px;
  letter-spacing: 0.04em;
}

.beeroom-canvas-minimap {
  width: 132px;
  height: 80px;
  overflow: hidden;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: linear-gradient(180deg, rgba(15, 17, 23, 0.94), rgba(10, 11, 16, 0.9));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.03),
    0 10px 26px rgba(0, 0, 0, 0.28);
  pointer-events: auto;
}

.beeroom-canvas-detail {
  position: relative;
  z-index: 1;
  display: flex;
  width: 296px;
  min-width: 296px;
  flex-direction: column;
  gap: 12px;
  padding: 14px;
  border-radius: 16px;
  border: 1px solid rgba(56, 189, 248, 0.16);
  background: rgba(8, 15, 32, 0.84);
  color: #e2e8f0;
  box-shadow: inset 0 0 0 1px rgba(148, 163, 184, 0.05);
  overflow: hidden;
}

.beeroom-canvas-detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.beeroom-canvas-detail-head-main {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
}

.beeroom-canvas-detail-title-row,
.beeroom-canvas-detail-status-row,
.beeroom-canvas-task-head,
.beeroom-canvas-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-detail-title {
  margin: 0;
  font-size: 17px;
  font-weight: 700;
  color: #f8fafc;
}

.beeroom-canvas-role-chip,
.beeroom-canvas-status-chip,
.beeroom-canvas-entry-flag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 0 9px;
  border-radius: 999px;
  font-size: 11px;
  white-space: nowrap;
}

.beeroom-canvas-role-chip,
.beeroom-canvas-entry-flag {
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: rgba(31, 41, 55, 0.7);
  color: #d1d5db;
}

.beeroom-canvas-status-chip.tone-muted {
  background: rgba(148, 163, 184, 0.14);
  color: #94a3b8;
}

.beeroom-canvas-status-chip.tone-running {
  background: rgba(239, 68, 68, 0.16);
  color: #fca5a5;
}

.beeroom-canvas-status-chip.tone-success {
  background: rgba(34, 197, 94, 0.16);
  color: #86efac;
}

.beeroom-canvas-status-chip.tone-danger {
  background: rgba(239, 68, 68, 0.16);
  color: #fca5a5;
}

.beeroom-canvas-status-chip.tone-warn {
  background: rgba(245, 158, 11, 0.16);
  color: #fcd34d;
}

.beeroom-canvas-open-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 34px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid rgba(56, 189, 248, 0.26);
  background: rgba(14, 116, 144, 0.16);
  color: #bae6fd;
  cursor: pointer;
  font-size: 12px;
}

.beeroom-canvas-detail-desc {
  margin: 0;
  line-height: 1.5;
  font-size: 13px;
  color: rgba(226, 232, 240, 0.78);
}

.beeroom-canvas-stat-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.beeroom-canvas-stat-card,
.beeroom-canvas-task-item {
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 14px;
  background: rgba(15, 23, 42, 0.72);
}

.beeroom-canvas-stat-card {
  padding: 10px;
}

.beeroom-canvas-stat-label {
  color: rgba(148, 163, 184, 0.9);
  font-size: 11px;
}

.beeroom-canvas-stat-value {
  margin-top: 6px;
  font-size: 15px;
  font-weight: 700;
  color: #f8fafc;
}

.beeroom-canvas-subsection,
.beeroom-canvas-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.beeroom-canvas-section {
  flex: 1;
  min-height: 0;
}

.beeroom-canvas-section--tasks {
  min-height: 0;
}

.beeroom-canvas-section-head h4 {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
}

.beeroom-canvas-section-head span {
  color: rgba(148, 163, 184, 0.9);
  font-size: 11px;
}

.beeroom-canvas-kv-list,
.beeroom-canvas-link-list {
  display: grid;
  gap: 8px;
}

.beeroom-canvas-kv-item,
.beeroom-canvas-link-item {
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 12px;
  background: rgba(15, 23, 42, 0.72);
  padding: 10px;
}

.beeroom-canvas-kv-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-kv-label,
.beeroom-canvas-link-subtitle,
.beeroom-canvas-link-meta,
.beeroom-canvas-tool-summary-text {
  color: rgba(191, 219, 254, 0.72);
  font-size: 11px;
}

.beeroom-canvas-kv-value,
.beeroom-canvas-link-title {
  color: #f8fafc;
  font-size: 13px;
  font-weight: 600;
}

.beeroom-canvas-link-item {
  display: grid;
  gap: 4px;
}

.beeroom-canvas-tool-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.beeroom-canvas-tool-tag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid rgba(56, 189, 248, 0.18);
  background: rgba(14, 116, 144, 0.14);
  color: #bae6fd;
  font-size: 11px;
}

.beeroom-canvas-task-list {
  display: grid;
  gap: 8px;
}

.beeroom-canvas-task-item {
  padding: 10px;
}

.beeroom-canvas-task-id {
  color: #f8fafc;
  font-weight: 600;
}

.beeroom-canvas-task-body,
.beeroom-canvas-section-empty,
.beeroom-canvas-detail-empty {
  color: rgba(226, 232, 240, 0.74);
  line-height: 1.6;
}

.beeroom-canvas-detail-empty,
.beeroom-canvas-empty {
  position: relative;
  z-index: 1;
  display: flex;
  flex: 1;
  min-height: 0;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
}

.beeroom-canvas-empty i,
.beeroom-canvas-detail-empty i {
  color: #38bdf8;
}

.beeroom-canvas-minimap :deep(canvas),
.beeroom-canvas-minimap :deep(svg),
.beeroom-canvas-surface :deep(canvas),
.beeroom-canvas-surface :deep(svg) {
  display: block;
  width: 100% !important;
  height: 100% !important;
}

.beeroom-canvas-minimap :deep(.minimap) {
  width: 100% !important;
  height: 100% !important;
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
    grid-template-columns: minmax(0, 1fr) var(--beeroom-chat-width);
  }

  .beeroom-canvas-board.chat-collapsed {
    grid-template-columns: minmax(0, 1fr) 0px;
  }
}
</style>
