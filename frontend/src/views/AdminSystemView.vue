<template>
  <div class="admin-view">
    <el-card>
      <h3>系统状态</h3>
      <div class="status-grid">
        <div class="status-item">
          <span class="label">服务器时间</span>
          <span class="value">{{ status?.server_time || '-' }}</span>
        </div>
        <div class="status-item">
          <span class="label">用户数</span>
          <span class="value">{{ status?.user_count || 0 }}</span>
        </div>
        <div class="status-item">
          <span class="label">活跃会话</span>
          <span class="value">{{ status?.active_sessions || 0 }}</span>
        </div>
      </div>
    </el-card>
  </div>
</template>

<script setup>
import { computed, onMounted } from 'vue';

import { useAdminStore } from '@/stores/admin';

const adminStore = useAdminStore();
const status = computed(() => adminStore.systemStatus);

onMounted(() => adminStore.loadSystemStatus());
</script>
