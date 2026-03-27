<template>
  <button
    class="beeroom-node-card"
    :class="[
      `is-${node.status}`,
      { 'is-mother': node.role === 'mother', 'is-selected': node.selected, 'is-condensed': condensed }
    ]"
    :aria-label="`${node.name} ${node.roleLabel} ${node.statusLabel}`"
    type="button"
    :style="cardStyle"
    @pointerdown.stop="emit('pointerdown', $event)"
    @click.stop="emit('click')"
    @dblclick.stop="emit('dblclick')"
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
  </button>
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
  (event: 'pointerdown', value: PointerEvent): void;
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
  padding: 16px 16px 14px;
  border: 1px solid rgba(148, 163, 184, 0.28);
  border-radius: 24px;
  background:
    linear-gradient(180deg, rgba(15, 23, 42, 0.96), rgba(15, 23, 42, 0.9)),
    linear-gradient(135deg, rgba(59, 130, 246, 0.12), rgba(245, 158, 11, 0.08));
  color: rgba(241, 245, 249, 0.98);
  text-align: left;
  cursor: pointer;
  box-shadow: 0 20px 44px rgba(15, 23, 42, 0.22);
  transition:
    border-color 0.18s ease,
    box-shadow 0.18s ease,
    transform 0.18s ease;
}

.beeroom-node-card::before {
  content: '';
  position: absolute;
  inset: 1px;
  border-radius: 23px;
  border: 1px solid color-mix(in srgb, var(--node-accent) 35%, rgba(255, 255, 255, 0.08));
  opacity: 0.78;
  pointer-events: none;
}

.beeroom-node-card::after {
  content: '';
  position: absolute;
  inset: 0 auto auto 20px;
  width: 82px;
  height: 4px;
  border-radius: 999px;
  background: linear-gradient(90deg, var(--node-accent), rgba(255, 255, 255, 0.08));
  pointer-events: none;
}

.beeroom-node-card:hover,
.beeroom-node-card:focus-visible {
  border-color: rgba(96, 165, 250, 0.42);
  transform: translateY(-2px);
  box-shadow: 0 26px 52px rgba(15, 23, 42, 0.3);
  outline: none;
}

.beeroom-node-card.is-selected {
  border-color: rgba(96, 165, 250, 0.72);
  box-shadow:
    0 0 0 2px rgba(96, 165, 250, 0.16),
    0 24px 56px rgba(15, 23, 42, 0.34);
}

.beeroom-node-card.is-mother {
  background:
    linear-gradient(180deg, rgba(30, 41, 59, 0.98), rgba(15, 23, 42, 0.9)),
    linear-gradient(135deg, rgba(245, 158, 11, 0.15), rgba(59, 130, 246, 0.08));
}

.beeroom-node-card-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.beeroom-node-card-head {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
}

.beeroom-node-avatar {
  width: 50px;
  height: 50px;
  border-radius: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, color-mix(in srgb, var(--node-accent) 78%, #ffffff 22%), rgba(15, 23, 42, 0.86));
  color: #fff7ed;
  font-size: 17px;
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
  gap: 6px;
  min-width: 0;
}

.beeroom-node-title {
  font-size: 15px;
  font-weight: 700;
  color: rgba(248, 250, 252, 0.98);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-node-role-chip,
.beeroom-node-entry-flag {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  width: fit-content;
  padding: 4px 9px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.14);
  border: 1px solid rgba(148, 163, 184, 0.22);
  color: rgba(226, 232, 240, 0.92);
  font-size: 11px;
  letter-spacing: 0.04em;
}

.beeroom-node-status {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 6px 10px;
  border-radius: 999px;
  background: rgba(30, 41, 59, 0.82);
  border: 1px solid rgba(148, 163, 184, 0.2);
  color: rgba(226, 232, 240, 0.92);
  font-size: 12px;
}

.beeroom-node-status-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.92);
  box-shadow: 0 0 0 4px rgba(148, 163, 184, 0.18);
}

.beeroom-node-card.is-running .beeroom-node-status-dot,
.beeroom-node-card.is-queued .beeroom-node-status-dot,
.beeroom-node-card.is-awaiting_idle .beeroom-node-status-dot {
  background: rgba(34, 197, 94, 0.95);
  box-shadow: 0 0 0 4px rgba(34, 197, 94, 0.18);
}

.beeroom-node-card.is-failed .beeroom-node-status-dot,
.beeroom-node-card.is-error .beeroom-node-status-dot,
.beeroom-node-card.is-timeout .beeroom-node-status-dot,
.beeroom-node-card.is-cancelled .beeroom-node-status-dot {
  background: rgba(239, 68, 68, 0.95);
  box-shadow: 0 0 0 4px rgba(239, 68, 68, 0.18);
}

.beeroom-node-card.is-completed .beeroom-node-status-dot,
.beeroom-node-card.is-success .beeroom-node-status-dot {
  background: rgba(96, 165, 250, 0.96);
  box-shadow: 0 0 0 4px rgba(96, 165, 250, 0.18);
}

.beeroom-node-metrics {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.beeroom-node-metric {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 14px;
  background: rgba(15, 23, 42, 0.34);
  border: 1px solid rgba(148, 163, 184, 0.14);
  color: rgba(226, 232, 240, 0.86);
  font-size: 12px;
}

.beeroom-node-workflow {
  min-height: 54px;
  display: flex;
  align-items: stretch;
  border-radius: 18px;
  background: rgba(15, 23, 42, 0.36);
  border: 1px solid rgba(148, 163, 184, 0.14);
  padding: 10px 12px;
}

.beeroom-node-workflow.is-completed {
  border-color: rgba(96, 165, 250, 0.18);
}

.beeroom-node-workflow.is-failed {
  border-color: rgba(239, 68, 68, 0.18);
}

.beeroom-node-workflow.is-loading {
  border-color: rgba(34, 197, 94, 0.18);
}

.beeroom-node-workflow-steps {
  display: flex;
  flex-direction: column;
  gap: 8px;
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
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--node-accent) 78%, #ffffff 22%);
  margin-top: 5px;
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
  color: rgba(248, 250, 252, 0.96);
}

.beeroom-node-workflow-step-detail,
.beeroom-node-workflow-empty {
  font-size: 11px;
  color: rgba(191, 219, 254, 0.82);
}

.beeroom-node-card.is-condensed {
  gap: 8px;
}

.beeroom-node-card.is-condensed .beeroom-node-workflow {
  min-height: 42px;
}

.beeroom-node-card.is-condensed .beeroom-node-metrics {
  gap: 8px;
}

.beeroom-node-card.is-condensed .beeroom-node-metric {
  padding: 6px 9px;
}
</style>
