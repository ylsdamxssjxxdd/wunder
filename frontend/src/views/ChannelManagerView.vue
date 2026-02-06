<template>
  <div class="portal-shell channel-manager-shell">
    <UserTopbar :title="t('channels.title')" :subtitle="t('channels.subtitle')" :hide-chat="true" />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section channel-manager-section">
            <div class="channel-manager-page">
              <div class="channel-sidebar">
                <div class="channel-sidebar-header">
                  <div class="channel-sidebar-title">{{ t('channels.list.title') }}</div>
                  <button
                    class="channel-refresh-btn"
                    type="button"
                    :disabled="loading"
                    @click="refreshAll"
                  >
                    {{ t('common.refresh') }}
                  </button>
                </div>
                <div v-if="loading" class="channel-empty">{{ t('common.loading') }}</div>
                <div v-else-if="!accounts.length" class="channel-empty">
                  {{ t('channels.list.empty') }}
                </div>
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
                      <div class="channel-account-title">{{ account.label }}</div>
                      <span
                        class="channel-account-status"
                        :class="{ disabled: !account.active }"
                      >
                        {{ account.active ? t('channels.status.enabled') : t('channels.status.disabled') }}
                      </span>
                    </div>
                    <div class="channel-account-meta">
                      <span class="channel-account-tag">{{ account.account_id }}</span>
                    </div>
                    <div class="channel-account-desc">{{ account.desc }}</div>
                  </button>
                </div>
              </div>
              <div class="channel-content">
                <div class="channel-content-header">
                  <div class="channel-content-title">
                    {{ selectedAccount ? selectedAccount.label : t('channels.detail.empty') }}
                  </div>
                  <div class="channel-actions">
                    <button
                      class="channel-action-btn"
                      type="button"
                      :disabled="!selectedAccount"
                      @click="resetForm"
                    >
                      {{ t('common.reset') }}
                    </button>
                  </div>
                </div>
                <div v-if="!selectedAccount" class="channel-empty">
                  {{ t('channels.detail.empty') }}
                </div>
                <div v-else class="channel-detail">
                  <div class="channel-detail-card">
                    <div class="channel-detail-title">{{ t('channels.detail.info') }}</div>
                    <div class="channel-detail-grid">
                      <div>
                        <div class="channel-detail-label">{{ t('channels.field.channel') }}</div>
                        <div class="channel-detail-value">{{ selectedAccount.label }}</div>
                      </div>
                      <div>
                        <div class="channel-detail-label">{{ t('channels.field.account') }}</div>
                        <div class="channel-detail-value">{{ selectedAccount.account_id }}</div>
                      </div>
                    </div>
                    <div class="channel-detail-hint">{{ t('channels.detail.hint') }}</div>
                  </div>

                  <div class="channel-detail-card">
                    <div class="channel-detail-title">{{ t('channels.bind.title') }}</div>
                    <div class="channel-form">
                      <div class="channel-form-field">
                        <label>{{ t('channels.bind.peerKind') }}</label>
                        <select v-model="form.peer_kind" class="channel-input">
                          <option
                            v-for="option in peerKindOptions"
                            :key="option.value"
                            :value="option.value"
                          >
                            {{ option.label }}
                          </option>
                        </select>
                      </div>
                      <div class="channel-form-field">
                        <label>{{ t('channels.bind.peerId') }}</label>
                        <input
                          v-model="form.peer_id"
                          class="channel-input"
                          :placeholder="t('channels.bind.peerId.placeholder')"
                        />
                      </div>
                      <div class="channel-form-field">
                        <label>{{ t('channels.bind.agent') }}</label>
                        <select v-model="form.agent_id" class="channel-input">
                          <option value="">{{ t('channels.bind.defaultAgent') }}</option>
                          <option
                            v-for="agent in agentOptions"
                            :key="agent.id"
                            :value="agent.id"
                          >
                            {{ agent.label }}
                          </option>
                        </select>
                      </div>
                    </div>
                    <div class="channel-form-actions">
                      <button
                        class="channel-action-btn"
                        type="button"
                        :disabled="bindingSaving"
                        @click="saveBinding"
                      >
                        {{ bindingSaving ? t('common.saving') : t('common.save') }}
                      </button>
                    </div>
                  </div>

                  <div class="channel-detail-card">
                    <div class="list-header">
                      <label>{{ t('channels.bindings.title') }}</label>
                      <button
                        class="channel-refresh-btn subtle"
                        type="button"
                        :disabled="bindingLoading"
                        @click="loadBindings"
                      >
                        {{ t('common.refresh') }}
                      </button>
                    </div>
                    <div v-if="bindingLoading" class="channel-empty">
                      {{ t('common.loading') }}
                    </div>
                    <div v-else-if="!filteredBindings.length" class="channel-empty">
                      {{ t('channels.bindings.empty') }}
                    </div>
                    <div v-else class="channel-binding-list">
                      <div
                        v-for="binding in filteredBindings"
                        :key="binding.key"
                        class="channel-binding-item"
                      >
                        <div class="channel-binding-main">
                          <div class="channel-binding-title">{{ binding.peer_id }}</div>
                          <div class="channel-binding-meta">
                            <span>{{ binding.peer_kind }}</span>
                            <span>{{ binding.agentLabel }}</span>
                          </div>
                        </div>
                        <button
                          class="channel-action-btn danger"
                          type="button"
                          @click="removeBinding(binding)"
                        >
                          {{ t('channels.action.unbind') }}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteChannelBinding,
  listChannelAccounts,
  listChannelBindings,
  upsertChannelBinding
} from '@/api/channels';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';

const { t } = useI18n();
const agentStore = useAgentStore();
const accounts = ref([]);
const bindings = ref([]);
const loading = ref(false);
const bindingLoading = ref(false);
const bindingSaving = ref(false);
const selectedKey = ref('');

const form = reactive({
  peer_kind: '',
  peer_id: '',
  agent_id: ''
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

const selectedAccount = computed(
  () => accounts.value.find((item) => item.key === selectedKey.value) || null
);

const agentOptions = computed(() => {
  const options = [];
  const append = (agent, suffix) => {
    if (!agent?.id) return;
    const name = agent.name || agent.id;
    options.push({
      id: agent.id,
      label: suffix ? `${name} ${suffix}` : name
    });
  };
  (agentStore.agents || []).forEach((agent) => append(agent, ''));
  (agentStore.sharedAgents || []).forEach((agent) =>
    append(agent, `(${t('channels.agent.shared')})`)
  );
  return options;
});

const agentNameMap = computed(() => {
  const map = new Map();
  agentOptions.value.forEach((agent) => {
    map.set(agent.id, agent.label);
  });
  return map;
});

const filteredBindings = computed(() => {
  if (!selectedAccount.value) return [];
  return bindings.value
    .filter(
      (binding) =>
        binding.channel === selectedAccount.value.channel &&
        binding.account_id === selectedAccount.value.account_id
    )
    .map((binding) => ({
      ...binding,
      agentLabel:
        agentNameMap.value.get(binding.agent_id) || t('channels.bind.defaultAgent')
    }));
});

const normalizeAccount = (record) => {
  const channel = String(record?.channel || '').trim();
  const accountId = String(record?.account_id || '').trim();
  if (!channel || !accountId) return null;
  const key = `${channel}::${accountId}`;
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
    key: `${channel}:${accountId}:${peerKind}:${peerId}`,
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
    const items = data?.data?.items || [];
    accounts.value = items
      .map((item) => normalizeAccount(item))
      .filter(Boolean);
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
    ElMessage.error(error?.response?.data?.detail?.message || t('channels.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const loadBindings = async () => {
  bindingLoading.value = true;
  try {
    const { data } = await listChannelBindings();
    const items = data?.data?.items || [];
    bindings.value = items
      .map((item) => normalizeBinding(item))
      .filter(Boolean);
  } catch (error) {
    ElMessage.error(error?.response?.data?.detail?.message || t('channels.bindings.loadFailed'));
  } finally {
    bindingLoading.value = false;
  }
};

const refreshAll = async () => {
  await loadAccounts();
  await loadBindings();
};

const selectAccount = (account) => {
  selectedKey.value = account.key;
  resetForm();
};

const resetForm = () => {
  const account = selectedAccount.value;
  form.peer_kind = account?.defaultPeerKind || 'dm';
  form.peer_id = '';
  form.agent_id = '';
};

const saveBinding = async () => {
  const account = selectedAccount.value;
  if (!account) {
    return;
  }
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
    await loadBindings();
  } catch (error) {
    ElMessage.error(error?.response?.data?.detail?.message || t('channels.bind.failed'));
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
    await deleteChannelBinding(
      binding.channel,
      binding.account_id,
      binding.peer_kind,
      binding.peer_id
    );
    ElMessage.success(t('channels.unbind.success'));
    await loadBindings();
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    ElMessage.error(error?.response?.data?.detail?.message || t('channels.unbind.failed'));
  }
};

onMounted(async () => {
  await agentStore.loadAgents();
  await refreshAll();
});
</script>
