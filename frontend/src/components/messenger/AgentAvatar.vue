<template>
  <span class="messenger-agent-avatar" :class="[sizeClass, stateClass]" :title="title">
    <i class="fa-solid fa-robot" aria-hidden="true"></i>
    <span class="messenger-agent-avatar-status" aria-hidden="true">
      <i :class="statusIconClass"></i>
    </span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';

type AgentAvatarSize = 'sm' | 'md' | 'lg';
type AgentRuntimeState = 'idle' | 'running' | 'done' | 'pending' | 'error';

const props = withDefaults(
  defineProps<{
    size?: AgentAvatarSize;
    state?: AgentRuntimeState;
    title?: string;
  }>(),
  {
    size: 'md',
    state: 'idle',
    title: ''
  }
);

const sizeClass = computed(() => `size-${props.size}`);
const stateClass = computed(() => `state-${props.state}`);
const statusIconClass = computed(() => {
  switch (props.state) {
    case 'running':
      return 'fa-solid fa-spinner fa-spin';
    case 'done':
      return 'fa-solid fa-check';
    case 'pending':
      return 'fa-solid fa-circle-question';
    case 'error':
      return 'fa-solid fa-triangle-exclamation';
    default:
      return 'fa-solid fa-pause';
  }
});
</script>

<style scoped>
.messenger-agent-avatar {
  --avatar-size: 36px;
  --agent-avatar-status-bg: rgba(var(--ui-accent-rgb), 0.2);
  --agent-avatar-status-color: var(--ui-accent-deep);
  --agent-avatar-status-ring: rgba(var(--ui-accent-rgb), 0.28);
  width: var(--avatar-size);
  height: var(--avatar-size);
  border-radius: 50%;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.26);
  background: var(--ui-accent-soft);
  color: var(--ui-accent-deep);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  position: relative;
  overflow: visible;
  flex-shrink: 0;
}

.messenger-agent-avatar.size-sm {
  --avatar-size: 34px;
}

.messenger-agent-avatar.size-md {
  --avatar-size: 36px;
}

.messenger-agent-avatar.size-lg {
  --avatar-size: 42px;
}

.messenger-agent-avatar i {
  font-size: calc(var(--avatar-size) * 0.46);
  line-height: 1;
}

.messenger-agent-avatar-status {
  width: calc(var(--avatar-size) * 0.46);
  min-width: 16px;
  max-width: 20px;
  height: calc(var(--avatar-size) * 0.46);
  min-height: 16px;
  max-height: 20px;
  border-radius: 999px;
  border: 2px solid #ffffff;
  background: var(--agent-avatar-status-bg);
  color: var(--agent-avatar-status-color);
  position: absolute;
  right: -3px;
  bottom: -3px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  transition: background-color 0.2s ease, color 0.2s ease;
  box-shadow:
    0 2px 5px rgba(15, 23, 42, 0.22),
    0 0 0 1px var(--agent-avatar-status-ring);
  z-index: 2;
}

.messenger-agent-avatar-status i {
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
}

.messenger-agent-avatar.state-running {
  border-color: rgba(var(--ui-accent-rgb), 0.56);
  background: var(--ui-accent-soft-4);
  color: var(--ui-accent-deep);
  --agent-avatar-status-bg: var(--ui-accent);
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(var(--ui-accent-rgb), 0.45);
}

.messenger-agent-avatar.state-running::after {
  content: '';
  position: absolute;
  inset: -1px;
  border-radius: 50%;
  border: 2px solid rgba(var(--ui-accent-rgb), 0.24);
  animation: agent-avatar-pulse 1.2s ease-in-out infinite;
}

.messenger-agent-avatar.state-done {
  border-color: rgba(56, 154, 108, 0.45);
  background: #eaf8ef;
  color: #2f9b6a;
  --agent-avatar-status-bg: #3ca976;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(56, 154, 108, 0.42);
}

.messenger-agent-avatar.state-pending {
  border-color: rgba(128, 108, 184, 0.45);
  background: #f4efff;
  color: #745cb7;
  --agent-avatar-status-bg: #8b6bd0;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(128, 108, 184, 0.42);
}

.messenger-agent-avatar.state-pending::after {
  content: '';
  position: absolute;
  inset: -1px;
  border-radius: 50%;
  border: 2px dashed rgba(128, 108, 184, 0.25);
  animation: agent-avatar-pulse 1.8s ease-in-out infinite;
}

.messenger-agent-avatar.state-error {
  border-color: rgba(193, 64, 83, 0.45);
  background: #fceef1;
  color: #c14053;
  --agent-avatar-status-bg: #cd4a60;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(193, 64, 83, 0.45);
}

@keyframes agent-avatar-pulse {
  0%,
  100% {
    opacity: 0.35;
    transform: scale(1);
  }
  50% {
    opacity: 0.72;
    transform: scale(1.06);
  }
}
</style>
