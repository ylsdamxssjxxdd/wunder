<template>
  <div class="portal-shell agent-square-shell">
    <UserTopbar
      title="智能体广场"
      subtitle="创建与管理你的专属智能体"
    >
      <template #actions>
        <button class="portal-action-btn" type="button" @click="openCreateDialog">新建智能体</button>
      </template>
    </UserTopbar>

    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <div class="portal-hero">
            <div class="portal-hero-title">我的智能体</div>
            <div class="portal-hero-sub">
              智能体提示词会在基础系统提示词之后追加生效，可随时调整工具挂载。
            </div>
          </div>

          <section class="portal-section">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">智能体列表</div>
                <div class="portal-section-desc">点击进入即可进入会话，或编辑工具与提示词</div>
              </div>
              <div class="portal-section-meta">共 {{ agents.length }} 个</div>
            </div>
            <div class="agent-grid">
              <div v-if="loading" class="agent-empty">加载中...</div>
              <div v-else-if="!agents.length" class="agent-empty">
                还没有智能体，点击“新建智能体”创建第一个吧。
              </div>
              <div
                v-else
                v-for="agent in agents"
                :key="agent.id"
                class="agent-card"
              >
                <div class="agent-card-head">
                  <div>
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || '暂无描述' }}</div>
                  </div>
                  <span class="agent-card-level">等级 {{ agent.access_level || '-' }}</span>
                </div>
                <div class="agent-card-meta">
                  <span>工具 {{ agent.tool_names?.length || 0 }}</span>
                  <span>更新 {{ formatTime(agent.updated_at) }}</span>
                </div>
                <div class="agent-card-actions">
                  <button class="user-tools-btn" type="button" @click="enterAgent(agent)">
                    进入
                  </button>
                  <button class="user-tools-btn secondary" type="button" @click="openEditDialog(agent)">
                    编辑
                  </button>
                  <button class="user-tools-btn danger" type="button" @click="confirmDelete(agent)">
                    删除
                  </button>
                </div>
              </div>
            </div>
          </section>
        </div>
      </section>
    </main>

    <el-dialog
      v-model="dialogVisible"
      class="user-tools-dialog agent-editor-dialog"
      width="760px"
      top="6vh"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ dialogTitle }}</div>
          <button class="icon-btn" type="button" @click="dialogVisible = false">×</button>
        </div>
      </template>
      <div class="agent-editor-body">
        <el-form :model="form" label-position="top">
          <el-form-item label="智能体名称">
            <el-input v-model="form.name" placeholder="例如：产品分析助手" />
          </el-form-item>
          <el-form-item label="描述">
            <el-input v-model="form.description" placeholder="一句话描述智能体用途" />
          </el-form-item>
          <el-form-item label="等级">
            <el-select v-model="form.access_level" placeholder="选择等级">
              <el-option v-for="level in accessLevelOptions" :key="level" :label="level" :value="level" />
            </el-select>
          </el-form-item>
          <el-form-item label="挂载工具与技能">
            <el-select
              v-model="form.tool_names"
              multiple
              filterable
              collapse-tags
              collapse-tags-tooltip
              placeholder="选择需要挂载的工具"
              style="width: 100%"
            >
              <el-option-group
                v-for="group in toolGroups"
                :key="group.label"
                :label="group.label"
              >
                <el-option
                  v-for="option in group.options"
                  :key="option.value"
                  :label="option.label"
                  :value="option.value"
                />
              </el-option-group>
            </el-select>
            <div v-if="sharedToolsNotice" class="agent-editor-hint">
              共享工具需要在工具管理中勾选后才能出现在这里。
            </div>
          </el-form-item>
          <el-form-item label="智能体提示词（追加）">
            <el-input
              v-model="form.system_prompt"
              type="textarea"
              :rows="8"
              placeholder="输入需要追加到基础系统提示词后的内容"
            />
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="saveAgent">
          保存
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserToolsCatalog } from '@/api/userTools';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';

const router = useRouter();
const route = useRoute();
const agentStore = useAgentStore();
const authStore = useAuthStore();

const dialogVisible = ref(false);
const saving = ref(false);
const editingId = ref('');
const toolCatalog = ref(null);

const form = reactive({
  name: '',
  description: '',
  access_level: 'C',
  tool_names: [],
  system_prompt: ''
});

const agents = computed(() => agentStore.agents || []);
const loading = computed(() => agentStore.loading);
const dialogTitle = computed(() => (editingId.value ? '编辑智能体' : '新建智能体'));

const accessLevelOptions = computed(() => {
  const level = String(authStore.user?.access_level || 'C').toUpperCase();
  const levels = ['A', 'B', 'C'];
  if (!levels.includes(level)) {
    return ['C'];
  }
  const maxIndex = levels.indexOf(level);
  return levels.slice(maxIndex);
});

const normalizeOptions = (list) =>
  (Array.isArray(list) ? list : []).map((item) => ({
    label: item.name,
    value: item.name
  }));

const toolGroups = computed(() => {
  const payload = toolCatalog.value || {};
  const sharedSelected = new Set(
    Array.isArray(payload.shared_tools_selected) ? payload.shared_tools_selected : []
  );
  const sharedTools = (Array.isArray(payload.shared_tools) ? payload.shared_tools : []).filter((tool) =>
    sharedSelected.has(tool.name)
  );
  return [
    { label: '内置工具 (C)', options: normalizeOptions(payload.builtin_tools) },
    { label: 'MCP 工具 (C)', options: normalizeOptions(payload.mcp_tools) },
    { label: 'A2A 工具 (C)', options: normalizeOptions(payload.a2a_tools) },
    { label: '技能工具 (A)', options: normalizeOptions(payload.skills) },
    { label: '知识库工具 (B)', options: normalizeOptions(payload.knowledge_tools) },
    { label: '我的工具', options: normalizeOptions(payload.user_tools) },
    { label: '共享工具', options: normalizeOptions(sharedTools) }
  ].filter((group) => group.options.length > 0);
});

const sharedToolsNotice = computed(() => {
  const payload = toolCatalog.value || {};
  const shared = Array.isArray(payload.shared_tools) ? payload.shared_tools : [];
  const selected = Array.isArray(payload.shared_tools_selected) ? payload.shared_tools_selected : [];
  return shared.length > 0 && selected.length === 0;
});

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.access_level = accessLevelOptions.value[0] || 'C';
  form.tool_names = [];
  form.system_prompt = '';
  editingId.value = '';
};

const openCreateDialog = () => {
  resetForm();
  dialogVisible.value = true;
};

const openEditDialog = (agent) => {
  if (!agent) return;
  form.name = agent.name || '';
  form.description = agent.description || '';
  form.access_level = agent.access_level || accessLevelOptions.value[0] || 'C';
  form.tool_names = Array.isArray(agent.tool_names) ? [...agent.tool_names] : [];
  form.system_prompt = agent.system_prompt || '';
  editingId.value = agent.id;
  dialogVisible.value = true;
};

const saveAgent = async () => {
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning('请填写智能体名称');
    return;
  }
  saving.value = true;
  try {
    const payload = {
      name,
      description: form.description || '',
      access_level: form.access_level,
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      system_prompt: form.system_prompt || ''
    };
    if (editingId.value) {
      await agentStore.updateAgent(editingId.value, payload);
      ElMessage.success('智能体已更新');
    } else {
      await agentStore.createAgent(payload);
      ElMessage.success('智能体已创建');
    }
    dialogVisible.value = false;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '保存失败');
  } finally {
    saving.value = false;
  }
};

const confirmDelete = async (agent) => {
  if (!agent) return;
  try {
    await ElMessageBox.confirm(`确认删除智能体 ${agent.name} 吗？`, '提示', {
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  try {
    await agentStore.deleteAgent(agent.id);
    ElMessage.success('已删除');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '删除失败');
  }
};

const enterAgent = (agent) => {
  const agentId = agent?.id;
  if (!agentId) return;
  const base = route.path.startsWith('/demo') ? '/demo' : '/app';
  router.push(`${base}/chat?agent_id=${encodeURIComponent(agentId)}`);
};

const formatTime = (value) => {
  if (!value) return '-';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return String(value);
  }
  const pad = (part) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}`;
};

const loadCatalog = async () => {
  try {
    const { data } = await fetchUserToolsCatalog();
    toolCatalog.value = data?.data || null;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工具清单加载失败');
  }
};

onMounted(async () => {
  if (!authStore.user) {
    await authStore.loadProfile();
  }
  await agentStore.loadAgents();
  await loadCatalog();
});
</script>
