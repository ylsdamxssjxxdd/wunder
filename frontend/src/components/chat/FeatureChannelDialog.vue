<template>
  <el-dialog
    v-model="visible"
    class="feature-window-dialog feature-window-dialog--channel"
    width="1080px"
    top="8vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="feature-window-header">
        <div class="feature-window-title">{{ t('chat.features.channels') }}</div>
        <button class="feature-window-close" type="button" @click="visible = false">×</button>
      </div>
    </template>
    <div class="feature-window-body">
      <div class="feature-window-toolbar">
        <div class="feature-window-hint">{{ t('channels.subtitle') }}</div>
        <button class="feature-window-btn" type="button" :disabled="loading" @click="refreshAll">
          {{ t('common.refresh') }}
        </button>
      </div>
      <div class="feature-window-grid channels-grid">
        <div class="feature-window-list">
          <div v-if="loading" class="feature-window-empty">{{ t('common.loading') }}</div>
          <div v-else-if="!accounts.length" class="feature-window-empty">{{ t('channels.list.empty') }}</div>
          <button
            v-for="account in accounts"
            :key="account.key"
            class="feature-window-item"
            :class="{ active: selectedKey === account.key }"
            type="button"
            @click="selectAccount(account)"
          >
            <div class="feature-window-item-title">{{ account.label }}</div>
            <div class="feature-window-item-meta">
              <span>{{ account.account_id }}</span>
              <span>{{ account.active ? t('channels.status.enabled') : t('channels.status.disabled') }}</span>
            </div>
            <div class="feature-window-item-sub">{{ account.desc }}</div>
          </button>
        </div>
        <div class="feature-window-detail">
          <div v-if="!selectedAccount" class="feature-window-empty">{{ t('channels.detail.empty') }}</div>
          <template v-else>
            <div class="feature-window-item-title">{{ selectedAccount.label }}</div>
            <div class="feature-window-kv">
              <div>{{ t('channels.field.account') }}</div>
              <div>{{ selectedAccount.account_id }}</div>
            </div>
            <div class="feature-window-form-grid">
              <label class="feature-window-field">
                <span>{{ t('channels.bind.peerKind') }}</span>
                <select v-model="form.peer_kind" class="feature-window-input">
                  <option v-for="option in peerKindOptions" :key="option.value" :value="option.value">
                    {{ option.label }}
                  </option>
                </select>
              </label>
              <label class="feature-window-field">
                <span>{{ t('channels.bind.peerId') }}</span>
                <input
                  v-model="form.peer_id"
                  class="feature-window-input"
                  :placeholder="t('channels.bind.peerId.placeholder')"
                />
              </label>
              <label class="feature-window-field feature-window-field-full">
                <span>{{ t('channels.bind.agent') }}</span>
                <select v-model="form.agent_id" class="feature-window-input">
                  <option value="">{{ t('channels.bind.defaultAgent') }}</option>
                  <option v-for="item in agentOptions" :key="item.id" :value="item.id">
                    {{ item.label }}
                  </option>
                </select>
              </label>
            </div>
            <div class="feature-window-actions">
              <button class="feature-window-btn" type="button" :disabled="bindingSaving" @click="saveBinding">
                {{ bindingSaving ? t('common.saving') : t('common.save') }}
              </button>
            </div>
            <div class="feature-window-runs-title">{{ t('channels.bindings.title') }}</div>
            <div v-if="bindingLoading" class="feature-window-empty">{{ t('common.loading') }}</div>
            <div v-else-if="!filteredBindings.length" class="feature-window-empty">
              {{ t('channels.bindings.empty') }}
            </div>
            <div v-else class="feature-window-runs">
              <div v-for="binding in filteredBindings" :key="binding.key" class="feature-window-run-item">
                <span>{{ binding.peer_kind }} · {{ binding.peer_id }}</span>
                <button class="feature-window-btn danger" type="button" @click="removeBinding(binding)">
                  {{ t('channels.action.unbind') }}
                </button>
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup>
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteChannelBinding,
  listChannelAccounts,
  listChannelBindings,
  upsertChannelBinding
} from '@/api/channels';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  },
  agentId: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['update:modelValue']);
const { t } = useI18n();
const agentStore = useAgentStore();

const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const contextAgentId = computed(() => {
  const value = String(props.agentId || '').trim();
  if (!value || value === '__default__' || value === 'default') {
    return '';
  }
  return value;
});

const CHANNEL_META = {
  whatsapp: {
    labelKey: 'channels.provider.whatsapp',
    descKey: 'channels.provider.whatsapp.desc',
    defaultPeerKind: 'dm'
  },
  wechat: {
    labelKey: 'channels.provider.wechat',
    descKey: 'channels.provider.wechat.desc',
    defaultPeerKind: 'user'
  },
  feishu: {
    labelKey: 'channels.provider.feishu',
    descKey: 'channels.provider.feishu.desc',
    defaultPeerKind: 'user'
  },
  qqbot: {
    labelKey: 'channels.provider.qqbot',
    descKey: 'channels.provider.qqbot.desc',
    defaultPeerKind: 'user'
  },
  telegram: {
    labelKey: 'channels.provider.telegram',
    descKey: 'channels.provider.telegram.desc',
    defaultPeerKind: 'user'
  }
};

const peerKindOptions = computed(() => [
  { value: 'dm', label: t('channels.peerKind.dm') },
  { value: 'group', label: t('channels.peerKind.group') },
  { value: 'channel', label: t('channels.peerKind.channel') },
  { value: 'user', label: t('channels.peerKind.user') }
]);

const agentOptions = computed(() => {
  const options = [];
  const pushItem = (agent, suffix) => {
    if (!agent?.id) return;
    const name = agent.name || agent.id;
    options.push({
      id: agent.id,
      label: suffix ? name + ' ' + suffix : name
    });
  };
  (agentStore.agents || []).forEach((agent) => pushItem(agent, ''));
  (agentStore.sharedAgents || []).forEach((agent) =>
    pushItem(agent, '(' + t('channels.agent.shared') + ')')
  );
  return options;
});

const accounts = ref([]);
const bindings = ref([]);
const loading = ref(false);
const bindingLoading = ref(false);
const bindingSaving = ref(false);
const selectedKey = ref('');

const form = reactive({
  peer_kind: 'dm',
  peer_id: '',
  agent_id: ''
});

const selectedAccount = computed(() => accounts.value.find((item) => item.key === selectedKey.value) || null);

const filteredBindings = computed(() => {
  if (!selectedAccount.value) return [];
  return bindings.value.filter((binding) => {
    const sameAccount =
      binding.channel === selectedAccount.value.channel &&
      binding.account_id === selectedAccount.value.account_id;
    if (!sameAccount) return false;
    if (!contextAgentId.value) return true;
    return String(binding.agent_id || '').trim() === contextAgentId.value;
  });
});

const normalizeAccount = (record) => {
  const channel = String(record?.channel || '').trim();
  const accountId = String(record?.account_id || '').trim();
  if (!channel || !accountId) return null;
  const key = channel + '::' + accountId;
  const meta = CHANNEL_META[channel.toLowerCase()] || {};
  return {
    key,
    channel,
    account_id: accountId,
    active: String(record?.status || '').toLowerCase() === 'active' || !record?.status,
    label: meta.labelKey ? t(meta.labelKey) : channel,
    desc: meta.descKey ? t(meta.descKey) : t('channels.provider.generic'),
    defaultPeerKind: meta.defaultPeerKind || 'dm'
  };
};

const normalizeBinding = (record) => {
  const channel = String(record?.channel || '').trim();
  const accountId = String(record?.account_id || '').trim();
  const peerKind = String(record?.peer_kind || '').trim();
  const peerId = String(record?.peer_id || '').trim();
  if (!channel || !accountId || !peerKind || !peerId) return null;
  return {
    key: channel + ':' + accountId + ':' + peerKind + ':' + peerId,
    channel,
    account_id: accountId,
    peer_kind: peerKind,
    peer_id: peerId,
    agent_id: record?.agent_id || ''
  };
};

const loadAccounts = async () => {
  loading.value = true;
  try {
    const { data } = await listChannelAccounts();
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    accounts.value = items.map((item) => normalizeAccount(item)).filter(Boolean);
    if (!accounts.value.length) {
      selectedKey.value = '';
      resetForm();
      return;
    }
    if (!selectedKey.value || !accounts.value.find((item) => item.key === selectedKey.value)) {
      selectedKey.value = accounts.value[0].key;
      resetForm();
    }
  } catch (error) {
    showApiError(error, t('channels.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const loadBindings = async () => {
  bindingLoading.value = true;
  try {
    const params = contextAgentId.value ? { agent_id: contextAgentId.value } : undefined;
    const { data } = await listChannelBindings(params);
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    bindings.value = items.map((item) => normalizeBinding(item)).filter(Boolean);
  } catch (error) {
    showApiError(error, t('channels.bindings.loadFailed'));
  } finally {
    bindingLoading.value = false;
  }
};

const refreshAll = async () => {
  await Promise.all([agentStore.loadAgents(), loadAccounts()]);
  await loadBindings();
};

const resetForm = () => {
  const account = selectedAccount.value;
  form.peer_kind = account?.defaultPeerKind || 'dm';
  form.peer_id = '';
  form.agent_id = contextAgentId.value || '';
};

const selectAccount = (account) => {
  selectedKey.value = account.key;
  resetForm();
};

const saveBinding = async () => {
  const account = selectedAccount.value;
  if (!account) return;
  const peerId = String(form.peer_id || '').trim();
  if (!peerId) {
    ElMessage.warning(t('channels.bind.peerIdRequired'));
    return;
  }
  bindingSaving.value = true;
  try {
    const payload = {
      channel: account.channel,
      account_id: account.account_id,
      peer_kind: form.peer_kind || account.defaultPeerKind || 'dm',
      peer_id: peerId,
      agent_id: form.agent_id || undefined
    };
    await upsertChannelBinding(payload);
    ElMessage.success(t('channels.bind.success'));
    form.peer_id = '';
    await loadBindings();
  } catch (error) {
    showApiError(error, t('channels.bind.failed'));
  } finally {
    bindingSaving.value = false;
  }
};

const removeBinding = async (binding) => {
  try {
    await ElMessageBox.confirm(t('channels.unbind.confirm'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    await deleteChannelBinding(binding.channel, binding.account_id, binding.peer_kind, binding.peer_id);
    ElMessage.success(t('channels.unbind.success'));
    await loadBindings();
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    showApiError(error, t('channels.unbind.failed'));
  }
};

watch(
  () => visible.value,
  (value) => {
    if (value) {
      refreshAll();
    }
  }
);

watch(
  () => contextAgentId.value,
  () => {
    resetForm();
    if (visible.value) {
      loadBindings();
    }
  }
);
</script>

<style scoped>
:global(.feature-window-dialog--channel.el-dialog) {
  --fw-text: #e2e8f0;
  --fw-muted: #94a3b8;
  --fw-bg: linear-gradient(160deg, #070d1a, #0b1426);
  --fw-shadow: 0 24px 56px rgba(8, 12, 24, 0.55);
  --fw-border: rgba(51, 65, 85, 0.72);
  --fw-border-soft: rgba(51, 65, 85, 0.62);
  --fw-divider: rgba(51, 65, 85, 0.62);
  --fw-surface: #0b1527;
  --fw-surface-alt: #0d182c;
  --fw-control-bg: #111c31;
  --fw-control-hover: #162844;
  --fw-focus-border: rgba(56, 189, 248, 0.65);
  --fw-focus-ring: rgba(56, 189, 248, 0.18);
  --fw-accent-border: rgba(77, 216, 255, 0.65);
  --fw-accent-shadow: rgba(77, 216, 255, 0.35);
  --fw-danger: #fca5a5;
  --fw-danger-border: rgba(248, 113, 113, 0.4);
  width: min(96vw, 1080px) !important;
  max-width: 1080px;
  height: min(82vh, 760px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--fw-bg);
  border: 1px solid var(--fw-border);
  border-radius: 14px;
  box-shadow: var(--fw-shadow);
  color: var(--fw-text);
  color-scheme: dark;
}

:global(:root[data-user-theme='light'] .feature-window-dialog--channel.el-dialog) {
  --fw-text: #0f172a;
  --fw-muted: #64748b;
  --fw-bg: linear-gradient(180deg, #ffffff, #f7faff);
  --fw-shadow: 0 18px 40px rgba(15, 23, 42, 0.16);
  --fw-border: rgba(148, 163, 184, 0.52);
  --fw-border-soft: rgba(148, 163, 184, 0.36);
  --fw-divider: rgba(148, 163, 184, 0.42);
  --fw-surface: #f8fafc;
  --fw-surface-alt: #ffffff;
  --fw-control-bg: #f1f5f9;
  --fw-control-hover: #e2e8f0;
  --fw-focus-border: rgba(37, 99, 235, 0.55);
  --fw-focus-ring: rgba(37, 99, 235, 0.16);
  --fw-accent-border: rgba(37, 99, 235, 0.42);
  --fw-accent-shadow: rgba(37, 99, 235, 0.22);
  --fw-danger: #b91c1c;
  --fw-danger-border: rgba(220, 38, 38, 0.32);
  color-scheme: light;
}

:global(.feature-window-dialog--channel .el-dialog__header) {
  border-bottom: 1px solid var(--fw-divider);
  padding: 14px 18px;
}

:global(.feature-window-dialog--channel .el-dialog__body) {
  padding: 16px 18px 18px;
  color: var(--fw-text);
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.feature-window-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-title {
  font-size: 15px;
  font-weight: 700;
}

.feature-window-close {
  width: 30px;
  height: 30px;
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  cursor: pointer;
}

.feature-window-close:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
}

.feature-window-close:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.feature-window-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-hint {
  color: var(--fw-muted);
  font-size: 12px;
}

.channels-grid {
  display: grid;
  grid-template-columns: minmax(280px, 320px) minmax(0, 1fr);
  gap: 14px;
  flex: 1;
  min-height: 0;
}

.feature-window-list {
  max-height: none;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.feature-window-item {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface-alt);
  color: var(--fw-text);
  padding: 10px;
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 4px;
  cursor: pointer;
}

.feature-window-item.active {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-item-title {
  font-size: 13px;
  font-weight: 700;
}

.feature-window-item-meta {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-item-sub {
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-detail {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}

.feature-window-kv {
  display: grid;
  grid-template-columns: 88px minmax(0, 1fr);
  gap: 8px;
  font-size: 12px;
}

.feature-window-form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.feature-window-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}

.feature-window-field-full {
  grid-column: 1 / -1;
}

.feature-window-input {
  border: 1px solid var(--fw-border);
  border-radius: 8px;
  background: var(--fw-surface-alt);
  color: var(--fw-text);
  padding: 7px 9px;
  font-size: 12px;
  outline: none;
}

.feature-window-input::placeholder {
  color: var(--fw-muted);
}

.feature-window-input:focus {
  border-color: var(--fw-focus-border);
  box-shadow: 0 0 0 2px var(--fw-focus-ring);
}

.feature-window-input option {
  background: var(--fw-surface);
  color: var(--fw-text);
}

.feature-window-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.feature-window-btn {
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
}

.feature-window-btn:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
  color: var(--fw-text);
}

.feature-window-btn:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.feature-window-btn.danger {
  border-color: var(--fw-danger-border);
  color: var(--fw-danger);
}

.feature-window-runs-title {
  margin-top: 2px;
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-runs {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}

.feature-window-run-item {
  border: 1px solid var(--fw-border-soft);
  border-radius: 8px;
  padding: 6px 8px;
  background: var(--fw-surface-alt);
  display: flex;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  align-items: center;
}

.feature-window-empty {
  color: var(--fw-muted);
  font-size: 12px;
  text-align: center;
  padding: 12px;
}

@media (max-width: 960px) {
  .channels-grid {
    grid-template-columns: 1fr;
  }

  .feature-window-list {
    max-height: 30vh;
  }

  .feature-window-form-grid {
    grid-template-columns: 1fr;
  }
}
</style>