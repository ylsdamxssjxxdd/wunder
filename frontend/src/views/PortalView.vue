<template>
  <div class="portal-shell">
    <UserTopbar
      title="功能广场"
      subtitle="智能体应用入口"
      show-search
      search-placeholder="搜索智能体应用"
      :hide-chat="true"
      v-model:search="searchQuery"
    >
    </UserTopbar>
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">我的智能体应用</div>
                <div class="portal-section-desc">创建、进入并管理你的智能体应用</div>
              </div>
              <div class="portal-section-meta">共 {{ filteredAgents.length }} 个</div>
            </div>
            <div class="agent-grid portal-agent-grid">
              <button class="agent-card agent-card--create" type="button" @click="openCreateDialog">
                <div class="agent-card-plus">+</div>
                <div class="agent-card-title">新建智能体应用</div>
                <div class="agent-card-desc">快速组装你的专属能力</div>
              </button>
              <div
                class="agent-card agent-card--compact agent-card--default agent-card--clickable"
                role="button"
                tabindex="0"
                @click="enterDefaultChat"
                @keydown.enter="enterDefaultChat"
              >
                <div class="agent-card-head">
                  <div>
                    <div class="agent-card-title">通用聊天</div>
                    <div class="agent-card-desc">默认聊天能力，随时开启新对话</div>
                  </div>
                </div>
                <div class="agent-card-meta">
                  <span>默认入口</span>
                  <span>无智能体</span>
                </div>
              </div>
              <div v-if="agentLoading" class="agent-empty">加载中...</div>
              <div v-else-if="!filteredAgents.length" class="agent-empty">
                {{ normalizedQuery ? '没有匹配的智能体应用，请尝试其他关键词。' : '还没有智能体应用，点击 + 创建一个吧。' }}
              </div>
              <div
                v-else
                v-for="agent in filteredAgents"
                :key="agent.id"
                class="agent-card agent-card--compact agent-card--clickable"
                role="button"
                tabindex="0"
                @click="enterAgent(agent)"
                @keydown.enter="enterAgent(agent)"
              >
                <div class="agent-card-head">
                  <div>
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || '暂无描述' }}</div>
                  </div>
                </div>
                <div v-if="isAgentRunning(agent.id)" class="agent-card-running">
                  <span class="agent-running-dot"></span>
                  <span>运行中</span>
                </div>
                <div class="agent-card-meta">
                  <span>工具 {{ agent.tool_names?.length || 0 }}</span>
                  <span>更新 {{ formatTime(agent.updated_at) }}</span>
                </div>
                <div class="agent-card-actions">
                  <button
                    class="user-tools-btn secondary"
                    type="button"
                    @click.stop="openEditDialog(agent)"
                  >
                    编辑
                  </button>
                  <button
                    class="user-tools-btn danger"
                    type="button"
                    @click.stop="confirmDelete(agent)"
                  >
                    删除
                  </button>
                </div>
              </div>
            </div>
          </section>
          <section class="portal-section">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">共享智能体应用</div>
                <div class="portal-section-desc">同等级用户共享的智能体应用入口</div>
              </div>
              <div class="portal-section-meta">共 {{ filteredSharedAgents.length }} 个</div>
            </div>
            <div class="agent-grid portal-agent-grid">
              <div v-if="agentLoading" class="agent-empty">加载中...</div>
              <div v-else-if="!filteredSharedAgents.length" class="agent-empty">
                {{ normalizedQuery ? '没有匹配的共享智能体应用。' : '暂无共享智能体应用。' }}
              </div>
              <div
                v-else
                v-for="agent in filteredSharedAgents"
                :key="agent.id"
                class="agent-card agent-card--compact agent-card--clickable"
                role="button"
                tabindex="0"
                @click="enterAgent(agent)"
                @keydown.enter="enterAgent(agent)"
              >
                <div class="agent-card-head">
                  <div>
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || '暂无描述' }}</div>
                  </div>
                </div>
                <div v-if="isAgentRunning(agent.id)" class="agent-card-running">
                  <span class="agent-running-dot"></span>
                  <span>运行中</span>
                </div>
                <div class="agent-card-meta">
                  <span>工具 {{ agent.tool_names?.length || 0 }}</span>
                  <span>更新 {{ formatTime(agent.updated_at) }}</span>
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
      width="820px"
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
          <el-form-item label="共享设置">
            <div class="agent-share-row">
              <el-switch v-model="form.is_shared" />
              <span>共享给同等级用户</span>
            </div>
          </el-form-item>
          <el-form-item label="挂载工具与技能">
            <div class="agent-tool-picker">
              <div v-if="toolLoading" class="agent-tool-loading">加载工具中...</div>
              <el-checkbox-group v-else v-model="form.tool_names" class="agent-tool-groups">
                <div v-for="group in toolGroups" :key="group.label" class="agent-tool-group">
                  <div class="agent-tool-group-header">
                    <div class="agent-tool-group-title">{{ group.label }}</div>
                    <button
                      class="agent-tool-group-select"
                      type="button"
                      @click.stop="selectToolGroup(group)"
                    >
                      {{ isToolGroupFullySelected(group) ? '取消全选' : '全选' }}
                    </button>
                  </div>
                  <div class="agent-tool-options">
                    <el-checkbox
                      v-for="option in group.options"
                      :key="option.value"
                      :label="option.value"
                    >
                      <span :title="option.description || option.label">{{ option.label }}</span>
                    </el-checkbox>
                  </div>
                </div>
              </el-checkbox-group>
              <div v-if="sharedToolsNotice" class="agent-editor-hint">
                共享工具需要在工具管理中勾选后才能出现在这里。
              </div>
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
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import { listRunningAgents } from '@/api/agents';
import { fetchUserToolsCatalog } from '@/api/userTools';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const agentStore = useAgentStore();
const searchQuery = ref('');
const dialogVisible = ref(false);
const saving = ref(false);
const editingId = ref('');
const toolCatalog = ref(null);
const toolLoading = ref(false);
const runningAgentIds = ref([]);
let runningTimer = null;

const RUNNING_REFRESH_MS = 6000;

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  tool_names: [],
  system_prompt: ''
});

const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const normalizedQuery = computed(() => searchQuery.value.trim().toLowerCase());

const matchesQuery = (agent, query) => {
  if (!query) return true;
  const source = [
    agent?.name,
    agent?.description,
    ...(agent?.tool_names || [])
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  return source.includes(query);
};

onMounted(() => {
  if (!authStore.user) {
    authStore.loadProfile();
  }
  agentStore.loadAgents();
  loadCatalog();
  loadRunningAgents();
  runningTimer = window.setInterval(loadRunningAgents, RUNNING_REFRESH_MS);
});

onBeforeUnmount(() => {
  if (runningTimer) {
    clearInterval(runningTimer);
    runningTimer = null;
  }
});

const agents = computed(() => agentStore.agents || []);
const sharedAgents = computed(() => agentStore.sharedAgents || []);
const agentLoading = computed(() => agentStore.loading);
const filteredAgents = computed(() => {
  const query = normalizedQuery.value;
  if (!query) return agents.value;
  return agents.value.filter((agent) => matchesQuery(agent, query));
});
const filteredSharedAgents = computed(() => {
  const query = normalizedQuery.value;
  if (!query) return sharedAgents.value;
  return sharedAgents.value.filter((agent) => matchesQuery(agent, query));
});

const runningAgentSet = computed(() => new Set(runningAgentIds.value));

const isAgentRunning = (agentId) => {
  const key = String(agentId || '').trim();
  if (!key) return false;
  return runningAgentSet.value.has(key);
};

const dialogTitle = computed(() => (editingId.value ? '编辑智能体应用' : '新建智能体应用'));

const normalizeOptions = (list) =>
  (Array.isArray(list) ? list : []).map((item) => ({
    label: item.name,
    value: item.name,
    description: item.description
  }));

const toolGroups = computed(() => {
  const payload = toolCatalog.value || {};
  const sharedSelected = new Set(
    Array.isArray(payload.shared_tools_selected) ? payload.shared_tools_selected : []
  );
  const sharedTools = (Array.isArray(payload.shared_tools) ? payload.shared_tools : []).filter(
    (tool) => sharedSelected.has(tool.name)
  );
  return [
    { label: '内置工具', options: normalizeOptions(payload.builtin_tools) },
    { label: 'MCP 工具', options: normalizeOptions(payload.mcp_tools) },
    { label: 'A2A 工具', options: normalizeOptions(payload.a2a_tools) },
    { label: '技能', options: normalizeOptions(payload.skills) },
    { label: '知识库', options: normalizeOptions(payload.knowledge_tools) },
    { label: '我的工具', options: normalizeOptions(payload.user_tools) },
    { label: '共享工具', options: normalizeOptions(sharedTools) }
  ].filter((group) => group.options.length > 0);
});

const allToolValues = computed(() => {
  const values = new Set();
  toolGroups.value.forEach((group) => {
    group.options.forEach((option) => values.add(option.value));
  });
  return Array.from(values);
});

const sharedToolsNotice = computed(() => {
  const payload = toolCatalog.value || {};
  const shared = Array.isArray(payload.shared_tools) ? payload.shared_tools : [];
  const selected = Array.isArray(payload.shared_tools_selected) ? payload.shared_tools_selected : [];
  return shared.length > 0 && selected.length === 0;
});

const applyDefaultTools = () => {
  form.tool_names = allToolValues.value.length ? [...allToolValues.value] : [];
};

const isToolGroupFullySelected = (group) => {
  if (!group || !Array.isArray(group.options) || group.options.length === 0) return false;
  const current = new Set(form.tool_names);
  return group.options.every((option) => current.has(option.value));
};

const selectToolGroup = (group) => {
  if (!group || !Array.isArray(group.options) || group.options.length === 0) return;
  const next = new Set(form.tool_names);
  const fullySelected = group.options.every((option) => next.has(option.value));
  if (fullySelected) {
    group.options.forEach((option) => next.delete(option.value));
  } else {
    group.options.forEach((option) => next.add(option.value));
  }
  form.tool_names = Array.from(next);
};

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.is_shared = false;
  form.system_prompt = '';
  applyDefaultTools();
  editingId.value = '';
};

const loadCatalog = async () => {
  toolLoading.value = true;
  try {
    const { data } = await fetchUserToolsCatalog();
    toolCatalog.value = data?.data || null;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '工具清单加载失败');
  } finally {
    toolLoading.value = false;
  }
};

const loadRunningAgents = async () => {
  try {
    const { data } = await listRunningAgents();
    const items = data?.data?.items || [];
    runningAgentIds.value = items
      .map((item) => String(item?.agent_id || '').trim())
      .filter(Boolean);
  } catch (error) {
    runningAgentIds.value = [];
  }
};

const openCreateDialog = async () => {
  if (!toolCatalog.value) {
    await loadCatalog();
  }
  resetForm();
  dialogVisible.value = true;
};

const openEditDialog = async (agent) => {
  if (!agent) return;
  if (!toolCatalog.value) {
    await loadCatalog();
  }
  form.name = agent.name || '';
  form.description = agent.description || '';
  form.is_shared = Boolean(agent.is_shared);
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
      is_shared: Boolean(form.is_shared),
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
    await ElMessageBox.confirm(`确认删除智能体应用 ${agent.name} 吗？`, '提示', {
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
  router.push(`${basePath.value}/chat?agent_id=${encodeURIComponent(agentId)}`);
};

const enterDefaultChat = () => {
  router.push({ path: `${basePath.value}/chat`, query: { entry: 'default' } });
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
</script>
