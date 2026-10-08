<template>
  <div class="admin-view">
    <el-card>
      <div class="wunder-header">
        <h3>{{ t('admin.agents.title') }}</h3>
        <div class="wunder-actions">
          <el-button type="primary" size="small" :loading="saving" @click="saveSettings">
            {{ t('admin.agents.save') }}
          </el-button>
          <el-button size="small" @click="reloadStatus">{{ t('admin.agents.refresh') }}</el-button>
        </div>
      </div>
      <div class="wunder-grid">
        <div class="wunder-settings">
          <el-form :model="form" label-position="top">
            <el-form-item :label="t('admin.agents.baseUrl')">
              <el-input v-model="form.base_url" placeholder="http://localhost:9000" />
            </el-form-item>
            <el-form-item :label="t('admin.agents.apiKey')">
              <el-input
                v-model="form.api_key"
                type="password"
                show-password
                :placeholder="t('admin.agents.apiKeyPlaceholder')"
              />
            </el-form-item>
            <el-form-item :label="t('admin.agents.timeout')">
              <el-input-number v-model="form.timeout_seconds" :min="5" :max="600" />
            </el-form-item>
          </el-form>
        </div>
        <div class="wunder-status">
          <div class="status-grid">
            <div class="status-item">
              <span class="label">{{ t('admin.agents.status') }}</span>
              <span class="value">{{
                status?.reachable ? t('admin.agents.status.online') : t('admin.agents.status.offline')
              }}</span>
            </div>
            <div class="status-item">
              <span class="label">{{ t('admin.agents.lastCheck') }}</span>
              <span class="value">{{ status?.checked_at || '-' }}</span>
            </div>
            <div class="status-item">
              <span class="label">{{ t('admin.agents.tools.builtin') }}</span>
              <span class="value">{{ toolCounts.builtin }}</span>
            </div>
            <div class="status-item">
              <span class="label">{{ t('admin.agents.tools.mcp') }}</span>
              <span class="value">{{ toolCounts.mcp }}</span>
            </div>
            <div class="status-item">
              <span class="label">{{ t('admin.agents.tools.skills') }}</span>
              <span class="value">{{ toolCounts.skills }}</span>
            </div>
            <div class="status-item">
              <span class="label">{{ t('admin.agents.tools.knowledge') }}</span>
              <span class="value">{{ toolCounts.knowledge }}</span>
            </div>
          </div>
          <el-text v-if="status?.message && status?.message !== 'ok'" type="danger" size="small">
            {{ status?.message }}
          </el-text>
        </div>
      </div>
    </el-card>

    <el-card class="tools-card" v-loading="toolsLoading">
      <div class="tools-header">
        <h3>{{ t('admin.agents.list.title') }}</h3>
        <el-button size="small" @click="reloadTools">{{ t('admin.agents.list.refresh') }}</el-button>
      </div>
      <el-tabs v-model="activeTab" class="tools-tabs">
        <el-tab-pane :label="t('admin.agents.tab.builtin', { count: toolCounts.builtin })" name="builtin">
          <wunder-tool-table :tools="toolCatalog.builtin_tools" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane :label="t('admin.agents.tab.mcp', { count: toolCounts.mcp })" name="mcp">
          <wunder-tool-table :tools="toolCatalog.mcp_tools" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane :label="t('admin.agents.tab.skills', { count: toolCounts.skills })" name="skills">
          <wunder-tool-table :tools="toolCatalog.skills" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane
          :label="t('admin.agents.tab.knowledge', { count: toolCounts.knowledge })"
          name="knowledge"
        >
          <wunder-tool-table :tools="toolCatalog.knowledge_tools" table-height="100%" />
        </el-tab-pane>
      </el-tabs>
    </el-card>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { useAdminStore } from '@/stores/admin';
import WunderToolTable from '@/components/admin/WunderToolTable.vue';
import { showApiError } from '@/utils/apiError';

const adminStore = useAdminStore();
const { t } = useI18n();
const saving = ref(false);
const toolsLoading = ref(false);
const activeTab = ref('builtin');
const form = reactive({
  base_url: '',
  api_key: '',
  timeout_seconds: 120
});

const status = computed(() => adminStore.wunderStatus);
const toolCatalog = computed(() => adminStore.wunderTools || {});
const toolCounts = computed(() => ({
  // 工具数量用于标签与状态展示
  builtin: toolCatalog.value.builtin_tools?.length || 0,
  mcp: toolCatalog.value.mcp_tools?.length || 0,
  skills: toolCatalog.value.skills?.length || 0,
  knowledge: toolCatalog.value.knowledge_tools?.length || 0
}));

const applySettings = (data) => {
  if (!data?.settings) return;
  form.base_url = data.settings.base_url || '';
  form.api_key = data.settings.api_key || '';
  form.timeout_seconds = data.settings.timeout_seconds || 120;
};

const loadSettings = async () => {
  try {
    const data = await adminStore.loadWunderSettings();
    applySettings(data);
  } catch (error) {
    showApiError(error, t('admin.agents.loadFailed'));
  }
};

const loadTools = async () => {
  toolsLoading.value = true;
  try {
    await adminStore.loadWunderTools();
  } catch (error) {
    showApiError(error, t('admin.agents.toolsFailed'));
  } finally {
    toolsLoading.value = false;
  }
};

const saveSettings = async () => {
  saving.value = true;
  try {
    const data = await adminStore.updateWunderSettings(form);
    applySettings(data);
    await loadTools();
    ElMessage.success(t('admin.agents.saveSuccess'));
  } catch (error) {
    showApiError(error, t('admin.agents.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const reloadStatus = async () => {
  await loadSettings();
};

const reloadTools = async () => {
  await loadTools();
};

onMounted(async () => {
  await loadSettings();
  await loadTools();
});
</script>

<style scoped>
.wunder-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.wunder-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.wunder-grid {
  display: grid;
  grid-template-columns: 1.1fr 1fr;
  gap: 16px;
  align-items: start;
}

.wunder-status {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tools-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.tools-card {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

:deep(.tools-card .el-card__body) {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tools-tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

:deep(.tools-tabs .el-tabs__content) {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

:deep(.tools-tabs .el-tab-pane) {
  height: 100%;
}
</style>