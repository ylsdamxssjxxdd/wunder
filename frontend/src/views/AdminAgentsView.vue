<template>
  <div class="admin-view">
    <el-card>
      <div class="wunder-header">
        <h3>Wunder 连接与状态</h3>
        <div class="wunder-actions">
          <el-button type="primary" size="small" :loading="saving" @click="saveSettings">
            保存设置
          </el-button>
          <el-button size="small" @click="reloadStatus">刷新状态</el-button>
        </div>
      </div>
      <div class="wunder-grid">
        <div class="wunder-settings">
          <el-form :model="form" label-position="top">
            <el-form-item label="Wunder 基础地址">
              <el-input v-model="form.base_url" placeholder="http://localhost:9000" />
            </el-form-item>
            <el-form-item label="Wunder API Key（选填）">
              <el-input
                v-model="form.api_key"
                type="password"
                show-password
                placeholder="请输入 API Key"
              />
            </el-form-item>
            <el-form-item label="请求超时（秒）">
              <el-input-number v-model="form.timeout_seconds" :min="5" :max="600" />
            </el-form-item>
          </el-form>
        </div>
        <div class="wunder-status">
          <div class="status-grid">
            <div class="status-item">
              <span class="label">状态</span>
              <span class="value">{{ status?.reachable ? '已连接' : '不可用' }}</span>
            </div>
            <div class="status-item">
              <span class="label">最近检测</span>
              <span class="value">{{ status?.checked_at || '-' }}</span>
            </div>
            <div class="status-item">
              <span class="label">内置工具</span>
              <span class="value">{{ toolCounts.builtin }}</span>
            </div>
            <div class="status-item">
              <span class="label">MCP 工具</span>
              <span class="value">{{ toolCounts.mcp }}</span>
            </div>
            <div class="status-item">
              <span class="label">技能工具</span>
              <span class="value">{{ toolCounts.skills }}</span>
            </div>
            <div class="status-item">
              <span class="label">知识库工具</span>
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
        <h3>工具清单</h3>
        <el-button size="small" @click="reloadTools">刷新工具</el-button>
      </div>
      <el-tabs v-model="activeTab" class="tools-tabs">
        <el-tab-pane :label="`内置工具 (${toolCounts.builtin})`" name="builtin">
          <wunder-tool-table :tools="toolCatalog.builtin_tools" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane :label="`MCP 工具 (${toolCounts.mcp})`" name="mcp">
          <wunder-tool-table :tools="toolCatalog.mcp_tools" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane :label="`技能工具 (${toolCounts.skills})`" name="skills">
          <wunder-tool-table :tools="toolCatalog.skills" table-height="100%" />
        </el-tab-pane>
        <el-tab-pane :label="`知识库工具 (${toolCounts.knowledge})`" name="knowledge">
          <wunder-tool-table :tools="toolCatalog.knowledge_tools" table-height="100%" />
        </el-tab-pane>
      </el-tabs>
    </el-card>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { useAdminStore } from '@/stores/admin';
import WunderToolTable from '@/components/admin/WunderToolTable.vue';

const adminStore = useAdminStore();
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
    ElMessage.error(error.response?.data?.detail || '加载失败');
  }
};

const loadTools = async () => {
  toolsLoading.value = true;
  try {
    await adminStore.loadWunderTools();
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工具获取失败');
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
    ElMessage.success('保存成功');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '保存失败');
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
