<template>
  <div class="beeroom-canvas-graph-shell">
    <div
      ref="viewportRef"
      class="beeroom-canvas-surface"
      @wheel.prevent="handleViewportWheel"
      @pointerdown="handleViewportPointerDown"
    >
      <div class="beeroom-swarm-world" :style="worldStyle">
        <svg
          class="beeroom-swarm-grid-layer"
          :viewBox="`0 0 ${worldSize.width} ${worldSize.height}`"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <defs>
            <pattern :id="gridPatternId" width="32" height="32" patternUnits="userSpaceOnUse">
              <path d="M 32 0 L 0 0 0 32" fill="none" stroke="rgba(148, 163, 184, 0.1)" stroke-width="1" />
            </pattern>
          </defs>
          <rect :width="worldSize.width" :height="worldSize.height" :fill="`url(#${gridPatternId})`" />
        </svg>

        <svg
          class="beeroom-swarm-edge-layer"
          :viewBox="`0 0 ${worldSize.width} ${worldSize.height}`"
          preserveAspectRatio="none"
          aria-hidden="true"
        >
          <g v-for="edge in worldEdges" :key="edge.id" class="beeroom-swarm-edge-group">
            <path
              class="beeroom-swarm-edge"
              :class="{ 'is-active': edge.active, 'is-selected': edge.selected }"
              :d="edge.path"
            />
            <text
              v-if="edge.label"
              class="beeroom-swarm-edge-label"
              :class="{ 'is-active': edge.active, 'is-selected': edge.selected }"
              :x="edge.labelX"
              :y="edge.labelY"
            >
              {{ edge.label }}
            </text>
          </g>
        </svg>

        <div class="beeroom-swarm-node-layer">
          <BeeroomSwarmNodeCard
            v-for="node in worldNodes"
            :key="node.id"
            :node="node"
            :condensed="condensedNodeCards"
            :empty-label="workflowEmptyLabel"
            :style="{ left: `${node.left}px`, top: `${node.top}px` }"
            @pointerdown="handleNodePointerDown(node.id, $event)"
            @click="handleNodeClick(node.id)"
            @dblclick="emit('open-agent', node.agentId)"
          />
        </div>
      </div>
    </div>

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
        :title="canvasControlLabels.resetZoom"
        :aria-label="canvasControlLabels.resetZoom"
        @click="resetZoom"
      >
        <i class="fa-solid fa-arrows-rotate" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.resetZoom }}</span>
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
        :title="canvasControlLabels.autoArrange"
        :aria-label="canvasControlLabels.autoArrange"
        @click="autoArrangeCanvas"
      >
        <i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i>
        <span class="beeroom-visually-hidden">{{ canvasControlLabels.autoArrange }}</span>
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

    <div class="beeroom-canvas-minimap-shell">
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
            :d="edge.path"
          />
          <rect
            v-for="node in minimapNodes"
            :key="node.id"
            class="beeroom-canvas-minimap-node"
            :class="`is-${node.status}`"
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
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState,
  type BeeroomCanvasPositionOverride,
  type BeeroomCanvasViewportState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type { BeeroomWorkflowItem, BeeroomTaskWorkflowPreview } from '@/components/beeroom/beeroomTaskWorkflow';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';

import BeeroomSwarmNodeCard from './BeeroomSwarmNodeCard.vue';
import {
  NODE_HEIGHT,
  NODE_WIDTH,
  WORLD_PADDING,
  buildBeeroomSwarmProjection,
  hasBeeroomSwarmNodes,
  resolveBeeroomSwarmScopeKey
} from './swarmCanvasModel';
import {
  SWARM_SCALE_STEP,
  clampSwarmScale,
  createDefaultSwarmViewportState,
  fitSwarmViewportToBounds,
  normalizeSwarmViewportSize,
  zoomSwarmViewportAroundPoint,
  type SwarmViewportState
} from './useBeeroomSwarmViewport';

const MINIMAP_WIDTH = 132;
const MINIMAP_HEIGHT = 80;

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  fullscreen?: boolean;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
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
const canvasControlLabels = {
  toolbar: 'Canvas Controls',
  zoomIn: 'Zoom In',
  zoomOut: 'Zoom Out',
  resetZoom: 'Reset Zoom 100%',
  fitView: 'Fit View',
  autoArrange: 'Auto Arrange',
  enterFullscreen: 'Enter Fullscreen',
  exitFullscreen: 'Exit Fullscreen'
} as const;
const viewportRef = ref<HTMLDivElement | null>(null);
const containerSize = ref(normalizeSwarmViewportSize({ width: 0, height: 0 }));
const selectedNodeId = ref('');
const nodePositionOverrides = ref<Record<string, BeeroomCanvasPositionOverride>>({});
const viewportState = ref<SwarmViewportState>(createDefaultSwarmViewportState());
const pendingViewportRestore = ref<BeeroomCanvasViewportState | null>(null);
const pendingFitView = ref(false);

let resizeObserver: ResizeObserver | null = null;
let viewportSaveTimer: number | null = null;
let dragState: DragState | null = null;
let panState: PanState | null = null;
let suppressSelection = false;
const gridPatternId = `beeroom-grid-${Math.random().toString(36).slice(2, 8)}`;

const scopeKey = computed(() =>
  resolveBeeroomSwarmScopeKey({
    missionId: props.mission?.mission_id,
    teamRunId: props.mission?.team_run_id,
    groupId: props.group?.group_id
  })
);

const projection = computed(() =>
  buildBeeroomSwarmProjection({
    group: props.group,
    mission: props.mission,
    agents: props.agents,
    selectedNodeId: selectedNodeId.value,
    nodePositionOverrides: nodePositionOverrides.value,
    workflowItemsByTask: props.workflowItemsByTask,
    workflowPreviewByTask: props.workflowPreviewByTask,
    t
  })
);

const hasNodes = computed(() =>
  hasBeeroomSwarmNodes({
    group: props.group,
    mission: props.mission,
    agents: props.agents
  })
);

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

const worldMetrics = computed(() => {
  const bounds = projection.value.bounds;
  const width = Math.max(NODE_WIDTH + WORLD_PADDING * 2, Math.ceil(bounds.width + WORLD_PADDING * 2));
  const height = Math.max(NODE_HEIGHT + WORLD_PADDING * 2, Math.ceil(bounds.height + WORLD_PADDING * 2));
  return {
    width,
    height,
    originX: Math.round(WORLD_PADDING - bounds.minX),
    originY: Math.round(WORLD_PADDING - bounds.minY)
  };
});

const worldSize = computed(() => ({
  width: worldMetrics.value.width,
  height: worldMetrics.value.height
}));

const worldNodes = computed(() =>
  projection.value.nodes.map((node) => {
    const centerX = Math.round(node.x + worldMetrics.value.originX);
    const centerY = Math.round(node.y + worldMetrics.value.originY);
    return {
      ...node,
      centerX,
      centerY,
      left: Math.round(centerX - node.width / 2),
      top: Math.round(centerY - node.height / 2)
    };
  })
);

const worldNodeMap = computed(() => {
  const map = new Map<string, (typeof worldNodes.value)[number]>();
  worldNodes.value.forEach((node) => {
    map.set(node.id, node);
  });
  return map;
});

const worldEdges = computed(() =>
  projection.value.edges
    .map((edge) => {
      const source = worldNodeMap.value.get(edge.source);
      const target = worldNodeMap.value.get(edge.target);
      if (!source || !target) return null;
      const dx = target.centerX - source.centerX;
      const controlOffset = Math.max(42, Math.abs(dx) * 0.22);
      const path = `M ${source.centerX} ${source.centerY} C ${source.centerX + controlOffset} ${source.centerY} ${target.centerX - controlOffset} ${target.centerY} ${target.centerX} ${target.centerY}`;
      return {
        ...edge,
        path,
        labelX: Math.round(source.centerX + dx / 2),
        labelY: Math.round(source.centerY + (target.centerY - source.centerY) / 2 - 10)
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

const worldStyle = computed(() => ({
  width: `${worldSize.value.width}px`,
  height: `${worldSize.value.height}px`,
  transform: `translate(${Math.round(viewportState.value.offsetX)}px, ${Math.round(viewportState.value.offsetY)}px) scale(${viewportState.value.scale})`
}));

const workflowEmptyLabel = computed(() => t('chat.toolWorkflow.empty'));

const minimapScale = computed(() =>
  Math.min(
    (MINIMAP_WIDTH - 10) / Math.max(1, worldSize.value.width),
    (MINIMAP_HEIGHT - 10) / Math.max(1, worldSize.value.height)
  )
);

const minimapOffset = computed(() => ({
  x: (MINIMAP_WIDTH - worldSize.value.width * minimapScale.value) / 2,
  y: (MINIMAP_HEIGHT - worldSize.value.height * minimapScale.value) / 2
}));

const minimapNodes = computed(() =>
  worldNodes.value.map((node) => ({
    id: node.id,
    status: node.status,
    x: minimapOffset.value.x + node.left * minimapScale.value,
    y: minimapOffset.value.y + node.top * minimapScale.value,
    width: Math.max(4, node.width * minimapScale.value),
    height: Math.max(4, node.height * minimapScale.value)
  }))
);

const minimapEdges = computed(() =>
  projection.value.edges
    .map((edge) => {
      const source = worldNodeMap.value.get(edge.source);
      const target = worldNodeMap.value.get(edge.target);
      if (!source || !target) return null;
      const scale = minimapScale.value;
      const sx = minimapOffset.value.x + source.centerX * scale;
      const sy = minimapOffset.value.y + source.centerY * scale;
      const tx = minimapOffset.value.x + target.centerX * scale;
      const ty = minimapOffset.value.y + target.centerY * scale;
      const dx = tx - sx;
      const controlOffset = Math.max(6, Math.abs(dx) * 0.22);
      return {
        id: edge.id,
        path: `M ${sx} ${sy} C ${sx + controlOffset} ${sy} ${tx - controlOffset} ${ty} ${tx} ${ty}`
      };
    })
    .filter((edge): edge is NonNullable<typeof edge> => Boolean(edge))
);

const minimapViewportRect = computed(() => {
  const scale = clampSwarmScale(viewportState.value.scale);
  const worldX = Math.max(0, -viewportState.value.offsetX / scale);
  const worldY = Math.max(0, -viewportState.value.offsetY / scale);
  const width = Math.min(worldSize.value.width, containerSize.value.width / scale);
  const height = Math.min(worldSize.value.height, containerSize.value.height / scale);
  return {
    x: minimapOffset.value.x + worldX * minimapScale.value,
    y: minimapOffset.value.y + worldY * minimapScale.value,
    width: Math.max(10, width * minimapScale.value),
    height: Math.max(10, height * minimapScale.value)
  };
});

const projectionSignature = computed(() =>
  [
    scopeKey.value,
    projection.value.nodes.map((node) => `${node.id}:${node.x}:${node.y}:${node.selected}:${node.workflowTaskId}:${node.status}`).join('|'),
    projection.value.edges.map((edge) => `${edge.id}:${edge.active}:${edge.selected}:${edge.label}`).join('|')
  ].join('||')
);

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

const fitView = async (force = false) => {
  await nextTick();
  if (!viewportRef.value || !projection.value.nodes.length) return;
  viewportState.value = fitSwarmViewportToBounds({
    bounds: projection.value.bounds,
    worldWidth: worldSize.value.width,
    worldHeight: worldSize.value.height,
    viewport: containerSize.value,
    padding: force ? 36 : 44
  });
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
const resetZoom = () => {
  viewportState.value = {
    ...viewportState.value,
    scale: 1
  };
  scheduleViewportStateSave();
};

const autoArrangeCanvas = async () => {
  nodePositionOverrides.value = {};
  saveNodeState();
  pendingFitView.value = true;
  await fitView(true);
};

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

const handleNodeClick = (nodeId: string) => {
  if (suppressSelection) {
    suppressSelection = false;
    return;
  }
  selectedNodeId.value = nodeId;
  saveNodeState();
};

const handleNodePointerDown = (nodeId: string, event: PointerEvent) => {
  if (event.button !== 0) return;
  event.preventDefault();
  const node = projection.value.nodes.find((item) => item.id === nodeId);
  if (!node) return;
  dragState = {
    nodeId,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    originX: node.x,
    originY: node.y,
    moved: false
  };
};

const handleViewportPointerDown = (event: PointerEvent) => {
  if (event.button !== 0) return;
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
  if (dragState?.moved) {
    suppressSelection = true;
    saveNodeState();
  }
  if (panState?.moved) {
    scheduleViewportStateSave(60);
  }
  dragState = null;
  panState = null;
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
  if ((dragState && event.pointerId === dragState.pointerId) || (panState && event.pointerId === panState.pointerId)) {
    clearInteractions();
  }
};

const handleMinimapClick = (event: MouseEvent) => {
  const target = event.currentTarget as HTMLElement | null;
  const rect = target?.getBoundingClientRect();
  if (!rect) return;
  const scale = minimapScale.value;
  const x = (event.clientX - rect.left - minimapOffset.value.x) / scale;
  const y = (event.clientY - rect.top - minimapOffset.value.y) / scale;
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
    hydrateCanvasState();
  },
  { immediate: true }
);

watch(
  projectionSignature,
  async () => {
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
  window.addEventListener('pointermove', handleGlobalPointerMove);
  window.addEventListener('pointerup', handleGlobalPointerUp);
  window.addEventListener('pointercancel', handleGlobalPointerUp);
});

onBeforeUnmount(() => {
  clearViewportSaveTimer();
  resizeObserver?.disconnect();
  resizeObserver = null;
  window.removeEventListener('resize', handleWindowResize);
  window.removeEventListener('pointermove', handleGlobalPointerMove);
  window.removeEventListener('pointerup', handleGlobalPointerUp);
  window.removeEventListener('pointercancel', handleGlobalPointerUp);
  clearInteractions();
});
</script>

<style scoped>
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

.beeroom-canvas-surface {
  position: relative;
  z-index: 1;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  touch-action: none;
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

.beeroom-swarm-grid-layer,
.beeroom-swarm-edge-layer {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  overflow: visible;
}

.beeroom-swarm-node-layer {
  position: absolute;
  inset: 0;
}

.beeroom-swarm-edge {
  fill: none;
  stroke: rgba(148, 163, 184, 0.42);
  stroke-width: 1.25;
  stroke-dasharray: 5 9;
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
}

.beeroom-canvas-legend,
.beeroom-canvas-tools,
.beeroom-canvas-minimap-shell {
  position: absolute;
  z-index: 5;
}

.beeroom-canvas-legend {
  top: 12px;
  right: 14px;
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

.beeroom-canvas-tools {
  left: 14px;
  top: 12px;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  background: rgba(12, 13, 18, 0.72);
  opacity: 0.78;
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.03),
    0 8px 16px rgba(0, 0, 0, 0.16);
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
  background: linear-gradient(180deg, rgba(22, 24, 31, 0.94), rgba(15, 17, 23, 0.9));
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
  background: linear-gradient(180deg, rgba(15, 17, 23, 0.94), rgba(10, 11, 16, 0.9));
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.03),
    0 10px 26px rgba(0, 0, 0, 0.28);
  cursor: pointer;
  pointer-events: auto;
}

.beeroom-canvas-minimap-svg {
  width: 100%;
  height: 100%;
}

.beeroom-canvas-minimap-edge {
  fill: none;
  stroke: rgba(148, 163, 184, 0.42);
  stroke-width: 0.8;
}

.beeroom-canvas-minimap-node {
  fill: rgba(148, 163, 184, 0.92);
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
  fill: rgba(148, 163, 184, 0.1);
  stroke: rgba(203, 213, 225, 0.62);
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

@media (max-width: 900px) {
  .beeroom-canvas-legend {
    left: 108px;
    right: 12px;
    flex-wrap: wrap;
  }

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
