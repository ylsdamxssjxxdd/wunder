<template>
  <div
    class="beeroom-node-card"
    :class="[
      `is-${node.status}`,
      `is-${node.role}`,
      `is-emphasis-${node.emphasis}`,
      {
        'is-mother': node.role === 'mother',
        'is-artifact-container': node.renderKind === 'artifact-container',
        'is-selected': node.selected,
        'is-condensed': condensed,
        'is-revealing': !!reveal,
        'is-live-activity': hasLiveActivity
      }
    ]"
    :aria-label="`${node.name} ${node.roleLabel} ${node.statusLabel}`"
    :data-testid="`beeroom-node:${node.id}`"
    :data-node-id="node.id"
    :data-node-status="node.status"
    :data-node-role="node.role"
    :data-node-emphasis="node.emphasis"
    role="button"
    tabindex="0"
    draggable="false"
    :style="cardStyle"
    @click.stop="emit('click')"
    @dblclick.stop="emit('dblclick')"
    @keydown.enter.prevent="emit('click')"
    @keydown.space.prevent="emit('click')"
    @dragstart.prevent
  >
    <template v-if="node.renderKind === 'artifact-container'">
      <div class="beeroom-node-artifact-container">
        <div class="beeroom-node-artifact-topbar">
          <span class="beeroom-node-artifact-title">{{ artifactTitle }}</span>
          <span class="beeroom-node-artifact-count">{{ artifactCountLabel }}</span>
        </div>
        <div class="beeroom-node-artifact-scroll" @wheel.stop>
          <div class="beeroom-node-artifact-grid" :class="`is-${artifactDisplayMode}`">
            <button
              v-for="slot in artifactSlots"
              :key="slot.key"
              class="beeroom-node-artifact-slot"
              :class="{
                'is-empty': !slot.item,
                'is-clickable': Boolean(slot.item?.previewable)
              }"
              type="button"
              :disabled="!slot.item || !slot.item.previewable"
              :title="slot.item?.title || ''"
              @click.stop="handleArtifactSlotClick(slot.item)"
            >
              <span
                class="beeroom-node-artifact-slot-frame"
                :class="[
                  slot.item ? `is-${slot.item.kind}` : 'is-empty',
                  slot.item?.previewable ? 'is-previewable' : ''
                ]"
              >
                <i v-if="slot.item" class="fa-solid" :class="slot.item.iconClass" aria-hidden="true"></i>
                <span v-if="slot.item" class="beeroom-node-artifact-slot-copy">
                  <span class="beeroom-node-artifact-slot-label">{{ slot.item.label }}</span>
                  <span v-if="slot.item.meta" class="beeroom-node-artifact-slot-meta">{{ slot.item.meta }}</span>
                </span>
              </span>
            </button>
          </div>
        </div>
      </div>
    </template>
    <template v-else>
      <div class="beeroom-node-card-body">
        <div class="beeroom-node-card-head">
          <span class="beeroom-node-avatar">
            <img v-if="node.avatarImageUrl" class="beeroom-node-avatar-img" :src="node.avatarImageUrl" alt="" />
            <span v-else class="beeroom-node-avatar-text">{{ node.avatarInitial }}</span>
          </span>
          <div class="beeroom-node-title-group">
            <div class="beeroom-node-title" :title="node.name">{{ node.displayName }}</div>
            <div class="beeroom-node-role-chip">{{ node.roleLabel }}</div>
          </div>
          <span class="beeroom-node-status">
            <i class="beeroom-node-status-dot"></i>
            <span>{{ node.statusLabel }}</span>
          </span>
        </div>
      </div>

      <div
        ref="workflowContainerRef"
        class="beeroom-node-workflow"
        :class="[`is-${node.workflowTone}`, { 'is-empty': !visibleWorkflowLines.length && !hasArtifactPanel }]"
      >
        <div v-if="hasArtifactPanel" class="beeroom-node-artifact-container is-embedded" @wheel.stop>
          <div class="beeroom-node-artifact-topbar">
            <span class="beeroom-node-artifact-title">{{ resolvedArtifactTitle }}</span>
            <span class="beeroom-node-artifact-count">{{ artifactCountLabel }}</span>
          </div>
          <div class="beeroom-node-artifact-scroll">
            <div class="beeroom-node-artifact-grid" :class="`is-${artifactDisplayMode}`">
              <button
                v-for="slot in artifactSlots"
                :key="slot.key"
                class="beeroom-node-artifact-slot"
                :class="{
                  'is-empty': !slot.item,
                  'is-clickable': Boolean(slot.item?.previewable)
                }"
                type="button"
                :disabled="!slot.item || !slot.item.previewable"
                :title="slot.item?.title || ''"
                @click.stop="handleArtifactSlotClick(slot.item)"
              >
                <span
                  class="beeroom-node-artifact-slot-frame"
                  :class="[
                    slot.item ? `is-${slot.item.kind}` : 'is-empty',
                    slot.item?.previewable ? 'is-previewable' : ''
                  ]"
                >
                  <i v-if="slot.item" class="fa-solid" :class="slot.item.iconClass" aria-hidden="true"></i>
                  <span v-if="slot.item" class="beeroom-node-artifact-slot-copy">
                    <span class="beeroom-node-artifact-slot-label">{{ slot.item.label }}</span>
                    <span v-if="slot.item.meta" class="beeroom-node-artifact-slot-meta">{{ slot.item.meta }}</span>
                  </span>
                </span>
              </button>
            </div>
          </div>
        </div>
        <div v-else-if="visibleWorkflowLines.length" ref="workflowStepsRef" class="beeroom-node-workflow-steps">
          <div
            v-for="line in visibleWorkflowLines"
            :key="line.key"
            class="beeroom-node-workflow-step"
            :title="line.title"
          >
            <span class="beeroom-node-workflow-step-dot"></span>
            <span class="beeroom-node-workflow-step-text">
              <span class="beeroom-node-workflow-step-main">{{ line.main }}</span>
              <span v-if="line.detail" class="beeroom-node-workflow-step-detail">{{ line.detail }}</span>
            </span>
          </div>
        </div>
        <div v-else class="beeroom-node-workflow-empty">{{ emptyLabel }}</div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import type { SwarmProjectionNode } from './swarmCanvasModel';

const props = defineProps<{
  node: SwarmProjectionNode;
  condensed?: boolean;
  emptyLabel: string;
  reveal?: {
    offsetX: number;
    offsetY: number;
    order: number;
  } | null;
}>();

const emit = defineEmits<{
  (event: 'click'): void;
  (event: 'dblclick'): void;
  (event: 'preview-artifact', payload: NonNullable<SwarmProjectionNode['artifactItems']>[number]): void;
}>();

const DEFAULT_ACTIVITY_ACCENT_RGB = '59, 130, 246';

const resolveAccentRgb = (color: string) => {
  const normalized = String(color || '').trim();
  const hexMatch = normalized.match(/^#([\da-f]{3}|[\da-f]{6})$/i);
  if (hexMatch) {
    const hex = hexMatch[1];
    if (hex.length === 3) {
      const [r, g, b] = hex.split('');
      return [
        Number.parseInt(`${r}${r}`, 16),
        Number.parseInt(`${g}${g}`, 16),
        Number.parseInt(`${b}${b}`, 16)
      ].join(', ');
    }
    return [
      Number.parseInt(hex.slice(0, 2), 16),
      Number.parseInt(hex.slice(2, 4), 16),
      Number.parseInt(hex.slice(4, 6), 16)
    ].join(', ');
  }
  const rgbMatch = normalized.match(/^rgba?\(([^)]+)\)$/i);
  if (rgbMatch) {
    const channels = rgbMatch[1]
      .split(',')
      .slice(0, 3)
      .map((value) => Number.parseInt(value.trim(), 10))
      .filter((value) => Number.isFinite(value) && value >= 0 && value <= 255);
    if (channels.length === 3) return channels.join(', ');
  }
  return DEFAULT_ACTIVITY_ACCENT_RGB;
};

const cardStyle = computed(() => {
  const accentRgb = resolveAccentRgb(props.node.accentColor);
  const avatarAccentRgb = resolveAccentRgb(props.node.avatarColor || props.node.accentColor);
  return {
    '--node-accent': props.node.accentColor,
    '--node-accent-rgb': accentRgb,
    '--node-avatar-accent': props.node.avatarColor || props.node.accentColor,
    '--node-avatar-accent-rgb': avatarAccentRgb,
    '--node-activity-glow': `rgba(${accentRgb}, 0.22)`,
    '--node-activity-border': `rgba(${accentRgb}, 0.56)`,
    '--node-activity-border-soft': `rgba(${accentRgb}, 0.18)`,
    '--node-activity-halo-strong': `rgba(${accentRgb}, 0.3)`,
    '--node-activity-shadow-soft': `rgba(${accentRgb}, 0.14)`,
    '--node-intro-x': `${Math.round(props.reveal?.offsetX || 0)}px`,
    '--node-intro-y': `${Math.round(props.reveal?.offsetY || 0)}px`,
    '--node-intro-delay': `${Math.max(0, Number(props.reveal?.order || 0)) * 70}ms`,
    width: `${props.node.width}px`,
    height: `${props.node.height}px`
  };
});

const visibleWorkflowLines = computed(() => (Array.isArray(props.node.workflowLines) ? props.node.workflowLines : []));
const visibleArtifactItems = computed(() => (Array.isArray(props.node.artifactItems) ? props.node.artifactItems : []));
const hasArtifactPanel = computed(() => props.node.renderKind !== 'artifact-container' && visibleArtifactItems.value.length > 0);
const resolvedArtifactTitle = computed(() => String(props.node.artifactTitle || props.node.roleLabel || '').trim() || '产物');
const artifactTitle = computed(() => String(props.node.roleLabel || '').trim() || '产物');
const artifactCount = computed(() => Math.max(Number(props.node.artifactCount || 0), visibleArtifactItems.value.length));
const artifactDisplayMode = computed(() =>
  String(props.node.artifactDisplayMode || '').trim().toLowerCase() === 'showcase' ? 'showcase' : 'compact'
);
const artifactSlots = computed(() => {
  const limit = artifactDisplayMode.value === 'showcase' ? 2 : 8;
  const items = visibleArtifactItems.value.map((item, index) => ({
    key: item?.key || `artifact:${props.node.id}:${index}`,
    item: item || null
  })).slice(0, limit);
  if (items.length >= limit) return items;
  return items.concat(
    Array.from({ length: limit - items.length }, (_, index) => ({
      key: `empty:${props.node.id}:${index}`,
      item: null
    }))
  );
});
const hasLiveActivity = computed(() => {
  const normalizedStatus = String(props.node.status || '').trim().toLowerCase();
  if (normalizedStatus === 'running' || normalizedStatus === 'queued' || normalizedStatus === 'awaiting_idle') {
    return true;
  }
  return props.node.role === 'subagent' && props.node.emphasis === 'active';
});

const artifactCountLabel = computed(() => String(artifactCount.value));

const handleArtifactSlotClick = (item: NonNullable<SwarmProjectionNode['artifactItems']>[number] | null | undefined) => {
  if (!item?.previewable) return;
  emit('preview-artifact', item);
};

const workflowContainerRef = ref<HTMLElement | null>(null);
const workflowStepsRef = ref<HTMLElement | null>(null);
let workflowResizeObserver: ResizeObserver | null = null;
let workflowTailFollowTimer: number | null = null;
let workflowFollowRafPrimary: number | null = null;
let workflowFollowRafSecondary: number | null = null;

const workflowLineSignature = computed(() =>
  visibleWorkflowLines.value
    .map((line) => `${line.key}:${line.main}:${line.detail}:${line.title}`)
    .join('||')
);

const shouldFollowWorkflowTail = computed(() => visibleWorkflowLines.value.length > 0);

const scrollWorkflowToBottom = () => {
  const element = workflowContainerRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const clearWorkflowTailFollowSchedule = () => {
  if (typeof window === 'undefined') return;
  if (workflowTailFollowTimer !== null) {
    window.clearTimeout(workflowTailFollowTimer);
    workflowTailFollowTimer = null;
  }
  if (workflowFollowRafPrimary !== null) {
    window.cancelAnimationFrame(workflowFollowRafPrimary);
    workflowFollowRafPrimary = null;
  }
  if (workflowFollowRafSecondary !== null) {
    window.cancelAnimationFrame(workflowFollowRafSecondary);
    workflowFollowRafSecondary = null;
  }
};

const releaseWorkflowResizeObserver = () => {
  if (workflowResizeObserver) {
    workflowResizeObserver.disconnect();
    workflowResizeObserver = null;
  }
};

const scheduleWorkflowTailFollow = async () => {
  if (!shouldFollowWorkflowTail.value || !visibleWorkflowLines.value.length) return;
  await nextTick();
  if (typeof window !== 'undefined') {
    clearWorkflowTailFollowSchedule();
    workflowFollowRafPrimary = window.requestAnimationFrame(() => {
      workflowFollowRafPrimary = null;
      scrollWorkflowToBottom();
      workflowFollowRafSecondary = window.requestAnimationFrame(() => {
        workflowFollowRafSecondary = null;
        scrollWorkflowToBottom();
      });
    });
    workflowTailFollowTimer = window.setTimeout(() => {
      workflowTailFollowTimer = null;
      scrollWorkflowToBottom();
    }, 42);
    return;
  }
  scrollWorkflowToBottom();
};

const attachWorkflowResizeObserver = () => {
  releaseWorkflowResizeObserver();
  const stepsElement = workflowStepsRef.value;
  const containerElement = workflowContainerRef.value;
  if ((!stepsElement && !containerElement) || typeof ResizeObserver === 'undefined') return;
  workflowResizeObserver = new ResizeObserver(() => {
    if (!shouldFollowWorkflowTail.value || !visibleWorkflowLines.value.length) return;
    void scheduleWorkflowTailFollow();
  });
  if (stepsElement) {
    workflowResizeObserver.observe(stepsElement);
  }
  if (containerElement) {
    workflowResizeObserver.observe(containerElement);
  }
};

watch(
  [workflowLineSignature, shouldFollowWorkflowTail],
  async () => {
    await scheduleWorkflowTailFollow();
  },
  {
    flush: 'post',
    immediate: true
  }
);

watch(
  workflowStepsRef,
  async () => {
    await nextTick();
    attachWorkflowResizeObserver();
    await scheduleWorkflowTailFollow();
  },
  {
    flush: 'post',
    immediate: true
  }
);

watch(
  () => [props.node.id, props.node.workflowTone, props.node.height].join('|'),
  async () => {
    await scheduleWorkflowTailFollow();
  },
  {
    flush: 'post',
    immediate: true
  }
);

onMounted(() => {
  attachWorkflowResizeObserver();
  void scheduleWorkflowTailFollow();
});

onBeforeUnmount(() => {
  clearWorkflowTailFollowSchedule();
  releaseWorkflowResizeObserver();
});
</script>

<style scoped>
.beeroom-node-card {
  --node-activity-glow: rgba(0, 0, 0, 0);
  --node-activity-border: rgba(255, 255, 255, 0);
  --node-activity-border-soft: rgba(255, 255, 255, 0);
  --node-activity-halo-strong: rgba(255, 255, 255, 0);
  --node-activity-shadow-soft: rgba(255, 255, 255, 0);
  --node-avatar-accent: var(--node-accent);
  --node-avatar-accent-rgb: var(--node-accent-rgb);
  position: absolute;
  display: flex;
  flex-direction: column;
  justify-content: flex-start;
  gap: 8px;
  padding: 14px 14px 12px 16px;
  border: 1px solid rgba(var(--node-accent-rgb), 0.24);
  border-radius: 18px;
  background:
    radial-gradient(circle at top left, rgba(var(--node-accent-rgb), 0.12), transparent 42%),
    linear-gradient(180deg, rgba(18, 22, 31, 0.98), rgba(12, 15, 22, 0.98));
  color: #e5e7eb;
  text-align: left;
  cursor: grab;
  overflow: hidden;
  user-select: none;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  touch-action: none;
  box-shadow:
    0 10px 22px rgba(2, 6, 23, 0.18),
    inset 0 1px 0 rgba(var(--node-accent-rgb), 0.06);
  -webkit-font-smoothing: antialiased;
  text-rendering: geometricPrecision;
  transition:
    border-color 0.56s cubic-bezier(0.22, 1, 0.36, 1),
    box-shadow 1.2s cubic-bezier(0.22, 1, 0.36, 1),
    transform 0.22s ease;
}

.beeroom-node-card::before {
  content: '';
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 3px;
  background: var(--node-accent);
  opacity: 0.92;
  pointer-events: none;
}

.beeroom-node-card::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  border: 1px solid var(--node-activity-border);
  box-shadow: inset 0 0 0 1px var(--node-activity-border-soft);
  opacity: 0;
  transform: scale(0.992);
  pointer-events: none;
  transition:
    opacity 1.05s cubic-bezier(0.22, 1, 0.36, 1),
    transform 1.2s cubic-bezier(0.22, 1, 0.36, 1),
    border-color 0.9s cubic-bezier(0.22, 1, 0.36, 1),
    box-shadow 1.2s cubic-bezier(0.22, 1, 0.36, 1);
}

.beeroom-node-card:hover,
.beeroom-node-card:focus-visible {
  border-color: rgba(96, 165, 250, 0.34);
  transform: translateY(-1px);
  box-shadow: 0 14px 26px rgba(2, 6, 23, 0.22);
  outline: none;
}

.beeroom-node-card.is-selected {
  border-color: rgba(96, 165, 250, 0.56);
  box-shadow: inset 0 0 0 1px rgba(96, 165, 250, 0.2);
}

.beeroom-node-card.is-mother {
  border-color: rgba(var(--node-accent-rgb), 0.32);
  background:
    radial-gradient(circle at top left, rgba(var(--node-accent-rgb), 0.16), transparent 46%),
    linear-gradient(180deg, rgba(26, 33, 45, 0.98), rgba(12, 15, 22, 0.98));
}

.beeroom-node-card.is-subagent {
  gap: 7px;
  padding: 12px 12px 10px 14px;
  border-radius: 16px;
  background:
    radial-gradient(circle at top left, rgba(var(--node-accent-rgb), 0.12), transparent 44%),
    linear-gradient(180deg, rgba(12, 18, 28, 0.96), rgba(8, 13, 22, 0.97));
  box-shadow: 0 10px 18px rgba(2, 6, 23, 0.16);
}

.beeroom-node-card.is-subagent.is-emphasis-active {
  border-color: rgba(34, 211, 238, 0.34);
  box-shadow:
    0 0 0 1px rgba(34, 211, 238, 0.08),
    0 14px 24px rgba(8, 47, 73, 0.18);
}

.beeroom-node-card.is-subagent.is-emphasis-dormant {
  border-color: rgba(100, 116, 139, 0.24);
  background:
    radial-gradient(circle at top left, rgba(var(--node-accent-rgb), 0.08), transparent 44%),
    linear-gradient(180deg, rgba(18, 24, 34, 0.94), rgba(12, 16, 24, 0.96));
  box-shadow: 0 8px 16px rgba(2, 6, 23, 0.1);
}

.beeroom-node-card.is-artifact-container {
  gap: 0;
  padding: 12px;
  border-color: rgba(245, 158, 11, 0.3);
  background:
    radial-gradient(circle at 18% 16%, rgba(251, 191, 36, 0.28), transparent 40%),
    radial-gradient(circle at 92% 90%, rgba(45, 212, 191, 0.12), transparent 42%),
    linear-gradient(180deg, rgba(128, 94, 46, 0.84), rgba(76, 62, 38, 0.88));
  box-shadow:
    0 12px 24px rgba(120, 53, 15, 0.1),
    inset 0 1px 0 rgba(255, 247, 237, 0.16);
}

.beeroom-node-card.is-artifact-container::before {
  display: none;
}

.beeroom-node-card.is-artifact-container.is-selected {
  border-color: rgba(251, 191, 36, 0.48);
  box-shadow:
    inset 0 0 0 1px rgba(255, 236, 179, 0.18),
    0 14px 26px rgba(120, 53, 15, 0.14);
}

.beeroom-node-card.is-live-activity {
  box-shadow:
    0 0 0 1px rgba(var(--node-accent-rgb), 0.14),
    0 16px 30px var(--node-activity-shadow-soft),
    0 0 28px var(--node-activity-glow);
}

.beeroom-node-card.is-live-activity::after {
  opacity: 0.88;
  animation: beeroom-node-border-breathe 2.15s cubic-bezier(0.33, 0, 0.2, 1) infinite;
}

.beeroom-node-card.is-live-activity .beeroom-node-status-dot {
  animation: beeroom-node-status-pulse 1.75s ease-in-out infinite;
}

.beeroom-node-card:active {
  cursor: grabbing;
}

.beeroom-node-card.is-dragging {
  z-index: 8;
  transition: none;
}

.beeroom-node-card-body {
  display: flex;
  flex-direction: column;
  flex: 0 0 auto;
  gap: 8px;
}

.beeroom-node-card-head {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
}

.beeroom-node-avatar {
  position: relative;
  isolation: isolate;
  width: 42px;
  height: 42px;
  border-radius: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(var(--node-avatar-accent-rgb), 0.34);
  background:
    radial-gradient(
      circle at center,
      rgba(var(--node-avatar-accent-rgb), 0.34) 0 18%,
      rgba(var(--node-avatar-accent-rgb), 0.2) 18% 34%,
      rgba(var(--node-avatar-accent-rgb), 0.12) 34% 52%,
      rgba(5, 12, 18, 0.94) 52% 100%
    ),
    linear-gradient(135deg, rgba(var(--node-avatar-accent-rgb), 0.36), rgba(8, 18, 30, 0.92));
  color: #f8fafc;
  font-size: 14px;
  font-weight: 700;
  overflow: hidden;
  box-shadow:
    inset 0 0 0 1px rgba(var(--node-avatar-accent-rgb), 0.18),
    0 8px 18px rgba(var(--node-avatar-accent-rgb), 0.22);
}

.beeroom-node-card.is-subagent .beeroom-node-avatar {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  border-color: rgba(var(--node-avatar-accent-rgb), 0.3);
}

.beeroom-node-card.is-subagent .beeroom-node-avatar::before {
  inset: 2px;
  border-radius: 8px;
}

.beeroom-node-avatar::before {
  content: '';
  position: absolute;
  inset: 3px;
  border-radius: 10px;
  background:
    repeating-radial-gradient(circle at center, rgba(var(--node-avatar-accent-rgb), 0.16) 0 1px, transparent 1px 8px),
    linear-gradient(180deg, rgba(8, 28, 21, 0.06), rgba(2, 12, 10, 0.42));
  opacity: 0.92;
  pointer-events: none;
}

.beeroom-node-avatar::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: conic-gradient(
    from 225deg,
    transparent 0 72%,
    rgba(var(--node-avatar-accent-rgb), 0.08) 78%,
    rgba(var(--node-avatar-accent-rgb), 0.34) 84%,
    rgba(var(--node-avatar-accent-rgb), 0.16) 90%,
    transparent 96% 100%
  );
  opacity: 0.95;
  pointer-events: none;
}

.beeroom-node-avatar-img {
  position: relative;
  z-index: 1;
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  image-rendering: auto;
  backface-visibility: hidden;
  transform: translateZ(0);
  filter: saturate(0.92) contrast(1.02) brightness(0.96);
}

.beeroom-node-avatar-text {
  position: relative;
  z-index: 1;
  text-shadow: 0 0 12px rgba(var(--node-avatar-accent-rgb), 0.3);
}

.beeroom-node-title-group {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}

.beeroom-node-title {
  font-size: 14px;
  font-weight: 700;
  color: #f8fafc;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-card.is-subagent .beeroom-node-title {
  font-size: 13px;
}

.beeroom-node-role-chip {
  display: inline-flex;
  align-items: center;
  width: fit-content;
  padding: 3px 8px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  background: rgba(31, 41, 55, 0.7);
  color: #d1d5db;
  font-size: 10px;
  letter-spacing: 0.06em;
}

.beeroom-node-card.is-subagent .beeroom-node-role-chip {
  padding: 2px 7px;
  border-color: rgba(71, 85, 105, 0.28);
  background: rgba(15, 23, 42, 0.74);
  color: #cbd5e1;
}

.beeroom-node-status {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 96px;
  padding: 5px 9px;
  border-radius: 999px;
  border: 1px solid rgba(148, 163, 184, 0.28);
  background: rgba(51, 65, 85, 0.35);
  color: #cbd5e1;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-status-dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.96);
  box-shadow: 0 0 0 2px rgba(148, 163, 184, 0.16);
}

.beeroom-node-card.is-running .beeroom-node-status,
.beeroom-node-card.is-queued .beeroom-node-status,
.beeroom-node-card.is-awaiting_idle .beeroom-node-status {
  border-color: rgba(239, 68, 68, 0.32);
  background: rgba(127, 29, 29, 0.24);
  color: #fecaca;
}

.beeroom-node-card.is-running .beeroom-node-status-dot,
.beeroom-node-card.is-queued .beeroom-node-status-dot,
.beeroom-node-card.is-awaiting_idle .beeroom-node-status-dot {
  background: rgba(239, 68, 68, 0.98);
  box-shadow: 0 0 0 2px rgba(239, 68, 68, 0.18);
}

.beeroom-node-card.is-failed .beeroom-node-status,
.beeroom-node-card.is-error .beeroom-node-status,
.beeroom-node-card.is-timeout .beeroom-node-status,
.beeroom-node-card.is-cancelled .beeroom-node-status {
  border-color: rgba(248, 113, 113, 0.34);
  background: rgba(127, 29, 29, 0.28);
  color: #fca5a5;
}

.beeroom-node-card.is-failed .beeroom-node-status-dot,
.beeroom-node-card.is-error .beeroom-node-status-dot,
.beeroom-node-card.is-timeout .beeroom-node-status-dot,
.beeroom-node-card.is-cancelled .beeroom-node-status-dot {
  background: rgba(248, 113, 113, 0.98);
  box-shadow: 0 0 0 2px rgba(248, 113, 113, 0.18);
}

.beeroom-node-card.is-completed .beeroom-node-status,
.beeroom-node-card.is-success .beeroom-node-status {
  border-color: rgba(59, 130, 246, 0.34);
  background: rgba(30, 64, 175, 0.24);
  color: #bfdbfe;
}

.beeroom-node-card.is-completed .beeroom-node-status-dot,
.beeroom-node-card.is-success .beeroom-node-status-dot {
  background: rgba(59, 130, 246, 0.98);
  box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.18);
}

.beeroom-node-card.is-subagent.is-emphasis-active .beeroom-node-status {
  border-color: rgba(34, 211, 238, 0.32);
  background: rgba(8, 47, 73, 0.28);
  color: #cffafe;
}

.beeroom-node-card.is-subagent.is-emphasis-active .beeroom-node-status-dot {
  background: rgba(34, 211, 238, 0.98);
  box-shadow: 0 0 0 2px rgba(34, 211, 238, 0.14);
}

.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-status {
  border-color: rgba(100, 116, 139, 0.22);
  background: rgba(30, 41, 59, 0.34);
  color: #94a3b8;
}

.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-status-dot {
  background: rgba(100, 116, 139, 0.92);
  box-shadow: 0 0 0 2px rgba(100, 116, 139, 0.12);
}

.beeroom-node-workflow {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  align-items: stretch;
  padding: 8px 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  background: rgba(15, 23, 42, 0.48);
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-gutter: stable;
}

.beeroom-node-card.is-subagent .beeroom-node-workflow {
  padding: 7px 10px;
  border-radius: 12px;
}

.beeroom-node-workflow.is-completed {
  border-color: rgba(59, 130, 246, 0.2);
}

.beeroom-node-workflow.is-failed {
  border-color: rgba(248, 113, 113, 0.2);
}

.beeroom-node-workflow.is-loading {
  border-color: rgba(239, 68, 68, 0.2);
}

.beeroom-node-workflow-steps {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
  min-height: max-content;
  padding-right: 4px;
}

.beeroom-node-workflow-step {
  display: grid;
  grid-template-columns: 8px minmax(0, 1fr);
  gap: 8px;
  align-items: center;
  min-width: 0;
}

.beeroom-node-workflow-step-dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: var(--node-accent);
  box-shadow: none;
}

.beeroom-node-workflow-step-text {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
}

.beeroom-node-workflow-step-main {
  font-size: 12px;
  font-weight: 600;
  line-height: 1.25;
  color: #f3f4f6;
  flex: 0 0 auto;
  white-space: nowrap;
}

.beeroom-node-card.is-subagent .beeroom-node-workflow-step-main {
  font-size: 11px;
}

.beeroom-node-workflow-step-detail,
.beeroom-node-workflow-empty {
  font-size: 11px;
  line-height: 1.25;
  color: rgba(148, 163, 184, 0.9);
}

.beeroom-node-workflow-step-detail {
  flex: 1 1 auto;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-workflow-step-detail::before {
  content: '·';
  margin-right: 6px;
  color: rgba(100, 116, 139, 0.92);
}

.beeroom-node-workflow-empty {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-artifact-container {
  flex: 1 1 auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  justify-content: flex-start;
  gap: 10px;
  padding: 4px 2px 2px;
  background: transparent;
}

.beeroom-node-artifact-container.is-embedded {
  padding: 0;
  gap: 8px;
}

.beeroom-node-workflow .beeroom-node-artifact-scroll {
  padding-right: 0;
}

.beeroom-node-artifact-topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 2px;
  flex: 0 0 auto;
}

.beeroom-node-artifact-title {
  min-width: 0;
  color: rgba(255, 247, 214, 0.94);
  font-size: 12px;
  font-weight: 700;
  letter-spacing: 0.08em;
}

.beeroom-node-artifact-count {
  flex: 0 0 auto;
  min-width: 24px;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid rgba(255, 236, 179, 0.18);
  background: rgba(255, 248, 235, 0.12);
  color: #fff8eb;
  font-size: 11px;
  font-weight: 700;
  line-height: 1.3;
  text-align: center;
  box-shadow: inset 0 1px 0 rgba(255, 251, 235, 0.12);
}

.beeroom-node-artifact-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
  overscroll-behavior: contain;
  touch-action: pan-y;
  scrollbar-width: thin;
  scrollbar-color: rgba(255, 236, 179, 0.28) transparent;
}

.beeroom-node-artifact-scroll::-webkit-scrollbar {
  width: 6px;
}

.beeroom-node-artifact-scroll::-webkit-scrollbar-track {
  background: transparent;
}

.beeroom-node-artifact-scroll::-webkit-scrollbar-thumb {
  border-radius: 999px;
  background: rgba(255, 236, 179, 0.24);
}

.beeroom-node-artifact-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 7px;
  width: 100%;
  min-height: 0;
  align-content: start;
  padding-bottom: 2px;
}

.beeroom-node-artifact-grid.is-showcase {
  grid-template-columns: minmax(0, 1fr);
  gap: 10px;
}

.beeroom-node-artifact-slot {
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: stretch;
  justify-content: stretch;
  aspect-ratio: 1 / 1;
  padding: 0;
  border-radius: 0;
  background: transparent;
  border: 0;
  cursor: default;
}

.beeroom-node-artifact-slot:not(.is-empty) {
  box-shadow: none;
}

.beeroom-node-artifact-slot.is-clickable {
  cursor: pointer;
}

.beeroom-node-artifact-slot-frame {
  width: 100%;
  height: 100%;
  border-radius: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: #fff7ed;
  background: rgba(255, 247, 214, 0.1);
  border: 1px solid rgba(253, 230, 138, 0.18);
  box-shadow: inset 0 1px 0 rgba(255, 251, 235, 0.1);
  font-size: 17px;
}

.beeroom-node-artifact-grid.is-showcase .beeroom-node-artifact-slot {
  aspect-ratio: auto;
  min-height: 58px;
}

.beeroom-node-artifact-grid.is-showcase .beeroom-node-artifact-slot-frame {
  min-height: 58px;
  justify-content: flex-start;
  gap: 12px;
  padding: 10px 12px;
  border-radius: 14px;
  text-align: left;
}

.beeroom-node-artifact-slot:not(.is-empty) .beeroom-node-artifact-slot-frame {
  box-shadow:
    inset 0 1px 0 rgba(255, 251, 235, 0.12),
    0 4px 10px rgba(15, 23, 42, 0.06);
}

.beeroom-node-artifact-slot-frame.is-dir {
  color: #fef3c7;
  background: rgba(245, 158, 11, 0.24);
  border-color: rgba(252, 211, 77, 0.28);
}

.beeroom-node-artifact-slot-frame.is-file {
  color: #ecfeff;
  background: rgba(45, 212, 191, 0.18);
  border-color: rgba(153, 246, 228, 0.24);
}

.beeroom-node-artifact-slot-frame.is-empty {
  color: rgba(255, 236, 179, 0.28);
  background: rgba(255, 248, 235, 0.08);
  border-color: rgba(255, 236, 179, 0.12);
  box-shadow: none;
}

.beeroom-node-artifact-slot-frame i {
  font-size: 16px;
}

.beeroom-node-artifact-grid.is-showcase .beeroom-node-artifact-slot-frame i {
  flex: 0 0 30px;
  font-size: 26px;
  text-align: center;
}

.beeroom-node-artifact-slot-copy {
  min-width: 0;
  display: grid;
  gap: 3px;
}

.beeroom-node-artifact-slot-label,
.beeroom-node-artifact-slot-meta {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.beeroom-node-artifact-slot-label {
  font-size: 12px;
  font-weight: 700;
  color: inherit;
}

.beeroom-node-artifact-slot-meta {
  font-size: 10px;
  color: rgba(241, 245, 249, 0.72);
}

.beeroom-node-artifact-slot.is-clickable .beeroom-node-artifact-slot-frame:hover,
.beeroom-node-artifact-slot.is-clickable .beeroom-node-artifact-slot-frame:focus-visible {
  transform: translateY(-1px);
  box-shadow:
    inset 0 1px 0 rgba(255, 251, 235, 0.16),
    0 8px 18px rgba(15, 23, 42, 0.14);
}

.beeroom-node-card.is-condensed {
  gap: 8px;
  padding-bottom: 10px;
}

.beeroom-node-card.is-condensed .beeroom-node-card-head {
  grid-template-columns: 38px minmax(0, 1fr) auto;
}

.beeroom-node-card.is-condensed .beeroom-node-avatar {
  width: 38px;
  height: 38px;
  border-radius: 10px;
}

.beeroom-node-card.is-condensed .beeroom-node-workflow {
  padding: 7px 10px;
}

.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-workflow,
.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-workflow-step-main,
.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-workflow-step-detail,
.beeroom-node-card.is-subagent.is-emphasis-dormant .beeroom-node-workflow-empty {
  color: rgba(148, 163, 184, 0.72);
}

.beeroom-node-card.is-revealing {
  animation: beeroom-subagent-bloom 640ms cubic-bezier(0.18, 0.9, 0.25, 1) both;
  animation-delay: var(--node-intro-delay, 0ms);
}

@keyframes beeroom-subagent-bloom {
  0% {
    opacity: 0;
    transform: translate(
        calc(var(--node-intro-x, 0px) * -0.26),
        calc(var(--node-intro-y, 0px) * -0.26)
      )
      scale(0.72);
  }

  70% {
    opacity: 1;
  }

  100% {
    opacity: 1;
    transform: translate(0, 0) scale(1);
  }
}

@keyframes beeroom-node-border-breathe {
  0%,
  100% {
    opacity: 0.42;
    transform: scale(0.991);
    border-color: var(--node-activity-border);
    box-shadow:
      0 0 0 1px var(--node-activity-border-soft),
      0 0 16px rgba(var(--node-accent-rgb), 0.16);
  }

  50% {
    opacity: 1;
    transform: scale(1.006);
    border-color: rgba(var(--node-accent-rgb), 0.82);
    box-shadow:
      0 0 0 1px rgba(var(--node-accent-rgb), 0.3),
      0 0 26px var(--node-activity-halo-strong);
  }
}

@keyframes beeroom-node-status-pulse {
  0%,
  100% {
    transform: scale(1);
    box-shadow: 0 0 0 2px rgba(var(--node-accent-rgb), 0.16);
  }

  50% {
    transform: scale(1.18);
    box-shadow:
      0 0 0 2px rgba(var(--node-accent-rgb), 0.2),
      0 0 12px rgba(var(--node-accent-rgb), 0.28);
  }
}

@media (prefers-reduced-motion: reduce) {
  .beeroom-node-card.is-live-activity::after,
  .beeroom-node-card.is-live-activity .beeroom-node-status-dot {
    animation: none;
  }
}

</style>
