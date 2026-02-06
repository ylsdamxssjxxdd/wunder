<template>
  <transition name="maintenance-fade">
    <div v-if="active" class="maintenance-overlay" role="alert" aria-live="assertive">
      <div class="maintenance-card">
        <div class="maintenance-icon" aria-hidden="true">
          <i class="fa-solid fa-triangle-exclamation"></i>
        </div>
        <div class="maintenance-title">{{ t('system.maintenance.title') }}</div>
        <div class="maintenance-desc">{{ t('system.maintenance.desc') }}</div>
        <div v-if="statusText" class="maintenance-meta">{{ statusText }}</div>
        <div class="maintenance-actions">
          <el-button type="primary" @click="handleRefresh">{{ t('common.refresh') }}</el-button>
        </div>
      </div>
    </div>
  </transition>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from '@/i18n';

import { getMaintenanceState, subscribeMaintenance } from '@/utils/maintenance';

const { t } = useI18n();
const snapshot = ref(getMaintenanceState());
let unsubscribe = null;

const active = computed(() => snapshot.value.active);
const statusText = computed(() => {
  if (!snapshot.value.status) return '';
  return t('system.maintenance.status', { status: snapshot.value.status });
});

const handleRefresh = () => {
  window.location.reload();
};

onMounted(() => {
  unsubscribe = subscribeMaintenance((next) => {
    snapshot.value = next;
  });
});

onBeforeUnmount(() => {
  if (unsubscribe) {
    unsubscribe();
  }
});
</script>

<style scoped>
.maintenance-fade-enter-active,
.maintenance-fade-leave-active {
  transition: opacity 0.2s ease;
}

.maintenance-fade-enter-from,
.maintenance-fade-leave-to {
  opacity: 0;
}

.maintenance-overlay {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: var(--maintenance-bg);
}

.maintenance-card {
  max-width: 520px;
  width: 100%;
  padding: 28px 28px 24px;
  border-radius: 16px;
  border: 1px solid var(--maintenance-border);
  background: var(--maintenance-card);
  color: var(--maintenance-text);
  box-shadow: 0 20px 50px rgba(0, 0, 0, 0.28);
  text-align: center;
}

.maintenance-icon {
  width: 56px;
  height: 56px;
  margin: 0 auto 16px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--maintenance-icon-bg);
  color: var(--maintenance-icon-color);
  font-size: 24px;
}

.maintenance-title {
  font-size: 20px;
  font-weight: 700;
  margin-bottom: 8px;
}

.maintenance-desc {
  font-size: 14px;
  line-height: 1.6;
  color: var(--maintenance-muted);
  margin-bottom: 12px;
}

.maintenance-meta {
  font-size: 13px;
  color: var(--maintenance-muted);
  margin-bottom: 18px;
}

.maintenance-actions :deep(.el-button) {
  min-width: 120px;
}
</style>
