<template>
  <div
    class="beeroom-node-card"
    :class="[
      `is-${node.status}`,
      `is-${node.role}`,
      `is-emphasis-${node.emphasis}`,
      {
        'is-mother': node.role === 'mother',
        'is-selected': node.selected,
        'is-condensed': condensed,
        'is-revealing': !!reveal
      }
    ]"
    :aria-label="`${node.name} ${node.roleLabel} ${node.statusLabel}`"
    :data-node-id="node.id"
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

    <div class="beeroom-node-workflow" :class="[`is-${node.workflowTone}`, { 'is-empty': !visibleWorkflowLines.length }]">
      <div v-if="visibleWorkflowLines.length" ref="workflowStepsRef" class="beeroom-node-workflow-steps">
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
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

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
}>();

const cardStyle = computed(() => ({
  '--node-accent': props.node.accentColor,
  '--node-intro-x': `${Math.round(props.reveal?.offsetX || 0)}px`,
  '--node-intro-y': `${Math.round(props.reveal?.offsetY || 0)}px`,
  '--node-intro-delay': `${Math.max(0, Number(props.reveal?.order || 0)) * 70}ms`,
  width: `${props.node.width}px`,
  height: `${props.node.height}px`
}));

const visibleWorkflowLines = computed(() =>
  (Array.isArray(props.node.workflowLines) ? props.node.workflowLines : [])
);

const workflowStepsRef = ref<HTMLElement | null>(null);

const workflowLineSignature = computed(() =>
  visibleWorkflowLines.value
    .map((line) => `${line.key}:${line.main}:${line.detail}:${line.title}`)
    .join('||')
);

const shouldFollowWorkflowTail = computed(
  () => props.node.workflowTone === 'loading' || props.node.workflowTone === 'pending'
);

const scrollWorkflowToBottom = () => {
  const element = workflowStepsRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

watch(
  [workflowLineSignature, shouldFollowWorkflowTail],
  async ([, shouldFollow]) => {
    if (!shouldFollow || !visibleWorkflowLines.value.length) return;
    await nextTick();
    if (typeof window !== 'undefined') {
      window.requestAnimationFrame(scrollWorkflowToBottom);
      return;
    }
    scrollWorkflowToBottom();
  },
  {
    flush: 'post',
    immediate: true
  }
);
</script>

<style scoped>
.beeroom-node-card {
  --node-activity-glow: rgba(0, 0, 0, 0);
  --node-activity-sheen: rgba(255, 255, 255, 0);
  position: absolute;
  display: flex;
  flex-direction: column;
  justify-content: flex-start;
  gap: 8px;
  padding: 14px 14px 12px 16px;
  border: 1px solid rgba(148, 163, 184, 0.22);
  border-radius: 18px;
  background: linear-gradient(180deg, rgba(18, 22, 31, 0.98), rgba(12, 15, 22, 0.98));
  color: #e5e7eb;
  text-align: left;
  cursor: grab;
  overflow: hidden;
  user-select: none;
  -webkit-user-select: none;
  -webkit-user-drag: none;
  touch-action: none;
  box-shadow: 0 10px 22px rgba(2, 6, 23, 0.18);
  -webkit-font-smoothing: antialiased;
  text-rendering: geometricPrecision;
  transition:
    border-color 0.18s ease,
    box-shadow 0.18s ease,
    transform 0.18s ease;
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
  inset: -36% -18%;
  background: linear-gradient(
    115deg,
    transparent 0 34%,
    rgba(255, 255, 255, 0.02) 42%,
    var(--node-activity-sheen) 50%,
    rgba(255, 255, 255, 0.02) 58%,
    transparent 68% 100%
  );
  opacity: 0;
  transform: translate3d(-54%, 0, 0) rotate(8deg);
  pointer-events: none;
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
  border-color: rgba(245, 158, 11, 0.3);
  background: linear-gradient(180deg, rgba(26, 33, 45, 0.98), rgba(12, 15, 22, 0.98));
}

.beeroom-node-card.is-subagent {
  gap: 7px;
  padding: 12px 12px 10px 14px;
  border-radius: 16px;
  background: linear-gradient(180deg, rgba(12, 18, 28, 0.96), rgba(8, 13, 22, 0.97));
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
  background: linear-gradient(180deg, rgba(18, 24, 34, 0.94), rgba(12, 16, 24, 0.96));
  box-shadow: 0 8px 16px rgba(2, 6, 23, 0.1);
}

.beeroom-node-card.is-running,
.beeroom-node-card.is-queued,
.beeroom-node-card.is-awaiting_idle {
  --node-activity-glow: rgba(248, 113, 113, 0.18);
  --node-activity-sheen: rgba(248, 113, 113, 0.18);
  box-shadow:
    0 0 0 1px rgba(248, 113, 113, 0.08),
    0 14px 28px rgba(127, 29, 29, 0.2),
    0 0 24px var(--node-activity-glow);
}

.beeroom-node-card.is-mother.is-running,
.beeroom-node-card.is-mother.is-queued,
.beeroom-node-card.is-mother.is-awaiting_idle {
  --node-activity-glow: rgba(245, 158, 11, 0.18);
  --node-activity-sheen: rgba(251, 191, 36, 0.22);
}

.beeroom-node-card.is-subagent.is-emphasis-active.is-running,
.beeroom-node-card.is-subagent.is-emphasis-active.is-queued,
.beeroom-node-card.is-subagent.is-emphasis-active.is-awaiting_idle {
  --node-activity-glow: rgba(34, 211, 238, 0.18);
  --node-activity-sheen: rgba(34, 211, 238, 0.2);
}

.beeroom-node-card.is-running::after,
.beeroom-node-card.is-queued::after,
.beeroom-node-card.is-awaiting_idle::after {
  opacity: 0.92;
  animation: beeroom-node-activity-sheen 1.9s linear infinite;
}

.beeroom-node-card.is-running .beeroom-node-avatar,
.beeroom-node-card.is-queued .beeroom-node-avatar,
.beeroom-node-card.is-awaiting_idle .beeroom-node-avatar {
  animation: beeroom-node-avatar-breathe 1.65s ease-in-out infinite;
}

.beeroom-node-card.is-running .beeroom-node-status-dot,
.beeroom-node-card.is-queued .beeroom-node-status-dot,
.beeroom-node-card.is-awaiting_idle .beeroom-node-status-dot {
  animation: beeroom-node-status-pulse 1.02s ease-in-out infinite;
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
  border: 1px solid rgba(74, 222, 128, 0.24);
  background:
    radial-gradient(
      circle at center,
      rgba(74, 222, 128, 0.28) 0 18%,
      rgba(16, 185, 129, 0.15) 18% 34%,
      rgba(5, 150, 105, 0.08) 34% 52%,
      rgba(5, 12, 18, 0.94) 52% 100%
    ),
    linear-gradient(135deg, rgba(34, 197, 94, 0.22), rgba(6, 78, 59, 0.92));
  color: #f8fafc;
  font-size: 14px;
  font-weight: 700;
  overflow: hidden;
  box-shadow:
    inset 0 0 0 1px rgba(187, 247, 208, 0.1),
    0 8px 18px rgba(6, 78, 59, 0.18);
}

.beeroom-node-card.is-subagent .beeroom-node-avatar {
  width: 36px;
  height: 36px;
  border-radius: 10px;
  border-color: rgba(56, 189, 248, 0.2);
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
    repeating-radial-gradient(circle at center, rgba(74, 222, 128, 0.16) 0 1px, transparent 1px 8px),
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
    rgba(16, 185, 129, 0.06) 78%,
    rgba(110, 231, 183, 0.34) 84%,
    rgba(16, 185, 129, 0.16) 90%,
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
  text-shadow: 0 0 12px rgba(110, 231, 183, 0.26);
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

@keyframes beeroom-node-activity-sheen {
  0% {
    transform: translate3d(-54%, 0, 0) rotate(8deg);
  }

  100% {
    transform: translate3d(54%, 0, 0) rotate(8deg);
  }
}

@keyframes beeroom-node-avatar-breathe {
  0%,
  100% {
    transform: translateY(0) scale(1);
    filter: saturate(1) brightness(1);
  }

  50% {
    transform: translateY(-1px) scale(1.04);
    filter: saturate(1.08) brightness(1.04);
  }
}

@keyframes beeroom-node-status-pulse {
  0%,
  100% {
    transform: scale(1);
    opacity: 1;
  }

  45% {
    transform: scale(1.32);
    opacity: 0.82;
  }
}

@media (prefers-reduced-motion: reduce) {
  .beeroom-node-card.is-running::after,
  .beeroom-node-card.is-queued::after,
  .beeroom-node-card.is-awaiting_idle::after,
  .beeroom-node-card.is-running .beeroom-node-avatar,
  .beeroom-node-card.is-queued .beeroom-node-avatar,
  .beeroom-node-card.is-awaiting_idle .beeroom-node-avatar,
  .beeroom-node-card.is-running .beeroom-node-status-dot,
  .beeroom-node-card.is-queued .beeroom-node-status-dot,
  .beeroom-node-card.is-awaiting_idle .beeroom-node-status-dot {
    animation: none;
  }
}

</style>
