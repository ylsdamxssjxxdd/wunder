<template>
  <div class="channel-manager-page" :class="{ 'channel-manager-page--dialog': props.mode === 'dialog' }">
    <div class="channel-sidebar">
      <div class="channel-sidebar-header">
        <div class="channel-sidebar-title">{{ t('channels.list.title') }}</div>
        <div class="channel-sidebar-actions">
          <button class="channel-refresh-btn" type="button" :disabled="loading || saving" @click="startCreate">
            {{ t('channels.action.add') }}
          </button>
          <button class="channel-refresh-btn subtle" type="button" :disabled="loading || saving" @click="refreshAll">
            {{ t('common.refresh') }}
          </button>
        </div>
      </div>

      <div v-if="creating" class="channel-create-card">
        <div class="channel-create-title">{{ t('channels.create.title') }}</div>
        <label class="channel-form-field">
          <span>{{ t('channels.create.channel') }}</span>
          <select v-model="createForm.channel" class="channel-input" @change="onCreateChannelChange">
            <option v-for="option in supportedChannelOptions" :key="option.channel" :value="option.channel">
              {{ option.label }}
            </option>
          </select>
        </label>
        <label class="channel-form-field">
          <span>{{ t('channels.create.name') }}</span>
          <input
            v-model.trim="createForm.account_name"
            class="channel-input"
            type="text"
            :placeholder="t('channels.create.namePlaceholder')"
          />
        </label>
        <label class="channel-form-field channel-form-checkbox">
          <input v-model="createForm.enabled" type="checkbox" />
          <span>{{ t('channels.field.enabled') }}</span>
        </label>

        <template v-if="isFeishuCreate">
          <label class="channel-form-field">
            <span>{{ t('channels.config.appId') }}</span>
            <input
              v-model.trim="createForm.app_id"
              class="channel-input"
              type="text"
              :placeholder="t('channels.config.appIdPlaceholder')"
              autocomplete="off"
            />
          </label>
          <label class="channel-form-field">
            <span>{{ t('channels.config.appSecret') }}</span>
            <input
              v-model.trim="createForm.app_secret"
              class="channel-input"
              type="password"
              :placeholder="t('channels.config.appSecretPlaceholder')"
              autocomplete="new-password"
            />
          </label>
          <label class="channel-form-field channel-form-checkbox">
            <input v-model="createForm.receive_group_chat" type="checkbox" />
            <span>{{ t('channels.config.receiveGroup') }}</span>
          </label>
        </template>

        <template v-else>
          <label class="channel-form-field">
            <span>{{ t('channels.field.peerKind') }}</span>
            <select v-model="createForm.peer_kind" class="channel-input">
              <option v-if="!isCreateUserOnlyChannel" value="group">{{ t('channels.peerKind.group') }}</option>
              <option value="user">{{ t('channels.peerKind.user') }}</option>
            </select>
          </label>
          <label class="channel-form-field channel-form-field-full">
            <span>{{ t('channels.config.json') }}</span>
            <textarea
              v-model="createForm.config_text"
              class="channel-textarea"
              :placeholder="t('channels.config.jsonPlaceholder')"
              rows="5"
            />
          </label>
          <div class="channel-detail-hint">{{ t('channels.config.jsonHint') }}</div>
        </template>

        <div class="channel-create-actions">
          <button class="channel-action-btn" type="button" :disabled="createSaving" @click="createAccount">
            {{ createSaving ? t('common.saving') : t('channels.create.create') }}
          </button>
          <button class="channel-action-btn" type="button" :disabled="createSaving" @click="cancelCreate">
            {{ t('channels.create.cancel') }}
          </button>
        </div>
      </div>

      <div v-if="loading" class="channel-empty">{{ t('common.loading') }}</div>
      <div v-else-if="!accounts.length" class="channel-empty">{{ t('channels.list.empty') }}</div>
      <div v-else class="channel-account-list">
        <button
          v-for="account in accounts"
          :key="account.key"
          class="channel-account-card"
          :class="{ active: account.key === selectedKey }"
          type="button"
          @click="selectAccount(account)"
        >
          <div class="channel-account-head">
            <div class="channel-account-title">{{ account.title }}</div>
            <span class="channel-account-status" :class="{ disabled: !account.active }">
              {{ account.active ? t('channels.status.enabled') : t('channels.status.disabled') }}
            </span>
          </div>
        </button>
      </div>
    </div>

    <div class="channel-content">
      <div class="channel-content-header">
        <div class="channel-content-title">
          {{ selectedAccount ? selectedAccount.title : t('channels.detail.empty') }}
        </div>
        <div class="channel-actions">
          <button class="channel-action-btn" type="button" :disabled="saving || !selectedAccount" @click="resetEditForm">
            {{ t('common.reset') }}
          </button>
        </div>
      </div>

      <div v-if="!selectedAccount" class="channel-empty">{{ t('channels.detail.empty') }}</div>
      <div v-else class="channel-detail">
        <div class="channel-detail-card">
          <div class="channel-detail-title">{{ t('channels.detail.info') }}</div>
          <div class="channel-detail-grid">
            <div>
              <div class="channel-detail-label">{{ t('channels.field.channel') }}</div>
              <div class="channel-detail-value">{{ selectedAccount.providerLabel }}</div>
            </div>
            <div>
              <div class="channel-detail-label">{{ t('channels.detail.accountId') }}</div>
              <div class="channel-detail-value">{{ selectedAccount.account_id }}</div>
            </div>
            <div>
              <div class="channel-detail-label">{{ t('channels.detail.state') }}</div>
              <div class="channel-detail-value">
                {{ selectedAccount.configured ? t('channels.detail.configured') : t('channels.detail.unconfigured') }}
              </div>
            </div>
            <div>
              <div class="channel-detail-label">{{ t('channels.detail.mode') }}</div>
              <div class="channel-detail-value">{{ peerKindLabel(selectedAccount.peerKind) }}</div>
            </div>
          </div>
        </div>

        <div class="channel-detail-card">
          <div class="channel-detail-title">{{ t('channels.config.title') }}</div>
          <div class="channel-form">
            <label class="channel-form-field">
              <span>{{ t('channels.create.name') }}</span>
              <input
                v-model.trim="editForm.account_name"
                class="channel-input"
                type="text"
                :placeholder="t('channels.create.namePlaceholder')"
              />
            </label>
            <label class="channel-form-field channel-form-checkbox">
              <input v-model="editForm.enabled" type="checkbox" />
              <span>{{ t('channels.field.enabled') }}</span>
            </label>

            <template v-if="selectedAccount.channel === 'feishu'">
              <label class="channel-form-field">
                <span>{{ t('channels.config.appId') }}</span>
                <input
                  v-model.trim="editForm.app_id"
                  class="channel-input"
                  type="text"
                  :placeholder="t('channels.config.appIdPlaceholder')"
                />
              </label>
              <label class="channel-form-field">
                <span>{{ t('channels.config.appSecret') }}</span>
                <input
                  v-model.trim="editForm.app_secret"
                  class="channel-input"
                  type="password"
                  autocomplete="new-password"
                  :placeholder="t('channels.config.appSecretPlaceholder')"
                />
                <span class="channel-detail-hint">
                  {{ selectedAccount.appSecretSet ? t('channels.config.secretExists') : t('channels.config.secretRequired') }}
                </span>
              </label>
              <label class="channel-form-field channel-form-checkbox">
                <input v-model="editForm.receive_group_chat" type="checkbox" />
                <span>{{ t('channels.config.receiveGroup') }}</span>
              </label>
            </template>

            <template v-else>
              <label class="channel-form-field">
                <span>{{ t('channels.field.peerKind') }}</span>
                <select v-model="editForm.peer_kind" class="channel-input">
                  <option v-if="!isEditUserOnlyChannel" value="group">{{ t('channels.peerKind.group') }}</option>
                  <option value="user">{{ t('channels.peerKind.user') }}</option>
                </select>
              </label>
              <label class="channel-form-field channel-form-field-full">
                <span>{{ t('channels.config.json') }}</span>
                <textarea
                  v-model="editForm.config_text"
                  class="channel-textarea"
                  :placeholder="t('channels.config.jsonPlaceholder')"
                  rows="8"
                />
              </label>
            </template>
          </div>

          <div class="channel-form-actions">
            <button class="channel-action-btn" type="button" :disabled="saving" @click="saveAccount">
              {{ saving ? t('common.saving') : t('common.save') }}
            </button>
            <button class="channel-action-btn danger" type="button" :disabled="saving" @click="removeAccount">
              {{ t('channels.action.delete') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteChannelAccount,
  listChannelAccounts,
  listChannelBindings,
  upsertChannelAccount
} from '@/api/channels';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  mode: {
    type: String,
    default: 'page'
  },
  agentId: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['changed']);

const { t } = useI18n();

const FALLBACK_CHANNELS = ['feishu', 'whatsapp', 'telegram', 'wechat', 'wechat_mp', 'qqbot'];
const USER_ONLY_CHANNELS = ['wechat', 'wechat_mp'];
const resolvedAgentId = computed(() => {
  const trimmed = String(props.agentId || '').trim();
  return trimmed || '';
});

const loading = ref(false);
const saving = ref(false);
const createSaving = ref(false);
const creating = ref(false);
const accounts = ref([]);
const supportedChannels = ref([]);
const selectedKey = ref('');

const createForm = reactive({
  channel: 'feishu',
  account_name: '',
  enabled: true,
  receive_group_chat: true,
  app_id: '',
  app_secret: '',
  peer_kind: 'group',
  config_text: '{}'
});

const editForm = reactive({
  account_name: '',
  enabled: true,
  receive_group_chat: true,
  app_id: '',
  app_secret: '',
  peer_kind: 'group',
  config_text: '{}'
});

type ChannelAccountPayload = {
  channel: string;
  create_new?: boolean;
  account_id?: string;
  account_name?: string;
  enabled: boolean;
  agent_id?: string;
  app_id?: string;
  app_secret?: string;
  receive_group_chat?: boolean;
  config?: Record<string, unknown>;
  peer_kind?: string;
};

const selectedAccount = computed(
  () => accounts.value.find((item) => item.key === selectedKey.value) || null
);

const supportedChannelOptions = computed(() => {
  const channels = supportedChannels.value.length
    ? supportedChannels.value
    : FALLBACK_CHANNELS.map((channel) => ({ channel }));
  return channels
    .map((item) => {
      const channel = String(item?.channel || '').trim().toLowerCase();
      if (!channel) {
        return null;
      }
      return {
        channel,
        label: providerLabel(channel)
      };
    })
    .filter(Boolean);
});

const isFeishuCreate = computed(() => createForm.channel === 'feishu');
const isCreateUserOnlyChannel = computed(() => USER_ONLY_CHANNELS.includes(createForm.channel));
const isEditUserOnlyChannel = computed(() =>
  selectedAccount.value ? USER_ONLY_CHANNELS.includes(selectedAccount.value.channel) : false
);

const providerLabel = (channel) => {
  const key = `channels.provider.${channel}`;
  const translated = t(key);
  return translated === key ? channel : translated;
};

const providerDesc = (channel) => {
  const key = `channels.provider.${channel}.desc`;
  const translated = t(key);
  return translated === key ? t('channels.provider.generic') : translated;
};

const peerKindLabel = (peerKind) => (peerKind === 'user' ? t('channels.peerKind.user') : t('channels.peerKind.group'));

const normalizeRawConfig = (record) => {
  const fromRaw = record?.raw_config;
  if (fromRaw && typeof fromRaw === 'object' && !Array.isArray(fromRaw)) {
    return fromRaw;
  }
  const fromConfig = record?.config;
  if (fromConfig && typeof fromConfig === 'object' && !Array.isArray(fromConfig)) {
    return fromConfig;
  }
  return {};
};

const normalizeAccount = (record) => {
  const channel = String(record?.channel || '').trim().toLowerCase();
  const accountId = String(record?.account_id || '').trim();
  if (!channel || !accountId) {
    return null;
  }
  const meta = record?.meta || {};
  const preview = record?.config || {};
  const raw = normalizeRawConfig(record);
  const appId = String(preview?.feishu?.app_id || '').trim();
  const appSecretSet = preview?.feishu?.app_secret_set === true;
  const name = String(record?.name || raw?.display_name || '').trim();
  const peerKind = String(meta?.peer_kind || '').trim().toLowerCase();
  const normalizedPeerKind = peerKind || (meta?.receive_group_chat === false ? 'user' : 'group');
  const active =
    record?.active === true ||
    String(record?.status || '').trim().toLowerCase() === 'active';

  return {
    key: `${channel}::${accountId}`,
    channel,
    account_id: accountId,
    title: name || `${providerLabel(channel)} · ${accountId}`,
    name,
    providerLabel: providerLabel(channel),
    desc: providerDesc(channel),
    configured: meta?.configured === true,
    active,
    peerKind: normalizedPeerKind,
    receiveGroupChat: meta?.receive_group_chat !== false,
    appId,
    appSecretSet,
    rawConfig: raw
  };
};

const parseJsonConfig = (rawText: string): Record<string, unknown> | null => {
  const text = String(rawText || '').trim();
  if (!text) {
    return null;
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    throw new Error(t('channels.config.jsonInvalid'));
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error(t('channels.config.jsonInvalid'));
  }
  return parsed;
};

const selectAccount = (account) => {
  selectedKey.value = account.key;
  resetEditForm();
};

const resetCreateForm = () => {
  createForm.channel = supportedChannelOptions.value[0]?.channel || 'feishu';
  createForm.account_name = '';
  createForm.enabled = true;
  createForm.receive_group_chat = true;
  createForm.app_id = '';
  createForm.app_secret = '';
  createForm.peer_kind = 'group';
  createForm.config_text = '{}';
};

const resetEditForm = () => {
  const account = selectedAccount.value;
  if (!account) {
    editForm.account_name = '';
    editForm.enabled = true;
    editForm.receive_group_chat = true;
    editForm.app_id = '';
    editForm.app_secret = '';
    editForm.peer_kind = 'group';
    editForm.config_text = '{}';
    return;
  }
  editForm.account_name = account.name || '';
  editForm.enabled = account.active;
  editForm.receive_group_chat = account.receiveGroupChat;
  editForm.app_id = account.appId || '';
  editForm.app_secret = '';
  editForm.peer_kind = account.peerKind || 'group';
  try {
    editForm.config_text = JSON.stringify(account.rawConfig || {}, null, 2);
  } catch (error) {
    editForm.config_text = '{}';
  }
};

const loadAccounts = async (preferred = undefined) => {
  loading.value = true;
  try {
    const [accountsResp, bindingsResp] = await Promise.all([
      listChannelAccounts(),
      resolvedAgentId.value ? listChannelBindings() : Promise.resolve({ data: null })
    ]);
    const data = accountsResp?.data;
    const payload = data?.data || {};
    const items = Array.isArray(payload.items) ? payload.items : [];
    const channels = Array.isArray(payload.supported_channels) ? payload.supported_channels : [];
    const bindingItems = Array.isArray(bindingsResp?.data?.data?.items)
      ? bindingsResp.data.data.items
      : [];

    supportedChannels.value = channels
      .map((item) => ({ channel: String(item?.channel || '').trim().toLowerCase() }))
      .filter((item) => item.channel);

    let normalizedAccounts = items.map((item) => normalizeAccount(item)).filter(Boolean);
    if (resolvedAgentId.value) {
      const allowedKeys = new Set();
      bindingItems.forEach((binding) => {
        if (binding?.enabled !== true) return;
        const channel = String(binding?.channel || '').trim().toLowerCase();
        const accountId = String(binding?.account_id || '').trim();
        if (!channel || !accountId) return;
        const agentId = String(binding?.agent_id || '').trim();
        if (agentId && agentId === resolvedAgentId.value) {
          allowedKeys.add(`${channel}::${accountId}`);
        }
      });
      normalizedAccounts = normalizedAccounts.filter((account) => allowedKeys.has(account.key));
    }
    accounts.value = normalizedAccounts;

    const preferredKey =
      preferred?.channel && preferred?.account_id
        ? `${String(preferred.channel).trim().toLowerCase()}::${String(preferred.account_id).trim()}`
        : '';

    if (preferredKey && accounts.value.some((item) => item.key === preferredKey)) {
      selectedKey.value = preferredKey;
    } else if (!accounts.value.some((item) => item.key === selectedKey.value)) {
      selectedKey.value = accounts.value[0]?.key || '';
    }

    if (!creating.value) {
      resetCreateForm();
    }
    resetEditForm();
  } catch (error) {
    showApiError(error, t('channels.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const refreshAll = async () => {
  await loadAccounts();
};

const startCreate = () => {
  creating.value = true;
  resetCreateForm();
};

const cancelCreate = () => {
  creating.value = false;
  resetCreateForm();
};

const onCreateChannelChange = () => {
  if (createForm.channel === 'feishu') {
    createForm.receive_group_chat = true;
  } else {
    createForm.peer_kind = USER_ONLY_CHANNELS.includes(createForm.channel) ? 'user' : 'group';
    createForm.config_text = '{}';
  }
};

const createAccount = async () => {
  const channel = String(createForm.channel || '').trim().toLowerCase();
  if (!channel) {
    ElMessage.warning(t('channels.config.channelRequired'));
    return;
  }

  const payload: ChannelAccountPayload = {
    channel,
    create_new: true,
    account_name: createForm.account_name || undefined,
    enabled: Boolean(createForm.enabled)
  };
  if (resolvedAgentId.value) {
    payload.agent_id = resolvedAgentId.value;
  }

  if (channel === 'feishu') {
    const appId = String(createForm.app_id || '').trim();
    const appSecret = String(createForm.app_secret || '').trim();
    if (!appId) {
      ElMessage.warning(t('channels.config.appIdRequired'));
      return;
    }
    if (!appSecret) {
      ElMessage.warning(t('channels.config.appSecretRequired'));
      return;
    }
    payload.app_id = appId;
    payload.app_secret = appSecret;
    payload.receive_group_chat = Boolean(createForm.receive_group_chat);
  } else {
    let config;
    try {
      config = parseJsonConfig(createForm.config_text);
    } catch (error) {
      ElMessage.warning(error.message || t('channels.config.jsonInvalid'));
      return;
    }
    if (!config) {
      ElMessage.warning(t('channels.config.jsonRequired'));
      return;
    }
    payload.config = config;
    payload.peer_kind = USER_ONLY_CHANNELS.includes(channel) ? 'user' : createForm.peer_kind || 'group';
  }

  createSaving.value = true;
  try {
    const { data } = await upsertChannelAccount(payload);
    const saved = data?.data;
    ElMessage.success(t('channels.create.createSuccess'));
    creating.value = false;
    await loadAccounts(saved);
    emit('changed');
  } catch (error) {
    showApiError(error, t('channels.create.createFailed'));
  } finally {
    createSaving.value = false;
  }
};

const saveAccount = async () => {
  const account = selectedAccount.value;
  if (!account) {
    return;
  }

  const payload: ChannelAccountPayload = {
    channel: account.channel,
    account_id: account.account_id,
    account_name: editForm.account_name || undefined,
    enabled: Boolean(editForm.enabled)
  };
  if (resolvedAgentId.value) {
    payload.agent_id = resolvedAgentId.value;
  }

  if (account.channel === 'feishu') {
    const appId = String(editForm.app_id || '').trim();
    const appSecret = String(editForm.app_secret || '').trim();
    if (!appId) {
      ElMessage.warning(t('channels.config.appIdRequired'));
      return;
    }
    if (!appSecret && !account.appSecretSet) {
      ElMessage.warning(t('channels.config.appSecretRequired'));
      return;
    }
    payload.app_id = appId;
    if (appSecret) {
      payload.app_secret = appSecret;
    }
    payload.receive_group_chat = Boolean(editForm.receive_group_chat);
  } else {
    let config;
    try {
      config = parseJsonConfig(editForm.config_text);
    } catch (error) {
      ElMessage.warning(error.message || t('channels.config.jsonInvalid'));
      return;
    }
    if (!config) {
      ElMessage.warning(t('channels.config.jsonRequired'));
      return;
    }
    payload.config = config;
    payload.peer_kind =
      USER_ONLY_CHANNELS.includes(account.channel) ? 'user' : editForm.peer_kind || 'group';
  }

  saving.value = true;
  try {
    const { data } = await upsertChannelAccount(payload);
    const saved = data?.data;
    ElMessage.success(t('channels.config.saveSuccess'));
    await loadAccounts(saved);
    emit('changed');
  } catch (error) {
    showApiError(error, t('channels.config.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const removeAccount = async () => {
  const account = selectedAccount.value;
  if (!account) {
    return;
  }

  try {
    await ElMessageBox.confirm(t('channels.config.removeConfirm'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
  } catch (error) {
    return;
  }

  saving.value = true;
  try {
    await deleteChannelAccount(account.channel, account.account_id);
    ElMessage.success(t('channels.config.removeSuccess'));
    await loadAccounts();
    emit('changed');
  } catch (error) {
    showApiError(error, t('channels.config.removeFailed'));
  } finally {
    saving.value = false;
  }
};

defineExpose({
  refreshAll
});

onMounted(() => {
  refreshAll();
});
</script>
