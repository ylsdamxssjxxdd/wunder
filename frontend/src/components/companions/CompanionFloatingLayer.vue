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
        @contextmenu.prevent="openEntryMenu($event, entry)"
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
          :state="resolveEntrySpriteState(entry)"
          :scale="entry.scale"
        />
      </div>
    </div>
    <div
      v-if="menuState"
      ref="menuRef"
      class="companion-floating-menu"
      :style="menuStyle"
      @mousedown.stop
      @contextmenu.prevent
    >
      <button class="companion-floating-menu__item" type="button" @click="openCompanionChat(menuState.entry)">
        {{ t('messenger.action.openConversation') }}
      </button>
      <button
        class="companion-floating-menu__item"
        type="button"
        @click="menuState.entry.config.show === false ? showCompanion(menuState.entry) : hideCompanion(menuState.entry)"
      >
        {{ menuState.entry.config.show === false ? t('portal.agent.companion.show') : t('common.hide') }}
      </button>
      <div class="companion-floating-menu__group">
        <span class="companion-floating-menu__label">{{ t('companions.scale') }}</span>
        <div class="companion-floating-menu__scales">
          <button
            v-for="value in scalePresets"
            :key="value"
            class="companion-floating-menu__scale"
            :class="{ 'is-active': Math.abs(resolveCompanionScale(menuState.entry) - value) < 0.001 }"
            type="button"
            @click="applyCompanionScale(menuState.entry, value)"
          >
            {{ value.toFixed(1) }}x
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { useRouter } from 'vue-router';

import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useChatStore } from '@/stores/chat';
import {
  useCompanionStore,
  type CompanionPackageRecord,
  type CompanionPosition,
  type CompanionSpriteStateId
} from '@/stores/companions';
import { parseAgentAvatarIconConfig, type AgentAvatarIconConfig } from '@/utils/agentAvatar';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';

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

type AgentRuntimeState = 'idle' | 'running' | 'done' | 'pending' | 'error';

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
    resolveAgentRuntimeState?: ((agentId: string) => AgentRuntimeState) | undefined;
    openAgentById?: ((agentId: string) => Promise<void> | void) | undefined;
  }>(),
  {
    desktopMode: false
  }
);

const agentStore = useAgentStore();
const chatStore = useChatStore();
const companionStore = useCompanionStore();
const router = useRouter();
const { t } = useI18n();
const now = ref(Date.now());
const draggingKey = ref('');
const spriteStateByKey = reactive<Record<string, CompanionSpriteStateId>>({});
const spriteStateTimeoutByKey = new Map<string, number>();
const positions = ref<Record<string, CompanionPosition>>(loadPositions());
const scalePresets = Object.freeze([0.5, 0.8, 1.0, 1.2, 1.4, 1.6]);
const menuRef = ref<HTMLElement | null>(null);
const menuPosition = ref({ x: 8, y: 8 });
let nowTimer: number | null = null;
let clickSuppressUntil = 0;
let desktopOverlayActive = false;
const menuState = ref<{ x: number; y: number; entry: FloatingEntry } | null>(null);
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

const activeSessionAgentId = computed(() => {
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (sessionId) {
    const session = Array.isArray(chatStore.sessions)
      ? chatStore.sessions.find((item) => String(item?.id || '').trim() === sessionId)
      : null;
    const sessionAgentId = String(session?.agent_id || chatStore.draftAgentId || '').trim();
    if (sessionAgentId) {
      return sessionAgentId;
    }
  }
  return String(chatStore.draftAgentId || '').trim();
});

const normalizeBubbleText = (value: unknown): string =>
  String(value ?? '')
    .replace(/!\[[^\]]*]\(([^)]+)\)/g, '')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '$1')
    .replace(/[`#>*_~-]/g, ' ')
    .replace(/\r?\n+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const truncateBubbleText = (value: string, max = 96): string => {
  if (value.length <= max) {
    return value;
  }
  return `${value.slice(0, Math.max(0, max - 1)).trimEnd()}…`;
};

const activeAgentAssistantMessage = computed<Record<string, unknown> | null>(() => {
  const targetAgentId = activeSessionAgentId.value;
  if (!targetAgentId) {
    return null;
  }
  const messages = Array.isArray(chatStore.messages) ? chatStore.messages : [];
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = (messages[index] || {}) as Record<string, unknown>;
    if (String(message?.role || '').trim().toLowerCase() !== 'assistant') {
      continue;
    }
    const content = truncateBubbleText(
      normalizeBubbleText(prepareMessageMarkdownContent(message?.content, message))
    );
    if (!content) {
      continue;
    }
    return {
      agentId: targetAgentId,
      content
    };
  }
  return null;
});

const visibleEntries = computed<FloatingEntry[]>(() => {
  const items: Array<FloatingEntry | null> = allAgents.value
    .map((agent, index) => {
      const config = parseAgentAvatarIconConfig(agent.icon);
      if (config.kind !== 'companion') {
        return null;
      }
      const companion = companionStore.findCompanion(config.scope || 'global', config.id || config.name);
      if (!companion) {
        return null;
      }
      const agentId = String(agent.id || config.id || index).trim();
      const override = companionStore.getAgentOverride(agentId);
      const effectiveShow = override?.show ?? config.show;
      if (effectiveShow === false) {
        return null;
      }
      const key = `${agentId}:${config.scope || 'global'}:${config.id || config.name}`;
      const scale = Number(override?.scale ?? config.scale ?? 1);
      const position = positions.value[key] || defaultPosition(index);
      const runtimeMessage = currentMessage.value;
      const agentMessage = activeAgentAssistantMessage.value;
      const hasMessageHints = config.messageHints !== false && companionStore.settings.messageHintsEnabled !== false;
      const isActiveAgent = agentMessage?.agentId === agentId;
      const runtimeMessageText = String(runtimeMessage?.text || '');
      const messageText = hasMessageHints && isActiveAgent
        ? String(agentMessage?.content || runtimeMessageText || '')
        : runtimeMessageText;
      const messageVisible = hasMessageHints && Boolean(messageText);
      return {
        key,
        agentId,
        name: String(agent.name || companion.displayName || agentId).trim(),
        config: {
          ...config,
          show: effectiveShow,
          scale
        },
        companion,
        scale,
        message: messageText,
        messageKind: runtimeMessage?.kind || 'info',
        messageVisible,
        style: {
          left: `${position.x}px`,
          top: `${position.y}px`
        }
      } satisfies FloatingEntry;
    })
  return items.filter((item): item is FloatingEntry => Boolean(item));
});

const menuStyle = computed(() => {
  if (!menuState.value) {
    return {};
  }
  return {
    left: `${menuPosition.value.x}px`,
    top: `${menuPosition.value.y}px`
  };
});

const routeBasePrefix = computed(() => {
  const path = String(router.currentRoute.value.path || '').trim();
  if (path.startsWith('/desktop')) {
    return '/desktop';
  }
  if (path.startsWith('/demo')) {
    return '/demo';
  }
  return '/app';
});

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

function normalizeAgentRuntimeState(value: unknown): AgentRuntimeState {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'running') return 'running';
  if (normalized === 'pending') return 'pending';
  if (normalized === 'done') return 'done';
  if (normalized === 'error') return 'error';
  return 'idle';
}

function resolveRuntimeSpriteState(agentId: string): CompanionSpriteStateId {
  const state = typeof props.resolveAgentRuntimeState === 'function'
    ? props.resolveAgentRuntimeState(String(agentId || '').trim())
    : 'idle';
  const activeAgentId = String(activeAgentAssistantMessage.value?.agentId || '').trim();
  const reviewingCurrentAgent =
    normalizeAgentRuntimeState(state) === 'pending' &&
    activeAgentId === String(agentId || '').trim() &&
    currentMessage.value?.visibleUntil &&
    currentMessage.value.visibleUntil > now.value;
  switch (normalizeAgentRuntimeState(state)) {
    case 'running':
      return 'running';
    case 'pending':
      return reviewingCurrentAgent ? 'review' : 'waiting';
    case 'done':
      return 'jumping';
    case 'error':
      return 'failed';
    default:
      return 'idle';
  }
}

function resolveEntrySpriteState(entry: FloatingEntry): CompanionSpriteStateId {
  return spriteStateByKey[entry.key] || resolveRuntimeSpriteState(entry.agentId);
}

function clearSpriteStateOverride(key: string): void {
  const timerId = spriteStateTimeoutByKey.get(key);
  if (timerId !== undefined && typeof window !== 'undefined') {
    window.clearTimeout(timerId);
  }
  spriteStateTimeoutByKey.delete(key);
  delete spriteStateByKey[key];
}

function setSpriteState(key: string, state: CompanionSpriteStateId, durationMs = 0): void {
  clearSpriteStateOverride(key);
  spriteStateByKey[key] = state;
  if (durationMs > 0 && typeof window !== 'undefined') {
    const timerId = window.setTimeout(() => {
      if (spriteStateByKey[key] === state) {
        delete spriteStateByKey[key];
      }
      spriteStateTimeoutByKey.delete(key);
    }, durationMs);
    spriteStateTimeoutByKey.set(key, timerId);
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
    state: resolveEntrySpriteState(entry),
    scale: entry.scale,
    x: position.x,
    y: position.y,
    message: entry.message,
    messageKind: entry.messageKind,
    messageVisible: entry.messageVisible
  }))) === true;
}

function handleClick(entry: FloatingEntry): void {
  closeEntryMenu();
  if (Date.now() < clickSuppressUntil) {
    return;
  }
  setSpriteState(entry.key, 'waving', 900);
  if (!entry.messageVisible) {
    companionStore.showMessage(entry.name || entry.companion.displayName, { durationMs: 1800 });
  }
}

function closeEntryMenu(): void {
  menuState.value = null;
}

function updateMenuPosition(): void {
  if (!menuState.value || typeof window === 'undefined') {
    return;
  }
  const MENU_MARGIN = 8;
  const menuWidth = Math.max(0, Math.round(menuRef.value?.offsetWidth || 0));
  const menuHeight = Math.max(0, Math.round(menuRef.value?.offsetHeight || 0));
  const maxX = Math.max(MENU_MARGIN, window.innerWidth - menuWidth - MENU_MARGIN);
  const maxY = Math.max(MENU_MARGIN, window.innerHeight - menuHeight - MENU_MARGIN);
  menuPosition.value = {
    x: Math.min(Math.max(MENU_MARGIN, menuState.value.x), maxX),
    y: Math.min(Math.max(MENU_MARGIN, menuState.value.y), maxY)
  };
}

function openEntryMenu(event: MouseEvent, entry: FloatingEntry): void {
  menuState.value = {
    x: Math.max(8, event.clientX),
    y: Math.max(8, event.clientY),
    entry
  };
  menuPosition.value = {
    x: Math.max(8, event.clientX),
    y: Math.max(8, event.clientY)
  };
  if (typeof window !== 'undefined') {
    window.requestAnimationFrame(() => {
      updateMenuPosition();
    });
  }
}

function resolveScaleValue(value: unknown): number {
  return Math.min(1.6, Math.max(0.5, Number(value) || 1));
}

function resolveCompanionScale(entry: FloatingEntry): number {
  return resolveScaleValue(entry.config.scale || entry.scale || 1);
}

async function persistCompanionConfig(entry: FloatingEntry, buildNext: (current: AgentAvatarIconConfig) => AgentAvatarIconConfig): Promise<void> {
  const nextConfig = buildNext(entry.config);
  companionStore.setAgentOverride(entry.agentId, {
    show: nextConfig.show !== false,
    scale: resolveScaleValue(nextConfig.scale)
  });
}

async function applyCompanionScale(entry: FloatingEntry, value: number): Promise<void> {
  await persistCompanionConfig(entry, (current) => ({
    ...current,
    scale: resolveScaleValue(value)
  }));
  closeEntryMenu();
}

async function hideCompanion(entry: FloatingEntry): Promise<void> {
  await persistCompanionConfig(entry, (current) => ({
    ...current,
    show: false,
    scale: resolveCompanionScale(entry)
  }));
  closeEntryMenu();
}

async function showCompanion(entry: FloatingEntry): Promise<void> {
  await persistCompanionConfig(entry, (current) => ({
    ...current,
    show: true,
    scale: resolveCompanionScale(entry)
  }));
  closeEntryMenu();
}

async function openCompanionChat(entry: FloatingEntry): Promise<void> {
  closeEntryMenu();
  const normalizedAgentId = String(entry.agentId || '').trim();
  const isDefaultAgent = !normalizedAgentId || normalizedAgentId === '__default__';
  if (typeof props.openAgentById === 'function') {
    await Promise.resolve(props.openAgentById(isDefaultAgent ? '__default__' : normalizedAgentId));
    return;
  }
  void router.replace({
    path: `${routeBasePrefix.value}/chat`,
    query: isDefaultAgent
      ? { section: 'messages', entry: 'default' }
      : { section: 'messages', agent_id: normalizedAgentId }
  });
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
  clearSpriteStateOverride(pointerState.key);
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
  updateMenuPosition();
}

onMounted(async () => {
  await companionStore.hydrate().catch(() => undefined);
  await companionStore.loadGlobalCompanions().catch(() => undefined);
  if (!agentStore.agents.length) {
    await agentStore.loadAgents().catch(() => undefined);
  }
  window.addEventListener('resize', clampAfterResize);
  document.addEventListener('mousedown', closeEntryMenu);
  window.addEventListener('blur', closeEntryMenu);
  nowTimer = window.setInterval(() => {
    now.value = Date.now();
    if (companionStore.message && companionStore.message.visibleUntil <= now.value) {
      companionStore.clearMessage();
    }
  }, 500);
});

onBeforeUnmount(() => {
  if (nowTimer !== null) window.clearInterval(nowTimer);
  Array.from(spriteStateTimeoutByKey.keys()).forEach((key) => clearSpriteStateOverride(key));
  window.removeEventListener('resize', clampAfterResize);
  window.removeEventListener('pointermove', handlePointerMove);
  document.removeEventListener('mousedown', closeEntryMenu);
  window.removeEventListener('blur', closeEntryMenu);
  void getDesktopBridge()?.hideCompanion?.({ persistEnabled: false });
});

watch(
  () => menuState.value ? `${menuState.value.x}:${menuState.value.y}:${menuState.value.entry.key}` : '',
  () => {
    if (menuState.value && typeof window !== 'undefined') {
      window.requestAnimationFrame(() => {
        updateMenuPosition();
      });
    }
  }
);

watch(
  () => menuRef.value,
  () => {
    if (menuState.value && typeof window !== 'undefined') {
      window.requestAnimationFrame(() => {
        updateMenuPosition();
      });
    }
  }
);

watch(
  () => visibleEntries.value.map((entry) => [
    entry.key,
    entry.scale,
    entry.message,
    entry.messageVisible,
    resolveEntrySpriteState(entry),
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
  display: block;
  cursor: default;
  user-select: none;
  touch-action: none;
}

.companion-floating-layer.is-dragging {
  cursor: grabbing;
}

.companion-floating-layer :deep(.companion-sprite) {
  cursor: pointer;
}

.companion-floating-layer.is-dragging :deep(.companion-sprite) {
  cursor: grabbing;
}

.companion-floating-layer__bubble {
  position: absolute;
  left: 50%;
  bottom: calc(100% + 4px);
  transform: translateX(-50%);
  width: max-content;
  min-width: 88px;
  max-width: min(320px, calc(100vw - 24px));
  padding: 8px 10px;
  border: 1px solid rgba(37, 99, 235, 0.22);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.96);
  box-shadow: 0 10px 28px rgba(15, 23, 42, 0.16);
  color: #1f2937;
  font-size: 13px;
  line-height: 1.45;
  text-align: center;
  white-space: normal;
  overflow-wrap: anywhere;
  box-sizing: border-box;
  pointer-events: none;
  z-index: 1;
}

.companion-floating-layer__bubble.is-success {
  border-color: rgba(20, 184, 166, 0.28);
  color: #0f766e;
}

.companion-floating-layer__bubble.is-warning {
  border-color: rgba(245, 158, 11, 0.3);
  color: #92400e;
}

.companion-floating-menu {
  position: fixed;
  z-index: 12010;
  min-width: 184px;
  padding: 8px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.98);
  box-shadow: 0 18px 42px rgba(15, 23, 42, 0.18);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.companion-floating-menu__item,
.companion-floating-menu__scale {
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: #0f172a;
  text-align: left;
  cursor: pointer;
}

.companion-floating-menu__item {
  padding: 9px 10px;
  font-size: 13px;
}

.companion-floating-menu__item:hover,
.companion-floating-menu__scale:hover {
  background: rgba(59, 130, 246, 0.08);
}

.companion-floating-menu__group {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 4px 2px 2px;
}

.companion-floating-menu__label {
  font-size: 12px;
  font-weight: 600;
  color: #64748b;
}

.companion-floating-menu__scales {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.companion-floating-menu__scale {
  padding: 6px 8px;
  font-size: 12px;
}

.companion-floating-menu__scale.is-active {
  background: rgba(59, 130, 246, 0.12);
  color: #1d4ed8;
}
</style>
