<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>我的 MCP 服务</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" :disabled="!hasConnected" @click="refreshAll">
          全部刷新
        </button>
        <button class="user-tools-btn secondary compact" type="button" @click="openImportModal">
          导入服务
        </button>
        <button class="user-tools-btn compact" type="button" @click="addServer">
          新增服务
        </button>
      </div>
    </div>
    <div class="tips">
      配置个人 MCP 服务并选择共享工具，工具会以 user_id@server@tool 形式注入系统提示词。
    </div>

    <div class="management-layout">
      <div class="management-list">
        <div class="list-header">
          <label>服务列表</label>
        </div>
        <div class="list-body">
          <template v-if="servers.length">
            <button
              v-for="(server, index) in servers"
              :key="`${server.name || index}`"
              class="list-item"
              :class="{ active: index === selectedIndex }"
              type="button"
              @click="selectServer(index)"
            >
              <div>{{ server.display_name || server.name || '未命名服务' }}</div>
              <small>{{ buildServerSubtitle(server) }}</small>
            </button>
          </template>
          <div v-else class="empty-text">暂无 MCP 服务，请新增或导入。</div>
        </div>
      </div>

      <div class="management-detail">
        <div class="detail-header">
          <div>
            <div class="detail-title">{{ detailTitle }}</div>
            <div class="muted">{{ detailMeta }}</div>
            <div class="muted">{{ detailDesc }}</div>
          </div>
        </div>

        <div class="detail-actions">
          <div class="actions">
            <button
              class="user-tools-btn"
              type="button"
              :disabled="!activeServer"
              @click="connectServer"
            >
              {{ connectLabel }}
            </button>
            <button
              class="user-tools-btn secondary"
              type="button"
              :disabled="!activeServer || !activeTools.length"
              @click="enableAllTools"
            >
              全选
            </button>
            <button
              class="user-tools-btn secondary"
              type="button"
              :disabled="!activeServer"
              @click="disableAllTools"
            >
              全不选
            </button>
          </div>
          <div class="actions">
            <button
              class="user-tools-btn secondary"
              type="button"
              :disabled="!activeServer"
              @click="openEditModal"
            >
              编辑服务
            </button>
            <button
              class="user-tools-btn danger"
              type="button"
              :disabled="!activeServer"
              @click="removeServer"
            >
              删除服务
            </button>
          </div>
        </div>
        <div class="muted">勾选启用/共享后将出现在系统提示词工具列表中。</div>

        <div class="tool-list">
          <div v-if="toolListMessage" class="empty-text">{{ toolListMessage }}</div>
          <div
            v-for="tool in activeTools"
            :key="tool.name"
            class="tool-item tool-item-dual"
            @click="openToolDetail(tool)"
          >
            <label class="tool-check" @click.stop>
              <input
                type="checkbox"
                :checked="isToolEnabled(tool)"
                @change="toggleToolEnable(tool, $event.target.checked)"
              />
              <span>启用</span>
            </label>
            <label class="tool-check" @click.stop>
              <input
                type="checkbox"
                :checked="isToolShared(tool)"
                @change="toggleToolShare(tool, $event.target.checked)"
              />
              <span>共享</span>
            </label>
            <label class="tool-item-info">
              <strong>{{ tool.name }}</strong>
              <span class="muted">{{ tool.description || '暂无描述' }}</span>
            </label>
          </div>
        </div>
      </div>
    </div>

    <el-dialog
      v-model="mcpModalVisible"
      class="user-tools-dialog user-tools-subdialog"
      width="640px"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ mcpModalTitle }}</div>
          <button class="icon-btn" type="button" @click="closeMcpModal">×</button>
        </div>
      </template>
      <div v-if="activeServer" class="user-tools-form">
        <div class="form-row">
          <label>服务名称</label>
          <el-input v-model="activeServer.name" placeholder="例如：local_tools" @input="scheduleSave" />
        </div>
        <div class="form-row">
          <label>显示名称（可选）</label>
          <el-input v-model="activeServer.display_name" placeholder="用于前端展示" @input="scheduleSave" />
        </div>
        <div class="form-row">
          <label>服务地址</label>
          <el-input v-model="activeServer.endpoint" placeholder="http://127.0.0.1:9000/mcp" @input="scheduleSave" />
        </div>
        <div class="form-row">
          <label>传输类型</label>
          <el-select v-model="activeServer.transport" placeholder="auto" @change="scheduleSave">
            <el-option label="auto" value="" />
            <el-option label="sse" value="sse" />
            <el-option label="http" value="http" />
            <el-option label="streamable-http" value="streamable-http" />
          </el-select>
        </div>
        <div class="form-row">
          <label>服务描述（可选）</label>
          <el-input
            v-model="activeServer.description"
            type="textarea"
            placeholder="用于记录服务作用"
            @input="scheduleSave"
          />
        </div>
        <div class="form-row">
          <label>Headers JSON（可选）</label>
          <el-input
            v-model="headersText"
            type="textarea"
            placeholder='{"Authorization":"Bearer ..."}'
            @input="handleHeadersInput"
          />
          <div class="error-text">{{ headersError }}</div>
        </div>
        <div class="form-row">
          <el-checkbox v-model="activeServer.enabled" @change="scheduleSave">启用服务</el-checkbox>
        </div>
        <div class="form-row">
          <label>JSON 结构体预览（可复制）</label>
          <el-input type="textarea" :model-value="structPreview" readonly />
        </div>
      </div>
      <div v-else class="empty-text">请先选择服务。</div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="closeMcpModal">取消</el-button>
        <el-button class="user-tools-footer-btn primary" @click="applyMcpModal">保存</el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="importModalVisible"
      class="user-tools-dialog user-tools-subdialog"
      width="640px"
      :show-close="false"
      :close-on-click-modal="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">导入 MCP 服务</div>
          <button class="icon-btn" type="button" @click="closeImportModal">×</button>
        </div>
      </template>
      <div class="user-tools-form">
        <div class="form-row">
          <label>MCP 结构体（JSON）</label>
          <el-input v-model="importContent" type="textarea" placeholder="请输入 JSON 结构体" />
        </div>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="closeImportModal">取消</el-button>
        <el-button class="user-tools-footer-btn primary" @click="applyImportModal">导入</el-button>
      </template>
    </el-dialog>

    <el-dialog
      v-model="toolDetailVisible"
      class="user-tools-dialog user-tools-subdialog"
      width="640px"
      :show-close="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ toolDetail?.title || '工具详情' }}</div>
          <button class="icon-btn" type="button" @click="toolDetailVisible = false">×</button>
        </div>
      </template>
      <div class="user-tools-detail">
        <div class="detail-line">
          <span class="label">说明</span>
          <span>{{ toolDetail?.meta || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">描述</span>
          <span>{{ toolDetail?.description || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">输入结构</span>
        </div>
        <pre class="detail-schema">{{ toolDetail?.schema || '（无输入结构）' }}</pre>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="toolDetailVisible = false">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserMcpServers, fetchUserMcpTools, saveUserMcpServers } from '@/api/userTools';

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  active: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['status']);

const servers = ref([]);
const toolsByIndex = ref([]);
const selectedIndex = ref(-1);
const loaded = ref(false);
const loading = ref(false);
const saving = ref(false);
const saveVersion = ref(0);
const saveTimer = ref(null);

const mcpModalVisible = ref(false);
const importModalVisible = ref(false);
const toolDetailVisible = ref(false);
const mcpModalTitle = ref('编辑 MCP 服务');

const headersText = ref('');
const headersError = ref('');
const importContent = ref('');
const toolDetail = ref(null);

const activeServer = computed(() => servers.value[selectedIndex.value] || null);
const activeTools = computed(() => toolsByIndex.value[selectedIndex.value] || []);
const hasConnected = computed(() =>
  toolsByIndex.value.some((tools) => Array.isArray(tools) && tools.length > 0)
);

const detailTitle = computed(() => activeServer.value?.display_name || activeServer.value?.name || '未选择服务');
const detailDesc = computed(() => activeServer.value?.description || '');
const detailMeta = computed(() => {
  const server = activeServer.value;
  if (!server) {
    return '';
  }
  const metaParts = [];
  if (server.display_name && server.name) {
    metaParts.push(`ID: ${server.name}`);
  }
  if (server.endpoint) {
    metaParts.push(server.endpoint);
  }
  if (server.transport) {
    metaParts.push(`transport=${server.transport}`);
  }
  metaParts.push(server.enabled !== false ? '已启用' : '未启用');
  return metaParts.join(' · ');
});

const connectLabel = computed(() => (activeTools.value.length ? '刷新' : '连接'));
const toolListMessage = computed(() => {
  if (!activeServer.value) {
    return '请选择一个服务。';
  }
  if (!activeTools.value.length) {
    return '尚未加载工具，请先连接服务。';
  }
  return '';
});

const structPreview = computed(() => buildUserMcpStructPreview(activeServer.value));

const emitStatus = (message) => {
  emit('status', message || '');
};

const isPlainObject = (value) =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

const parseHeadersValue = (raw) => {
  if (!raw || !raw.trim()) {
    return { headers: {}, error: '' };
  }
  try {
    const parsed = JSON.parse(raw);
    if (!isPlainObject(parsed)) {
      return { headers: null, error: '请求头 JSON 必须是对象' };
    }
    return { headers: parsed, error: '' };
  } catch (error) {
    return { headers: null, error: '请求头 JSON 解析失败' };
  }
};

const getToolInputSchema = (tool) =>
  tool?.input_schema ?? tool?.inputSchema ?? tool?.args_schema ?? tool?.argsSchema ?? null;

const formatToolSchema = (schema) => {
  if (schema === null || schema === undefined) {
    return '（无输入结构）';
  }
  if (typeof schema === 'string') {
    const trimmed = schema.trim();
    return trimmed ? trimmed : '（无输入结构）';
  }
  if (Array.isArray(schema) && schema.length === 0) {
    return '（无输入结构）';
  }
  if (isPlainObject(schema) && Object.keys(schema).length === 0) {
    return '（无输入结构）';
  }
  try {
    return JSON.stringify(schema, null, 2);
  } catch (error) {
    return String(schema || '');
  }
};

const normalizeUserMcpServer = (server) => {
  const headers = isPlainObject(server?.headers) ? server.headers : {};
  const rawToolSpecs = Array.isArray(server?.tool_specs)
    ? server.tool_specs
    : Array.isArray(server?.toolSpecs)
    ? server.toolSpecs
    : [];
  return {
    name: server?.name || '',
    display_name: server?.display_name || server?.displayName || '',
    endpoint: server?.endpoint || server?.baseUrl || server?.base_url || server?.url || '',
    transport: server?.transport || server?.type || '',
    description: server?.description || '',
    headers,
    auth: server?.auth || '',
    allow_tools: Array.isArray(server?.allow_tools) ? server.allow_tools : [],
    shared_tools: Array.isArray(server?.shared_tools) ? server.shared_tools : [],
    enabled: server?.enabled !== false,
    tool_specs: rawToolSpecs
  };
};

const buildUserMcpStructPreview = (server) => {
  if (!server || !server.name || !server.endpoint) {
    return '填写服务名称与服务地址后生成结构体。';
  }
  const config = {
    type: server.transport || undefined,
    description: server.description || undefined,
    isActive: server.enabled !== false,
    name: server.display_name || server.name,
    baseUrl: server.endpoint,
    headers: server.headers && Object.keys(server.headers).length ? server.headers : undefined
  };
  const cleaned = {};
  Object.entries(config).forEach(([key, value]) => {
    if (value !== undefined && value !== '') {
      cleaned[key] = value;
    }
  });
  return JSON.stringify({ mcpServers: { [server.name]: cleaned } }, null, 2);
};

const buildUserMcpServerFromConfig = (serverId, rawConfig) => {
  const config = rawConfig || {};
  const endpoint = config.baseUrl || config.base_url || config.url || config.endpoint || '';
  const name = String(serverId || config.id || config.name || '').trim();
  if (!name || !endpoint) {
    return null;
  }
  let displayName = config.display_name || config.displayName || '';
  displayName = String(displayName || '').trim();
  let headers = config.headers || {};
  if (typeof headers === 'string') {
    try {
      headers = JSON.parse(headers);
    } catch (error) {
      headers = {};
    }
  }
  if (!isPlainObject(headers)) {
    headers = {};
  }
  return normalizeUserMcpServer({
    name,
    display_name: displayName,
    endpoint,
    transport: config.type || config.transport || '',
    description: config.description || '',
    headers,
    auth: config.auth || '',
    allow_tools: config.allow_tools || config.allowTools || [],
    enabled: config.isActive ?? config.enabled ?? true,
    tool_specs: []
  });
};

const upsertUserMcpServer = (incoming) => {
  const targetIndex = servers.value.findIndex((item) => item.name === incoming.name);
  if (targetIndex >= 0) {
    const previous = servers.value[targetIndex];
    const allowTools =
      Array.isArray(incoming.allow_tools) && incoming.allow_tools.length
        ? incoming.allow_tools
        : previous.allow_tools;
    const toolSpecs =
      Array.isArray(incoming.tool_specs) && incoming.tool_specs.length
        ? incoming.tool_specs
        : previous.tool_specs;
    servers.value[targetIndex] = { ...previous, ...incoming, allow_tools: allowTools, tool_specs: toolSpecs };
    toolsByIndex.value[targetIndex] = toolSpecs || [];
    return targetIndex;
  }
  servers.value.push(incoming);
  toolsByIndex.value.push(incoming.tool_specs || []);
  return servers.value.length - 1;
};

const loadServers = async () => {
  if (loading.value) return;
  loading.value = true;
  try {
    const { data } = await fetchUserMcpServers();
    const payload = data?.data || {};
    const list = Array.isArray(payload.servers) ? payload.servers : [];
    const normalized = list.map(normalizeUserMcpServer);
    servers.value = normalized;
    toolsByIndex.value = normalized.map((server) => server.tool_specs || []);
    selectedIndex.value = servers.value.length ? 0 : -1;
    loaded.value = true;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || 'MCP 服务加载失败');
  } finally {
    loading.value = false;
  }
};

const saveServers = async () => {
  const currentVersion = ++saveVersion.value;
  saving.value = true;
  emitStatus('正在保存...');
  try {
    const payload = {
      servers: servers.value.map((server) => ({
        name: server.name,
        display_name: server.display_name,
        endpoint: server.endpoint,
        transport: server.transport,
        description: server.description,
        headers: server.headers || {},
        auth: server.auth || '',
        tool_specs: Array.isArray(server.tool_specs) ? server.tool_specs : [],
        allow_tools: Array.isArray(server.allow_tools) ? server.allow_tools : [],
        shared_tools: Array.isArray(server.shared_tools) ? server.shared_tools : [],
        enabled: server.enabled !== false
      }))
    };
    const { data } = await saveUserMcpServers(payload);
    if (currentVersion !== saveVersion.value) {
      return;
    }
    const result = data?.data || {};
    const list = Array.isArray(result.servers) ? result.servers : [];
    const normalized = list.map(normalizeUserMcpServer);
    servers.value = normalized;
    toolsByIndex.value = normalized.map((server) => server.tool_specs || []);
    if (!servers.value.length) {
      selectedIndex.value = -1;
    } else if (selectedIndex.value >= servers.value.length) {
      selectedIndex.value = 0;
    }
    emitStatus('已自动保存。');
  } catch (error) {
    if (currentVersion !== saveVersion.value) {
      return;
    }
    emitStatus(`保存失败：${error.message || '请求失败'}`);
    ElMessage.error(error.response?.data?.detail || '自建 MCP 保存失败');
  } finally {
    if (currentVersion === saveVersion.value) {
      saving.value = false;
    }
  }
};

// 输入即保存，节流避免频繁写入
const scheduleSave = () => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
  saveTimer.value = setTimeout(() => {
    saveTimer.value = null;
    saveServers();
  }, 600);
};

const selectServer = (index) => {
  selectedIndex.value = index;
};

const buildServerSubtitle = (server) => {
  const parts = [];
  if (server.display_name && server.name) {
    parts.push(`ID: ${server.name}`);
  }
  parts.push(server.endpoint || '-');
  return parts.join(' · ');
};

const handleHeadersInput = () => {
  const server = activeServer.value;
  if (!server) return;
  const parsed = parseHeadersValue(headersText.value);
  if (parsed.error) {
    headersError.value = parsed.error;
    return;
  }
  headersError.value = '';
  server.headers = parsed.headers || {};
  scheduleSave();
};

const openEditModal = () => {
  if (!activeServer.value) return;
  mcpModalTitle.value = '编辑 MCP 服务';
  headersText.value = activeServer.value.headers && Object.keys(activeServer.value.headers).length
    ? JSON.stringify(activeServer.value.headers, null, 2)
    : '';
  headersError.value = '';
  mcpModalVisible.value = true;
};

const closeMcpModal = () => {
  mcpModalVisible.value = false;
};

const applyMcpModal = async () => {
  if (headersError.value) {
    ElMessage.warning('Headers JSON 格式有误，请先修正。');
    return;
  }
  await saveServers();
  closeMcpModal();
  ElMessage.success('MCP 服务已保存。');
};

const openImportModal = () => {
  importContent.value = '';
  importModalVisible.value = true;
};

const closeImportModal = () => {
  importModalVisible.value = false;
};

const applyImportModal = async () => {
  const raw = (importContent.value || '').trim();
  if (!raw) {
    ElMessage.warning('请先输入 MCP 结构体。');
    return;
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    ElMessage.error('MCP 结构体 JSON 解析失败。');
    return;
  }
  const imported = [];
  if (parsed.mcpServers && isPlainObject(parsed.mcpServers)) {
    Object.entries(parsed.mcpServers).forEach(([serverId, config]) => {
      const server = buildUserMcpServerFromConfig(serverId, config);
      if (server) {
        imported.push(server);
      }
    });
  } else {
    const serverId = parsed.id || parsed.name || '';
    const server = buildUserMcpServerFromConfig(serverId, parsed);
    if (server) {
      imported.push(server);
    }
  }
  if (!imported.length) {
    ElMessage.warning('未识别到可用的 MCP 服务结构。');
    return;
  }
  let lastIndex = selectedIndex.value;
  imported.forEach((server) => {
    lastIndex = upsertUserMcpServer(server);
  });
  selectedIndex.value = lastIndex;
  await saveServers();
  closeImportModal();
  ElMessage.success('MCP 服务已导入并保存。');
};

const addServer = () => {
  const next = normalizeUserMcpServer({
    name: '',
    display_name: '',
    endpoint: '',
    transport: '',
    description: '',
    headers: {},
    allow_tools: [],
    shared_tools: [],
    enabled: true,
    tool_specs: []
  });
  servers.value.push(next);
  toolsByIndex.value.push([]);
  selectedIndex.value = servers.value.length - 1;
  mcpModalTitle.value = '新增 MCP 服务';
  headersText.value = '';
  headersError.value = '';
  mcpModalVisible.value = true;
};

const removeServer = async () => {
  if (selectedIndex.value < 0) return;
  const removed = servers.value[selectedIndex.value];
  const removedName = removed?.display_name || removed?.name || 'MCP 服务';
  try {
    await ElMessageBox.confirm(`确认删除 ${removedName} 吗？`, '提示', {
      confirmButtonText: '删除',
      cancelButtonText: '取消',
      type: 'warning'
    });
  } catch (error) {
    return;
  }
  servers.value.splice(selectedIndex.value, 1);
  toolsByIndex.value.splice(selectedIndex.value, 1);
  if (!servers.value.length) {
    selectedIndex.value = -1;
  } else {
    selectedIndex.value = Math.max(0, selectedIndex.value - 1);
  }
  scheduleSave();
  ElMessage.success(`已删除 ${removedName}`);
};

const connectServerAtIndex = async (index) => {
  const server = servers.value[index];
  if (!server || !server.name || !server.endpoint) {
    return false;
  }
  const payload = {
    name: server.name,
    endpoint: server.endpoint,
    transport: server.transport || null,
    headers: server.headers || {},
    auth: server.auth || null
  };
  try {
    const { data } = await fetchUserMcpTools(payload);
    const result = data?.data || {};
    const tools = Array.isArray(result.tools) ? result.tools : [];
    toolsByIndex.value[index] = tools;
    server.tool_specs = tools;
    scheduleSave();
    return true;
  } catch (error) {
    return false;
  }
};

const connectServer = async () => {
  const index = selectedIndex.value;
  if (index < 0) return;
  const wasConnected = toolsByIndex.value[index]?.length;
  const ok = await connectServerAtIndex(index);
  if (!ok) {
    ElMessage.error('MCP 连接失败，请检查服务信息。');
    return;
  }
  ElMessage.success(wasConnected ? 'MCP 工具已刷新。' : 'MCP 工具已连接。');
};

const refreshAll = async () => {
  const connectedIndexes = toolsByIndex.value
    .map((tools, index) => (Array.isArray(tools) && tools.length ? index : -1))
    .filter((index) => index >= 0);
  if (!connectedIndexes.length) return;
  let updated = false;
  for (const index of connectedIndexes) {
    const ok = await connectServerAtIndex(index);
    if (ok) {
      updated = true;
    }
  }
  if (!updated) {
    ElMessage.error('MCP 刷新失败，请检查服务信息。');
    return;
  }
  ElMessage.success('已刷新所有已连接 MCP 服务。');
};

const isToolEnabled = (tool) => {
  const server = activeServer.value;
  if (!server) return false;
  const allowList = Array.isArray(server.allow_tools) ? server.allow_tools : [];
  const implicitAll = server.enabled !== false && allowList.length === 0;
  return implicitAll || allowList.includes(tool.name);
};

const isToolShared = (tool) => {
  const server = activeServer.value;
  if (!server) return false;
  const sharedList = Array.isArray(server.shared_tools) ? server.shared_tools : [];
  return sharedList.includes(tool.name);
};

const toggleToolEnable = (tool, checked) => {
  const server = activeServer.value;
  if (!server) return;
  const tools = activeTools.value || [];
  const allowList = Array.isArray(server.allow_tools) ? server.allow_tools : [];
  const implicitAll = server.enabled !== false && allowList.length === 0;
  let nextAllow = implicitAll ? tools.map((item) => item.name) : allowList.slice();
  if (checked) {
    if (!nextAllow.includes(tool.name)) {
      nextAllow.push(tool.name);
    }
    server.enabled = true;
  } else {
    nextAllow = nextAllow.filter((name) => name !== tool.name);
    server.shared_tools = (Array.isArray(server.shared_tools) ? server.shared_tools : []).filter(
      (name) => name !== tool.name
    );
    if (nextAllow.length === 0) {
      server.enabled = false;
    }
  }
  server.allow_tools = nextAllow;
  scheduleSave();
};

const toggleToolShare = (tool, checked) => {
  const server = activeServer.value;
  if (!server) return;
  const tools = activeTools.value || [];
  const allowList = Array.isArray(server.allow_tools) ? server.allow_tools : [];
  const implicitAll = server.enabled !== false && allowList.length === 0;
  let nextAllow = implicitAll ? tools.map((item) => item.name) : allowList.slice();
  let nextShared = Array.isArray(server.shared_tools) ? server.shared_tools.slice() : [];
  if (checked) {
    if (!nextShared.includes(tool.name)) {
      nextShared.push(tool.name);
    }
    if (!nextAllow.includes(tool.name)) {
      nextAllow.push(tool.name);
    }
    server.enabled = true;
  } else {
    nextShared = nextShared.filter((name) => name !== tool.name);
  }
  server.allow_tools = nextAllow;
  server.shared_tools = nextShared;
  scheduleSave();
};

const enableAllTools = () => {
  const server = activeServer.value;
  if (!server || !activeTools.value.length) return;
  server.enabled = true;
  server.allow_tools = activeTools.value.map((tool) => tool.name);
  scheduleSave();
};

const disableAllTools = () => {
  const server = activeServer.value;
  if (!server) return;
  server.allow_tools = [];
  server.shared_tools = [];
  server.enabled = false;
  scheduleSave();
};

const openToolDetail = (tool) => {
  const server = activeServer.value;
  if (!tool || !server) return;
  const serverTitle = server.display_name || server.name || '未命名服务';
  const metaParts = ['自建 MCP 工具', `服务: ${serverTitle}`];
  metaParts.push(server.enabled !== false ? '服务已启用' : '服务未启用');
  metaParts.push(isToolEnabled(tool) ? '已启用' : '未启用');
  metaParts.push(isToolShared(tool) ? '已共享' : '未共享');
  toolDetail.value = {
    title: tool.name || '工具详情',
    meta: metaParts.join(' · '),
    description: tool.description || '',
    schema: formatToolSchema(getToolInputSchema(tool))
  };
  toolDetailVisible.value = true;
};

watch(
  () => props.visible,
  (value) => {
    if (value && !loaded.value) {
      loadServers();
    }
  }
);

watch(activeServer, (server) => {
  if (!mcpModalVisible.value || !server) return;
  headersText.value =
    server.headers && Object.keys(server.headers).length ? JSON.stringify(server.headers, null, 2) : '';
  headersError.value = '';
});

onBeforeUnmount(() => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
});
</script>
