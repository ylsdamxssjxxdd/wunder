<template>
  <div
    class="messenger-settings-frame-category"
    data-testid="settings-category-help"
  >
    <section class="messenger-settings-card messenger-settings-card--manual">
      <div class="messenger-settings-group-head">
        <div class="messenger-settings-title">{{ t('messenger.settings.helpManual') }}</div>
        <div class="messenger-settings-subtitle">{{ t('messenger.settings.helpManualHint') }}</div>
      </div>
      <div class="messenger-settings-page-manual">
        <MessengerHelpManualPanel @loading-change="handleHelpManualLoadingChange" />
      </div>
    </section>

    <HoneycombWaitingOverlay
      :visible="Boolean(showHelpManualWaitingOverlay?.value)"
      :title="t('messenger.waiting.title')"
      :target-name="t('messenger.settings.helpManual')"
      :phase-label="t('messenger.waiting.phase.loading')"
      :summary-label="t('messenger.waiting.summary.helpManual')"
      :progress="42"
      :teleport-to-body="false"
    />
  </div>
</template>

<script setup lang="ts">
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import { useI18n } from '@/i18n';

const props = defineProps<{ controller: MessengerControllerContext }>();
const { t } = useI18n();


const handleHelpManualLoadingChange = props.controller.handleHelpManualLoadingChange;
const showHelpManualWaitingOverlay = props.controller.showHelpManualWaitingOverlay;
const MessengerHelpManualPanel = props.controller.MessengerHelpManualPanel;
const HoneycombWaitingOverlay = props.controller.HoneycombWaitingOverlay;
</script>
