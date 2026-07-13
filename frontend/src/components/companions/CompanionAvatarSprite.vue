<template>
  <CompanionSprite
    v-if="spriteUrl"
    :source="spriteUrl"
    :state="spriteState"
    fit
    :paused="!shouldAnimate"
  />
</template>

<script setup lang="ts">
import { computed, watchEffect } from 'vue';

import CompanionSprite from '@/components/companions/CompanionSprite.vue';
import { useCompanionStore, type CompanionSpriteStateId } from '@/stores/companions';
import type { AgentAvatarIconConfig } from '@/utils/agentAvatar';
import {
  STATIC_COMPANION_AVATAR_STATE,
  resolveCompanionSpriteStateForRuntime
} from '@/utils/companionRuntimeState';

type AgentRuntimeState = 'idle' | 'running' | 'done' | 'pending' | 'error';

const props = withDefaults(
  defineProps<{
    icon: AgentAvatarIconConfig;
    state?: AgentRuntimeState;
    animated?: boolean;
  }>(),
  {
    state: 'idle',
    animated: false
  }
);

const companionStore = useCompanionStore();
void companionStore.hydrate().catch(() => undefined);

const companionId = computed(() => String(props.icon.id || props.icon.name || '').trim());
const companionScope = computed(() => props.icon.scope || 'global');
watchEffect(() => {
  if (companionScope.value !== 'global' || !companionId.value) {
    return;
  }
  void companionStore.ensureGlobalCompanion(companionId.value).catch(() => undefined);
});

const record = computed(() => companionStore.findCompanion(companionScope.value, companionId.value));
const spriteUrl = computed(() => record.value?.spritesheetDataUrl || record.value?.spritesheetUrl || '');
const runtimeState = computed<CompanionSpriteStateId>(() =>
  resolveCompanionSpriteStateForRuntime(props.state, { pendingState: 'review' })
);
const spriteState = computed<CompanionSpriteStateId>(() =>
  props.animated ? runtimeState.value : STATIC_COMPANION_AVATAR_STATE
);
const shouldAnimate = computed(() => props.animated && spriteState.value !== 'idle');
</script>
