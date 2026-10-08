<template>
  <div class="messenger-welcome" data-testid="messenger-welcome">
    <div class="messenger-welcome-greeting">
      {{ t('messenger.welcome.greetingPrefix') }}
      <strong>{{ workspaceName }}</strong>
      {{ t('messenger.welcome.greetingSuffix') }}
    </div>
    <p class="messenger-welcome-hint">{{ t('messenger.welcome.hint') }}</p>
    <section class="messenger-welcome-card">
      <div class="messenger-welcome-card-title">{{ t('messenger.welcome.overviewTitle') }}</div>
      <div class="messenger-welcome-card-placeholder">
        <i class="fa-regular fa-folder-open" aria-hidden="true"></i>
        <span>{{ t('messenger.welcome.overviewPlaceholder') }}</span>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { MessengerControllerContext } from '../controller/messengerControllerContext';
import { workspaceDisplayNameOverride } from '@/views/messenger/workspaceDisplayName';

const props = defineProps<{ controller: MessengerControllerContext }>();
const t = props.controller.t;
const workspaceName = computed(
  () => workspaceDisplayNameOverride.value || t('messenger.workspace.defaultName')
);
</script>
