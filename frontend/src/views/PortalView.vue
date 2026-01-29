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
                  <div class="agent-card-default-icon" aria-hidden="true">
                    <svg class="agent-card-default-icon-svg" viewBox="0 0 24 24">
                      <path d="M7 13h6a4 4 0 0 0 0-8H7a4 4 0 0 0 0 8z" />
                      <path d="M7 13v4l4-2" />
                    </svg>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">通用聊天</div>
                    <div class="agent-card-desc">默认聊天能力，随时开启新对话</div>
                  </div>
                </div>
                <div
                  v-if="isAgentRunning(DEFAULT_AGENT_KEY) || isAgentWaiting(DEFAULT_AGENT_KEY)"
                  class="agent-card-status"
                >
                  <div v-if="isAgentRunning(DEFAULT_AGENT_KEY)" class="agent-card-running">
                    <span class="agent-running-dot"></span>
                    <span>运行中</span>
                  </div>
                  <div v-if="isAgentWaiting(DEFAULT_AGENT_KEY)" class="agent-card-waiting">
                    <span class="agent-waiting-dot"></span>
                    <span>待选择</span>
                  </div>
                </div>
                <div class="agent-card-meta">
                  <span>默认入口</span>
                </div>
              </div>
              <div v-if="agentLoading" class="agent-empty">加载中...</div>
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
                  <div class="agent-card-avatar" :style="getAgentAvatarStyle(agent)">
                    <svg
                      v-if="hasAgentIcon(agent)"
                      class="agent-card-icon"
                      viewBox="0 0 24 24"
                      aria-hidden="true"
                    >
                      <path
                        v-for="(path, index) in getAgentIconPaths(agent)"
                        :key="`${agent.id}-icon-${index}`"
                        :d="path"
                      />
                    </svg>
                    <span v-else>{{ getAgentAvatarText(agent.name) }}</span>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || '暂无描述' }}</div>
                  </div>
                </div>
                <div
                  v-if="isAgentRunning(agent.id) || isAgentWaiting(agent.id)"
                  class="agent-card-status"
                >
                  <div v-if="isAgentRunning(agent.id)" class="agent-card-running">
                    <span class="agent-running-dot"></span>
                    <span>运行中</span>
                  </div>
                  <div v-if="isAgentWaiting(agent.id)" class="agent-card-waiting">
                    <span class="agent-waiting-dot"></span>
                    <span>待选择</span>
                  </div>
                </div>
                <div class="agent-card-meta">
                  <span>工具 {{ agent.tool_names?.length || 0 }}</span>
                  <span>更新 {{ formatTime(agent.updated_at) }}</span>
                </div>
                <div class="agent-card-actions">
                  <button
                    class="agent-card-icon-btn"
                    type="button"
                    title="编辑"
                    aria-label="编辑"
                    @click.stop="openEditDialog(agent)"
                  >
                    <svg class="agent-card-icon" viewBox="0 0 24 24" aria-hidden="true">
                      <path d="M4 16.5V20h3.5L19 8.5 15.5 5 4 16.5z" />
                      <path d="M13.5 6.5L17 10" />
                    </svg>
                  </button>
                  <button
                    class="agent-card-icon-btn danger"
                    type="button"
                    title="删除"
                    aria-label="删除"
                    @click.stop="confirmDelete(agent)"
                  >
                    <svg class="agent-card-icon" viewBox="0 0 24 24" aria-hidden="true">
                      <path d="M4 7h16" />
                      <path d="M9 7V5h6v2" />
                      <path d="M7 7l1 12h8l1-12" />
                      <path d="M10 11v5M14 11v5" />
                    </svg>
                  </button>
                </div>
              </div>
            </div>
          </section>
          <section class="portal-section portal-section--shared">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">共享智能体应用</div>
                <div class="portal-section-desc">同等级用户共享的智能体应用入口</div>
              </div>
              <div class="portal-section-meta">共 {{ filteredSharedAgents.length }} 个</div>
            </div>
            <div class="agent-grid portal-agent-grid">
              <div v-if="agentLoading" class="agent-empty">加载中...</div>
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
                  <div class="agent-card-avatar" :style="getAgentAvatarStyle(agent)">
                    <svg
                      v-if="hasAgentIcon(agent)"
                      class="agent-card-icon"
                      viewBox="0 0 24 24"
                      aria-hidden="true"
                    >
                      <path
                        v-for="(path, index) in getAgentIconPaths(agent)"
                        :key="`${agent.id}-icon-${index}`"
                        :d="path"
                      />
                    </svg>
                    <span v-else>{{ getAgentAvatarText(agent.name) }}</span>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || '暂无描述' }}</div>
                  </div>
                </div>
                <div
                  v-if="isAgentRunning(agent.id) || isAgentWaiting(agent.id)"
                  class="agent-card-status"
                >
                  <div v-if="isAgentRunning(agent.id)" class="agent-card-running">
                    <span class="agent-running-dot"></span>
                    <span>运行中</span>
                  </div>
                  <div v-if="isAgentWaiting(agent.id)" class="agent-card-waiting">
                    <span class="agent-waiting-dot"></span>
                    <span>待选择</span>
                  </div>
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
          <el-form-item label="基础设置">
            <div class="agent-basic-settings">
              <div class="agent-avatar-card">
                <div class="agent-avatar-header">
                  <div class="agent-avatar-header-left">
                    <div class="agent-avatar-title">头像设置</div>
                    <div
                      class="agent-avatar-preview"
                      :style="getAvatarStyle({ name: form.icon_name, color: form.icon_color })"
                    >
                      <span
                        v-if="form.icon_name === DEFAULT_ICON_NAME"
                        class="agent-avatar-option-text"
                        >Aa</span
                      >
                      <svg
                        v-else-if="getAvatarIconOption(form.icon_name)"
                        class="agent-avatar-option-icon"
                        viewBox="0 0 24 24"
                      >
                        <path
                          v-for="(path, index) in getAvatarIconOption(form.icon_name).paths"
                          :key="`${form.icon_name}-${index}`"
                          :d="path"
                        />
                      </svg>
                      <span v-else class="agent-avatar-option-text">Aa</span>
                    </div>
                  </div>
                  <button
                    class="agent-avatar-toggle"
                    type="button"
                    :aria-expanded="avatarPanelVisible"
                    @click="avatarPanelVisible = !avatarPanelVisible"
                  >
                    {{ avatarPanelVisible ? '收起' : '设置' }}
                  </button>
                </div>
                <div v-show="avatarPanelVisible" class="agent-avatar-panel">
                  <div class="agent-avatar-section">
                    <div class="agent-avatar-section-title">头像图标</div>
                    <div class="agent-avatar-options">
                      <button
                        v-for="option in avatarIconOptions"
                        :key="option.name"
                        class="agent-avatar-option"
                        :class="{ active: form.icon_name === option.name }"
                        type="button"
                        :title="option.label"
                        @click="selectAvatarIcon(option)"
                      >
                        <span v-if="option.name === DEFAULT_ICON_NAME" class="agent-avatar-option-text"
                          >Aa</span
                        >
                        <svg v-else class="agent-avatar-option-icon" viewBox="0 0 24 24">
                          <path
                            v-for="(path, index) in option.paths"
                            :key="`${option.name}-${index}`"
                            :d="path"
                          />
                        </svg>
                      </button>
                    </div>
                  </div>
                  <div class="agent-avatar-section">
                    <div class="agent-avatar-section-title">头像颜色</div>
                    <div class="agent-avatar-colors">
                      <button
                        v-for="color in avatarColorOptions"
                        :key="color || 'default'"
                        class="agent-avatar-color"
                        :class="{ active: (form.icon_color || '') === (color || '') }"
                        type="button"
                        :title="color ? color : '默认'"
                        :style="color ? { background: color } : {}"
                        @click="selectAvatarColor(color)"
                      >
                        <span v-if="!color" class="agent-avatar-color-text">默认</span>
                      </button>
                    </div>
                    <div class="agent-avatar-custom">
                      <input
                        class="agent-avatar-custom-input"
                        type="color"
                        :value="customColor || '#6ad9ff'"
                        @input="updateCustomColor($event.target.value)"
                      />
                      <input
                        class="agent-avatar-custom-text"
                        type="text"
                        :value="customColor"
                        placeholder="自定义颜色，例如 #6ad9ff"
                        @input="updateCustomColor($event.target.value)"
                      />
                    </div>
                  </div>
                </div>
              </div>
              <div class="agent-share-card">
                <div class="agent-share-title">共享设置</div>
                <div class="agent-share-row">
                  <el-switch v-model="form.is_shared" />
                  <span>共享给同等级用户</span>
                </div>
              </div>
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
const waitingAgentIds = ref([]);
const avatarPanelVisible = ref(false);
const customColor = ref('');
let runningTimer = null;

const RUNNING_REFRESH_MS = 6000;
const DEFAULT_AGENT_KEY = '__default__';
const DEFAULT_ICON_NAME = 'initial';

const avatarIconOptions = [
  { name: DEFAULT_ICON_NAME, label: '首字母' },
  {
    name: 'chat',
    label: '聊天',
    paths: ['M7 13h6a4 4 0 0 0 0-8H7a4 4 0 0 0 0 8z', 'M7 13v4l4-2']
  },
  {
    name: 'bot',
    label: '机器人',
    paths: [
      'M9 7h6a4 4 0 0 1 4 4v4a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4v-4a4 4 0 0 1 4-4z',
      'M12 3v2',
      'M9 12h.01',
      'M15 12h.01'
    ]
  },
  {
    name: 'idea',
    label: '灵感',
    paths: ['M9 18h6', 'M10 22h4', 'M12 2a7 7 0 0 0-4 12c1 1 1.5 2 1.5 3h5c0-1 .5-2 1.5-3A7 7 0 0 0 12 2z']
  },
  {
    name: 'target',
    label: '目标',
    paths: ['M12 2a10 10 0 1 0 0 20a10 10 0 0 0 0-20z', 'M12 8l3 4-3 4-3-4 3-4z']
  },
  {
    name: 'bolt',
    label: '加速',
    paths: ['M13 2L3 14h7l-1 8 10-12h-7l1-8z']
  },
  {
    name: 'code',
    label: '代码',
    paths: ['M8 9l-4 3 4 3', 'M16 9l4 3-4 3', 'M10 19l4-14']
  },
  {
    name: 'chart',
    label: '分析',
    paths: ['M4 19V5', 'M4 19h16', 'M10 19V9', 'M16 19V7']
  },
  {
    name: 'doc',
    label: '文档',
    paths: ['M6 3h8l4 4v14H6z', 'M14 3v4h4', 'M8 11h8', 'M8 15h6']
  },
  {
    name: 'pen',
    label: '写作',
    paths: ['M4 20h4l10-10-4-4L4 16v4z', 'M13.5 6.5L17 10']
  },
  {
    name: 'calendar',
    label: '日程',
    paths: ['M7 4v3', 'M17 4v3', 'M5 7h14', 'M5 7h14v12H5z', 'M9 11h2', 'M13 11h2']
  },
  {
    name: 'briefcase',
    label: '办公',
    paths: ['M9 7V5h6v2', 'M4 8h16v11H4z', 'M4 12h16']
  },
  {
    name: 'clipboard',
    label: '清单',
    paths: ['M9 4h6v2H9z', 'M7 6h10v14H7z', 'M9 10h6', 'M9 14h6']
  },
  {
    name: 'book',
    label: '知识',
    paths: ['M4 5h8v14H4z', 'M12 5h8v14h-8z', 'M12 5v14']
  },
  {
    name: 'check',
    label: '校验',
    paths: ['M5 13l4 4L19 7']
  },
  {
    name: 'shield',
    label: '守护',
    paths: ['M12 3l7 4v5c0 5-3.5 8-7 9-3.5-1-7-4-7-9V7l7-4z']
  },
  {
    name: 'spark',
    label: '星光',
    paths: ['M12 3l2.5 5 5.5.8-4 3.8.9 5.4-4.9-2.6-4.9 2.6.9-5.4-4-3.8 5.5-.8z']
  }
];

const avatarColorOptions = [
  '',
  '#6ad9ff',
  '#a78bfa',
  '#34d399',
  '#f472b6',
  '#fbbf24',
  '#60a5fa',
  '#f97316',
  '#22d3ee',
  '#94a3b8',
  '#f87171'
];

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  tool_names: [],
  system_prompt: '',
  icon_name: DEFAULT_ICON_NAME,
  icon_color: ''
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

const getAgentAvatarText = (name) => {
  const trimmed = String(name || '').trim();
  if (!trimmed) return 'AI';
  const [first] = Array.from(trimmed);
  return first ? (first.match(/[a-z]/i) ? first.toUpperCase() : first) : 'AI';
};

const normalizeIconName = (name) => {
  const trimmed = String(name || '').trim();
  if (!trimmed) return DEFAULT_ICON_NAME;
  return avatarIconOptions.some((option) => option.name === trimmed) ? trimmed : DEFAULT_ICON_NAME;
};

const getAvatarIconOption = (name) => avatarIconOptions.find((option) => option.name === name);

const parseIconValue = (value) => {
  if (!value || typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === 'object') {
      return parsed;
    }
    if (typeof parsed === 'string') {
      return { name: parsed };
    }
  } catch (error) {
    return { name: trimmed };
  }
  return { name: trimmed };
};

const getIconConfig = (value) => {
  const parsed = parseIconValue(value);
  return {
    name: normalizeIconName(parsed?.name),
    color: typeof parsed?.color === 'string' ? parsed.color : ''
  };
};

const hasAgentIcon = (agent) => {
  const config = getIconConfig(agent?.icon);
  if (config.name === DEFAULT_ICON_NAME) return false;
  const option = getAvatarIconOption(config.name);
  return Boolean(option && option.paths && option.paths.length);
};

const getAgentIconPaths = (agent) => {
  const config = getIconConfig(agent?.icon);
  const option = getAvatarIconOption(config.name);
  return option?.paths || [];
};

const hexToRgba = (hex, alpha) => {
  const trimmed = String(hex || '').trim();
  const match = trimmed.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!match) return '';
  let value = match[1];
  if (value.length === 3) {
    value = value
      .split('')
      .map((char) => char + char)
      .join('');
  }
  const parsed = Number.parseInt(value, 16);
  const r = (parsed >> 16) & 255;
  const g = (parsed >> 8) & 255;
  const b = parsed & 255;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
};

const getAvatarStyle = (config) => {
  if (!config?.color) return {};
  const strong = hexToRgba(config.color, 0.55);
  const soft = hexToRgba(config.color, 0.12);
  const border = hexToRgba(config.color, 0.6);
  if (!strong || !soft || !border) return {};
  const style = {
    background: `radial-gradient(circle at 30% 30%, ${strong}, ${soft})`,
    borderColor: border
  };
  if (config.name !== DEFAULT_ICON_NAME) {
    style.color = config.color;
  }
  return style;
};

const getAgentAvatarStyle = (agent) => getAvatarStyle(getIconConfig(agent?.icon));

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
const waitingAgentSet = computed(() => new Set(waitingAgentIds.value));

const isAgentRunning = (agentId) => {
  const key = String(agentId || '').trim();
  if (!key) return false;
  return runningAgentSet.value.has(key);
};

const isAgentWaiting = (agentId) => {
  const key = String(agentId || '').trim();
  if (!key) return false;
  return waitingAgentSet.value.has(key);
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

const selectAvatarIcon = (option) => {
  if (!option) return;
  form.icon_name = option.name;
};

const selectAvatarColor = (color) => {
  form.icon_color = color || '';
  customColor.value = color || '';
};

const updateCustomColor = (value) => {
  const next = String(value || '').trim();
  form.icon_color = next;
  customColor.value = next;
};

const applyIconToForm = (value) => {
  const config = getIconConfig(value);
  form.icon_name = config.name;
  form.icon_color = config.color || '';
  customColor.value = form.icon_color || '';
};

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.is_shared = false;
  form.system_prompt = '';
  form.icon_name = DEFAULT_ICON_NAME;
  form.icon_color = '';
  customColor.value = '';
  avatarPanelVisible.value = false;
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
    const running = new Set();
    const waiting = new Set();
    items.forEach((item) => {
      const rawAgentId = String(item?.agent_id || '').trim();
      const agentId = rawAgentId || (item?.is_default ? DEFAULT_AGENT_KEY : '');
      if (!agentId) return;
      const state = String(item?.state || '').trim().toLowerCase();
      const pending = item?.pending_question === true || state === 'waiting';
      const isRunning =
        state === 'running' ||
        state === 'waiting' ||
        (!state && String(item?.expires_at || '').trim());
      if (isRunning) {
        running.add(agentId);
      }
      if (pending) {
        waiting.add(agentId);
      }
    });
    runningAgentIds.value = Array.from(running);
    waitingAgentIds.value = Array.from(waiting);
  } catch (error) {
    runningAgentIds.value = [];
    waitingAgentIds.value = [];
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
  applyIconToForm(agent.icon);
  avatarPanelVisible.value = false;
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
    const iconPayload = (() => {
      const name = normalizeIconName(form.icon_name);
      const color = String(form.icon_color || '').trim();
      if (name === DEFAULT_ICON_NAME && !color) return '';
      const payload = { name };
      if (color) {
        payload.color = color;
      }
      return JSON.stringify(payload);
    })();
    const payload = {
      name,
      description: form.description || '',
      is_shared: Boolean(form.is_shared),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      system_prompt: form.system_prompt || '',
      icon: iconPayload
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
