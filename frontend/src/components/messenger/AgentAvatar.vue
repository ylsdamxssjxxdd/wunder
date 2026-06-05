<template>
  <span class="messenger-agent-avatar" :class="[sizeClass, stateClass, motionClass]" :title="title">
    <span class="messenger-agent-avatar-image-shell" :style="avatarFaceStyle" aria-hidden="true">
      <CompanionSprite
        v-if="companionSpriteUrl"
        class="messenger-agent-avatar-sprite"
        :source="companionSpriteUrl"
        :state="companionSpriteState"
        :scale="companionSpriteScale"
        fit
        :paused="!shouldAnimateCompanionSprite"
      />
      <img v-else-if="avatarImageUrl" class="messenger-agent-avatar-image" :src="avatarImageUrl" alt="" />
      <span v-else class="messenger-agent-avatar-initial">{{ avatarInitial }}</span>
    </span>
    <span class="messenger-agent-avatar-status" aria-hidden="true">
      <span
        v-if="showRunningSpinner"
        class="messenger-agent-avatar-status-dot-spinner messenger-agent-avatar-status-icon--spinning"
      ></span>
      <i v-else :class="statusIconClass"></i>
    </span>
  </span>
</template>

<script setup lang="ts">
import { computed, watchEffect } from 'vue';

import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import { useCompanionStore } from '@/stores/companions';
import type { CompanionSpriteStateId } from '@/stores/companions';
import {
  parseAgentAvatarIconConfig,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';
import { resolveCompanionSpriteStateForRuntime } from '@/utils/companionRuntimeState';

type AgentAvatarSize = 'sm' | 'md' | 'lg';
type AgentRuntimeState = 'idle' | 'running' | 'done' | 'pending' | 'error';

const props = withDefaults(
  defineProps<{
    size?: AgentAvatarSize;
    state?: AgentRuntimeState;
    title?: string;
    icon?: unknown;
    imageUrl?: string;
    name?: string;
    animated?: boolean;
  }>(),
  {
    size: 'md',
    state: 'idle',
    title: '',
    imageUrl: '',
    name: '',
    animated: false
  }
);

const sizeClass = computed(() => `size-${props.size}`);
const stateClass = computed(() => `state-${props.state}`);
const motionClass = computed(() => (props.animated ? 'is-motion-enabled' : 'is-motion-static'));
const showRunningSpinner = computed(() => props.state === 'running');
const avatarConfig = computed(() => parseAgentAvatarIconConfig(props.icon));
const companionStore = useCompanionStore();
void companionStore.hydrate().catch(() => undefined);
watchEffect(() => {
  if (avatarConfig.value.kind !== 'companion') {
    return;
  }
  const scope = avatarConfig.value.scope || 'global';
  const id = avatarConfig.value.id || avatarConfig.value.name;
  if (scope !== 'global' || !String(id || '').trim()) {
    return;
  }
  void companionStore.ensureGlobalCompanion(String(id || '').trim()).catch(() => undefined);
});
const companionRecord = computed(() =>
  avatarConfig.value.kind === 'companion'
    ? companionStore.findCompanion(avatarConfig.value.scope || 'global', avatarConfig.value.id || avatarConfig.value.name)
    : null
);
const companionSpriteUrl = computed(() => companionRecord.value?.spritesheetDataUrl || companionRecord.value?.spritesheetUrl || '');
const companionSpriteState = computed<CompanionSpriteStateId>(() =>
  resolveCompanionSpriteStateForRuntime(props.state, {
    pendingState: 'review'
  })
);
const shouldAnimateCompanionSprite = computed(() => props.animated && companionSpriteState.value !== 'idle');
// The companion display scale is only for the floating character layer.
// Agent avatars should stay visually stable inside message/list UI.
const companionSpriteScale = computed(() => 1);
const avatarImageUrl = computed(
  () =>
    String(props.imageUrl || '').trim() ||
    (avatarConfig.value.kind === 'companion' ? '' : resolveAgentAvatarImageByConfig(avatarConfig.value))
);
const avatarInitial = computed(() => resolveAgentAvatarInitial(props.name || props.title));
const avatarFaceStyle = computed(() => ({
  background: avatarImageUrl.value || companionSpriteUrl.value ? 'transparent' : avatarConfig.value.color
}));
const statusIconClass = computed(() => {
  switch (props.state) {
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
  --agent-avatar-bg: var(--ui-accent-soft);
  --agent-avatar-border: rgba(var(--ui-accent-rgb), 0.26);
  --agent-avatar-color: var(--ui-accent-deep);
  --agent-avatar-status-bg: #f3f4f6;
  --agent-avatar-status-color: #6b7280;
  --agent-avatar-status-ring: rgba(107, 114, 128, 0.28);
  width: var(--avatar-size);
  height: var(--avatar-size);
  border-radius: 50%;
  border: 1px solid var(--agent-avatar-border);
  background: var(--agent-avatar-bg);
  color: var(--agent-avatar-color);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  position: relative;
  overflow: visible;
  flex-shrink: 0;
  isolation: isolate;
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

.messenger-agent-avatar-image-shell {
  width: 100%;
  height: 100%;
  border-radius: inherit;
  overflow: hidden;
  display: block;
  background: #ffffff;
  position: relative;
}

.messenger-agent-avatar-image {
  width: 100%;
  height: 100%;
  display: block;
  object-fit: cover;
}

.messenger-agent-avatar-sprite {
  position: absolute;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
}

.messenger-agent-avatar-initial {
  width: 100%;
  height: 100%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  color: #ffffff;
  font-size: 14px;
  font-weight: 700;
  line-height: 1;
  text-transform: uppercase;
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
  background: var(--agent-avatar-status-bg) !important;
  color: var(--agent-avatar-status-color) !important;
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
  mix-blend-mode: normal;
}

.messenger-agent-avatar-status i {
  font-size: 10px;
  font-weight: 700;
  line-height: 1;
  display: inline-block;
  transform-origin: center center;
  color: inherit !important;
}

.messenger-agent-avatar-status-dot-spinner {
  width: 10px;
  height: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  position: relative;
  flex-shrink: 0;
  transform-origin: center center;
}

.messenger-agent-avatar-status-dot-spinner::before {
  content: '';
  position: absolute;
  top: 50%;
  left: 50%;
  width: 1.7px;
  height: 1.7px;
  margin-top: -0.85px;
  margin-left: -0.85px;
  border-radius: 50%;
  background: transparent;
  box-shadow:
    0 -4.1px 0 0.85px currentColor,
    3.55px -2.05px 0 0.85px currentColor,
    3.55px 2.05px 0 0.85px currentColor,
    0 4.1px 0 0.85px currentColor,
    -3.55px 2.05px 0 0.85px currentColor,
    -3.55px -2.05px 0 0.85px currentColor;
}

.messenger-agent-avatar-status .messenger-agent-avatar-status-icon--spinning {
  animation: messenger-agent-avatar-spin 1.48s linear infinite;
}

.messenger-agent-avatar.state-idle {
  --agent-avatar-status-bg: #f3f4f6;
  --agent-avatar-status-color: #6b7280;
  --agent-avatar-status-ring: rgba(107, 114, 128, 0.28);
}

.messenger-agent-avatar.state-running {
  --agent-avatar-status-bg: var(--ui-accent);
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(var(--ui-accent-rgb), 0.45);
}

.messenger-agent-avatar.state-done {
  --agent-avatar-status-bg: #3ca976;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(56, 154, 108, 0.42);
}

.messenger-agent-avatar.state-pending {
  --agent-avatar-status-bg: #8b6bd0;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(128, 108, 184, 0.42);
}

.messenger-agent-avatar.state-error {
  --agent-avatar-status-bg: #cd4a60;
  --agent-avatar-status-color: #ffffff;
  --agent-avatar-status-ring: rgba(193, 64, 83, 0.45);
}

.messenger-agent-avatar.is-motion-enabled.state-running .messenger-agent-avatar-image-shell {
  animation: messenger-agent-avatar-work 1.42s ease-in-out infinite;
}

.messenger-agent-avatar.is-motion-enabled.state-pending .messenger-agent-avatar-image-shell {
  animation: messenger-agent-avatar-review 1.58s ease-in-out infinite;
}

.messenger-agent-avatar.is-motion-enabled.state-done .messenger-agent-avatar-image-shell {
  animation: messenger-agent-avatar-done 760ms cubic-bezier(0.2, 0.9, 0.24, 1.16) both;
}

.messenger-agent-avatar.is-motion-enabled.state-error .messenger-agent-avatar-image-shell {
  animation: messenger-agent-avatar-error 620ms cubic-bezier(0.36, 0, 0.66, -0.56) both;
}

@keyframes messenger-agent-avatar-work {
  0%,
  100% {
    transform: translateY(0) scale(1);
  }
  45% {
    transform: translateY(-1px) scale(1.035);
  }
}

@keyframes messenger-agent-avatar-review {
  0%,
  100% {
    transform: rotate(0deg) scale(1);
  }
  40% {
    transform: rotate(-2deg) scale(1.02);
  }
  70% {
    transform: rotate(2deg) scale(1.02);
  }
}

@keyframes messenger-agent-avatar-done {
  0% {
    transform: translateY(0) scale(0.98);
  }
  48% {
    transform: translateY(-2px) scale(1.08);
  }
  100% {
    transform: translateY(0) scale(1);
  }
}

@keyframes messenger-agent-avatar-error {
  0%,
  100% {
    transform: translateX(0);
  }
  25% {
    transform: translateX(-1.5px);
  }
  50% {
    transform: translateX(1.5px);
  }
  75% {
    transform: translateX(-1px);
  }
}

@media (prefers-reduced-motion: reduce) {
  .messenger-agent-avatar.is-motion-enabled.state-running .messenger-agent-avatar-image-shell,
  .messenger-agent-avatar.is-motion-enabled.state-pending .messenger-agent-avatar-image-shell,
  .messenger-agent-avatar.is-motion-enabled.state-done .messenger-agent-avatar-image-shell,
  .messenger-agent-avatar.is-motion-enabled.state-error .messenger-agent-avatar-image-shell,
  .messenger-agent-avatar-status .messenger-agent-avatar-status-icon--spinning {
    animation: none;
  }
}
</style>
