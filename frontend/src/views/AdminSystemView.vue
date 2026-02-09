<template>
  <div class="admin-view">
    <el-card>
      <h3>{{ t('admin.system.title') }}</h3>
      <div class="status-grid">
        <div class="status-item">
          <span class="label">{{ t('admin.system.serverTime') }}</span>
          <span class="value">{{ status?.server_time || '-' }}</span>
        </div>
        <div class="status-item">
          <span class="label">{{ t('admin.system.userCount') }}</span>
          <span class="value">{{ status?.user_count || 0 }}</span>
        </div>
        <div class="status-item">
          <span class="label">{{ t('admin.system.activeSessions') }}</span>
          <span class="value">{{ status?.active_sessions || 0 }}</span>
        </div>
      </div>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';

import { useI18n } from '@/i18n';
import { useAdminStore } from '@/stores/admin';

const adminStore = useAdminStore();
const { t } = useI18n();
const status = computed(() => adminStore.systemStatus);

onMounted(() => adminStore.loadSystemStatus());
</script>
