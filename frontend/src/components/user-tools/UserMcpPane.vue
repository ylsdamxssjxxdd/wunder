<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>{{ t('userTools.mcp.title') }}</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" :disabled="!hasConnected" @click="refreshAll">
          {{ t('userTools.mcp.action.refreshAll') }}
        </button>
        <button class="user-tools-btn secondary compact" type="button" @click="openImportModal">
          {{ t('userTools.mcp.action.import') }}
        </button>
        <button class="user-tools-btn compact" type="button" @click="addServer">
          {{ t('userTools.mcp.action.add') }}
        </button>
      </div>
    </div>
    <div class="tips">
      {{ t('userTools.mcp.tip') }}
    </div>

    <div class="management-layout">
      <div class="management-list">
        <div class="list-header">
          <label>{{ t('userTools.mcp.list.title') }}</label>
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
              <div>{{ server.display_name || server.name || t('userTools.mcp.server.unnamed') }}</div>
              <small>{{ buildServerSubtitle(server) }}</small>
            </button>
          </template>
          <div v-else class="empty-text">{{ t('userTools.mcp.list.emptyHint') }}</div>
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
              {{ t('common.selectAll') }}
            </button>
            <button
              class="user-tools-btn secondary"
              type="button"
              :disabled="!activeServer"
              @click="disableAllTools"
            >
              {{ t('common.unselectAll') }}
            </button>
          </div>
          <div class="actions">
            <button
              class="user-tools-btn secondary"
              type="button"
              :disabled="!activeServer"
              @click="openEditModal"
            >
              {{ t('userTools.mcp.action.edit') }}
            </button>
            <button
              class="user-tools-btn danger"
              type="button"
              :disabled="!activeServer"
              @click="removeServer"
            >
              {{ t('userTools.mcp.action.delete') }}
            </button>
          </div>
        </div>
        <div class="muted">{{ t('userTools.mcp.tip.tools') }}</div>

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
                <span>{{ t('userTools.action.enable') }}</span>
              </label>
              <label class="tool-check" @click.stop>
                <input
                  type="checkbox"
                  :checked="isToolShared(tool)"
                  @change="toggleToolShare(tool, $event.target.checked)"
                />
                <span>{{ t('userTools.action.share') }}</span>
              </label>
              <label class="tool-item-info">
                <strong>{{ tool.name }}</strong>
                <span class="muted">{{ tool.description || t('common.noDescription') }}</span>
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
          <label>{{ t('userTools.mcp.form.name') }}</label>
          <el-input
            v-model="activeServer.name"
            :placeholder="t('userTools.mcp.form.placeholder.name')"
            @input="scheduleSave"
          />
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.displayName') }}</label>
          <el-input
            v-model="activeServer.display_name"
            :placeholder="t('userTools.mcp.form.placeholder.displayName')"
            @input="scheduleSave"
          />
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.endpoint') }}</label>
          <el-input v-model="activeServer.endpoint" placeholder="http://127.0.0.1:9000/mcp" @input="scheduleSave" />
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.transport') }}</label>
          <el-select v-model="activeServer.transport" placeholder="auto" @change="scheduleSave">
            <el-option label="auto" value="" />
            <el-option label="sse" value="sse" />
            <el-option label="http" value="http" />
            <el-option label="streamable-http" value="streamable-http" />
          </el-select>
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.description') }}</label>
          <el-input
            v-model="activeServer.description"
            type="textarea"
            :placeholder="t('userTools.mcp.form.placeholder.description')"
            @input="scheduleSave"
          />
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.headers') }}</label>
          <el-input
            v-model="headersText"
            type="textarea"
            placeholder='{"Authorization":"Bearer ..."}'
            @input="handleHeadersInput"
          />
          <div class="error-text">{{ headersError }}</div>
        </div>
        <div class="form-row">
          <el-checkbox v-model="activeServer.enabled" @change="scheduleSave">
            {{ t('userTools.mcp.form.enabled') }}
          </el-checkbox>
        </div>
        <div class="form-row">
          <label>{{ t('userTools.mcp.form.structPreview') }}</label>
          <el-input type="textarea" :model-value="structPreview" readonly />
        </div>
      </div>
      <div v-else class="empty-text">{{ t('userTools.mcp.modal.empty') }}</div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="closeMcpModal">{{ t('common.cancel') }}</el-button>
        <el-button class="user-tools-footer-btn primary" @click="applyMcpModal">
          {{ t('common.save') }}
        </el-button>
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
          <div class="user-tools-title">{{ t('userTools.mcp.import.title') }}</div>
          <button class="icon-btn" type="button" @click="closeImportModal">×</button>
        </div>
      </template>
      <div class="user-tools-form">
        <div class="form-row">
          <label>{{ t('userTools.mcp.import.structLabel') }}</label>
          <el-input
            v-model="importContent"
            type="textarea"
            :placeholder="t('userTools.mcp.import.placeholder')"
          />
        </div>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="closeImportModal">{{ t('common.cancel') }}</el-button>
        <el-button class="user-tools-footer-btn primary" @click="applyImportModal">
          {{ t('userTools.mcp.action.import') }}
        </el-button>
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
          <div class="user-tools-title">
            {{ toolDetail?.title || t('userTools.mcp.tool.detailTitle') }}
          </div>
          <button class="icon-btn" type="button" @click="toolDetailVisible = false">×</button>
        </div>
      </template>
      <div class="user-tools-detail">
        <div class="detail-line">
          <span class="label">{{ t('userTools.mcp.tool.metaLabel') }}</span>
          <span>{{ toolDetail?.meta || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">{{ t('common.description') }}</span>
          <span>{{ toolDetail?.description || '-' }}</span>
        </div>
        <div class="detail-line">
          <span class="label">{{ t('userTools.mcp.tool.schemaLabel') }}</span>
        </div>
        <pre class="detail-schema">{{ toolDetail?.schema || t('userTools.mcp.tool.schemaEmpty') }}</pre>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="toolDetailVisible = false">
          {{ t('common.close') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserMcpServers, fetchUserMcpTools, saveUserMcpServers } from '@/api/userTools';
import { useI18n } from '@/i18n';

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
const { t } = useI18n();

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
const mcpModalTitle = ref(t('userTools.mcp.modal.editTitle'));

const headersText = ref('');
const headersError = ref('');
const importContent = ref('');
const toolDetail = ref(null);

const activeServer = computed(() => servers.value[selectedIndex.value] || null);
const activeTools = computed(() => toolsByIndex.value[selectedIndex.value] || []);
const hasConnected = computed(() =>
  toolsByIndex.value.some((tools) => Array.isArray(tools) && tools.length > 0)
);

const detailTitle = computed(
  () => activeServer.value?.display_name || activeServer.value?.name || t('userTools.mcp.detail.empty')
);
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
  metaParts.push(server.enabled !== false ? t('common.enabled') : t('common.disabled'));
  return metaParts.join(' · ');
});

const connectLabel = computed(() =>
  activeTools.value.length ? t('common.refresh') : t('userTools.mcp.action.connect')
);
const toolListMessage = computed(() => {
  if (!activeServer.value) {
    return t('userTools.mcp.tools.select');
  }
  if (!activeTools.value.length) {
    return t('userTools.mcp.tools.connectHint');
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
      return { headers: null, error: t('userTools.mcp.headers.mustObject') };
    }
    return { headers: parsed, error: '' };
  } catch (error) {
    return { headers: null, error: t('userTools.mcp.headers.parseFailed') };
  }
};

const getToolInputSchema = (tool) =>
  tool?.input_schema ?? tool?.inputSchema ?? tool?.args_schema ?? tool?.argsSchema ?? null;

const formatToolSchema = (schema) => {
  if (schema === null || schema === undefined) {
    return t('userTools.mcp.tool.schemaEmpty');
  }
  if (typeof schema === 'string') {
    const trimmed = schema.trim();
    return trimmed ? trimmed : t('userTools.mcp.tool.schemaEmpty');
  }
  if (Array.isArray(schema) && schema.length === 0) {
    return t('userTools.mcp.tool.schemaEmpty');
  }
  if (isPlainObject(schema) && Object.keys(schema).length === 0) {
    return t('userTools.mcp.tool.schemaEmpty');
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
    return t('userTools.mcp.struct.tip');
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
    ElMessage.error(error.response?.data?.detail || t('userTools.mcp.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveServers = async () => {
  const currentVersion = ++saveVersion.value;
  saving.value = true;
  emitStatus(t('userTools.saving'));
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
    emitStatus(t('userTools.autoSaved'));
  } catch (error) {
    if (currentVersion !== saveVersion.value) {
      return;
    }
    emitStatus(t('userTools.saveFailed', { message: error.message || t('common.requestFailed') }));
    ElMessage.error(error.response?.data?.detail || t('userTools.mcp.saveFailed'));
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
  mcpModalTitle.value = t('userTools.mcp.modal.editTitle');
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
    ElMessage.warning(t('userTools.mcp.headers.invalid'));
    return;
  }
  await saveServers();
  closeMcpModal();
  ElMessage.success(t('userTools.mcp.saved'));
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
    ElMessage.warning(t('userTools.mcp.struct.required'));
    return;
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    ElMessage.error(t('userTools.mcp.struct.parseFailed'));
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
    ElMessage.warning(t('userTools.mcp.struct.noValid'));
    return;
  }
  let lastIndex = selectedIndex.value;
  imported.forEach((server) => {
    lastIndex = upsertUserMcpServer(server);
  });
  selectedIndex.value = lastIndex;
  await saveServers();
  closeImportModal();
  ElMessage.success(t('userTools.mcp.import.success'));
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
  mcpModalTitle.value = t('userTools.mcp.modal.addTitle');
  headersText.value = '';
  headersError.value = '';
  mcpModalVisible.value = true;
};

const removeServer = async () => {
  if (selectedIndex.value < 0) return;
  const removed = servers.value[selectedIndex.value];
  const removedName =
    removed?.display_name || removed?.name || t('userTools.mcp.server.defaultName');
  try {
    await ElMessageBox.confirm(
      t('userTools.mcp.deleteConfirm', { name: removedName }),
      t('common.notice'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
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
  ElMessage.success(t('userTools.mcp.deleteSuccess', { name: removedName }));
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
    ElMessage.error(t('userTools.mcp.connectFailed'));
    return;
  }
  ElMessage.success(
    wasConnected ? t('userTools.mcp.refreshSuccess') : t('userTools.mcp.connectSuccess')
  );
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
    ElMessage.error(t('userTools.mcp.refreshFailed'));
    return;
  }
  ElMessage.success(t('userTools.mcp.refreshAllSuccess'));
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
  const serverTitle = server.display_name || server.name || t('userTools.mcp.server.unnamed');
  const metaParts = [
    t('userTools.mcp.meta.title'),
    t('userTools.mcp.meta.server', { name: serverTitle })
  ];
  metaParts.push(
    server.enabled !== false ? t('userTools.mcp.meta.serverEnabled') : t('userTools.mcp.meta.serverDisabled')
  );
  metaParts.push(isToolEnabled(tool) ? t('common.enabled') : t('common.disabled'));
  metaParts.push(isToolShared(tool) ? t('userTools.shared.on') : t('userTools.shared.off'));
  toolDetail.value = {
    title: tool.name || t('userTools.mcp.tool.detailTitle'),
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
