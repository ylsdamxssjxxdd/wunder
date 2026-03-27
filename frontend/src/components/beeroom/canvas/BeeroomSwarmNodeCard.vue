<template>
  <div
    class="beeroom-node-card"
    :class="[
      `is-${node.status}`,
      { 'is-mother': node.role === 'mother', 'is-selected': node.selected, 'is-condensed': condensed }
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

      <div class="beeroom-node-metrics">
        <span class="beeroom-node-metric">
          <i class="fa-solid fa-list-check" aria-hidden="true"></i>
          <b>{{ node.taskTotal }}</b>
        </span>
        <span class="beeroom-node-metric">
          <i class="fa-solid fa-layer-group" aria-hidden="true"></i>
          <b>{{ node.activeSessionTotal }}</b>
        </span>
        <span v-if="node.entryAgent" class="beeroom-node-entry-flag">ENTRY</span>
      </div>
    </div>

    <div class="beeroom-node-workflow" :class="[`is-${node.workflowTone}`, { 'is-empty': !visibleWorkflowLines.length }]">
      <div v-if="visibleWorkflowLines.length" class="beeroom-node-workflow-steps">
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
import { computed } from 'vue';

import type { SwarmProjectionNode } from './swarmCanvasModel';

const props = defineProps<{
  node: SwarmProjectionNode;
  condensed?: boolean;
  emptyLabel: string;
}>();

const emit = defineEmits<{
  (event: 'click'): void;
  (event: 'dblclick'): void;
}>();

const cardStyle = computed(() => ({
  '--node-accent': props.node.accentColor,
  width: `${props.node.width}px`,
  height: `${props.node.height}px`
}));

const visibleWorkflowLines = computed(() =>
  (Array.isArray(props.node.workflowLines) ? props.node.workflowLines : []).slice(
    0,
    props.condensed ? 1 : 2
  )
);
</script>

<style scoped>
.beeroom-node-card {
  position: absolute;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 10px;
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
  display: none;
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
  gap: 10px;
}

.beeroom-node-card-head {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
}

.beeroom-node-avatar {
  width: 42px;
  height: 42px;
  border-radius: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, var(--node-accent), rgba(15, 23, 42, 0.9));
  color: #f8fafc;
  font-size: 14px;
  font-weight: 700;
  overflow: hidden;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12);
}

.beeroom-node-avatar-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
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

.beeroom-node-role-chip,
.beeroom-node-entry-flag {
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

.beeroom-node-entry-flag {
  border-color: rgba(245, 158, 11, 0.3);
  color: #fde68a;
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

.beeroom-node-metrics {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.beeroom-node-metric {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 9px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  background: rgba(15, 23, 42, 0.42);
  color: rgba(226, 232, 240, 0.88);
  font-size: 11px;
}

.beeroom-node-workflow {
  min-height: 50px;
  display: flex;
  align-items: stretch;
  padding: 10px 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.14);
  background: rgba(15, 23, 42, 0.48);
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
  gap: 7px;
  width: 100%;
}

.beeroom-node-workflow-step {
  display: grid;
  grid-template-columns: 10px minmax(0, 1fr);
  gap: 10px;
  align-items: start;
  min-width: 0;
}

.beeroom-node-workflow-step-dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: var(--node-accent);
  margin-top: 5px;
  box-shadow: none;
}

.beeroom-node-workflow-step-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.beeroom-node-workflow-step-main,
.beeroom-node-workflow-step-detail,
.beeroom-node-workflow-empty {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-workflow-step-main {
  font-size: 12px;
  font-weight: 600;
  color: #f3f4f6;
}

.beeroom-node-workflow-step-detail,
.beeroom-node-workflow-empty {
  font-size: 11px;
  color: rgba(148, 163, 184, 0.9);
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
  min-height: 40px;
  padding: 8px 10px;
}
</style>
