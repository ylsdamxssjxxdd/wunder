<template>
  <HoneycombWaitingOverlay
    :visible="visible"
    :title="actionTitle"
    :target-name="targetName"
    :phase-label="phaseLabel"
    :summary-label="summaryLabel"
    :progress="displayProgress"
  />
</template>

<script setup lang="ts">
import { computed } from 'vue';

import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import { useI18n } from '@/i18n';

type PackOverlayMode = 'import' | 'export';

const props = withDefaults(
  defineProps<{
    visible: boolean;
    mode: PackOverlayMode;
    phase?: string | null;
    progress?: number | null;
    summary?: string | null;
    targetName?: string | null;
    customTitle?: string | null;
    customPhaseLabel?: string | null;
    customSummaryLabel?: string | null;
  }>(),
  {
    phase: '',
    progress: null,
    summary: '',
    targetName: '',
    customTitle: '',
    customPhaseLabel: '',
    customSummaryLabel: ''
  }
);

const { t } = useI18n();

const normalizePhaseToken = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return '';
  return raw.replace(/[-_\s]+([a-z0-9])/g, (_, char: string) => char.toUpperCase());
};

const resolveText = (key: string): string => {
  const translated = t(key);
  return translated === key ? '' : translated;
};

const actionTitle = computed(
  () =>
    String(props.customTitle || '').trim() ||
    (props.mode === 'import' ? t('beeroom.pack.action.import') : t('beeroom.pack.action.exportFull'))
);

const targetName = computed(() => String(props.targetName || '').trim());

const displayProgress = computed(() => {
  const raw = Number(props.progress);
  if (!Number.isFinite(raw)) return 0;
  return Math.max(0, Math.min(100, Math.round(raw)));
});

const normalizedPhase = computed(() => normalizePhaseToken(props.phase) || 'unknown');

const phaseLabel = computed(
  () =>
    String(props.customPhaseLabel || '').trim() ||
    resolveText(`beeroom.pack.phase.${normalizedPhase.value}`) ||
    String(props.phase || '').trim() ||
    t('beeroom.pack.phase.unknown')
);

const summaryLabel = computed(() => {
  const customSummary = String(props.customSummaryLabel || '').trim();
  if (customSummary) {
    return customSummary;
  }
  const detailedKey = `beeroom.pack.wait.${props.mode}.${normalizedPhase.value}`;
  const detailed = resolveText(detailedKey);
  if (detailed) {
    return detailed;
  }
  const summary = String(props.summary || '').trim();
  if (summary) {
    return summary;
  }
  return props.mode === 'import'
    ? t('beeroom.pack.message.importPending')
    : t('beeroom.pack.message.exportPending');
});
</script>
