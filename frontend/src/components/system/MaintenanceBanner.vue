<template>
  <transition name="maintenance-banner-fade">
    <div v-if="active" class="maintenance-banner" role="status" aria-live="polite">
      <span class="maintenance-banner-icon" aria-hidden="true">
        <i class="fa-solid" :class="offline ? 'fa-link-slash' : 'fa-triangle-exclamation'"></i>
      </span>
      <div class="maintenance-banner-text">
        <span class="maintenance-banner-title">{{ title }}</span>
        <span class="maintenance-banner-desc">{{ desc }}</span>
      </div>
      <span v-if="statusText" class="maintenance-banner-meta">{{ statusText }}</span>
      <el-button
        class="maintenance-banner-refresh"
        size="small"
        text
        bg
        @click="handleRefresh"
      >
        {{ t('common.refresh') }}
      </el-button>
    </div>
  </transition>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from '@/i18n';

import api from '@/api/http';
import { getMaintenanceState, subscribeMaintenance } from '@/utils/maintenance';

const PROBE_INTERVAL_MS = 15000;
const PROBE_TIMEOUT_MS = 8000;

const { t } = useI18n();
const snapshot = ref(getMaintenanceState());
let unsubscribe = null;

const active = computed(() => snapshot.value.active);
const offline = computed(() => !snapshot.value.status);
const title = computed(() =>
  offline.value ? t('system.maintenance.offlineTitle') : t('system.maintenance.title')
);
const desc = computed(() =>
  offline.value ? t('system.maintenance.offlineDesc') : t('system.maintenance.desc')
);
const statusText = computed(() => {
  if (!snapshot.value.status) return '';
  return t('system.maintenance.status', { status: snapshot.value.status });
});

const handleRefresh = () => {
  window.location.reload();
};

// 断联期间低频探活：任一请求成功后拦截器会自动清除提示，无需在探活回调里处理状态。
let probeTimer: ReturnType<typeof setInterval> | null = null;
let probing = false;

const probeBackend = async (): Promise<void> => {
  if (probing) return;
  probing = true;
  try {
    await api.get('/auth/settings', { timeout: PROBE_TIMEOUT_MS });
  } catch {
    // 后端仍未恢复，保持提示条展示
  } finally {
    probing = false;
  }
};

const handleWindowOnline = (): void => {
  if (snapshot.value.active) {
    probeBackend();
  }
};

const syncProbeLoop = (): void => {
  if (snapshot.value.active && !probeTimer) {
    probeTimer = setInterval(probeBackend, PROBE_INTERVAL_MS);
    probeBackend();
    return;
  }
  if (!snapshot.value.active && probeTimer) {
    clearInterval(probeTimer);
    probeTimer = null;
  }
};

onMounted(() => {
  // subscribeMaintenance 订阅时会立即回调一次，携带当前状态并驱动探活循环。
  unsubscribe = subscribeMaintenance((next) => {
    snapshot.value = next;
    syncProbeLoop();
  });
  window.addEventListener('online', handleWindowOnline);
});

onBeforeUnmount(() => {
  if (unsubscribe) {
    unsubscribe();
  }
  if (probeTimer) {
    clearInterval(probeTimer);
    probeTimer = null;
  }
  window.removeEventListener('online', handleWindowOnline);
});
</script>

<style scoped>
.maintenance-banner {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding: 7px 16px;
  background: var(--maintenance-banner-bg);
  border-bottom: 1px solid var(--maintenance-banner-border);
  color: var(--maintenance-banner-text);
}

.maintenance-banner-icon {
  flex: 0 0 auto;
  width: 26px;
  height: 26px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--maintenance-banner-icon-bg);
  color: var(--maintenance-banner-icon-color);
  font-size: 13px;
}

.maintenance-banner-text {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
  flex: 1 1 auto;
  flex-wrap: wrap;
}

.maintenance-banner-title {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
}

.maintenance-banner-desc {
  font-size: 12px;
  line-height: 1.5;
  color: var(--maintenance-banner-muted);
  min-width: 0;
}

.maintenance-banner-meta {
  flex: 0 0 auto;
  font-size: 12px;
  color: var(--maintenance-banner-muted);
  white-space: nowrap;
}

.maintenance-banner-refresh {
  flex: 0 0 auto;
  margin-left: auto;
}

.maintenance-banner-fade-enter-active,
.maintenance-banner-fade-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.maintenance-banner-fade-enter-from,
.maintenance-banner-fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}
</style>
