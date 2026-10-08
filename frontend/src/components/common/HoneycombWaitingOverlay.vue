<template>
  <Teleport to="body" :disabled="!teleportToBody">
    <Transition name="honeycomb-waiting-overlay-fade">
      <div
        v-if="visible"
        class="honeycomb-waiting-overlay"
        :class="{ 'honeycomb-waiting-overlay--contained': !teleportToBody }"
        role="status"
        aria-live="polite"
        aria-busy="true"
      >
        <div class="honeycomb-waiting-overlay__panel">
          <div class="honeycomb-waiting-loader" aria-hidden="true">
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
            <span class="honeycomb-waiting-loader__cell"></span>
          </div>

          <div class="honeycomb-waiting-overlay__title">{{ title }}</div>
          <div class="honeycomb-waiting-overlay__target">{{ displayTarget }}</div>

          <div class="honeycomb-waiting-overlay__progress">
            <div
              class="honeycomb-waiting-overlay__progress-fill"
              :style="{ width: `${displayProgress}%` }"
            ></div>
          </div>
          <div class="honeycomb-waiting-overlay__meta">
            <span>{{ displayPhaseLabel }}</span>
            <span>{{ progressText }}</span>
          </div>

          <div class="honeycomb-waiting-overlay__summary">{{ displaySummaryLabel }}</div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    visible: boolean;
    title: string;
    targetName?: string | null;
    phaseLabel?: string | null;
    summaryLabel?: string | null;
    progress?: number | null;
    teleportToBody?: boolean;
  }>(),
  {
    targetName: '',
    phaseLabel: '',
    summaryLabel: '',
    progress: 0,
    teleportToBody: true
  }
);

const { t } = useI18n();

const displayTarget = computed(() => String(props.targetName || '').trim() || t('common.loading'));
const displayPhaseLabel = computed(() => String(props.phaseLabel || '').trim() || t('common.loading'));
const displaySummaryLabel = computed(() => String(props.summaryLabel || '').trim() || t('common.loading'));

const displayProgress = computed(() => {
  const raw = Number(props.progress);
  if (!Number.isFinite(raw)) return 0;
  return Math.max(0, Math.min(100, Math.round(raw)));
});

const progressText = computed(() => `${displayProgress.value}%`);
</script>

<style scoped>
.honeycomb-waiting-overlay {
  position: fixed;
  inset: 0;
  z-index: 2400;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background:
    radial-gradient(circle at 20% 20%, rgba(255, 163, 76, 0.16), rgba(8, 14, 26, 0) 46%),
    radial-gradient(circle at 84% 76%, rgba(41, 161, 156, 0.18), rgba(6, 13, 24, 0) 44%),
    rgba(8, 12, 21, 0.86);
}

.honeycomb-waiting-overlay--contained {
  position: absolute;
  z-index: 48;
  border-radius: inherit;
}

.honeycomb-waiting-overlay__panel {
  width: min(100%, 420px);
  padding: 24px;
  border-radius: 18px;
  border: 1px solid rgba(255, 188, 92, 0.45);
  background: linear-gradient(165deg, rgba(19, 30, 46, 0.98), rgba(10, 17, 31, 0.98));
  box-shadow: 0 28px 80px rgba(0, 0, 0, 0.44);
}

.honeycomb-waiting-loader {
  position: relative;
  width: 152px;
  height: 128px;
  margin: 0 auto 12px;
}

.honeycomb-waiting-loader__cell {
  position: absolute;
  width: 40px;
  height: 46px;
  clip-path: polygon(50% 0%, 100% 25%, 100% 75%, 50% 100%, 0% 75%, 0% 25%);
  background: linear-gradient(150deg, #ffcc6a 0%, #ff9157 52%, #ff6d42 100%);
  box-shadow: 0 6px 22px rgba(255, 127, 58, 0.28);
  opacity: 0.38;
  animation: honeycomb-waiting-hive-pulse 1.75s ease-in-out infinite;
}

.honeycomb-waiting-loader__cell:nth-child(1) {
  left: 56px;
  top: 40px;
  animation-delay: 0.02s;
}

.honeycomb-waiting-loader__cell:nth-child(2) {
  left: 56px;
  top: 2px;
  animation-delay: 0.18s;
}

.honeycomb-waiting-loader__cell:nth-child(3) {
  left: 88px;
  top: 22px;
  animation-delay: 0.34s;
}

.honeycomb-waiting-loader__cell:nth-child(4) {
  left: 88px;
  top: 58px;
  animation-delay: 0.5s;
}

.honeycomb-waiting-loader__cell:nth-child(5) {
  left: 56px;
  top: 78px;
  animation-delay: 0.66s;
}

.honeycomb-waiting-loader__cell:nth-child(6) {
  left: 24px;
  top: 58px;
  animation-delay: 0.82s;
}

.honeycomb-waiting-loader__cell:nth-child(7) {
  left: 24px;
  top: 22px;
  animation-delay: 0.98s;
}

.honeycomb-waiting-overlay__title {
  margin-top: 4px;
  color: #f5f8ff;
  font-size: 16px;
  font-weight: 700;
  text-align: center;
}

.honeycomb-waiting-overlay__target {
  margin-top: 8px;
  color: rgba(226, 236, 255, 0.76);
  font-size: 13px;
  text-align: center;
  word-break: break-word;
}

.honeycomb-waiting-overlay__progress {
  position: relative;
  height: 8px;
  margin-top: 16px;
  border-radius: 999px;
  background: rgba(146, 169, 210, 0.26);
  overflow: hidden;
}

.honeycomb-waiting-overlay__progress-fill {
  position: relative;
  height: 100%;
  min-width: 6px;
  border-radius: inherit;
  background: linear-gradient(90deg, #ffbd56 0%, #ff8f47 44%, #ff6840 100%);
  transition: width 280ms ease;
}

.honeycomb-waiting-overlay__progress-fill::after {
  content: '';
  position: absolute;
  top: 0;
  right: -16px;
  width: 24px;
  height: 100%;
  background: linear-gradient(90deg, rgba(255, 255, 255, 0), rgba(255, 249, 235, 0.72));
  animation: honeycomb-waiting-progress-glow 1.3s linear infinite;
}

.honeycomb-waiting-overlay__meta {
  display: flex;
  justify-content: space-between;
  margin-top: 8px;
  color: rgba(226, 236, 255, 0.76);
  font-size: 12px;
}

.honeycomb-waiting-overlay__summary {
  margin-top: 10px;
  color: rgba(226, 236, 255, 0.64);
  font-size: 12px;
  line-height: 1.45;
  min-height: 18px;
  text-align: center;
}

.honeycomb-waiting-overlay-fade-enter-active,
.honeycomb-waiting-overlay-fade-leave-active {
  transition: opacity 160ms ease;
}

.honeycomb-waiting-overlay-fade-enter-from,
.honeycomb-waiting-overlay-fade-leave-to {
  opacity: 0;
}

@keyframes honeycomb-waiting-hive-pulse {
  0%,
  100% {
    opacity: 0.34;
    transform: translateY(0) scale(0.95);
  }
  45% {
    opacity: 1;
    transform: translateY(-2px) scale(1.04);
  }
}

@keyframes honeycomb-waiting-progress-glow {
  0% {
    transform: translateX(-8px);
    opacity: 0.2;
  }
  100% {
    transform: translateX(20px);
    opacity: 0.92;
  }
}

@media (max-width: 640px) {
  .honeycomb-waiting-overlay {
    padding: 18px;
  }

  .honeycomb-waiting-overlay__panel {
    padding: 20px 18px;
  }
}
</style>
