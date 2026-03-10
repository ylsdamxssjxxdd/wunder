<template>
  <div class="channel-manager-page" :class="{ 'channel-manager-page--dialog': props.mode === 'dialog' }">
    <div class="channel-sidebar">
      <div class="channel-sidebar-header">
        <div class="channel-sidebar-title">{{ t('channels.list.title') }}</div>
        <div class="channel-sidebar-actions">
          <button
            class="channel-refresh-btn"
            type="button"
            :disabled="loading || saving || permissionDenied"
            @click="startCreate"
          >
            {{ t('channels.action.add') }}
          </button>
          <button class="channel-refresh-btn subtle" type="button" :disabled="loading || saving" @click="refreshAll">
            {{ t('common.refresh') }}
          </button>
        </div>
      </div>

      <div v-if="creating" class="channel-create-card">
        <div class="channel-create-title">{{ t('channels.create.title') }}</div>
        <div class="channel-create-body">
          <label class="channel-form-field">
            <span>{{ t('channels.create.channel') }}</span>
            <select v-model="createForm.channel" class="channel-input" @change="onCreateChannelChange">
              <option v-for="option in supportedChannelOptions" :key="option.channel" :value="option.channel">
                {{ option.label }}
              </option>
            </select>
            <span class="channel-detail-hint">{{ providerDesc(createForm.channel) }}</span>
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
          <div class="channel-create-checks">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createForm.enabled" type="checkbox" />
              <span>{{ t('channels.field.enabled') }}</span>
            </label>
            <label
              v-if="isFeishuCreate"
              class="channel-form-field channel-form-checkbox channel-form-checkbox--inline"
            >
              <input v-model="createForm.receive_group_chat" type="checkbox" />
              <span>{{ t('channels.config.receiveGroup') }}</span>
            </label>
          </div>

          <template v-if="showCreatePeerKind">
            <label class="channel-form-field">
              <span>{{ t('channels.field.peerKind') }}</span>
              <select v-model="createForm.peer_kind" class="channel-input">
                <option v-if="!isCreateUserOnlyChannel" value="group">{{ t('channels.peerKind.group') }}</option>
                <option value="user">{{ t('channels.peerKind.user') }}</option>
              </select>
            </label>
          </template>

          <template v-for="field in createChannelFields" :key="`create-${field.key}`">
            <label
              v-if="field.type === 'checkbox'"
              class="channel-form-field channel-form-checkbox channel-form-checkbox--inline"
            >
              <input v-model="createDynamicFields[field.key]" type="checkbox" />
              <span>{{ t(field.labelKey) }}</span>
            </label>
            <label v-else class="channel-form-field">
              <span>{{ t(field.labelKey) }}</span>
              <input
                v-model.trim="createDynamicFields[field.key]"
                class="channel-input"
                :type="field.type === 'password' ? 'password' : 'text'"
                :placeholder="field.placeholderKey ? t(field.placeholderKey) : ''"
                autocomplete="off"
              />
            </label>
          </template>

          <div v-if="showCreateAdvancedConfigToggle" class="channel-advanced-toggle">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createAdvancedEnabled" type="checkbox" />
              <span>{{ t('channels.config.advancedToggle') }}</span>
            </label>
          </div>
          <template v-if="showCreateAdvancedConfigEditor">
            <label class="channel-form-field channel-form-field-full">
              <span>{{ t('channels.config.json') }}</span>
              <textarea
                v-model="createForm.config_text"
                class="channel-textarea"
                :placeholder="t('channels.config.jsonPlaceholder')"
                rows="5"
              />
            </label>
            <div class="channel-detail-hint">
              {{ createChannelFields.length ? t('channels.config.jsonHintOptional') : t('channels.config.jsonHint') }}
            </div>
          </template>
        </div>

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
      <div v-else-if="permissionDenied" class="channel-empty">{{ t('auth.login.noPermission') }}</div>
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

      <div v-if="permissionDenied" class="channel-empty">{{ t('auth.login.noPermission') }}</div>
      <div v-else-if="!selectedAccount" class="channel-empty">{{ t('channels.detail.empty') }}</div>
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

            <template v-if="showEditPeerKind">
              <label class="channel-form-field">
                <span>{{ t('channels.field.peerKind') }}</span>
                <select v-model="editForm.peer_kind" class="channel-input">
                  <option v-if="!isEditUserOnlyChannel" value="group">{{ t('channels.peerKind.group') }}</option>
                  <option value="user">{{ t('channels.peerKind.user') }}</option>
                </select>
              </label>
            </template>

            <label v-if="isFeishuEdit" class="channel-form-field channel-form-checkbox">
              <input v-model="editForm.receive_group_chat" type="checkbox" />
              <span>{{ t('channels.config.receiveGroup') }}</span>
            </label>

            <template v-for="field in editChannelFields" :key="`edit-${field.key}`">
              <label
                v-if="field.type === 'checkbox'"
                class="channel-form-field channel-form-checkbox channel-form-checkbox--inline"
              >
                <input v-model="editDynamicFields[field.key]" type="checkbox" />
                <span>{{ t(field.labelKey) }}</span>
              </label>
              <label v-else class="channel-form-field">
                <span>{{ t(field.labelKey) }}</span>
                <input
                  v-model.trim="editDynamicFields[field.key]"
                  class="channel-input"
                  :type="field.type === 'password' ? 'password' : 'text'"
                  :placeholder="field.placeholderKey ? t(field.placeholderKey) : ''"
                  autocomplete="off"
                />
                <span v-if="field.type === 'password' && isEditSecretSaved(field.key)" class="channel-detail-hint">
                  {{ t('channels.config.secretExists') }}
                </span>
              </label>
            </template>

            <div v-if="showEditAdvancedConfigToggle" class="channel-advanced-toggle">
              <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
                <input v-model="editAdvancedEnabled" type="checkbox" />
                <span>{{ t('channels.config.advancedToggle') }}</span>
              </label>
            </div>
            <template v-if="showEditAdvancedConfigEditor">
              <label class="channel-form-field channel-form-field-full">
                <span>{{ t('channels.config.json') }}</span>
                <textarea
                  v-model="editForm.config_text"
                  class="channel-textarea"
                  :placeholder="t('channels.config.jsonPlaceholder')"
                  rows="8"
                />
              </label>
              <div class="channel-detail-hint">
                {{ editChannelFields.length ? t('channels.config.jsonHintOptional') : t('channels.config.jsonHint') }}
              </div>
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

type DynamicFieldType = 'text' | 'password' | 'checkbox';

type ChannelFieldDef = {
  key: string;
  labelKey: string;
  placeholderKey?: string;
  type?: DynamicFieldType;
  required?: boolean;
  defaultValue?: string | boolean;
};

type ChannelSchema = {
  mode: 'feishu' | 'wechat' | 'wechat_mp' | 'config';
  configRoot?: string;
  fields: ChannelFieldDef[];
};

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
  domain?: string;
  config?: Record<string, unknown>;
  peer_kind?: string;
  feishu?: {
    app_id?: string;
    app_secret?: string;
    domain?: string;
  };
  wechat?: {
    corp_id?: string;
    agent_id?: string;
    secret?: string;
    token?: string;
    encoding_aes_key?: string;
    domain?: string;
  };
  wechat_mp?: {
    app_id?: string;
    app_secret?: string;
    token?: string;
    encoding_aes_key?: string;
    original_id?: string;
    domain?: string;
  };
};

type ChannelAccountItem = {
  key: string;
  channel: string;
  account_id: string;
  title: string;
  name: string;
  providerLabel: string;
  desc: string;
  configured: boolean;
  active: boolean;
  peerKind: string;
  receiveGroupChat: boolean;
  appSecretSet: boolean;
  wechatSecretSet: boolean;
  wechatMpAppSecretSet: boolean;
  rawConfig: Record<string, unknown>;
};

const CHANNEL_SCHEMAS: Record<string, ChannelSchema> = {
  feishu: {
    mode: 'feishu',
    fields: [
      {
        key: 'app_id',
        labelKey: 'channels.form.feishu.appId',
        placeholderKey: 'channels.form.feishu.appIdPlaceholder',
        required: true
      },
      {
        key: 'app_secret',
        labelKey: 'channels.form.feishu.appSecret',
        placeholderKey: 'channels.form.feishu.appSecretPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'domain',
        labelKey: 'channels.form.feishu.domain',
        placeholderKey: 'channels.form.feishu.domainPlaceholder',
        defaultValue: 'open.feishu.cn'
      }
    ]
  },
  wechat: {
    mode: 'wechat',
    fields: [
      {
        key: 'corp_id',
        labelKey: 'channels.form.wechat.corpId',
        placeholderKey: 'channels.form.wechat.corpIdPlaceholder',
        required: true
      },
      {
        key: 'agent_id',
        labelKey: 'channels.form.wechat.agentId',
        placeholderKey: 'channels.form.wechat.agentIdPlaceholder',
        required: true
      },
      {
        key: 'secret',
        labelKey: 'channels.form.wechat.secret',
        placeholderKey: 'channels.form.wechat.secretPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'token',
        labelKey: 'channels.form.wechat.token',
        placeholderKey: 'channels.form.wechat.tokenPlaceholder',
        type: 'password'
      },
      {
        key: 'encoding_aes_key',
        labelKey: 'channels.form.wechat.encodingAesKey',
        placeholderKey: 'channels.form.wechat.encodingAesKeyPlaceholder',
        type: 'password'
      },
      {
        key: 'domain',
        labelKey: 'channels.form.wechat.domain',
        placeholderKey: 'channels.form.wechat.domainPlaceholder',
        defaultValue: 'qyapi.weixin.qq.com'
      }
    ]
  },
  wechat_mp: {
    mode: 'wechat_mp',
    fields: [
      {
        key: 'app_id',
        labelKey: 'channels.form.wechatMp.appId',
        placeholderKey: 'channels.form.wechatMp.appIdPlaceholder',
        required: true
      },
      {
        key: 'app_secret',
        labelKey: 'channels.form.wechatMp.appSecret',
        placeholderKey: 'channels.form.wechatMp.appSecretPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'token',
        labelKey: 'channels.form.wechatMp.token',
        placeholderKey: 'channels.form.wechatMp.tokenPlaceholder',
        type: 'password'
      },
      {
        key: 'encoding_aes_key',
        labelKey: 'channels.form.wechatMp.encodingAesKey',
        placeholderKey: 'channels.form.wechatMp.encodingAesKeyPlaceholder',
        type: 'password'
      },
      {
        key: 'original_id',
        labelKey: 'channels.form.wechatMp.originalId',
        placeholderKey: 'channels.form.wechatMp.originalIdPlaceholder'
      },
      {
        key: 'domain',
        labelKey: 'channels.form.wechatMp.domain',
        placeholderKey: 'channels.form.wechatMp.domainPlaceholder',
        defaultValue: 'api.weixin.qq.com'
      }
    ]
  },
  qqbot: {
    mode: 'config',
    configRoot: 'qqbot',
    fields: [
      {
        key: 'app_id',
        labelKey: 'channels.form.qqbot.appId',
        placeholderKey: 'channels.form.qqbot.appIdPlaceholder',
        required: true
      },
      {
        key: 'client_secret',
        labelKey: 'channels.form.qqbot.clientSecret',
        placeholderKey: 'channels.form.qqbot.clientSecretPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'markdown_support',
        labelKey: 'channels.form.qqbot.markdownSupport',
        type: 'checkbox',
        defaultValue: false
      }
    ]
  },
  whatsapp: {
    mode: 'config',
    configRoot: 'whatsapp_cloud',
    fields: [
      {
        key: 'phone_number_id',
        labelKey: 'channels.form.whatsapp.phoneNumberId',
        placeholderKey: 'channels.form.whatsapp.phoneNumberIdPlaceholder',
        required: true
      },
      {
        key: 'access_token',
        labelKey: 'channels.form.whatsapp.accessToken',
        placeholderKey: 'channels.form.whatsapp.accessTokenPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'verify_token',
        labelKey: 'channels.form.whatsapp.verifyToken',
        placeholderKey: 'channels.form.whatsapp.verifyTokenPlaceholder',
        type: 'password'
      },
      {
        key: 'api_version',
        labelKey: 'channels.form.whatsapp.apiVersion',
        placeholderKey: 'channels.form.whatsapp.apiVersionPlaceholder',
        defaultValue: 'v19.0'
      }
    ]
  },
  telegram: {
    mode: 'config',
    configRoot: 'telegram',
    fields: [
      {
        key: 'bot_token',
        labelKey: 'channels.form.telegram.botToken',
        placeholderKey: 'channels.form.telegram.botTokenPlaceholder',
        type: 'password',
        required: true
      }
    ]
  },
  discord: {
    mode: 'config',
    configRoot: 'discord',
    fields: [
      {
        key: 'bot_token',
        labelKey: 'channels.form.discord.botToken',
        placeholderKey: 'channels.form.discord.botTokenPlaceholder',
        type: 'password',
        required: true
      }
    ]
  },
  slack: {
    mode: 'config',
    configRoot: 'slack',
    fields: [
      {
        key: 'app_token',
        labelKey: 'channels.form.slack.appToken',
        placeholderKey: 'channels.form.slack.appTokenPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'bot_token',
        labelKey: 'channels.form.slack.botToken',
        placeholderKey: 'channels.form.slack.botTokenPlaceholder',
        type: 'password',
        required: true
      }
    ]
  },
  line: {
    mode: 'config',
    configRoot: 'line',
    fields: [
      {
        key: 'channel_secret',
        labelKey: 'channels.form.line.channelSecret',
        placeholderKey: 'channels.form.line.channelSecretPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'access_token',
        labelKey: 'channels.form.line.accessToken',
        placeholderKey: 'channels.form.line.accessTokenPlaceholder',
        type: 'password',
        required: true
      }
    ]
  },
  dingtalk: {
    mode: 'config',
    configRoot: 'dingtalk',
    fields: [
      {
        key: 'access_token',
        labelKey: 'channels.form.dingtalk.accessToken',
        placeholderKey: 'channels.form.dingtalk.accessTokenPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'secret',
        labelKey: 'channels.form.dingtalk.secret',
        placeholderKey: 'channels.form.dingtalk.secretPlaceholder',
        type: 'password',
        required: true
      }
    ]
  },
  xmpp: {
    mode: 'config',
    configRoot: 'xmpp',
    fields: [
      {
        key: 'jid',
        labelKey: 'channels.form.xmpp.jid',
        placeholderKey: 'channels.form.xmpp.jidPlaceholder',
        required: true
      },
      {
        key: 'password',
        labelKey: 'channels.form.xmpp.password',
        placeholderKey: 'channels.form.xmpp.passwordPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'domain',
        labelKey: 'channels.form.xmpp.domain',
        placeholderKey: 'channels.form.xmpp.domainPlaceholder'
      },
      {
        key: 'host',
        labelKey: 'channels.form.xmpp.host',
        placeholderKey: 'channels.form.xmpp.hostPlaceholder'
      },
      {
        key: 'port',
        labelKey: 'channels.form.xmpp.port',
        placeholderKey: 'channels.form.xmpp.portPlaceholder'
      },
      {
        key: 'direct_tls',
        labelKey: 'channels.form.xmpp.directTls',
        type: 'checkbox',
        defaultValue: false
      },
      {
        key: 'muc_nick',
        labelKey: 'channels.form.xmpp.mucNick',
        placeholderKey: 'channels.form.xmpp.mucNickPlaceholder'
      },
      {
        key: 'muc_rooms',
        labelKey: 'channels.form.xmpp.mucRooms',
        placeholderKey: 'channels.form.xmpp.mucRoomsPlaceholder'
      },
      {
        key: 'heartbeat_enabled',
        labelKey: 'channels.form.xmpp.heartbeatEnabled',
        type: 'checkbox',
        defaultValue: true
      },
      {
        key: 'heartbeat_interval_s',
        labelKey: 'channels.form.xmpp.heartbeatIntervalS',
        placeholderKey: 'channels.form.xmpp.heartbeatIntervalSPlaceholder'
      },
      {
        key: 'heartbeat_timeout_s',
        labelKey: 'channels.form.xmpp.heartbeatTimeoutS',
        placeholderKey: 'channels.form.xmpp.heartbeatTimeoutSPlaceholder'
      },
      {
        key: 'respond_ping',
        labelKey: 'channels.form.xmpp.respondPing',
        type: 'checkbox',
        defaultValue: true
      }
    ]
  }
};

const FALLBACK_CHANNELS = [
  'feishu',
  'wechat',
  'wechat_mp',
  'qqbot',
  'whatsapp',
  'telegram',
  'discord',
  'slack',
  'line',
  'dingtalk',
  'xmpp'
];
const USER_ONLY_CHANNELS = ['wechat', 'wechat_mp'];
const resolvedAgentId = computed(() => {
  const trimmed = String(props.agentId || '').trim();
  return trimmed || '';
});

const loading = ref(false);
const saving = ref(false);
const createSaving = ref(false);
const creating = ref(false);
const permissionDenied = ref(false);
const accounts = ref<ChannelAccountItem[]>([]);
const supportedChannels = ref<{ channel: string }[]>([]);
const selectedKey = ref('');
const createAdvancedEnabled = ref(false);
const editAdvancedEnabled = ref(false);
const createDynamicFields = reactive<Record<string, string | boolean>>({});
const editDynamicFields = reactive<Record<string, string | boolean>>({});
const editSecretSaved = reactive<Record<string, boolean>>({});

const createForm = reactive({
  channel: 'feishu',
  account_name: '',
  enabled: true,
  receive_group_chat: true,
  peer_kind: 'group',
  config_text: '{}'
});

const editForm = reactive({
  account_name: '',
  enabled: true,
  receive_group_chat: true,
  peer_kind: 'group',
  config_text: '{}'
});

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
const isFeishuEdit = computed(() => selectedAccount.value?.channel === 'feishu');
const isCreateUserOnlyChannel = computed(() => USER_ONLY_CHANNELS.includes(createForm.channel));
const isEditUserOnlyChannel = computed(() =>
  selectedAccount.value ? USER_ONLY_CHANNELS.includes(selectedAccount.value.channel) : false
);
const showCreatePeerKind = computed(() => createForm.channel !== 'feishu');
const showEditPeerKind = computed(() => selectedAccount.value?.channel !== 'feishu');
const createChannelSchema = computed(() => schemaForChannel(createForm.channel));
const editChannelSchema = computed(() =>
  selectedAccount.value ? schemaForChannel(selectedAccount.value.channel) : null
);
const createChannelFields = computed(() => CHANNEL_SCHEMAS[createForm.channel]?.fields || []);
const editChannelFields = computed(() =>
  selectedAccount.value ? CHANNEL_SCHEMAS[selectedAccount.value.channel]?.fields || [] : []
);
const showCreateAdvancedConfigToggle = computed(
  () => createChannelSchema.value?.mode === 'config' && createChannelFields.value.length > 0
);
const showEditAdvancedConfigToggle = computed(
  () => editChannelSchema.value?.mode === 'config' && editChannelFields.value.length > 0
);
const showCreateAdvancedConfigEditor = computed(
  () =>
    createChannelFields.value.length === 0 ||
    (createChannelSchema.value?.mode === 'config' && createAdvancedEnabled.value)
);
const showEditAdvancedConfigEditor = computed(
  () =>
    editChannelFields.value.length === 0 ||
    (editChannelSchema.value?.mode === 'config' && editAdvancedEnabled.value)
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

const isObjectRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value) && typeof value === 'object' && !Array.isArray(value);

const normalizeRawConfig = (record): Record<string, unknown> => {
  const fromRaw = record?.raw_config;
  if (isObjectRecord(fromRaw)) {
    return fromRaw;
  }
  const fromConfig = record?.config;
  if (isObjectRecord(fromConfig)) {
    return fromConfig;
  }
  return {};
};

const trimmedText = (value: unknown) => String(value ?? '').trim();

const getConfigNode = (raw: Record<string, unknown>, key: string): Record<string, unknown> => {
  const node = raw[key];
  return isObjectRecord(node) ? node : {};
};

const cloneRecord = (value: unknown): Record<string, unknown> => {
  if (!isObjectRecord(value)) {
    return {};
  }
  try {
    return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
  } catch (error) {
    return { ...value };
  }
};

const clearDynamicFields = (target: Record<string, string | boolean>) => {
  Object.keys(target).forEach((key) => {
    delete target[key];
  });
};

const clearSecretState = () => {
  Object.keys(editSecretSaved).forEach((key) => {
    delete editSecretSaved[key];
  });
};

const schemaForChannel = (channel: string): ChannelSchema | null => CHANNEL_SCHEMAS[channel] || null;

const resolveSchemaSource = (channel: string, rawConfig: Record<string, unknown>): Record<string, unknown> => {
  const schema = schemaForChannel(channel);
  if (!schema) {
    return {};
  }
  if (schema.mode === 'feishu') {
    return getConfigNode(rawConfig, 'feishu');
  }
  if (schema.mode === 'wechat') {
    return getConfigNode(rawConfig, 'wechat');
  }
  if (schema.mode === 'wechat_mp') {
    return getConfigNode(rawConfig, 'wechat_mp');
  }
  return getConfigNode(rawConfig, schema.configRoot || channel);
};

const initDynamicFields = (
  target: Record<string, string | boolean>,
  channel: string,
  rawConfig: Record<string, unknown>,
  preserveSecrets: boolean
) => {
  clearDynamicFields(target);
  const schema = schemaForChannel(channel);
  if (!schema) {
    return;
  }
  const source = resolveSchemaSource(channel, rawConfig);
  for (const field of schema.fields) {
    if (field.type === 'checkbox') {
      const fallback = Boolean(field.defaultValue);
      target[field.key] = Boolean(source[field.key] ?? fallback);
      continue;
    }
    const current = trimmedText(source[field.key]);
    if (field.type === 'password' && !preserveSecrets) {
      target[field.key] = '';
      editSecretSaved[field.key] = Boolean(current);
      continue;
    }
    if (current) {
      target[field.key] = current;
      continue;
    }
    target[field.key] = typeof field.defaultValue === 'string' ? field.defaultValue : '';
  }
};

const normalizeAccount = (record): ChannelAccountItem | null => {
  const channel = String(record?.channel || '').trim().toLowerCase();
  const accountId = String(record?.account_id || '').trim();
  if (!channel || !accountId) {
    return null;
  }
  const meta = record?.meta || {};
  const preview = record?.config || {};
  const raw = normalizeRawConfig(record);
  const feishuNode = getConfigNode(raw, 'feishu');
  const wechatNode = getConfigNode(raw, 'wechat');
  const wechatMpNode = getConfigNode(raw, 'wechat_mp');
  const appSecretSet =
    preview?.feishu?.app_secret_set === true || Boolean(trimmedText(feishuNode.app_secret));
  const wechatSecretSet = Boolean(trimmedText(wechatNode.secret));
  const wechatMpAppSecretSet = Boolean(trimmedText(wechatMpNode.app_secret));
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
    appSecretSet,
    wechatSecretSet,
    wechatMpAppSecretSet,
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
  createForm.peer_kind = USER_ONLY_CHANNELS.includes(createForm.channel) ? 'user' : 'group';
  createForm.config_text = '{}';
  createAdvancedEnabled.value = false;
  initDynamicFields(createDynamicFields, createForm.channel, {}, true);
};

const resetEditForm = () => {
  clearSecretState();
  const account = selectedAccount.value;
  if (!account) {
    editForm.account_name = '';
    editForm.enabled = true;
    editForm.receive_group_chat = true;
    editForm.peer_kind = 'group';
    editForm.config_text = '{}';
    editAdvancedEnabled.value = false;
    clearDynamicFields(editDynamicFields);
    return;
  }
  editForm.account_name = account.name || '';
  editForm.enabled = account.active;
  editForm.receive_group_chat = account.receiveGroupChat;
  editForm.peer_kind = account.peerKind || 'group';
  try {
    editForm.config_text = JSON.stringify(account.rawConfig || {}, null, 2);
  } catch (error) {
    editForm.config_text = '{}';
  }
  editAdvancedEnabled.value = false;
  const schema = schemaForChannel(account.channel);
  initDynamicFields(
    editDynamicFields,
    account.channel,
    account.rawConfig || {},
    schema?.mode === 'config'
  );
};

const resolveHttpStatus = (error: unknown): number => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

const mergeConfigObject = (target: Record<string, unknown>, patch: Record<string, unknown>) => {
  Object.entries(patch).forEach(([key, value]) => {
    const current = target[key];
    if (isObjectRecord(current) && isObjectRecord(value)) {
      mergeConfigObject(current, value);
      return;
    }
    target[key] = value;
  });
};

const isEditSecretSaved = (fieldKey: string) => Boolean(editSecretSaved[fieldKey]);

const validateChannelFields = (
  schema: ChannelSchema | null,
  values: Record<string, string | boolean>,
  secretFallback: Record<string, boolean> = {}
): string | null => {
  if (!schema) {
    return null;
  }
  for (const field of schema.fields) {
    if (!field.required || field.type === 'checkbox') {
      continue;
    }
    const rawValue = trimmedText(values[field.key]);
    if (rawValue) {
      continue;
    }
    if (field.type === 'password' && secretFallback[field.key]) {
      continue;
    }
    return t('channels.config.fieldRequired', { field: t(field.labelKey) });
  }
  return null;
};

const buildStructuredConfigPatch = (
  channel: string,
  values: Record<string, string | boolean>,
  baseRawConfig?: Record<string, unknown>
): Record<string, unknown> => {
  const schema = schemaForChannel(channel);
  if (!schema || schema.mode !== 'config') {
    return {};
  }
  const configRoot = schema.configRoot || channel;
  const baseNode = isObjectRecord(baseRawConfig?.[configRoot]) ? cloneRecord(baseRawConfig?.[configRoot]) : {};
  for (const field of schema.fields) {
    if (field.type === 'checkbox') {
      baseNode[field.key] = Boolean(values[field.key]);
      continue;
    }
    const value = trimmedText(values[field.key]);
    if (configRoot === 'xmpp' && field.key === 'port') {
      const parsedPort = Number.parseInt(value, 10);
      if (Number.isFinite(parsedPort) && parsedPort > 0 && parsedPort <= 65535) {
        baseNode[field.key] = parsedPort;
      }
      continue;
    }
    if (configRoot === 'xmpp' && field.key === 'muc_rooms') {
      const rooms = value
        .split(/[,\n]/)
        .map((item) => item.trim())
        .filter(Boolean);
      if (rooms.length) {
        baseNode[field.key] = rooms;
      }
      continue;
    }
    if (configRoot === 'xmpp' && (field.key === 'heartbeat_interval_s' || field.key === 'heartbeat_timeout_s')) {
      const parsedValue = Number.parseInt(value, 10);
      if (Number.isFinite(parsedValue) && parsedValue > 0) {
        baseNode[field.key] = parsedValue;
      }
      continue;
    }
    if (value) {
      baseNode[field.key] = value;
    }
  }
  if (!Object.keys(baseNode).length) {
    return {};
  }
  return { [configRoot]: baseNode };
};

const shouldApplyAdvancedJson = (fieldCount: number, advancedEnabled: boolean) =>
  fieldCount === 0 || advancedEnabled;

const parseAdvancedJsonConfig = (
  rawText: string,
  fieldCount: number,
  advancedEnabled: boolean
): Record<string, unknown> | null => {
  if (!shouldApplyAdvancedJson(fieldCount, advancedEnabled)) {
    return null;
  }
  return parseJsonConfig(rawText);
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
    permissionDenied.value = false;
  } catch (error) {
    const status = resolveHttpStatus(error);
    if (status === 401 || status === 403) {
      permissionDenied.value = true;
      accounts.value = [];
      selectedKey.value = '';
      if (!creating.value) {
        resetCreateForm();
      }
      resetEditForm();
      return;
    }
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
  createForm.receive_group_chat = true;
  createForm.peer_kind = USER_ONLY_CHANNELS.includes(createForm.channel) ? 'user' : 'group';
  createForm.config_text = '{}';
  createAdvancedEnabled.value = false;
  initDynamicFields(createDynamicFields, createForm.channel, {}, true);
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

  const schema = schemaForChannel(channel);
  const fieldError = validateChannelFields(schema, createDynamicFields);
  if (fieldError) {
    ElMessage.warning(fieldError);
    return;
  }

  if (schema?.mode === 'feishu') {
    payload.app_id = trimmedText(createDynamicFields.app_id);
    payload.app_secret = trimmedText(createDynamicFields.app_secret);
    const domain = trimmedText(createDynamicFields.domain);
    if (domain) {
      payload.domain = domain;
    }
    payload.receive_group_chat = Boolean(createForm.receive_group_chat);
    return submitCreate(payload);
  }

  if (schema?.mode === 'wechat') {
    payload.wechat = {
      corp_id: trimmedText(createDynamicFields.corp_id),
      agent_id: trimmedText(createDynamicFields.agent_id),
      secret: trimmedText(createDynamicFields.secret),
      token: trimmedText(createDynamicFields.token) || undefined,
      encoding_aes_key: trimmedText(createDynamicFields.encoding_aes_key) || undefined,
      domain: trimmedText(createDynamicFields.domain) || undefined
    };
    payload.peer_kind = 'user';
    return submitCreate(payload);
  }

  if (schema?.mode === 'wechat_mp') {
    payload.wechat_mp = {
      app_id: trimmedText(createDynamicFields.app_id),
      app_secret: trimmedText(createDynamicFields.app_secret),
      token: trimmedText(createDynamicFields.token) || undefined,
      encoding_aes_key: trimmedText(createDynamicFields.encoding_aes_key) || undefined,
      original_id: trimmedText(createDynamicFields.original_id) || undefined,
      domain: trimmedText(createDynamicFields.domain) || undefined
    };
    payload.peer_kind = 'user';
    return submitCreate(payload);
  }

  const configPayload: Record<string, unknown> = {};
  if (schema?.mode === 'config') {
    mergeConfigObject(configPayload, buildStructuredConfigPatch(channel, createDynamicFields));
  }
  let advancedConfig: Record<string, unknown> | null = null;
  try {
    advancedConfig = parseAdvancedJsonConfig(
      createForm.config_text,
      createChannelFields.value.length,
      createAdvancedEnabled.value
    );
  } catch (error) {
    ElMessage.warning(error.message || t('channels.config.jsonInvalid'));
    return;
  }
  if (advancedConfig) {
    mergeConfigObject(configPayload, advancedConfig);
  }

  if (!Object.keys(configPayload).length) {
    ElMessage.warning(t('channels.config.jsonRequired'));
    return;
  }

  payload.config = configPayload;
  payload.peer_kind = USER_ONLY_CHANNELS.includes(channel) ? 'user' : createForm.peer_kind || 'group';
  await submitCreate(payload);
};

const submitCreate = async (payload: ChannelAccountPayload) => {
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

const submitUpdate = async (payload: ChannelAccountPayload) => {
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

  const schema = schemaForChannel(account.channel);
  const secretFallback: Record<string, boolean> = {};
  if (schema?.mode === 'feishu') {
    secretFallback.app_secret = account.appSecretSet;
  } else if (schema?.mode === 'wechat') {
    secretFallback.secret = account.wechatSecretSet;
  } else if (schema?.mode === 'wechat_mp') {
    secretFallback.app_secret = account.wechatMpAppSecretSet;
  }
  const fieldError = validateChannelFields(schema, editDynamicFields, secretFallback);
  if (fieldError) {
    ElMessage.warning(fieldError);
    return;
  }

  if (schema?.mode === 'feishu') {
    payload.app_id = trimmedText(editDynamicFields.app_id);
    const appSecret = trimmedText(editDynamicFields.app_secret);
    if (appSecret) {
      payload.app_secret = appSecret;
    }
    const domain = trimmedText(editDynamicFields.domain);
    if (domain) {
      payload.domain = domain;
    }
    payload.receive_group_chat = Boolean(editForm.receive_group_chat);
    await submitUpdate(payload);
    return;
  }

  if (schema?.mode === 'wechat') {
    const wechatPayload: NonNullable<ChannelAccountPayload['wechat']> = {
      corp_id: trimmedText(editDynamicFields.corp_id),
      agent_id: trimmedText(editDynamicFields.agent_id),
      domain: trimmedText(editDynamicFields.domain) || undefined
    };
    const secret = trimmedText(editDynamicFields.secret);
    if (secret) {
      wechatPayload.secret = secret;
    }
    const token = trimmedText(editDynamicFields.token);
    if (token) {
      wechatPayload.token = token;
    }
    const encodingAesKey = trimmedText(editDynamicFields.encoding_aes_key);
    if (encodingAesKey) {
      wechatPayload.encoding_aes_key = encodingAesKey;
    }
    payload.wechat = wechatPayload;
    payload.peer_kind = 'user';
    await submitUpdate(payload);
    return;
  }

  if (schema?.mode === 'wechat_mp') {
    const wechatMpPayload: NonNullable<ChannelAccountPayload['wechat_mp']> = {
      app_id: trimmedText(editDynamicFields.app_id),
      domain: trimmedText(editDynamicFields.domain) || undefined
    };
    const appSecret = trimmedText(editDynamicFields.app_secret);
    if (appSecret) {
      wechatMpPayload.app_secret = appSecret;
    }
    const token = trimmedText(editDynamicFields.token);
    if (token) {
      wechatMpPayload.token = token;
    }
    const encodingAesKey = trimmedText(editDynamicFields.encoding_aes_key);
    if (encodingAesKey) {
      wechatMpPayload.encoding_aes_key = encodingAesKey;
    }
    const originalId = trimmedText(editDynamicFields.original_id);
    if (originalId) {
      wechatMpPayload.original_id = originalId;
    }
    payload.wechat_mp = wechatMpPayload;
    payload.peer_kind = 'user';
    await submitUpdate(payload);
    return;
  }

  const configPayload: Record<string, unknown> = {};
  if (schema?.mode === 'config') {
    mergeConfigObject(
      configPayload,
      buildStructuredConfigPatch(account.channel, editDynamicFields, account.rawConfig)
    );
  }

  let advancedConfig: Record<string, unknown> | null = null;
  try {
    advancedConfig = parseAdvancedJsonConfig(
      editForm.config_text,
      editChannelFields.value.length,
      editAdvancedEnabled.value
    );
  } catch (error) {
    ElMessage.warning(error.message || t('channels.config.jsonInvalid'));
    return;
  }
  if (advancedConfig) {
    mergeConfigObject(configPayload, advancedConfig);
  }

  if (!Object.keys(configPayload).length) {
    ElMessage.warning(t('channels.config.jsonRequired'));
    return;
  }

  payload.config = configPayload;
  payload.peer_kind = USER_ONLY_CHANNELS.includes(account.channel) ? 'user' : editForm.peer_kind || 'group';
  await submitUpdate(payload);
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

<style scoped>
.channel-manager-page {
  display: grid;
  grid-template-columns: minmax(280px, 340px) minmax(0, 1fr);
  gap: 12px;
  min-height: 0;
  height: 100%;
  color: #202020;
}

.channel-manager-page--dialog {
  min-height: 0;
  height: 100%;
}

.channel-sidebar,
.channel-content {
  border: 1px solid #e3e3e3;
  border-radius: 12px;
  background: #ffffff;
  min-height: 0;
}

.channel-sidebar {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 10px;
}

.channel-sidebar-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.channel-sidebar-title,
.channel-content-title {
  font-size: 14px;
  font-weight: 700;
}

.channel-sidebar-actions,
.channel-actions,
.channel-form-actions,
.channel-create-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.channel-refresh-btn,
.channel-action-btn {
  height: 30px;
  padding: 0 12px;
  border: 1px solid #d8d8d8;
  border-radius: 8px;
  background: #f7f7f7;
  color: #4b4b4b;
  font-size: 12px;
  cursor: pointer;
  transition: border-color 0.18s ease, color 0.18s ease, background-color 0.18s ease;
}

.channel-refresh-btn:hover,
.channel-action-btn:hover:not(:disabled) {
  border-color: rgba(var(--ui-accent-rgb), 0.4);
  color: var(--ui-accent);
  background: var(--ui-accent-soft);
}

.channel-refresh-btn.subtle {
  background: #ffffff;
}

.channel-action-btn:disabled,
.channel-refresh-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.channel-action-btn.danger {
  border-color: #f2c8ce;
  background: #fbeff1;
  color: #c14053;
}

.channel-create-card {
  border: 1px solid #e3e3e3;
  border-radius: 10px;
  background: #fafafa;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.channel-create-title,
.channel-detail-title {
  font-size: 13px;
  font-weight: 700;
}

.channel-create-body,
.channel-form {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.channel-form-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: #606060;
}

.channel-form-field-full {
  grid-column: 1 / -1;
}

.channel-form-checkbox {
  flex-direction: row;
  align-items: center;
  gap: 8px;
  color: #444444;
}

.channel-form-checkbox--inline {
  flex-direction: row;
}

.channel-create-checks {
  grid-column: 1 / -1;
  display: inline-flex;
  gap: 10px;
  flex-wrap: wrap;
}

.channel-advanced-toggle {
  grid-column: 1 / -1;
}

.channel-input,
.channel-textarea {
  width: 100%;
  border: 1px solid #dcdcdc;
  border-radius: 8px;
  background: #ffffff;
  color: #202020;
  font-size: 13px;
  padding: 7px 10px;
  outline: none;
}

.channel-input:focus,
.channel-textarea:focus {
  border-color: rgba(var(--ui-accent-rgb), 0.5);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.12);
}

.channel-textarea {
  resize: vertical;
  min-height: 88px;
}

.channel-empty {
  font-size: 12px;
  color: #808080;
  text-align: center;
  padding: 16px 10px;
}

.channel-account-list {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-right: 2px;
}

.channel-account-card {
  border: 1px solid #e3e3e3;
  border-radius: 10px;
  background: #ffffff;
  color: #202020;
  text-align: left;
  padding: 9px 10px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.channel-account-card:hover {
  background: #f4f7fb;
}

.channel-account-card.active {
  background: var(--ui-accent-soft);
  border-color: rgba(var(--ui-accent-rgb), 0.4);
}

.channel-account-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.channel-account-title {
  font-size: 13px;
  font-weight: 600;
}

.channel-account-status {
  font-size: 11px;
  color: var(--ui-accent);
  background: #eaf4f1;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.3);
  border-radius: 999px;
  padding: 2px 7px;
}

.channel-account-status.disabled {
  color: #8d8d8d;
  border-color: #d6d6d6;
  background: #f1f1f1;
}

.channel-content {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  overflow: auto;
}

.channel-content-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.channel-detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.channel-detail-card {
  border: 1px solid #e7e7e7;
  border-radius: 10px;
  background: #fafafa;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.channel-detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.channel-detail-label {
  font-size: 11px;
  color: #868686;
}

.channel-detail-value {
  margin-top: 4px;
  font-size: 13px;
  color: #252525;
  word-break: break-word;
}

.channel-detail-hint {
  font-size: 11px;
  color: #8a8a8a;
}

@media (max-width: 980px) {
  .channel-manager-page {
    grid-template-columns: 1fr;
  }

  .channel-create-body,
  .channel-form,
  .channel-detail-grid {
    grid-template-columns: 1fr;
  }
}
</style>
