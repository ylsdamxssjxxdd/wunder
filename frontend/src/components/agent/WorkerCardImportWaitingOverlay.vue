<template>
  <BeeroomPackWaitingOverlay
    :visible="visible"
    mode="import"
    :progress="progress"
    :target-name="displayTargetName"
    :custom-title="t('portal.agent.importWorkerCard')"
    :custom-phase-label="displayPhaseLabel"
    :custom-summary-label="displaySummaryLabel"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import BeeroomPackWaitingOverlay from '@/components/beeroom/BeeroomPackWaitingOverlay.vue';
import { useI18n } from '@/i18n';

type WorkerCardImportPhase = 'preparing' | 'creating' | 'refreshing';

const props = withDefaults(
  defineProps<{
    visible: boolean;
    phase?: WorkerCardImportPhase | string | null;
    progress?: number | null;
    targetName?: string | null;
    current?: number | null;
    total?: number | null;
  }>(),
  {
    phase: 'preparing',
    progress: 0,
    targetName: '',
    current: 0,
    total: 0
  }
);

const { t } = useI18n();

const normalizedPhase = computed<WorkerCardImportPhase>(() => {
  const raw = String(props.phase || '').trim().toLowerCase();
  if (raw === 'creating' || raw === 'refreshing') {
    return raw;
  }
  return 'preparing';
});

const displayTargetName = computed(() => {
  const targetName = String(props.targetName || '').trim();
  if (targetName) {
    return targetName;
  }
  return normalizedPhase.value === 'refreshing'
    ? t('portal.agent.workerCardImportWaiting.targetRefreshing')
    : t('common.loading');
});

const displayPhaseLabel = computed(() => {
  switch (normalizedPhase.value) {
    case 'creating':
      return t('portal.agent.workerCardImportWaiting.phase.creating');
    case 'refreshing':
      return t('portal.agent.workerCardImportWaiting.phase.refreshing');
    default:
      return t('portal.agent.workerCardImportWaiting.phase.preparing');
  }
});

const displaySummaryLabel = computed(() => {
  switch (normalizedPhase.value) {
    case 'creating': {
      const total = Math.max(0, Number(props.total || 0));
      const current = Math.max(0, Math.min(total || 0, Number(props.current || 0)));
      if (total > 0 && current > 0) {
        return t('portal.agent.workerCardImportWaiting.summary.creating', {
          current,
          total
        });
      }
      return t('portal.agent.workerCardImportWaiting.summary.preparing');
    }
    case 'refreshing':
      return t('portal.agent.workerCardImportWaiting.summary.refreshing');
    default:
      return t('portal.agent.workerCardImportWaiting.summary.preparing');
  }
});
</script>
