<template>
  <div class="beeroom-canvas-graph-shell">
    <div
      ref="viewportRef"
      class="beeroom-canvas-surface"
      @wheel.prevent="handleViewportWheel"
      @pointerdown="handleViewportPointerDown"
    >
      <svg class="beeroom-canvas-grid" aria-hidden="true" focusable="false">
        <defs>
          <pattern
            :id="surfaceGridPattern.id"
            patternUnits="userSpaceOnUse"
            patternContentUnits="userSpaceOnUse"
            :x="surfaceGridPattern.offsetX"
            :y="surfaceGridPattern.offsetY"
            :width="surfaceGridPattern.tileWidth"
            :height="surfaceGridPattern.tileHeight"
          >
            <path
              class="beeroom-canvas-grid-path is-major"
              :d="surfaceGridPattern.path"
              :stroke-width="surfaceGridPattern.strokeWidth"
            />
          </pattern>
        </defs>
        <rect width="100%" height="100%" :fill="`url(#${surfaceGridPattern.id})`" />
      </svg>
      <div class="beeroom-swarm-world" :style="worldStyle">
        <svg
          class="beeroom-swarm-edge-layer"
          :viewBox="`0 0 ${worldSize.width} ${worldSize.height}`"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <g v-for="edge in worldEdges" :key="edge.id" class="beeroom-swarm-edge-group">
            <path
              v-if="edge.active"
              class="beeroom-swarm-edge-activity"
              :class="{
                'is-selected': edge.selected,
                'is-subagent': edge.kind === 'subagent',
                'is-revealing': edge.revealing
              }"
              :d="edge.path"
            />
            <path
              class="beeroom-swarm-edge"
              :class="{
                'is-active': edge.active,
                'is-selected': edge.selected,
                'is-subagent': edge.kind === 'subagent',
                'is-revealing': edge.revealing
              }"
              :d="edge.path"
            />
            <circle
              v-if="edge.active"
              class="beeroom-swarm-edge-orb"
              :class="{ 'is-subagent': edge.kind === 'subagent' }"
              r="4.5"
            >
              <animateMotion
                :dur="edge.motionDuration"
                :begin="edge.motionDelay"
                repeatCount="indefinite"
                :path="edge.path"
                rotate="auto"
              />
            </circle>
            <circle
              v-if="edge.active"
              class="beeroom-swarm-edge-orb is-trailing"
              :class="{ 'is-subagent': edge.kind === 'subagent' }"
              r="3"
            >
              <animateMotion
                :dur="edge.motionDuration"
                :begin="edge.motionTrailDelay"
                repeatCount="indefinite"
                :path="edge.path"
                rotate="auto"
              />
            </circle>
            <text
              v-if="edge.label"
              class="beeroom-swarm-edge-label"
              :class="{
                'is-active': edge.active,
                'is-selected': edge.selected,
                'is-subagent': edge.kind === 'subagent'
              }"
              :x="edge.labelX"
              :y="edge.labelY"
            >
              {{ edge.label }}
            </text>
          </g>
        </svg>

        <div class="beeroom-swarm-node-layer" @pointerdown.capture="handleNodeLayerPointerDown">
          <BeeroomSwarmNodeCard
            v-for="node in worldNodes"
            :key="node.id"
            :node="node"
            :reveal="node.reveal"
            :condensed="condensedNodeCards"
            :empty-label="workflowEmptyLabel"
            :style="{ left: `${node.left}px`, top: `${node.top}px` }"
            @click="handleNodeClick(node.id)"
            @dblclick="handleNodeDoubleClick(node.id)"
            @preview-artifact="handleArtifactPreview(node.id, $event)"
          />
        </div>
      </div>
    </div>
    <div class="beeroom-canvas-tools" role="toolbar" :aria-label="canvasControlLabels.toolbar">
      <button
        class="beeroom-canvas-tool-btn"
        type="button"
        :title="canvasControlLabels.zoomIn"
        :aria-label="canvasControlLabels.zoomIn"
        @click="zoomIn"
      >
        <i class="fa-solid fa-magnifying-glass-plus" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.zoomIn }}</span>
      </button>
      <button
        class="beeroom-canvas-tool-btn"
        type="button"
        :title="canvasControlLabels.zoomOut"
        :aria-label="canvasControlLabels.zoomOut"
        @click="zoomOut"
      >
        <i class="fa-solid fa-magnifying-glass-minus" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.zoomOut }}</span>
      </button>
      <button
        class="beeroom-canvas-tool-btn"
        type="button"
        :title="canvasControlLabels.fitView"
        :aria-label="canvasControlLabels.fitView"
        @click="fitView(true)"
      >
        <i class="fa-solid fa-expand" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.fitView }}</span>
      </button>
      <button
        class="beeroom-canvas-tool-btn"
        type="button"
        :title="canvasControlLabels.regularize"
        :aria-label="canvasControlLabels.regularize"
        @click="regularizeLayout"
      >
        <i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.regularize }}</span>
      </button>
      <button
        class="beeroom-canvas-tool-btn"
        :class="{ 'is-active': fullscreen }"
        type="button"
        :title="fullscreen ? canvasControlLabels.exitFullscreen : canvasControlLabels.enterFullscreen"
        :aria-label="fullscreen ? canvasControlLabels.exitFullscreen : canvasControlLabels.enterFullscreen"
        :aria-pressed="fullscreen"
        @click="emit('toggle-fullscreen')"
      >
        <i class="fa-solid" :class="fullscreen ? 'fa-minimize' : 'fa-maximize'" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">
          {{ fullscreen ? canvasControlLabels.exitFullscreen : canvasControlLabels.enterFullscreen }}
        </span>
      </button>
    </div>

    <div v-if="props.showMinimap !== false" class="beeroom-canvas-minimap-shell">
      <div class="beeroom-canvas-minimap-label">{{ t('beeroom.canvas.minimap') }}</div>
      <button class="beeroom-canvas-minimap" type="button" @click="handleMinimapClick">
        <svg
          class="beeroom-canvas-minimap-svg"
          :viewBox="`0 0 ${MINIMAP_WIDTH} ${MINIMAP_HEIGHT}`"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <path
            v-for="edge in minimapEdges"
            :key="edge.id"
            class="beeroom-canvas-minimap-edge"
            :class="{ 'is-subagent': edge.kind === 'subagent' }"
            :d="edge.path"
          />
          <rect
            v-for="node in minimapNodes"
            :key="node.id"
            class="beeroom-canvas-minimap-node"
            :class="[`is-${node.status}`, `is-${node.role}`, `is-emphasis-${node.emphasis}`]"
            :x="node.x"
            :y="node.y"
            :width="node.width"
            :height="node.height"
            rx="3"
          />
          <rect
            class="beeroom-canvas-minimap-viewport"
            :x="minimapViewportRect.x"
            :y="minimapViewportRect.y"
            :width="minimapViewportRect.width"
            :height="minimapViewportRect.height"
            rx="8"
          />
        </svg>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import { chatDebugLog } from '@/utils/chatDebug';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState,
  clearBeeroomMissionCanvasState,
  type BeeroomCanvasPositionOverride,
  type BeeroomCanvasViewportState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type { BeeroomMissionSubagentItem } from '@/components/beeroom/useBeeroomMissionSubagentPreview';
import type { BeeroomWorkflowItem, BeeroomTaskWorkflowPreview } from '@/components/beeroom/beeroomTaskWorkflow';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';
import {
  buildBeeroomSwarmSubagentProjectionContext,
  resolveBeeroomSwarmSubagentProjectionDecision
} from '@/components/beeroom/canvas/beeroomSwarmSubagentProjection';

import BeeroomSwarmNodeCard from './BeeroomSwarmNodeCard.vue';
import {
  NODE_HEIGHT,
  NODE_WIDTH,
  WORLD_PADDING,
  type SwarmProjection,
  type SwarmProjectionArtifactItem,
  type BeeroomSwarmDispatchPreview,
  buildBeeroomSwarmProjection,
  hasBeeroomSwarmNodes,
  resolveBeeroomSwarmScopeKey
} from './swarmCanvasModel';
import {
  SWARM_SCALE_STEP,
  clampSwarmScale,
  createDefaultSwarmViewportState,
  normalizeSwarmViewportSize,
  zoomSwarmViewportAroundPoint,
  type SwarmViewportState
} from './useBeeroomSwarmViewport';

const MINIMAP_WIDTH = 132;
const MINIMAP_HEIGHT = 80;
const WORLD_DRAG_BUFFER = 720;
const MINIMAP_REFERENCE_BUFFER = 720;
const NODE_OUTPUT_PREVIEW_CLICK_DELAY_MS = 220;
const GRID_PATTERN_ID_BASE = `beeroom-swarm-grid-${Math.random().toString(36).slice(2, 10)}`;

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  dispatchPreview: BeeroomSwarmDispatchPreview | null;
  subagentsByTask: Record<string, BeeroomMissionSubagentItem[]>;
  motherWorkflowItems: BeeroomWorkflowItem[];
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  resolveAgentAvatarImageByAgentId?: (agentId: unknown) => string;
  resolveAgentAvatarColorByAgentId?: (agentId: unknown) => string;
  fullscreen?: boolean;
  externalProjection?: SwarmProjection | null;
  externalScopeKey?: string;
  externalHasNodes?: boolean;
  showMinimap?: boolean;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'preview-node-output', payload: {
    nodeId: string;
    agentId: string;
    agentName: string;
    roleLabel: string;
    statusLabel: string;
  }): void;
  (event: 'preview-artifact', payload: { nodeId: string; item: SwarmProjectionArtifactItem }): void;
  (event: 'toggle-fullscreen'): void;
}>();

type DragState = {
  nodeId: string;
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  moved: boolean;
};

type PanState = {
  pointerId: number;
  startX: number;
  startY: number;
  originOffsetX: number;
  originOffsetY: number;
  moved: boolean;
};

const { t } = useI18n();
const __canvasControlLabelsLegacy = {
  toolbar: '画布工具',
  zoomIn: '放大',
  zoomOut: '缩小',
  fitView: '适应视图',
  enterFullscreen: '进入全屏',
  exitFullscreen: '退出全屏'
} as const;
const canvasControlLabels = computed(() => ({
  toolbar: t('beeroom.canvas.toolbar'),
  zoomIn: t('beeroom.canvas.zoomIn'),
  zoomOut: t('beeroom.canvas.zoomOut'),
  fitView: t('beeroom.canvas.fitView'),
  regularize: t('beeroom.canvas.regularize'),
  enterFullscreen: t('beeroom.canvas.enterFullscreen'),
  exitFullscreen: t('beeroom.canvas.exitFullscreen')
}));
const viewportRef = ref<HTMLDivElement | null>(null);
const containerSize = ref(normalizeSwarmViewportSize({ width: 0, height: 0 }));
const selectedNodeId = ref('');
const nodePositionOverrides = ref<Record<string, BeeroomCanvasPositionOverride>>({});
const viewportState = ref<SwarmViewportState>(createDefaultSwarmViewportState());
const pendingViewportRestore = ref<BeeroomCanvasViewportState | null>(null);
const pendingFitView = ref(false);
const nodeRevealMap = ref<Record<string, { fromId: string; order: number }>>({});

let resizeObserver: ResizeObserver | null = null;
let viewportSaveTimer: number | null = null;
let dragState: DragState | null = null;
let panState: PanState | null = null;
let releaseInteractionListeners: (() => void) | null = null;
let dragPointerTarget: HTMLElement | null = null;
let suppressSelection = false;
let pendingNodeOutputPreviewTimer: number | null = null;
const knownProjectionNodeIds = new Set<string>();
const knownProjectionNodeDispatchActivity = new Map<string, boolean>();
const revealCleanupTimers = new Map<string, number>();
const revealBaselineReady = ref(false);

const scopeKey = computed(() =>
  String(props.externalScopeKey || '').trim() ||
  resolveBeeroomSwarmScopeKey({
    missionId: props.mission?.mission_id,
    teamRunId: props.mission?.team_run_id,
    groupId: props.group?.group_id
  })
);

const applyProjectionInteractionOverrides = (
  source: SwarmProjection | null | undefined,
  options: {
    selectedNodeId: string;
    nodePositionOverrides: Record<string, BeeroomCanvasPositionOverride>;
  }
): SwarmProjection => {
  const base = source || {
    nodes: [],
    edges: [],
    nodeMetaMap: new Map(),
    memberMap: new Map(),
    tasksByAgent: new Map(),
    motherNodeId: '',
    bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 }
  };
  const selectedNodeId = String(options.selectedNodeId || '').trim();
  const nodes = base.nodes.map((node) => {
    const override = options.nodePositionOverrides[node.id];
    const x = Number.isFinite(Number(override?.x)) ? Math.round(Number(override.x)) : node.x;
    const y = Number.isFinite(Number(override?.y)) ? Math.round(Number(override.y)) : node.y;
    return {
      ...node,
      x,
      y,
      selected: node.id === selectedNodeId
    };
  });
  if (!nodes.length) {
    return {
      ...base,
      nodes,
      edges: base.edges.map((edge) => ({ ...edge, selected: false })),
      bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 }
    };
  }
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  nodes.forEach((node) => {
    minX = Math.min(minX, node.x - node.width / 2);
    minY = Math.min(minY, node.y - node.height / 2);
    maxX = Math.max(maxX, node.x + node.width / 2);
    maxY = Math.max(maxY, node.y + node.height / 2);
  });
  return {
    ...base,
    nodes,
    edges: base.edges.map((edge) => ({
      ...edge,
      selected: Boolean(selectedNodeId) && (edge.source === selectedNodeId || edge.target === selectedNodeId)
    })),
    bounds: {
      minX,
      minY,
      maxX,
      maxY,
      width: Math.max(0, maxX - minX),
      height: Math.max(0, maxY - minY)
    }
  };
};

const projection = computed(() => {
  if (props.externalProjection) {
    return applyProjectionInteractionOverrides(props.externalProjection, {
      selectedNodeId: selectedNodeId.value,
      nodePositionOverrides: nodePositionOverrides.value
    });
  }
  return buildBeeroomSwarmProjection({
    group: props.group,
    mission: props.mission,
    agents: props.agents,
    selectedNodeId: selectedNodeId.value,
    nodePositionOverrides: nodePositionOverrides.value,
    dispatchPreview: props.dispatchPreview,
    subagentsByTask: props.subagentsByTask,
    motherWorkflowItems: props.motherWorkflowItems,
    workflowItemsByTask: props.workflowItemsByTask,
    workflowPreviewByTask: props.workflowPreviewByTask,
    resolveAgentAvatarImageByAgentId: props.resolveAgentAvatarImageByAgentId,
    resolveAgentAvatarColorByAgentId: props.resolveAgentAvatarColorByAgentId,
    t
  });
});
const swarmTaskProjectionContext = computed(() =>
  buildBeeroomSwarmSubagentProjectionContext(Array.isArray(props.mission?.tasks) ? props.mission.tasks : [])
);

const baseProjection = computed(() => {
  if (props.externalProjection) {
    return applyProjectionInteractionOverrides(props.externalProjection, {
      selectedNodeId: '',
      nodePositionOverrides: {}
    });
  }
  return buildBeeroomSwarmProjection({
    group: props.group,
    mission: props.mission,
    agents: props.agents,
    selectedNodeId: selectedNodeId.value,
    nodePositionOverrides: {},
    dispatchPreview: props.dispatchPreview,
    subagentsByTask: props.subagentsByTask,
    motherWorkflowItems: props.motherWorkflowItems,
    workflowItemsByTask: props.workflowItemsByTask,
    workflowPreviewByTask: props.workflowPreviewByTask,
    resolveAgentAvatarImageByAgentId: props.resolveAgentAvatarImageByAgentId,
    resolveAgentAvatarColorByAgentId: props.resolveAgentAvatarColorByAgentId,
    t
  });
});

const hasNodes = computed(() =>
  props.externalHasNodes === true ||
  hasBeeroomSwarmNodes({
    group: props.group,
    mission: props.mission,
    agents: props.agents,
    dispatchPreview: props.dispatchPreview
  })
);

const worldMetrics = computed(() => {
  const bounds = projection.value.bounds;
  const baseBounds = baseProjection.value.bounds;
  const worldMinX = baseBounds.minX - WORLD_DRAG_BUFFER;
  const worldMinY = baseBounds.minY - WORLD_DRAG_BUFFER;
  const worldMaxX = Math.max(bounds.maxX, baseBounds.maxX);
  const worldMaxY = Math.max(bounds.maxY, baseBounds.maxY);
  return {
    width: Math.max(NODE_WIDTH + WORLD_PADDING * 2, Math.ceil(worldMaxX - worldMinX + WORLD_PADDING * 2)),
    height: Math.max(NODE_HEIGHT + WORLD_PADDING * 2, Math.ceil(worldMaxY - worldMinY + WORLD_PADDING * 2)),
    originX: Math.round(WORLD_PADDING - worldMinX),
    originY: Math.round(WORLD_PADDING - worldMinY)
  };
});

const worldSize = computed(() => ({
  width: worldMetrics.value.width,
  height: worldMetrics.value.height
}));

const worldNodes = computed(() => {
  const baseNodes = projection.value.nodes.map((node) => {
    const centerX = Math.round(node.x + worldMetrics.value.originX);
    const centerY = Math.round(node.y + worldMetrics.value.originY);
    return {
      ...node,
      centerX,
      centerY,
      left: Math.round(centerX - node.width / 2),
      top: Math.round(centerY - node.height / 2)
    };
  });
  const baseNodeMap = new Map<string, (typeof baseNodes)[number]>();
  baseNodes.forEach((node) => {
    baseNodeMap.set(node.id, node);
  });
  return baseNodes.map((node) => {
    const reveal = nodeRevealMap.value[node.id];
    const sourceNode = reveal ? baseNodeMap.get(reveal.fromId) : null;
    return {
      ...node,
      reveal: reveal
        ? {
            offsetX: Math.round(node.centerX - Number(sourceNode?.centerX || node.centerX)),
            offsetY: Math.round(node.centerY - Number(sourceNode?.centerY || node.centerY)),
            order: reveal.order
          }
        : null
    };
  });
});

const worldNodeMap = computed(() => {
  const map = new Map<string, (typeof worldNodes.value)[number]>();
  worldNodes.value.forEach((node) => {
    map.set(node.id, node);
  });
  return map;
});

const worldEdges = computed(() =>
  projection.value.edges
    .map((edge, index) => {
      const source = worldNodeMap.value.get(edge.source);
      const target = worldNodeMap.value.get(edge.target);
      if (!source || !target) return null;
      const dx = target.centerX - source.centerX;
      const controlOffset = Math.max(42, Math.abs(dx) * 0.22);
      const revealing = Boolean(nodeRevealMap.value[target.id]);
      const motionSeed = index % 4;
      const path = `M ${source.centerX} ${source.centerY} C ${source.centerX + controlOffset} ${source.centerY} ${target.centerX - controlOffset} ${target.centerY} ${target.centerX} ${target.centerY}`;
      return {
        ...edge,
        path,
        revealing,
        labelX: Math.round(source.centerX + dx / 2),
        labelY: Math.round(source.centerY + (target.centerY - source.centerY) / 2 - 10),
        motionDuration: `${edge.kind === 'subagent' ? 1.18 : 1.4 + motionSeed * 0.12}s`,
        motionDelay: `${-(motionSeed * 0.18)}s`,
        motionTrailDelay: `${-(0.72 + motionSeed * 0.18)}s`
      };
    })
    .filter((edge): edge is NonNullable<typeof edge> => Boolean(edge))
);

const condensedNodeCards = computed(
  () =>
    projection.value.nodes.length > 48 ||
    containerSize.value.width < 920 ||
    clampSwarmScale(viewportState.value.scale) < 0.82
);

const worldStyle = computed(() => {
  // Always render with normalized scale to avoid stale fractional values from cache/history.
  const scale = clampSwarmScale(viewportState.value.scale);
  return {
    width: `${worldSize.value.width}px`,
    height: `${worldSize.value.height}px`,
    transform: `translate(${Math.round(viewportState.value.offsetX)}px, ${Math.round(viewportState.value.offsetY)}px) scale(${scale})`
  };
});

const surfaceGridPattern = computed(() => {
  const scale = clampSwarmScale(viewportState.value.scale);
  const dpr = typeof window !== 'undefined' ? Math.max(1, Number(window.devicePixelRatio || 1)) : 1;
  const snapToDevicePixel = (value: number) => Math.round(value * dpr) / dpr;
  const formatUnit = (value: number) => Number(value.toFixed(3));
  const normalizeOffset = (value: number, size: number) => {
    const remainder = value % size;
    const normalized = remainder < 0 ? remainder + size : remainder;
    return formatUnit(snapToDevicePixel(normalized));
  };

  const buildHexagonPath = (centerX: number, centerY: number, side: number) => {
    const halfHeight = (Math.sqrt(3) * side) / 2;
    const points = [
      [centerX - side, centerY],
      [centerX - side / 2, centerY - halfHeight],
      [centerX + side / 2, centerY - halfHeight],
      [centerX + side, centerY],
      [centerX + side / 2, centerY + halfHeight],
      [centerX - side / 2, centerY + halfHeight]
    ];
    return points
      .map(([x, y], index) => `${index === 0 ? 'M' : 'L'} ${formatUnit(x)} ${formatUnit(y)}`)
      .join(' ')
      .concat(' Z');
  };

  // Repeat the honeycomb on a minimal tile so pan/zoom keep the pattern aligned with world space.
  const buildPatternLayer = (side: number, strokeWidth: number, suffix: string) => {
    const tileWidth = formatUnit(side * 3);
    const tileHeight = formatUnit(Math.sqrt(3) * side);
    const leftColumnCenterX = 0;
    const middleColumnCenterX = side * 1.5;
    const rightColumnCenterX = side * 3;
    const path = [
      buildHexagonPath(leftColumnCenterX, 0, side),
      buildHexagonPath(leftColumnCenterX, tileHeight, side),
      buildHexagonPath(middleColumnCenterX, tileHeight / 2, side),
      buildHexagonPath(rightColumnCenterX, 0, side),
      buildHexagonPath(rightColumnCenterX, tileHeight, side)
    ].join(' ');
    return {
      id: `${GRID_PATTERN_ID_BASE}-${suffix}`,
      tileWidth,
      tileHeight,
      offsetX: normalizeOffset(viewportState.value.offsetX, tileWidth),
      offsetY: normalizeOffset(viewportState.value.offsetY, tileHeight),
      strokeWidth: formatUnit(strokeWidth),
      path
    };
  };

  const majorSide = Math.max(54, snapToDevicePixel(54 * scale));
  return buildPatternLayer(majorSide, 1 / dpr, 'major');
});

const workflowEmptyLabel = computed(() => t('chat.toolWorkflow.empty'));

const minimapReferenceWorldSize = computed(() => {
  const baseBounds = baseProjection.value.bounds;
  const worldMinX = baseBounds.minX - WORLD_DRAG_BUFFER;
  const worldMinY = baseBounds.minY - WORLD_DRAG_BUFFER;
  const worldMaxX = baseBounds.maxX + MINIMAP_REFERENCE_BUFFER;
  const worldMaxY = baseBounds.maxY + MINIMAP_REFERENCE_BUFFER;
  return {
    width: Math.max(NODE_WIDTH + WORLD_PADDING * 2, Math.ceil(worldMaxX - worldMinX + WORLD_PADDING * 2)),
    height: Math.max(NODE_HEIGHT + WORLD_PADDING * 2, Math.ceil(worldMaxY - worldMinY + WORLD_PADDING * 2))
  };
});

const minimapScale = computed(() =>
  Math.min(
    (MINIMAP_WIDTH - 10) / Math.max(1, minimapReferenceWorldSize.value.width),
    (MINIMAP_HEIGHT - 10) / Math.max(1, minimapReferenceWorldSize.value.height)
  )
);

const minimapOffset = computed(() => ({
  x: (MINIMAP_WIDTH - minimapReferenceWorldSize.value.width * minimapScale.value) / 2,
  y: (MINIMAP_HEIGHT - minimapReferenceWorldSize.value.height * minimapScale.value) / 2
}));

const minimapWorldWindow = computed(() => {
  const width = minimapReferenceWorldSize.value.width;
  const height = minimapReferenceWorldSize.value.height;
  const scale = clampSwarmScale(viewportState.value.scale);
  const viewportCenterX = (-viewportState.value.offsetX + containerSize.value.width / 2) / scale;
  const viewportCenterY = (-viewportState.value.offsetY + containerSize.value.height / 2) / scale;
  return {
    left: viewportCenterX - width / 2,
    top: viewportCenterY - height / 2,
    width,
    height
  };
});

const mapWorldXToMinimap = (worldX: number) =>
  minimapOffset.value.x + (worldX - minimapWorldWindow.value.left) * minimapScale.value;

const mapWorldYToMinimap = (worldY: number) =>
  minimapOffset.value.y + (worldY - minimapWorldWindow.value.top) * minimapScale.value;

const minimapNodes = computed(() =>
  worldNodes.value.map((node) => {
    const width = Math.max(4, node.width * minimapScale.value);
    const height = Math.max(4, node.height * minimapScale.value);
    const rawX = mapWorldXToMinimap(node.left);
    const rawY = mapWorldYToMinimap(node.top);
    return {
      id: node.id,
      status: node.status,
      role: node.role,
      emphasis: node.emphasis,
      x: rawX,
      y: rawY,
      width,
      height
    };
  })
);

const minimapEdges = computed(() =>
  projection.value.edges
    .map((edge) => {
      const source = worldNodeMap.value.get(edge.source);
      const target = worldNodeMap.value.get(edge.target);
      if (!source || !target) return null;
      const sx = mapWorldXToMinimap(source.centerX);
      const sy = mapWorldYToMinimap(source.centerY);
      const tx = mapWorldXToMinimap(target.centerX);
      const ty = mapWorldYToMinimap(target.centerY);
      const dx = tx - sx;
      const controlOffset = Math.max(6, Math.abs(dx) * 0.22);
      return {
        id: edge.id,
        kind: edge.kind,
        path: `M ${sx} ${sy} C ${sx + controlOffset} ${sy} ${tx - controlOffset} ${ty} ${tx} ${ty}`
      };
    })
    .filter((edge): edge is NonNullable<typeof edge> => Boolean(edge))
);

const minimapViewportRect = computed(() => {
  const scale = clampSwarmScale(viewportState.value.scale);
  const viewportWorldWidth = Math.min(minimapWorldWindow.value.width, containerSize.value.width / scale);
  const viewportWorldHeight = Math.min(minimapWorldWindow.value.height, containerSize.value.height / scale);
  const worldX = -viewportState.value.offsetX / scale;
  const worldY = -viewportState.value.offsetY / scale;
  const width = Math.max(6, viewportWorldWidth * minimapScale.value);
  const height = Math.max(6, viewportWorldHeight * minimapScale.value);
  return {
    x: mapWorldXToMinimap(worldX),
    y: mapWorldYToMinimap(worldY),
    width,
    height
  };
});

const projectionSignature = computed(() =>
  [
    scopeKey.value,
    projection.value.nodes
      .map(
        (node) =>
          `${node.id}:${node.role}:${node.parentId}:${node.x}:${node.y}:${node.selected}:${node.workflowTaskId}:${node.status}:${node.emphasis}`
      )
      .join('|'),
    projection.value.edges.map((edge) => `${edge.id}:${edge.kind}:${edge.active}:${edge.selected}:${edge.label}`).join('|')
  ].join('||')
);

const logCanvasProjection = (event: string, payload?: unknown) => {
  chatDebugLog('beeroom.canvas-projection', event, payload);
};

const summarizeInputSubagentForCanvas = (item: BeeroomMissionSubagentItem) => {
  const decision = resolveBeeroomSwarmSubagentProjectionDecision(item, swarmTaskProjectionContext.value);
  return {
    key: item.key,
    sessionId: item.sessionId,
    runId: item.runId,
    runKind: item.runKind,
    requestedBy: item.requestedBy,
    projectable: decision.projectable,
    reason: decision.reason,
    status: item.status
  };
};

const buildCanvasProjectionDebugSnapshot = () => {
  const nodes = projection.value.nodes;
  const edges = projection.value.edges;
  const projectedSubagentNodes = nodes
    .filter((node) => node.role === 'subagent')
    .map((node) => ({
      id: node.id,
      parentId: node.parentId,
      agentId: node.agentId,
      status: node.status,
      emphasis: node.emphasis
    }));
  const taskSubagentBuckets = Object.entries(props.subagentsByTask || {})
    .slice(0, 8)
    .map(([taskId, items]) => ({
      taskId,
      total: items.length,
      projectable: items.filter((item) =>
        resolveBeeroomSwarmSubagentProjectionDecision(item, swarmTaskProjectionContext.value).projectable
      ).length,
      items: items.slice(0, 4).map((item) => summarizeInputSubagentForCanvas(item))
    }));

  return {
    scopeKey: scopeKey.value,
    motherNodeId: projection.value.motherNodeId,
    selectedNodeId: selectedNodeId.value,
    runtimeDispatchSessionId: String(props.dispatchPreview?.sessionId || '').trim(),
    runtimeDispatchTargetAgentId: String(props.dispatchPreview?.targetAgentId || '').trim(),
    runtimeDispatchSubagents: (Array.isArray(props.dispatchPreview?.subagents) ? props.dispatchPreview?.subagents : [])
      .slice(0, 8)
      .map((item) => summarizeInputSubagentForCanvas(item)),
    nodeCount: nodes.length,
    edgeCount: edges.length,
    workerNodeCount: nodes.filter((node) => node.role === 'worker').length,
    subagentNodeCount: projectedSubagentNodes.length,
    projectedSubagentNodes,
    taskSubagentBuckets
  };
};

const syncNodeRevealState = () => {
  const currentNodes = projection.value.nodes;
  const currentIds = new Set(currentNodes.map((node) => node.id));
  if (!revealBaselineReady.value) {
    knownProjectionNodeIds.clear();
    knownProjectionNodeDispatchActivity.clear();
    currentNodes.forEach((node) => {
      const normalizedStatus = String(node.status || '').trim().toLowerCase();
      const workerDispatchActive =
        node.role === 'worker' &&
        (normalizedStatus === 'queued' || normalizedStatus === 'running' || normalizedStatus === 'awaiting_idle');
      knownProjectionNodeIds.add(node.id);
      knownProjectionNodeDispatchActivity.set(node.id, workerDispatchActive);
    });
    if (Object.keys(nodeRevealMap.value).length > 0) {
      nodeRevealMap.value = {};
    }
    revealBaselineReady.value = true;
    return;
  }
  const activeDispatchSourceByTarget = new Map<string, string>();
  projection.value.edges.forEach((edge) => {
    if (edge.kind !== 'dispatch' || !edge.active) return;
    const targetId = String(edge.target || '').trim();
    const sourceId = String(edge.source || '').trim();
    if (!targetId || !sourceId) return;
    activeDispatchSourceByTarget.set(targetId, sourceId);
  });
  const nextRevealState = { ...nodeRevealMap.value };
  let changed = false;

  Object.keys(nextRevealState).forEach((nodeId) => {
    if (currentIds.has(nodeId)) return;
    const cleanupTimer = revealCleanupTimers.get(nodeId);
    if (cleanupTimer !== undefined && typeof window !== 'undefined') {
      window.clearTimeout(cleanupTimer);
      revealCleanupTimers.delete(nodeId);
    }
    delete nextRevealState[nodeId];
    changed = true;
  });

  Array.from(knownProjectionNodeIds).forEach((nodeId) => {
    if (currentIds.has(nodeId)) return;
    knownProjectionNodeIds.delete(nodeId);
    knownProjectionNodeDispatchActivity.delete(nodeId);
  });

  currentNodes.forEach((node) => {
    const isKnownNode = knownProjectionNodeIds.has(node.id);
    const normalizedStatus = String(node.status || '').trim().toLowerCase();
    const workerRevealSourceId = activeDispatchSourceByTarget.get(node.id) || '';
    const workerDispatchActive =
      node.role === 'worker' &&
      Boolean(workerRevealSourceId) &&
      (normalizedStatus === 'queued' || normalizedStatus === 'running' || normalizedStatus === 'awaiting_idle');
    const wasWorkerDispatchActive = knownProjectionNodeDispatchActivity.get(node.id) === true;
    const shouldRevealSubagent = node.role === 'subagent' && Boolean(node.introFromId) && !isKnownNode;
    const shouldRevealWorker = workerDispatchActive && (!isKnownNode || !wasWorkerDispatchActive);
    if (shouldRevealSubagent || shouldRevealWorker) {
      const revealSourceId =
        node.role === 'subagent' ? String(node.introFromId || '').trim() : workerRevealSourceId;
      nextRevealState[node.id] = {
        fromId: revealSourceId,
        order: node.role === 'subagent' ? Number(node.introOrder || 0) : 0
      };
      if (typeof window !== 'undefined') {
        const existingTimer = revealCleanupTimers.get(node.id);
        if (existingTimer !== undefined) {
          window.clearTimeout(existingTimer);
        }
        const revealDuration =
          node.role === 'subagent'
            ? 900 + Math.max(0, Number(node.introOrder || 0)) * 70
            : 760;
        const timer = window.setTimeout(() => {
          revealCleanupTimers.delete(node.id);
          if (!nodeRevealMap.value[node.id]) return;
          const next = { ...nodeRevealMap.value };
          delete next[node.id];
          nodeRevealMap.value = next;
        }, revealDuration);
        revealCleanupTimers.set(node.id, timer);
      }
      changed = true;
    }
    knownProjectionNodeDispatchActivity.set(node.id, workerDispatchActive);
    knownProjectionNodeIds.add(node.id);
  });

  if (changed) {
    nodeRevealMap.value = nextRevealState;
  }
};

const freezeCurrentProjectionAsRevealBaseline = () => {
  knownProjectionNodeIds.clear();
  knownProjectionNodeDispatchActivity.clear();
  projection.value.nodes.forEach((node) => {
    const normalizedStatus = String(node.status || '').trim().toLowerCase();
    const workerDispatchActive =
      node.role === 'worker' &&
      (normalizedStatus === 'queued' || normalizedStatus === 'running' || normalizedStatus === 'awaiting_idle');
    knownProjectionNodeIds.add(node.id);
    knownProjectionNodeDispatchActivity.set(node.id, workerDispatchActive);
  });
  revealCleanupTimers.forEach((timer) => {
    if (typeof window !== 'undefined') {
      window.clearTimeout(timer);
    }
  });
  revealCleanupTimers.clear();
  if (Object.keys(nodeRevealMap.value).length > 0) {
    nodeRevealMap.value = {};
  }
  revealBaselineReady.value = true;
};

const clearViewportSaveTimer = () => {
  if (viewportSaveTimer !== null) {
    window.clearTimeout(viewportSaveTimer);
    viewportSaveTimer = null;
  }
};

const saveNodeState = () => {
  mergeBeeroomMissionCanvasState(scopeKey.value, {
    nodePositionOverrides: nodePositionOverrides.value,
    activeNodeId: selectedNodeId.value
  });
};

const saveViewportState = () => {
  mergeBeeroomMissionCanvasState(scopeKey.value, {
    viewport: {
      scale: viewportState.value.scale,
      offsetX: viewportState.value.offsetX,
      offsetY: viewportState.value.offsetY
    }
  });
};

const scheduleViewportStateSave = (delayMs = 120) => {
  clearViewportSaveTimer();
  viewportSaveTimer = window.setTimeout(() => {
    viewportSaveTimer = null;
    saveViewportState();
  }, delayMs);
};

const hydrateCanvasState = () => {
  const cached = getBeeroomMissionCanvasState(scopeKey.value);
  nodePositionOverrides.value = { ...(cached?.nodePositionOverrides || {}) };
  selectedNodeId.value = String(cached?.activeNodeId || '').trim();
  pendingViewportRestore.value = cached?.viewport || null;
  pendingFitView.value = !cached?.viewport;
};

const regularizeLayout = async () => {
  clearPendingNodeOutputPreview();
  clearInteractions();
  clearBeeroomMissionCanvasState(scopeKey.value);
  nodePositionOverrides.value = {};
  pendingViewportRestore.value = null;
  pendingFitView.value = false;
  await fitView(true);
};

const fitView = async (force = false) => {
  await nextTick();
  if (!viewportRef.value || !projection.value.nodes.length) return;
  const viewport = normalizeSwarmViewportSize(containerSize.value);
  const padding = force ? 36 : 44;
  const bounds = projection.value.bounds;
  const contentWidth = Math.max(1, Math.ceil(bounds.width));
  const contentHeight = Math.max(1, Math.ceil(bounds.height));
  const scale = clampSwarmScale(
    Math.min((viewport.width - padding * 2) / contentWidth, (viewport.height - padding * 2) / contentHeight, 1)
  );
  const contentCenterX = worldMetrics.value.originX + bounds.minX + bounds.width / 2;
  const contentCenterY = worldMetrics.value.originY + bounds.minY + bounds.height / 2;
  viewportState.value = {
    scale,
    offsetX: Math.round(viewport.width / 2 - contentCenterX * scale),
    offsetY: Math.round(viewport.height / 2 - contentCenterY * scale)
  };
  saveViewportState();
};

const applyRestoredViewport = () => {
  if (!pendingViewportRestore.value) return false;
  viewportState.value = {
    scale: clampSwarmScale(Number(pendingViewportRestore.value.scale || 1)),
    offsetX: Number(pendingViewportRestore.value.offsetX || 0),
    offsetY: Number(pendingViewportRestore.value.offsetY || 0)
  };
  pendingViewportRestore.value = null;
  return true;
};

const updateContainerSize = () => {
  const rect = viewportRef.value?.getBoundingClientRect();
  containerSize.value = normalizeSwarmViewportSize({
    width: Math.floor(rect?.width || viewportRef.value?.clientWidth || 0),
    height: Math.floor(rect?.height || viewportRef.value?.clientHeight || 0)
  });
};

const handleWindowResize = () => {
  updateContainerSize();
};

const zoomTo = (nextScale: number, anchorX = containerSize.value.width / 2, anchorY = containerSize.value.height / 2) => {
  viewportState.value = zoomSwarmViewportAroundPoint({
    viewportState: viewportState.value,
    nextScale,
    anchorX,
    anchorY
  });
  scheduleViewportStateSave();
};

const zoomIn = () => zoomTo(viewportState.value.scale + SWARM_SCALE_STEP);
const zoomOut = () => zoomTo(viewportState.value.scale - SWARM_SCALE_STEP);

const handleViewportWheel = (event: WheelEvent) => {
  const rect = viewportRef.value?.getBoundingClientRect();
  if (!rect) return;
  const anchorX = Number(event.clientX || 0) - rect.left;
  const anchorY = Number(event.clientY || 0) - rect.top;
  zoomTo(
    viewportState.value.scale + (event.deltaY < 0 ? SWARM_SCALE_STEP : -SWARM_SCALE_STEP),
    anchorX,
    anchorY
  );
};

const clearPendingNodeOutputPreview = () => {
  if (pendingNodeOutputPreviewTimer === null || typeof window === 'undefined') return;
  window.clearTimeout(pendingNodeOutputPreviewTimer);
  pendingNodeOutputPreviewTimer = null;
};

const emitNodeOutputPreview = (nodeId: string) => {
  const node = worldNodeMap.value.get(String(nodeId || '').trim());
  const agentId = String(node?.agentId || '').trim();
  if (!node || !agentId) return;
  emit('preview-node-output', {
    nodeId: node.id,
    agentId,
    agentName: String(node.name || node.displayName || agentId).trim() || agentId,
    roleLabel: String(node.roleLabel || '').trim(),
    statusLabel: String(node.statusLabel || '').trim()
  });
};

const scheduleNodeOutputPreview = (nodeId: string) => {
  clearPendingNodeOutputPreview();
  if (typeof window === 'undefined') {
    emitNodeOutputPreview(nodeId);
    return;
  }
  pendingNodeOutputPreviewTimer = window.setTimeout(() => {
    pendingNodeOutputPreviewTimer = null;
    emitNodeOutputPreview(nodeId);
  }, NODE_OUTPUT_PREVIEW_CLICK_DELAY_MS);
};

const handleNodeClick = (nodeId: string) => {
  if (suppressSelection) {
    suppressSelection = false;
    return;
  }
  selectedNodeId.value = nodeId;
  saveNodeState();
  scheduleNodeOutputPreview(nodeId);
};

const handleNodeDoubleClick = (nodeId: string) => {
  clearPendingNodeOutputPreview();
  const node = worldNodeMap.value.get(String(nodeId || '').trim());
  const agentId = String(node?.agentId || '').trim();
  if (agentId) {
    emit('open-agent', agentId);
  }
};

const handleArtifactPreview = (nodeId: string, item: SwarmProjectionArtifactItem) => {
  clearPendingNodeOutputPreview();
  emit('preview-artifact', {
    nodeId: String(nodeId || '').trim(),
    item
  });
};

const handleNodePointerDown = (nodeId: string, event: PointerEvent, pointerTarget?: HTMLElement | null) => {
  if (event.button !== 0) return;
  clearPendingNodeOutputPreview();
  event.preventDefault();
  event.stopPropagation();
  event.stopImmediatePropagation?.();
  const node = projection.value.nodes.find((item) => item.id === nodeId);
  if (!node) return;
  panState = null;
  dragPointerTarget = pointerTarget || null;
  dragPointerTarget?.setPointerCapture?.(event.pointerId);
  dragPointerTarget?.classList.add('is-dragging');
  dragState = {
    nodeId,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    originX: node.x,
    originY: node.y,
    moved: false
  };
  bindInteractionListeners();
};

const handleNodeLayerPointerDown = (event: PointerEvent) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest?.('.beeroom-node-artifact-scroll')) {
    return;
  }
  const card = target?.closest?.('.beeroom-node-card') as HTMLElement | null;
  const nodeId = String(card?.dataset.nodeId || '').trim();
  if (!card || !nodeId) {
    return;
  }
  handleNodePointerDown(nodeId, event, card);
};

const handleViewportPointerDown = (event: PointerEvent) => {
  if (event.button !== 0) return;
  clearPendingNodeOutputPreview();
  const target = event.target as HTMLElement | null;
  if (target?.closest?.('.beeroom-node-card')) return;
  if (dragState) return;
  event.preventDefault();
  bindInteractionListeners();
  panState = {
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    originOffsetX: viewportState.value.offsetX,
    originOffsetY: viewportState.value.offsetY,
    moved: false
  };
};

const clearInteractions = () => {
  const dragPointerId = dragState?.pointerId ?? null;
  if (dragState?.moved) {
    suppressSelection = true;
    saveNodeState();
  }
  if (panState?.moved) {
    scheduleViewportStateSave(60);
  }
  if (dragPointerTarget && dragPointerId !== null && dragPointerTarget.hasPointerCapture?.(dragPointerId)) {
    dragPointerTarget.releasePointerCapture(dragPointerId);
  }
  if (dragPointerTarget) {
    dragPointerTarget.classList.remove('is-dragging');
  }
  dragPointerTarget = null;
  dragState = null;
  panState = null;
  releaseInteractionListeners?.();
};

const bindInteractionListeners = () => {
  if (releaseInteractionListeners || typeof document === 'undefined') {
    return;
  }
  const move = (event: PointerEvent) => {
    handleGlobalPointerMove(event);
  };
  const up = (event: PointerEvent) => {
    handleGlobalPointerUp(event);
  };
  document.addEventListener('pointermove', move, true);
  document.addEventListener('pointerup', up, true);
  document.addEventListener('pointercancel', up, true);
  releaseInteractionListeners = () => {
    document.removeEventListener('pointermove', move, true);
    document.removeEventListener('pointerup', up, true);
    document.removeEventListener('pointercancel', up, true);
    releaseInteractionListeners = null;
  };
};

const handleGlobalPointerMove = (event: PointerEvent) => {
  if (dragState && event.pointerId === dragState.pointerId) {
    const deltaX = (event.clientX - dragState.startX) / clampSwarmScale(viewportState.value.scale);
    const deltaY = (event.clientY - dragState.startY) / clampSwarmScale(viewportState.value.scale);
    if (Math.abs(deltaX) > 1 || Math.abs(deltaY) > 1) {
      dragState.moved = true;
    }
    nodePositionOverrides.value = {
      ...nodePositionOverrides.value,
      [dragState.nodeId]: {
        x: Math.round(dragState.originX + deltaX),
        y: Math.round(dragState.originY + deltaY)
      }
    };
    return;
  }
  if (panState && event.pointerId === panState.pointerId) {
    const deltaX = event.clientX - panState.startX;
    const deltaY = event.clientY - panState.startY;
    if (Math.abs(deltaX) > 1 || Math.abs(deltaY) > 1) {
      panState.moved = true;
    }
    viewportState.value = {
      ...viewportState.value,
      offsetX: Math.round(panState.originOffsetX + deltaX),
      offsetY: Math.round(panState.originOffsetY + deltaY)
    };
  }
};

const handleGlobalPointerUp = (event: PointerEvent) => {
  if (dragState && event.pointerId === dragState.pointerId) {
    clearInteractions();
    return;
  }
  if (panState && event.pointerId === panState.pointerId) {
    clearInteractions();
  }
};

const handleMinimapClick = (event: MouseEvent) => {
  const target = event.currentTarget as HTMLElement | null;
  const rect = target?.getBoundingClientRect();
  if (!rect) return;
  const scale = minimapScale.value;
  const x = minimapWorldWindow.value.left + (event.clientX - rect.left - minimapOffset.value.x) / scale;
  const y = minimapWorldWindow.value.top + (event.clientY - rect.top - minimapOffset.value.y) / scale;
  viewportState.value = {
    ...viewportState.value,
    offsetX: Math.round(containerSize.value.width / 2 - x * viewportState.value.scale),
    offsetY: Math.round(containerSize.value.height / 2 - y * viewportState.value.scale)
  };
  scheduleViewportStateSave();
};

watch(
  scopeKey,
  () => {
    revealCleanupTimers.forEach((timer) => {
      if (typeof window !== 'undefined') {
        window.clearTimeout(timer);
      }
    });
    revealCleanupTimers.clear();
    knownProjectionNodeIds.clear();
    knownProjectionNodeDispatchActivity.clear();
    nodeRevealMap.value = {};
    revealBaselineReady.value = false;
    hydrateCanvasState();
  },
  { immediate: true }
);

watch(
  () => props.dispatchPreview?.sessionId || '',
  (sessionId, previousSessionId) => {
    if (sessionId && sessionId !== previousSessionId) {
      freezeCurrentProjectionAsRevealBaseline();
    }
  }
);

watch(
  projectionSignature,
  async () => {
    logCanvasProjection('projection-change', buildCanvasProjectionDebugSnapshot());
    syncNodeRevealState();
    if (!projection.value.nodes.length) {
      selectedNodeId.value = '';
      return;
    }
    if (!selectedNodeId.value || !projection.value.nodeMetaMap.has(selectedNodeId.value)) {
      selectedNodeId.value = projection.value.motherNodeId || projection.value.nodes[0]?.id || '';
      saveNodeState();
    }
    await nextTick();
    if (applyRestoredViewport()) {
      return;
    }
    if (pendingFitView.value) {
      pendingFitView.value = false;
      await fitView(true);
    }
  },
  { immediate: true }
);

watch(
  () => [containerSize.value.width, containerSize.value.height] as const,
  async ([width, height]) => {
    if (width <= 0 || height <= 0 || !hasNodes.value) return;
    if (pendingViewportRestore.value && applyRestoredViewport()) {
      return;
    }
    if (pendingFitView.value) {
      pendingFitView.value = false;
      await fitView(true);
    }
  },
  { immediate: true }
);

onMounted(() => {
  updateContainerSize();
  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      updateContainerSize();
    });
    if (viewportRef.value) {
      resizeObserver.observe(viewportRef.value);
    }
  }
  window.addEventListener('resize', handleWindowResize);
});

onBeforeUnmount(() => {
  clearPendingNodeOutputPreview();
  clearViewportSaveTimer();
  resizeObserver?.disconnect();
  resizeObserver = null;
  window.removeEventListener('resize', handleWindowResize);
  releaseInteractionListeners?.();
  clearInteractions();
  revealCleanupTimers.forEach((timer) => {
    window.clearTimeout(timer);
  });
  revealCleanupTimers.clear();
  knownProjectionNodeIds.clear();
  knownProjectionNodeDispatchActivity.clear();
  nodeRevealMap.value = {};
  revealBaselineReady.value = false;
});
</script>

<style scoped>
.beeroom-canvas-graph-shell {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.beeroom-canvas-graph-shell::before {
  content: '';
  position: absolute;
  inset: 0;
  border: 1px solid rgba(148, 163, 184, 0.08);
  pointer-events: none;
}

.beeroom-canvas-graph-shell::after {
  display: none;
}

.beeroom-canvas-surface {
  position: relative;
  z-index: 1;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  touch-action: none;
  background: linear-gradient(180deg, rgba(8, 11, 17, 0.98), rgba(7, 10, 15, 0.98));
  cursor: grab;
}

.beeroom-canvas-surface:active {
  cursor: grabbing;
}

.beeroom-swarm-world {
  position: absolute;
  left: 0;
  top: 0;
  transform-origin: 0 0;
  will-change: transform;
}

.beeroom-canvas-grid {
  position: absolute;
  inset: 0;
  z-index: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.beeroom-canvas-grid-path {
  fill: none;
  stroke-linecap: round;
  stroke-linejoin: round;
  vector-effect: non-scaling-stroke;
}

.beeroom-canvas-grid-path.is-major {
  stroke: rgba(253, 224, 71, 0.16);
}

.beeroom-swarm-edge-layer {
  position: absolute;
  inset: 0;
  z-index: 1;
  width: 100%;
  height: 100%;
  overflow: visible;
  pointer-events: none;
}

.beeroom-swarm-node-layer {
  position: absolute;
  inset: 0;
  z-index: 2;
}

.beeroom-swarm-edge {
  fill: none;
  stroke: rgba(148, 163, 184, 0.42);
  stroke-width: 1.25;
  stroke-dasharray: 5 9;
  stroke-linecap: round;
  stroke-linejoin: round;
  transition:
    stroke 180ms ease,
    stroke-width 180ms ease,
    opacity 180ms ease;
}

.beeroom-swarm-edge-group {
  isolation: isolate;
}

.beeroom-swarm-edge-activity {
  fill: none;
  stroke: rgba(248, 113, 113, 0.22);
  stroke-width: 5.6;
  stroke-linecap: round;
  stroke-linejoin: round;
  opacity: 0.82;
  animation: beeroom-edge-breathe 1.45s ease-in-out infinite;
}

.beeroom-swarm-edge-activity.is-selected {
  stroke: rgba(248, 113, 113, 0.3);
}

.beeroom-swarm-edge-activity.is-subagent {
  stroke: rgba(34, 211, 238, 0.2);
}

.beeroom-swarm-edge.is-selected {
  stroke: rgba(96, 165, 250, 0.84);
}

.beeroom-swarm-edge.is-active {
  stroke: rgba(248, 113, 113, 0.94);
  stroke-width: 1.52;
  stroke-dasharray: 10 8;
  animation: beeroom-edge-flow 1.8s linear infinite;
}

.beeroom-swarm-edge.is-subagent {
  stroke: rgba(103, 132, 161, 0.42);
  stroke-dasharray: 4 7;
}

.beeroom-swarm-edge.is-subagent.is-active {
  stroke: rgba(34, 211, 238, 0.9);
  stroke-width: 1.46;
  stroke-dasharray: 8 7;
}

.beeroom-swarm-edge.is-revealing,
.beeroom-swarm-edge-activity.is-revealing {
  animation: beeroom-edge-reveal 620ms cubic-bezier(0.22, 1, 0.36, 1) both;
}

.beeroom-swarm-edge-activity.is-subagent.is-revealing {
  animation: beeroom-edge-reveal 620ms cubic-bezier(0.22, 1, 0.36, 1) both;
}

.beeroom-swarm-edge-orb {
  fill: rgba(254, 202, 202, 0.96);
  stroke: rgba(255, 241, 242, 0.9);
  stroke-width: 1.1;
  opacity: 0.96;
  animation: beeroom-edge-orb-pulse 1.15s ease-in-out infinite;
}

.beeroom-swarm-edge-orb.is-trailing {
  fill: rgba(248, 113, 113, 0.7);
  stroke: rgba(254, 226, 226, 0.58);
  opacity: 0.7;
}

.beeroom-swarm-edge-orb.is-subagent {
  fill: rgba(165, 243, 252, 0.96);
  stroke: rgba(224, 242, 254, 0.9);
}

.beeroom-swarm-edge-orb.is-trailing.is-subagent {
  fill: rgba(34, 211, 238, 0.72);
  stroke: rgba(165, 243, 252, 0.52);
}

.beeroom-swarm-edge-label {
  fill: rgba(226, 232, 240, 0.8);
  font-size: 11px;
  font-weight: 600;
  text-anchor: middle;
  paint-order: stroke;
  stroke: rgba(8, 11, 17, 0.92);
  stroke-width: 4px;
  stroke-linejoin: round;
}

.beeroom-swarm-edge-label.is-active {
  fill: rgba(254, 202, 202, 0.96);
  animation: beeroom-edge-label-breathe 1.3s ease-in-out infinite;
}

.beeroom-swarm-edge-label.is-subagent {
  fill: rgba(186, 230, 253, 0.88);
}

.beeroom-canvas-tools,
.beeroom-canvas-minimap-shell {
  position: absolute;
  z-index: 5;
}

.beeroom-canvas-tools {
  left: 14px;
  top: 12px;
  display: inline-flex;
  flex-direction: column;
  align-items: center;
  gap: 5px;
  padding: 5px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(10, 13, 19, 0.88);
  opacity: 0.94;
  box-shadow: 0 8px 18px rgba(0, 0, 0, 0.14);
}

.beeroom-canvas-tool-btn {
  width: 30px;
  height: 30px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(148, 163, 184, 0.2);
  border-radius: 8px;
  background: rgba(30, 41, 59, 0.28);
  color: #e2e8f0;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition:
    border-color 140ms cubic-bezier(0.22, 1, 0.36, 1),
    background 140ms cubic-bezier(0.22, 1, 0.36, 1),
    color 140ms cubic-bezier(0.22, 1, 0.36, 1),
    transform 140ms cubic-bezier(0.22, 1, 0.36, 1);
}

.beeroom-canvas-tool-btn:hover,
.beeroom-canvas-tool-btn:focus-visible,
.beeroom-canvas-tool-btn.is-active {
  border-color: rgba(96, 165, 250, 0.48);
  background: rgba(30, 64, 175, 0.32);
  color: #dbeafe;
  transform: translateY(-1px);
  outline: none;
}

.beeroom-canvas-tool-btn:disabled {
  opacity: 0.46;
  cursor: not-allowed;
  transform: none;
}

.beeroom-canvas-minimap-shell {
  left: 12px;
  bottom: 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  pointer-events: none;
}

.beeroom-canvas-minimap-label {
  align-self: flex-start;
  padding: 2px 6px;
  border-radius: 999px;
  background: rgba(10, 13, 19, 0.94);
  border: 1px solid rgba(148, 163, 184, 0.24);
  color: rgba(209, 213, 219, 0.86);
  font-size: 9px;
  letter-spacing: 0.04em;
}

.beeroom-canvas-minimap {
  width: 132px;
  height: 80px;
  padding: 0;
  overflow: hidden;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(10, 13, 19, 0.94);
  box-shadow: 0 10px 22px rgba(0, 0, 0, 0.18);
  cursor: pointer;
  pointer-events: auto;
}

.beeroom-canvas-minimap-svg {
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.beeroom-canvas-minimap-edge {
  fill: none;
  stroke: rgba(148, 163, 184, 0.42);
  stroke-width: 0.8;
}

.beeroom-canvas-minimap-edge.is-subagent {
  stroke: rgba(103, 132, 161, 0.46);
}

.beeroom-canvas-minimap-node {
  fill: rgba(148, 163, 184, 0.92);
}

.beeroom-canvas-minimap-node.is-subagent {
  fill: rgba(100, 116, 139, 0.84);
}

.beeroom-canvas-minimap-node.is-subagent.is-emphasis-active {
  fill: rgba(34, 211, 238, 0.9);
}

.beeroom-canvas-minimap-node.is-subagent.is-emphasis-dormant {
  fill: rgba(100, 116, 139, 0.62);
}

.beeroom-canvas-minimap-node.is-running,
.beeroom-canvas-minimap-node.is-queued,
.beeroom-canvas-minimap-node.is-awaiting_idle {
  fill: rgba(239, 68, 68, 0.9);
}

.beeroom-canvas-minimap-node.is-failed,
.beeroom-canvas-minimap-node.is-error,
.beeroom-canvas-minimap-node.is-timeout,
.beeroom-canvas-minimap-node.is-cancelled {
  fill: rgba(248, 113, 113, 0.92);
}

.beeroom-canvas-minimap-node.is-completed,
.beeroom-canvas-minimap-node.is-success {
  fill: rgba(59, 130, 246, 0.9);
}

.beeroom-canvas-minimap-viewport {
  fill: rgba(16, 185, 129, 0.12);
  stroke: rgba(52, 211, 153, 0.78);
  stroke-width: 1.2;
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

@keyframes beeroom-edge-flow {
  from {
    stroke-dashoffset: 0;
  }

  to {
    stroke-dashoffset: -36;
  }
}

@keyframes beeroom-edge-breathe {
  0%,
  100% {
    opacity: 0.42;
  }

  50% {
    opacity: 0.88;
  }
}

@keyframes beeroom-edge-orb-pulse {
  0%,
  100% {
    opacity: 0.62;
  }

  50% {
    opacity: 1;
  }
}

@keyframes beeroom-edge-label-breathe {
  0%,
  100% {
    opacity: 0.82;
  }

  50% {
    opacity: 1;
  }
}

@keyframes beeroom-edge-reveal {
  0% {
    opacity: 0;
    stroke-dashoffset: -28;
  }

  100% {
    opacity: 1;
    stroke-dashoffset: 0;
  }
}

@media (max-width: 900px) {
  .beeroom-canvas-tools {
    left: 12px;
  }

  .beeroom-canvas-minimap-shell {
    left: auto;
    right: 12px;
    bottom: 12px;
  }
}
</style>
