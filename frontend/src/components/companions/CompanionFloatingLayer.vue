<template>
  <Teleport to="body">
    <div class="companion-floating-host">
      <div
        v-for="entry in visibleEntries"
        :key="entry.key"
        class="companion-floating-layer"
        :class="{ 'is-dragging': draggingKey === entry.key }"
        :style="entry.style"
        role="button"
        tabindex="0"
        :aria-label="entry.name"
        @pointerdown="handlePointerDown($event, entry)"
        @click="handleClick(entry)"
        @keydown.enter.prevent="handleClick(entry)"
        @keydown.space.prevent="handleClick(entry)"
      >
        <div
          v-if="entry.messageVisible"
          class="companion-floating-layer__bubble"
          :class="`is-${entry.messageKind}`"
        >
          {{ entry.message }}
        </div>
        <CompanionSprite
          :source="entry.companion.spritesheetDataUrl"
          :state="spriteStateByKey[entry.key] || 'idle'"
          :scale="entry.scale"
        />
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';

import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import { useAgentStore } from '@/stores/agents';
import {
  useCompanionStore,
  type CompanionPackageRecord,
  type CompanionPosition,
  type CompanionSpriteStateId
} from '@/stores/companions';
import { parseAgentAvatarIconConfig, type AgentAvatarIconConfig } from '@/utils/agentAvatar';

type FloatingEntry = {
  key: string;
  agentId: string;
  name: string;
  config: AgentAvatarIconConfig;
  companion: CompanionPackageRecord;
  scale: number;
  message: string;
  messageKind: 'info' | 'success' | 'warning';
  messageVisible: boolean;
  style: {
    left: string;
    top: string;
  };
};

type DesktopCompanionBridge = {
  showCompanion?: (payload: Record<string, unknown>) => Promise<boolean> | boolean;
  updateCompanion?: (payload: Record<string, unknown>) => Promise<boolean> | boolean;
  hideCompanion?: (payload?: Record<string, unknown>) => Promise<boolean> | boolean;
};

const BASE_WIDTH = 192;
const BASE_HEIGHT = 208;
const SCREEN_MARGIN = 8;
const POSITION_STORAGE_KEY = 'wunder_agent_companion_positions';

const props = withDefaults(
  defineProps<{
    desktopMode?: boolean;
  }>(),
  {
    desktopMode: false
  }
);

const agentStore = useAgentStore();
const companionStore = useCompanionStore();
const now = ref(Date.now());
const draggingKey = ref('');
const spriteStateByKey = reactive<Record<string, CompanionSpriteStateId>>({});
const positions = ref<Record<string, CompanionPosition>>(loadPositions());
let nowTimer: number | null = null;
let clickSuppressUntil = 0;
let desktopOverlayActive = false;
let pointerState:
  | {
      pointerId: number;
      key: string;
      startClientX: number;
      startClientY: number;
      startX: number;
      startY: number;
      scale: number;
    }
  | null = null;

const allAgents = computed(() => {
  const map = new Map<string, Record<string, unknown>>();
  [...agentStore.agents, ...agentStore.sharedAgents].forEach((agent) => {
    const id = String(agent?.id || '').trim();
    if (id) map.set(id, agent);
  });
  Object.entries(agentStore.agentMap || {}).forEach(([id, agent]) => {
    if (agent && id) map.set(id, agent);
  });
  return Array.from(map.values());
});

const currentMessage = computed(() => {
  const message = companionStore.message;
  if (!message || message.visibleUntil <= now.value) {
    return null;
  }
  return message;
});

const visibleEntries = computed<FloatingEntry[]>(() =>
  allAgents.value
    .map((agent, index) => {
      const config = parseAgentAvatarIconConfig(agent.icon);
      if (config.kind !== 'companion' || config.show === false) {
        return null;
      }
      const companion = companionStore.findCompanion(config.scope || 'global', config.id || config.name);
      if (!companion) {
        return null;
      }
      const agentId = String(agent.id || config.id || index).trim();
      const key = `${agentId}:${config.scope || 'global'}:${config.id || config.name}`;
      const scale = Number(config.scale || 1);
      const position = positions.value[key] || defaultPosition(index);
      const message = currentMessage.value;
      return {
        key,
        agentId,
        name: String(agent.name || companion.displayName || agentId).trim(),
        config,
        companion,
        scale,
        message: message?.text || '',
        messageKind: message?.kind || 'info',
        messageVisible: Boolean(message?.text),
        style: {
          left: `${position.x}px`,
          top: `${position.y}px`
        }
      };
    })
    .filter((item): item is FloatingEntry => Boolean(item))
);

function loadPositions(): Record<string, CompanionPosition> {
  try {
    const parsed = JSON.parse(localStorage.getItem(POSITION_STORAGE_KEY) || '{}');
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      return {};
    }
    const output: Record<string, CompanionPosition> = {};
    Object.entries(parsed as Record<string, unknown>).forEach(([key, value]) => {
      const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
      const x = Math.max(0, Math.round(Number(source.x) || 0));
      const y = Math.max(0, Math.round(Number(source.y) || 0));
      if (key) output[key] = { x, y };
    });
    return output;
  } catch {
    return {};
  }
}

function savePositions(): void {
  try {
    localStorage.setItem(POSITION_STORAGE_KEY, JSON.stringify(positions.value));
  } catch {
    // Ignore storage failures; positions remain valid for the current session.
  }
}

function defaultPosition(index: number): CompanionPosition {
  return {
    x: 28 + (index % 4) * 46,
    y: 28 + Math.floor(index / 4) * 34
  };
}

function getDesktopBridge(): DesktopCompanionBridge | null {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopCompanionBridge }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
}

function clampPosition(x: number, y: number, scale: number): CompanionPosition {
  if (typeof window === 'undefined') {
    return { x: Math.max(0, Math.round(x)), y: Math.max(0, Math.round(y)) };
  }
  const width = BASE_WIDTH * scale;
  const height = BASE_HEIGHT * scale;
  return {
    x: Math.min(Math.max(SCREEN_MARGIN, Math.round(x)), Math.max(SCREEN_MARGIN, window.innerWidth - width - SCREEN_MARGIN)),
    y: Math.min(Math.max(SCREEN_MARGIN, Math.round(y)), Math.max(SCREEN_MARGIN, window.innerHeight - height - SCREEN_MARGIN))
  };
}

function setSpriteState(key: string, state: CompanionSpriteStateId, durationMs = 0): void {
  spriteStateByKey[key] = state;
  if (durationMs > 0 && typeof window !== 'undefined') {
    window.setTimeout(() => {
      spriteStateByKey[key] = 'idle';
    }, durationMs);
  }
}

function setPosition(key: string, position: CompanionPosition): void {
  positions.value = {
    ...positions.value,
    [key]: position
  };
  savePositions();
}

async function syncDesktopOverlay(): Promise<void> {
  const entry = visibleEntries.value[0] || null;
  const bridge = getDesktopBridge();
  if (!props.desktopMode || !entry || !bridge) {
    if (desktopOverlayActive && typeof bridge?.hideCompanion === 'function') {
      await Promise.resolve(bridge.hideCompanion({ persistEnabled: false }));
    }
    desktopOverlayActive = false;
    return;
  }
  const handler = desktopOverlayActive && typeof bridge.updateCompanion === 'function'
    ? bridge.updateCompanion
    : bridge.showCompanion;
  if (typeof handler !== 'function') {
    desktopOverlayActive = false;
    return;
  }
  const position = positions.value[entry.key] || defaultPosition(0);
  desktopOverlayActive = (await Promise.resolve(handler.call(bridge, {
    id: entry.key,
    selectedId: entry.key,
    displayName: entry.name,
    description: entry.companion.description,
    spritesheetDataUrl: entry.companion.spritesheetDataUrl,
    state: spriteStateByKey[entry.key] || 'idle',
    scale: entry.scale,
    x: position.x,
    y: position.y,
    message: entry.message,
    messageKind: entry.messageKind,
    messageVisible: entry.messageVisible
  }))) === true;
}

function handleClick(entry: FloatingEntry): void {
  if (Date.now() < clickSuppressUntil) {
    return;
  }
  setSpriteState(entry.key, 'waving', 900);
  if (!entry.messageVisible) {
    companionStore.showMessage(entry.name || entry.companion.displayName, { durationMs: 1800 });
  }
}

function handlePointerDown(event: PointerEvent, entry: FloatingEntry): void {
  if (event.button !== 0) return;
  const target = event.currentTarget as HTMLElement | null;
  const position = positions.value[entry.key] || defaultPosition(0);
  pointerState = {
    pointerId: event.pointerId,
    key: entry.key,
    startClientX: event.clientX,
    startClientY: event.clientY,
    startX: position.x,
    startY: position.y,
    scale: entry.scale
  };
  target?.setPointerCapture(event.pointerId);
  window.addEventListener('pointermove', handlePointerMove);
  window.addEventListener('pointerup', stopDrag, { once: true });
  window.addEventListener('pointercancel', stopDrag, { once: true });
}

function handlePointerMove(event: PointerEvent): void {
  if (!pointerState) return;
  const deltaX = event.clientX - pointerState.startClientX;
  const deltaY = event.clientY - pointerState.startClientY;
  if (!draggingKey.value && Math.hypot(deltaX, deltaY) > 3) {
    draggingKey.value = pointerState.key;
    setSpriteState(pointerState.key, deltaX < 0 ? 'running-left' : 'running-right');
  }
  if (!draggingKey.value) return;
  setPosition(
    pointerState.key,
    clampPosition(pointerState.startX + deltaX, pointerState.startY + deltaY, pointerState.scale)
  );
}

function stopDrag(): void {
  if (!pointerState) return;
  window.removeEventListener('pointermove', handlePointerMove);
  if (draggingKey.value) {
    clickSuppressUntil = Date.now() + 250;
  }
  setSpriteState(pointerState.key, 'idle');
  pointerState = null;
  draggingKey.value = '';
}

function clampAfterResize(): void {
  const next = { ...positions.value };
  let changed = false;
  visibleEntries.value.forEach((entry) => {
    const current = next[entry.key] || defaultPosition(0);
    const clamped = clampPosition(current.x, current.y, entry.scale);
    if (current.x !== clamped.x || current.y !== clamped.y) {
      next[entry.key] = clamped;
      changed = true;
    }
  });
  if (changed) {
    positions.value = next;
    savePositions();
  }
}

onMounted(async () => {
  await companionStore.hydrate().catch(() => undefined);
  await companionStore.loadGlobalCompanions().catch(() => undefined);
  if (!agentStore.agents.length) {
    await agentStore.loadAgents().catch(() => undefined);
  }
  window.addEventListener('resize', clampAfterResize);
  nowTimer = window.setInterval(() => {
    now.value = Date.now();
    if (companionStore.message && companionStore.message.visibleUntil <= now.value) {
      companionStore.clearMessage();
    }
  }, 500);
});

onBeforeUnmount(() => {
  if (nowTimer !== null) window.clearInterval(nowTimer);
  window.removeEventListener('resize', clampAfterResize);
  window.removeEventListener('pointermove', handlePointerMove);
  void getDesktopBridge()?.hideCompanion?.({ persistEnabled: false });
});

watch(
  () => visibleEntries.value.map((entry) => [
    entry.key,
    entry.scale,
    entry.message,
    entry.messageVisible,
    spriteStateByKey[entry.key] || 'idle',
    positions.value[entry.key]?.x,
    positions.value[entry.key]?.y
  ]),
  () => {
    void syncDesktopOverlay();
  },
  { immediate: true, deep: true }
);
</script>

<style scoped>
.companion-floating-layer {
  position: fixed;
  z-index: 12000;
  display: flex;
  flex-direction: column;
  align-items: center;
  cursor: grab;
  user-select: none;
  touch-action: none;
}

.companion-floating-layer.is-dragging {
  cursor: grabbing;
}

.companion-floating-layer__bubble {
  max-width: 260px;
  margin-bottom: 4px;
  padding: 8px 10px;
  border: 1px solid rgba(37, 99, 235, 0.22);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.96);
  box-shadow: 0 10px 28px rgba(15, 23, 42, 0.16);
  color: #1f2937;
  font-size: 13px;
  line-height: 1.45;
  text-align: center;
  overflow-wrap: anywhere;
}

.companion-floating-layer__bubble.is-success {
  border-color: rgba(20, 184, 166, 0.28);
  color: #0f766e;
}

.companion-floating-layer__bubble.is-warning {
  border-color: rgba(245, 158, 11, 0.3);
  color: #92400e;
}
</style>
