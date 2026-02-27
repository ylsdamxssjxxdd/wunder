<template>
  <span class="messenger-agent-avatar" :class="[sizeClass, stateClass]" :title="title">
    <i class="fa-solid fa-robot" aria-hidden="true"></i>
    <span class="messenger-agent-avatar-badge" aria-hidden="true"></span>
  </span>
</template>

<script setup lang="ts">
import { computed } from 'vue';

type AgentAvatarSize = 'sm' | 'md' | 'lg';
type AgentRuntimeState = 'idle' | 'running' | 'done' | 'error';

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
</script>

<style scoped>
.messenger-agent-avatar {
  --avatar-size: 36px;
  width: var(--avatar-size);
  height: var(--avatar-size);
  border-radius: 50%;
  border: 1px solid #dce9e5;
  background: #eef5f3;
  color: #4f7c73;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  position: relative;
  overflow: hidden;
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

.messenger-agent-avatar-badge {
  width: 9px;
  height: 9px;
  border-radius: 999px;
  border: 1px solid #ffffff;
  background: #9aa4b2;
  position: absolute;
  right: 0;
  bottom: 1px;
  box-sizing: border-box;
}

.messenger-agent-avatar.state-running {
  border-color: rgba(19, 152, 127, 0.44);
  background: #e7f6f2;
  color: #13886f;
}

.messenger-agent-avatar.state-running i {
  animation: agent-avatar-spin 1.2s linear infinite;
}

.messenger-agent-avatar.state-running::after {
  content: '';
  position: absolute;
  inset: -2px;
  border-radius: 50%;
  border: 2px solid rgba(19, 152, 127, 0.28);
  animation: agent-avatar-pulse 1.2s ease-in-out infinite;
}

.messenger-agent-avatar.state-running .messenger-agent-avatar-badge {
  background: #0fa087;
  animation: agent-avatar-badge-pulse 1.2s ease-in-out infinite;
}

.messenger-agent-avatar.state-done {
  border-color: rgba(35, 172, 118, 0.42);
  background: #e7f7ef;
  color: #1a9b68;
}

.messenger-agent-avatar.state-done .messenger-agent-avatar-badge {
  background: #23ac76;
}

.messenger-agent-avatar.state-error {
  border-color: rgba(193, 64, 83, 0.45);
  background: #fceef1;
  color: #c14053;
}

.messenger-agent-avatar.state-error .messenger-agent-avatar-badge {
  background: #c14053;
}

@keyframes agent-avatar-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
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

@keyframes agent-avatar-badge-pulse {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(1.2);
  }
}
</style>
