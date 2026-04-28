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
              <option v-for="option in createChannelOptions" :key="option.channel" :value="option.channel">
                {{ option.label }}
              </option>
            </select>
          </label>
          <div v-if="isFeishuCreate" class="channel-create-checks">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createForm.receive_group_chat" type="checkbox" />
              <span>{{ t('channels.config.receiveGroup') }}</span>
            </label>
          </div>

          <template v-for="field in createChannelFields" :key="`create-${field.key}`">
            <label
              v-if="field.type === 'checkbox' && !(createForm.channel === 'qqbot' && field.key === 'markdown_support')"
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
          <div v-if="createForm.channel === 'weixin'" class="channel-weixin-qr-panel">
            <div class="channel-detail-label">{{ t('channels.form.weixin.qrTitle') }}</div>
            <div v-if="createWeixinQrPreviewUrl" class="channel-weixin-qr-preview">
              <img
                class="channel-weixin-qr-image"
                :src="createWeixinQrPreviewUrl"
                :alt="t('channels.form.weixin.qrImageAlt')"
                referrerpolicy="no-referrer"
                role="button"
                tabindex="0"
                @click="refreshCreateWeixinQr"
                @keydown.enter.prevent="refreshCreateWeixinQr"
                @keydown.space.prevent="refreshCreateWeixinQr"
              />
            </div>
            <div v-if="createWeixinQrState.sessionKey" class="channel-detail-hint">
              {{ t('channels.form.weixin.qrSessionKey') }}: {{ createWeixinQrState.sessionKey }}
            </div>
            <div v-if="createWeixinQrState.status" class="channel-detail-hint">
              {{ t('channels.form.weixin.qrStatusLabel') }}: {{ formatWeixinQrStatus(createWeixinQrState.status) }}
            </div>
            <div v-if="createWeixinQrState.message" class="channel-detail-hint">
              {{ createWeixinQrState.message }}
            </div>
          </div>
          <div v-if="showCreateWeixinAdvancedToggle" class="channel-advanced-toggle">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createWeixinAdvancedEnabled" type="checkbox" />
              <span>{{ t('channels.form.weixin.advancedToggle') }}</span>
            </label>
          </div>

          <div v-if="showCreateXmppAdvancedToggle" class="channel-advanced-toggle">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createXmppAdvancedEnabled" type="checkbox" />
              <span>{{ t('channels.form.xmpp.advancedToggle') }}</span>
            </label>
            <div class="channel-detail-hint">{{ t('channels.form.xmpp.advancedHint') }}</div>
          </div>

          <div v-if="createForm.channel === 'qqbot'" class="channel-inline-options">
            <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
              <input v-model="createDynamicFields.markdown_support" type="checkbox" />
              <span>{{ t('channels.form.qqbot.markdownSupport') }}</span>
            </label>
          </div>
        </div>

        <div class="channel-create-actions">
          <button
            v-if="createForm.channel !== 'weixin'"
            class="channel-action-btn"
            type="button"
            :disabled="createSaving"
            @click="createAccount"
          >
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
            <div class="channel-account-title" :title="account.title">{{ account.title }}</div>
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
          <button class="channel-action-btn" type="button" :disabled="saving || !selectedAccount" @click="saveAccount">
            {{ saving ? t('common.saving') : t('common.save') }}
          </button>
          <button class="channel-action-btn danger" type="button" :disabled="saving || !selectedAccount" @click="removeAccount">
            {{ t('channels.action.delete') }}
          </button>
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
          </div>
        </div>

        <div class="channel-detail-card">
          <div class="channel-detail-title">{{ t('channels.config.title') }}</div>
          <div class="channel-form">
            <label v-if="isFeishuEdit" class="channel-form-field channel-form-checkbox">
              <input v-model="editForm.receive_group_chat" type="checkbox" />
              <span>{{ t('channels.config.receiveGroup') }}</span>
            </label>

            <template v-for="field in editChannelFields" :key="`edit-${field.key}`">
              <label
                v-if="field.type === 'checkbox' && !(selectedAccount?.channel === 'qqbot' && field.key === 'markdown_support')"
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
            <div v-if="selectedAccount?.channel === 'weixin'" class="channel-weixin-qr-panel">
              <div class="channel-detail-label">{{ t('channels.form.weixin.qrTitle') }}</div>
              <div class="channel-inline-options">
                <button
                  class="channel-refresh-btn subtle"
                  type="button"
                  :disabled="saving || editWeixinQrState.loadingStart || editWeixinQrState.loadingWait"
                  @click="startEditWeixinQr"
                >
                  {{
                    editWeixinQrState.loadingStart || editWeixinQrState.loadingWait
                      ? t('common.loading')
                      : t('channels.form.weixin.qrGenerate')
                  }}
                </button>
              </div>
              <div v-if="editWeixinQrPreviewUrl" class="channel-weixin-qr-preview">
                <img
                  class="channel-weixin-qr-image"
                  :src="editWeixinQrPreviewUrl"
                  :alt="t('channels.form.weixin.qrImageAlt')"
                  referrerpolicy="no-referrer"
                />
              </div>
              <div v-if="editWeixinQrState.sessionKey" class="channel-detail-hint">
                {{ t('channels.form.weixin.qrSessionKey') }}: {{ editWeixinQrState.sessionKey }}
              </div>
              <div v-if="editWeixinQrState.status" class="channel-detail-hint">
                {{ t('channels.form.weixin.qrStatusLabel') }}: {{ formatWeixinQrStatus(editWeixinQrState.status) }}
              </div>
              <div v-if="editWeixinQrState.message" class="channel-detail-hint">
                {{ editWeixinQrState.message }}
              </div>
            </div>
            <div v-if="showEditWeixinAdvancedToggle" class="channel-advanced-toggle">
              <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
                <input v-model="editWeixinAdvancedEnabled" type="checkbox" />
                <span>{{ t('channels.form.weixin.advancedToggle') }}</span>
              </label>
            </div>

            <div v-if="showEditXmppAdvancedToggle" class="channel-advanced-toggle">
              <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
                <input v-model="editXmppAdvancedEnabled" type="checkbox" />
                <span>{{ t('channels.form.xmpp.advancedToggle') }}</span>
              </label>
              <div class="channel-detail-hint">{{ t('channels.form.xmpp.advancedHint') }}</div>
            </div>

            <div v-if="selectedAccount?.channel === 'qqbot'" class="channel-inline-options">
              <label class="channel-form-field channel-form-checkbox channel-form-checkbox--inline">
                <input v-model="editDynamicFields.markdown_support" type="checkbox" />
                <span>{{ t('channels.form.qqbot.markdownSupport') }}</span>
              </label>
            </div>
          </div>

        </div>
      </div>

      <div v-if="!permissionDenied" class="channel-runtime-log-card">
        <div class="channel-runtime-log-header">
          <div>
            <div class="channel-detail-title">{{ t('channels.runtime.title') }}</div>
            <div v-if="runtimeStatusText" class="channel-runtime-log-status">{{ runtimeStatusText }}</div>
          </div>
          <div class="channel-runtime-log-actions">
            <button
              class="channel-refresh-btn subtle"
              type="button"
              :disabled="runtimeProbeLoading || runtimeLogsLoading"
              @click="writeRuntimeProbe"
            >
              {{ runtimeProbeLoading ? t('common.saving') : t('channels.runtime.probe') }}
            </button>
            <button
              class="channel-refresh-btn subtle"
              type="button"
              :disabled="runtimeLogsLoading"
              @click="refreshRuntimeLogs()"
            >
              {{ t('common.refresh') }}
            </button>
            <button class="channel-refresh-btn subtle" type="button" @click="clearRuntimeLogsView">
              {{ t('common.clear') }}
            </button>
          </div>
        </div>
        <div v-if="runtimeLogsLoading && !runtimeLogs.length" class="channel-empty">{{ t('common.loading') }}</div>
        <div v-else-if="runtimeLogsError" class="channel-runtime-log-error">{{ runtimeLogsError }}</div>
        <div v-else-if="!visibleRuntimeLogs.length" class="channel-empty">{{ t('channels.runtime.empty') }}</div>
        <div v-else class="channel-runtime-log-list">
          <div
            v-for="item in visibleRuntimeLogs"
            :key="item.id"
            class="channel-runtime-log-item"
            :class="`channel-runtime-log-item--${item.level}`"
          >
            <div class="channel-runtime-log-meta">
              <span class="channel-runtime-log-level">{{ runtimeLevelLabel(item.level) }}</span>
              <span>{{ providerLabel(item.channel) }}</span>
              <span v-if="item.account_id">{{ item.account_id }}</span>
              <span>{{ formatRuntimeLogTime(item.ts) }}</span>
              <span v-if="item.repeat_count > 1" class="channel-runtime-log-repeat">
                x{{ item.repeat_count }}
              </span>
            </div>
            <div class="channel-runtime-log-message">{{ item.message }}</div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteChannelAccount,
  listChannelAccounts,
  listChannelBindings,
  listChannelRuntimeLogs,
  startWeixinQrLogin,
  upsertChannelAccount,
  waitWeixinQrLogin,
  writeChannelRuntimeProbe
} from '@/api/channels';
import { getDesktopLocalToken, isDesktopModeEnabled } from '@/config/desktop';
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
  },
  active: {
    type: Boolean,
    default: true
  }
});

const emit = defineEmits(['changed']);

const { t } = useI18n();
const isPanelActive = computed(() => props.active !== false);

type DynamicFieldType = 'text' | 'password' | 'checkbox';

type ChannelFieldDef = {
  key: string;
  labelKey: string;
  placeholderKey?: string;
  type?: DynamicFieldType;
  required?: boolean;
  defaultValue?: string | boolean;
  advanced?: boolean;
};

type ChannelSchema = {
  mode: 'feishu' | 'wechat' | 'wechat_mp' | 'weixin' | 'config';
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
  weixin?: {
    api_base?: string;
    cdn_base?: string;
    bot_token?: string;
    ilink_bot_id?: string;
    ilink_user_id?: string;
    bot_type?: string;
    long_connection_enabled?: boolean;
    allow_from?: string[];
    poll_timeout_ms?: number;
    api_timeout_ms?: number;
    max_consecutive_failures?: number;
    backoff_ms?: number;
    route_tag?: string;
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
  weixinBotTokenSet: boolean;
  rawConfig: Record<string, unknown>;
};

type SupportedChannelItem = {
  channel: string;
  display_name?: string;
  description?: string;
  docs_hint?: string;
};

type ChannelRuntimeLogItem = {
  id: string;
  ts: number;
  level: string;
  channel: string;
  account_id: string;
  event: string;
  message: string;
  repeat_count: number;
};

type ChannelRuntimeLogStatus = {
  collector_alive: boolean;
  server_ts: number;
  owned_accounts: number;
  scanned_total: number;
};

type WeixinQrState = {
  sessionKey: string;
  qrcode: string;
  qrcodeUrl: string;
  qrcodeOpenUrl: string;
  botType: string;
  status: string;
  message: string;
  apiBase: string;
  loadingStart: boolean;
  loadingWait: boolean;
};

const DEFAULT_WEIXIN_API_BASE = 'https://ilinkai.weixin.qq.com';
const DEFAULT_WEIXIN_BOT_TYPE = '3';

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
  weixin: {
    mode: 'weixin',
    fields: [
      {
        key: 'api_base',
        labelKey: 'channels.form.weixin.apiBase',
        placeholderKey: 'channels.form.weixin.apiBasePlaceholder',
        defaultValue: DEFAULT_WEIXIN_API_BASE
      },
      {
        key: 'cdn_base',
        labelKey: 'channels.form.weixin.cdnBase',
        placeholderKey: 'channels.form.weixin.cdnBasePlaceholder'
      },
      {
        key: 'bot_token',
        labelKey: 'channels.form.weixin.botToken',
        placeholderKey: 'channels.form.weixin.botTokenPlaceholder',
        type: 'password',
        required: true
      },
      {
        key: 'ilink_bot_id',
        labelKey: 'channels.form.weixin.ilinkBotId',
        placeholderKey: 'channels.form.weixin.ilinkBotIdPlaceholder',
        required: true
      },
      {
        key: 'ilink_user_id',
        labelKey: 'channels.form.weixin.ilinkUserId',
        placeholderKey: 'channels.form.weixin.ilinkUserIdPlaceholder'
      },
      {
        key: 'bot_type',
        labelKey: 'channels.form.weixin.botType',
        placeholderKey: 'channels.form.weixin.botTypePlaceholder',
        defaultValue: DEFAULT_WEIXIN_BOT_TYPE
      },
      {
        key: 'allow_from',
        labelKey: 'channels.form.weixin.allowFrom',
        placeholderKey: 'channels.form.weixin.allowFromPlaceholder'
      },
      {
        key: 'long_connection_enabled',
        labelKey: 'channels.form.weixin.longConnectionEnabled',
        type: 'checkbox',
        defaultValue: true
      },
      {
        key: 'poll_timeout_ms',
        labelKey: 'channels.form.weixin.pollTimeoutMs',
        placeholderKey: 'channels.form.weixin.pollTimeoutMsPlaceholder'
      },
      {
        key: 'api_timeout_ms',
        labelKey: 'channels.form.weixin.apiTimeoutMs',
        placeholderKey: 'channels.form.weixin.apiTimeoutMsPlaceholder'
      },
      {
        key: 'max_consecutive_failures',
        labelKey: 'channels.form.weixin.maxConsecutiveFailures',
        placeholderKey: 'channels.form.weixin.maxConsecutiveFailuresPlaceholder'
      },
      {
        key: 'backoff_ms',
        labelKey: 'channels.form.weixin.backoffMs',
        placeholderKey: 'channels.form.weixin.backoffMsPlaceholder'
      },
      {
        key: 'route_tag',
        labelKey: 'channels.form.weixin.routeTag',
        placeholderKey: 'channels.form.weixin.routeTagPlaceholder'
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
        placeholderKey: 'channels.form.qqbot.appIdPlaceholder'
      },
      {
        key: 'client_secret',
        labelKey: 'channels.form.qqbot.clientSecret',
        placeholderKey: 'channels.form.qqbot.clientSecretPlaceholder',
        type: 'password'
      },
      {
        key: 'token',
        labelKey: 'channels.form.qqbot.token',
        placeholderKey: 'channels.form.qqbot.tokenPlaceholder',
        type: 'password'
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
        placeholderKey: 'channels.form.xmpp.domainPlaceholder',
        advanced: true
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
        key: 'trust_self_signed',
        labelKey: 'channels.form.xmpp.trustSelfSigned',
        type: 'checkbox',
        defaultValue: true
      },
      {
        key: 'direct_tls',
        labelKey: 'channels.form.xmpp.directTls',
        type: 'checkbox',
        defaultValue: false,
        advanced: true
      },
      {
        key: 'muc_nick',
        labelKey: 'channels.form.xmpp.mucNick',
        placeholderKey: 'channels.form.xmpp.mucNickPlaceholder',
        advanced: true
      },
      {
        key: 'muc_rooms',
        labelKey: 'channels.form.xmpp.mucRooms',
        placeholderKey: 'channels.form.xmpp.mucRoomsPlaceholder',
        advanced: true
      },
      {
        key: 'heartbeat_enabled',
        labelKey: 'channels.form.xmpp.heartbeatEnabled',
        type: 'checkbox',
        defaultValue: true,
        advanced: true
      },
      {
        key: 'heartbeat_interval_s',
        labelKey: 'channels.form.xmpp.heartbeatIntervalS',
        placeholderKey: 'channels.form.xmpp.heartbeatIntervalSPlaceholder',
        advanced: true
      },
      {
        key: 'heartbeat_timeout_s',
        labelKey: 'channels.form.xmpp.heartbeatTimeoutS',
        placeholderKey: 'channels.form.xmpp.heartbeatTimeoutSPlaceholder',
        advanced: true
      },
      {
        key: 'respond_ping',
        labelKey: 'channels.form.xmpp.respondPing',
        type: 'checkbox',
        defaultValue: true,
        advanced: true
      }
    ]
  }
};

const FALLBACK_CHANNELS = [
  'qqbot',
  'weixin',
  'wechat',
  'wechat_mp',
  'feishu',
  'whatsapp',
  'telegram',
  'discord',
  'slack',
  'line',
  'dingtalk',
  'xmpp'
];
const USER_ONLY_CHANNELS = ['wechat', 'wechat_mp', 'weixin'];
const AUTO_ACCOUNT_NAME_PREFIX: Record<string, string> = {
  weixin: '微信',
  wechat: '微信',
  wechat_mp: '微信',
  qqbot: 'qq',
  feishu: '飞书',
  whatsapp: 'wa',
  telegram: 'tg',
  discord: 'dc',
  slack: 'slack',
  line: 'line',
  dingtalk: '钉钉',
  xmpp: 'xmpp'
};
const CHANNEL_PRIORITY: Record<string, number> = {
  qqbot: 0,
  weixin: 1,
  wechat: 2,
  wechat_mp: 3,
  feishu: 4,
  whatsapp: 5,
  telegram: 6,
  discord: 7,
  slack: 8,
  line: 9,
  dingtalk: 10,
  xmpp: 11
};
const RUNTIME_LOG_POLL_INTERVAL_MS = 5000;
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
const supportedChannels = ref<SupportedChannelItem[]>([]);
const selectedKey = ref('');
const createXmppAdvancedEnabled = ref(false);
const editXmppAdvancedEnabled = ref(false);
const createWeixinAdvancedEnabled = ref(false);
const editWeixinAdvancedEnabled = ref(false);
const createWeixinAutoCreating = ref(false);
const createDynamicFields = reactive<Record<string, string | boolean>>({});
const editDynamicFields = reactive<Record<string, string | boolean>>({});
const editSecretSaved = reactive<Record<string, boolean>>({});
const createWeixinQrState = reactive<WeixinQrState>({
  sessionKey: '',
  qrcode: '',
  qrcodeUrl: '',
  qrcodeOpenUrl: '',
  botType: DEFAULT_WEIXIN_BOT_TYPE,
  status: '',
  message: '',
  apiBase: '',
  loadingStart: false,
  loadingWait: false
});
const editWeixinQrState = reactive<WeixinQrState>({
  sessionKey: '',
  qrcode: '',
  qrcodeUrl: '',
  qrcodeOpenUrl: '',
  botType: DEFAULT_WEIXIN_BOT_TYPE,
  status: '',
  message: '',
  apiBase: '',
  loadingStart: false,
  loadingWait: false
});
const runtimeLogs = ref<ChannelRuntimeLogItem[]>([]);
const runtimeStatus = ref<ChannelRuntimeLogStatus | null>(null);
const runtimeLogsLoading = ref(false);
const runtimeProbeLoading = ref(false);
const runtimeLogsError = ref('');
const runtimeLogsClearedAt = ref(0);
const mounted = ref(false);
const disposed = ref(false);
let runtimeLogsPollTimer: ReturnType<typeof setTimeout> | null = null;
let loadAccountsRequestId = 0;
let runtimeLogsRequestId = 0;
let lastLoadedAgentKey = '';

const createForm = reactive({
  channel: 'qqbot',
  receive_group_chat: true
});

const editForm = reactive({
  receive_group_chat: true
});

const selectedAccount = computed(
  () => accounts.value.find((item) => item.key === selectedKey.value) || null
);
const visibleRuntimeLogs = computed(() =>
  runtimeLogs.value.filter((item) => item.ts > runtimeLogsClearedAt.value)
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
        label: providerLabel(channel),
        priority: CHANNEL_PRIORITY[channel] ?? 99
      };
    })
    .filter(
      (item): item is { channel: string; label: string; priority: number } => Boolean(item)
    )
    .sort((left, right) => {
      if (left.priority !== right.priority) {
        return left.priority - right.priority;
      }
      return left.label.localeCompare(right.label, 'zh-Hans-CN');
    });
});
const createChannelOptions = computed(() => supportedChannelOptions.value);

const isFeishuCreate = computed(() => createForm.channel === 'feishu');
const isFeishuEdit = computed(() => selectedAccount.value?.channel === 'feishu');
const createChannelSchema = computed(() => schemaForChannel(createForm.channel));
const editChannelSchema = computed(() =>
  selectedAccount.value ? schemaForChannel(selectedAccount.value.channel) : null
);
const createSchemaFields = computed(() => CHANNEL_SCHEMAS[createForm.channel]?.fields || []);
const editSchemaFields = computed(() =>
  selectedAccount.value ? CHANNEL_SCHEMAS[selectedAccount.value.channel]?.fields || [] : []
);
const filterVisibleChannelFields = (
  channel: string,
  fields: ChannelFieldDef[],
  xmppAdvancedEnabled: boolean,
  weixinAdvancedEnabled: boolean
) => {
  if (channel === 'xmpp') {
    return xmppAdvancedEnabled ? fields : fields.filter((field) => !field.advanced);
  }
  if (channel === 'weixin' && !weixinAdvancedEnabled) {
    return [];
  }
  return fields;
};
const createChannelFields = computed(() =>
  filterVisibleChannelFields(
    createForm.channel,
    createSchemaFields.value,
    createXmppAdvancedEnabled.value,
    createWeixinAdvancedEnabled.value
  )
);
const editChannelFields = computed(() =>
  filterVisibleChannelFields(
    selectedAccount.value?.channel || '',
    editSchemaFields.value,
    editXmppAdvancedEnabled.value,
    editWeixinAdvancedEnabled.value
  )
);
const showCreateXmppAdvancedToggle = computed(
  () =>
    createForm.channel === 'xmpp' &&
    createSchemaFields.value.some((field) => field.advanced) &&
    createChannelSchema.value?.mode === 'config'
);
const showEditXmppAdvancedToggle = computed(
  () =>
    selectedAccount.value?.channel === 'xmpp' &&
    editSchemaFields.value.some((field) => field.advanced) &&
    editChannelSchema.value?.mode === 'config'
);
const showCreateWeixinAdvancedToggle = computed(
  () =>
    createForm.channel === 'weixin' &&
    createSchemaFields.value.length > 0 &&
    createChannelSchema.value?.mode === 'weixin'
);
const showEditWeixinAdvancedToggle = computed(
  () =>
    selectedAccount.value?.channel === 'weixin' &&
    editSchemaFields.value.length > 0 &&
    editChannelSchema.value?.mode === 'weixin'
);

const providerLabel = (channel: string) => {
  const normalized = String(channel || '').trim().toLowerCase();
  const item = supportedChannels.value.find((entry) => entry.channel === normalized);
  const displayName = String(item?.display_name || '').trim();
  if (displayName) {
    return displayName;
  }
  const key = `channels.provider.${normalized}`;
  const translated = t(key);
  return translated === key ? normalized : translated;
};

const providerDesc = (channel: string) => {
  const normalized = String(channel || '').trim().toLowerCase();
  const item = supportedChannels.value.find((entry) => entry.channel === normalized);
  const description = String(item?.description || '').trim();
  if (description) {
    return description;
  }
  const key = `channels.provider.${normalized}.desc`;
  const translated = t(key);
  return translated === key ? t('channels.provider.generic') : translated;
};

const runtimeLevelLabel = (level: string) => {
  const key = `channels.runtime.level.${String(level || '').trim().toLowerCase() || 'info'}`;
  const translated = t(key);
  return translated === key ? String(level || '').toUpperCase() : translated;
};
const formatRuntimeLogTime = (ts: number) => {
  const parsed = Number(ts);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return '';
  }
  return new Date(parsed * 1000).toLocaleString();
};
const runtimeStatusText = computed(() => {
  const status = runtimeStatus.value;
  if (!status) {
    return '';
  }
  if (!status.owned_accounts) {
    return t('channels.runtime.statusNoOwnedAccounts');
  }
  const tsText = formatRuntimeLogTime(status.server_ts);
  return `${t('channels.runtime.statusAlive')} · ${t('channels.runtime.statusOwnedAccounts')}: ${
    status.owned_accounts
  }${tsText ? ` · ${tsText}` : ''}`;
});

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

const escapeRegExp = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const buildAutoAccountName = (channel: string): string => {
  const normalized = String(channel || '').trim().toLowerCase();
  const prefix = AUTO_ACCOUNT_NAME_PREFIX[normalized] || providerLabel(normalized) || normalized || '账号';
  const matcher = new RegExp(`^${escapeRegExp(prefix)}(\\d+)$`);
  let maxSeq = 0;
  for (const account of accounts.value) {
    if (account.channel !== normalized) {
      continue;
    }
    const name = trimmedText(account.name);
    if (!name) {
      continue;
    }
    const matched = name.match(matcher);
    if (!matched) {
      continue;
    }
    const parsed = Number.parseInt(matched[1], 10);
    if (Number.isFinite(parsed) && parsed > maxSeq) {
      maxSeq = parsed;
    }
  }
  return `${prefix}${String(maxSeq + 1).padStart(3, '0')}`;
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
  if (schema.mode === 'weixin') {
    return getConfigNode(rawConfig, 'weixin');
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
  const weixinNode = getConfigNode(raw, 'weixin');
  const appSecretSet =
    preview?.feishu?.app_secret_set === true || Boolean(trimmedText(feishuNode.app_secret));
  const wechatSecretSet = Boolean(trimmedText(wechatNode.secret));
  const wechatMpAppSecretSet = Boolean(trimmedText(wechatMpNode.app_secret));
  const weixinBotTokenSet =
    preview?.weixin?.bot_token_set === true || Boolean(trimmedText(weixinNode.bot_token));
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
    weixinBotTokenSet,
    rawConfig: raw
  };
};

const selectAccount = (account) => {
  selectedKey.value = account.key;
  resetEditForm();
};

const resolveDefaultCreateChannel = () => {
  const preferred = createChannelOptions.value.find((item) => item.channel === 'qqbot');
  if (preferred) {
    return preferred.channel;
  }
  if (createChannelOptions.value.length > 0) {
    return createChannelOptions.value[0].channel;
  }
  return 'qqbot';
};

const applyCreateChannelDefaults = () => {
  clearWeixinQrState(createWeixinQrState);
  createForm.receive_group_chat = true;
  createXmppAdvancedEnabled.value = false;
  createWeixinAdvancedEnabled.value = false;
  initDynamicFields(createDynamicFields, createForm.channel, {}, true);
};

const resetCreateForm = () => {
  createWeixinAutoCreating.value = false;
  createForm.channel = resolveDefaultCreateChannel();
  applyCreateChannelDefaults();
};

const resetEditForm = () => {
  clearWeixinQrState(editWeixinQrState);
  clearSecretState();
  const account = selectedAccount.value;
  if (!account) {
    editForm.receive_group_chat = true;
    editXmppAdvancedEnabled.value = false;
    editWeixinAdvancedEnabled.value = false;
    clearDynamicFields(editDynamicFields);
    return;
  }
  editForm.receive_group_chat = account.receiveGroupChat;
  editXmppAdvancedEnabled.value = false;
  editWeixinAdvancedEnabled.value = false;
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

const validateQqbotCredentialFields = (
  values: Record<string, string | boolean>,
  secretFallback: Record<string, boolean> = {}
): string | null => {
  const token = trimmedText(values.token);
  if (token) {
    return null;
  }
  const appId = trimmedText(values.app_id);
  const clientSecret = trimmedText(values.client_secret);
  const hasClientSecret = Boolean(clientSecret || secretFallback.client_secret);
  if (appId && hasClientSecret) {
    return null;
  }
  return t('channels.form.qqbot.credentialRequired');
};

const parseCommaSeparatedList = (rawValue: unknown): string[] => {
  const text = trimmedText(rawValue);
  if (!text) {
    return [];
  }
  return text
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
};

const parsePositiveInteger = (rawValue: unknown): number | undefined => {
  const text = trimmedText(rawValue);
  if (!text) {
    return undefined;
  }
  const parsed = Number.parseInt(text, 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return undefined;
  }
  return parsed;
};

const clearWeixinQrState = (target: WeixinQrState) => {
  target.sessionKey = '';
  target.qrcode = '';
  target.qrcodeUrl = '';
  target.qrcodeOpenUrl = '';
  target.botType = DEFAULT_WEIXIN_BOT_TYPE;
  target.status = '';
  target.message = '';
  target.apiBase = '';
  target.loadingStart = false;
  target.loadingWait = false;
};

const normalizeWeixinQrImageValue = (rawValue: unknown, apiBaseRaw?: unknown): string => {
  let value = trimmedText(rawValue);
  const apiBase = trimmedText(apiBaseRaw);
  if (!value) {
    return '';
  }
  if (
    (value.startsWith('"') && value.endsWith('"')) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    value = value.slice(1, -1).trim();
  }
  value = value
    .replace(/\\r\\n/g, '')
    .replace(/\\n/g, '')
    .replace(/\\r/g, '')
    .replace(/\r\n/g, '')
    .replace(/\n/g, '')
    .replace(/\r/g, '')
    .trim();
  if (!value) {
    return '';
  }

  if (value.startsWith('data:image/')) {
    return value;
  }
  const compact = value.replace(/\s+/g, '');
  const base64Candidate = compact.replace(/^data:image\/[a-z0-9.+-]+;base64,/i, '');
  const looksLikeBase64 =
    base64Candidate.length > 64 && /^[A-Za-z0-9+/]+=*$/.test(base64Candidate);
  if (looksLikeBase64) {
    return `data:image/png;base64,${base64Candidate}`;
  }

  if (value.startsWith('blob:') || /^https?:\/\//i.test(value)) {
    return value;
  }
  if (value.startsWith('//')) {
    return `${window.location.protocol}${value}`;
  }
  if (value.startsWith('/')) {
    if (apiBase) {
      try {
        return new URL(value, apiBase).toString();
      } catch (error) {
        // Fall through to current origin below.
      }
    }
    return `${window.location.origin}${value}`;
  }
  return '';
};

const buildWeixinQrRenderUrl = (rawValue: unknown, apiBaseRaw?: unknown): string => {
  const text = trimmedText(rawValue);
  if (!text) {
    return '';
  }
  const params = new URLSearchParams({ text });
  if (isDesktopModeEnabled()) {
    const desktopToken = getDesktopLocalToken();
    if (desktopToken) {
      params.set('access_token', desktopToken);
    }
  }
  const apiBase = trimmedText(apiBaseRaw);
  if (apiBase) {
    params.set('api_base', apiBase);
  }
  return `/wunder/channels/weixin/qr/render?${params.toString()}`;
};

const buildWeixinQrPromptPageUrl = (qrcodeRaw: unknown, botTypeRaw: unknown): string => {
  const qrcode = trimmedText(qrcodeRaw);
  if (!qrcode) {
    return '';
  }
  if (
    qrcode.startsWith('http://') ||
    qrcode.startsWith('https://') ||
    qrcode.startsWith('blob:') ||
    qrcode.startsWith('data:image/')
  ) {
    return '';
  }
  const botType = trimmedText(botTypeRaw) || DEFAULT_WEIXIN_BOT_TYPE;
  const params = new URLSearchParams({
    qrcode,
    bot_type: botType
  });
  return `https://liteapp.weixin.qq.com/q/7GiQu1?${params.toString()}`;
};

const resolveWeixinQrOpenUrl = (state: WeixinQrState): string => {
  const openRaw = normalizeWeixinQrImageValue(state.qrcodeOpenUrl, state.apiBase);
  if (
    openRaw.startsWith('http://') ||
    openRaw.startsWith('https://') ||
    openRaw.startsWith('blob:')
  ) {
    return openRaw;
  }
  const promptFromQrcode = buildWeixinQrPromptPageUrl(state.qrcode, state.botType);
  if (promptFromQrcode) {
    return promptFromQrcode;
  }

  const fromRaw = normalizeWeixinQrImageValue(state.qrcodeUrl, state.apiBase);
  if (
    fromRaw.startsWith('http://') ||
    fromRaw.startsWith('https://') ||
    fromRaw.startsWith('blob:')
  ) {
    return fromRaw;
  }

  const fromQrcode = normalizeWeixinQrImageValue(state.qrcode, state.apiBase);
  if (
    fromQrcode.startsWith('http://') ||
    fromQrcode.startsWith('https://') ||
    fromQrcode.startsWith('blob:')
  ) {
    return fromQrcode;
  }
  return '';
};

const resolveWeixinQrPreviewUrl = (state: WeixinQrState): string => {
  const openUrl = resolveWeixinQrOpenUrl(state);
  if (openUrl.startsWith('http://') || openUrl.startsWith('https://')) {
    return buildWeixinQrRenderUrl(openUrl, state.apiBase);
  }
  if (openUrl.startsWith('blob:') || openUrl.startsWith('data:image/')) {
    return openUrl;
  }
  const fromUrl = normalizeWeixinQrImageValue(state.qrcodeUrl, state.apiBase);
  if (fromUrl) {
    return fromUrl;
  }
  const fromQrcode = normalizeWeixinQrImageValue(state.qrcode, state.apiBase);
  if (fromQrcode) {
    return fromQrcode;
  }
  return '';
};
const createWeixinQrPreviewUrl = computed(() => resolveWeixinQrPreviewUrl(createWeixinQrState));
const editWeixinQrPreviewUrl = computed(() => resolveWeixinQrPreviewUrl(editWeixinQrState));

const formatWeixinQrStatus = (status: string) => {
  const normalized = String(status || '').trim().toLowerCase();
  if (!normalized) {
    return '';
  }
  const key = `channels.form.weixin.qrStatus.${normalized}`;
  const translated = t(key);
  return translated === key ? normalized : translated;
};

const applyWeixinQrResultToFields = (
  values: Record<string, string | boolean>,
  result: Record<string, unknown>
) => {
  const botToken = trimmedText(result.bot_token);
  if (botToken) {
    values.bot_token = botToken;
  }
  const ilinkBotId = trimmedText(result.ilink_bot_id);
  if (ilinkBotId) {
    values.ilink_bot_id = ilinkBotId;
  }
  const ilinkUserId = trimmedText(result.ilink_user_id);
  if (ilinkUserId) {
    values.ilink_user_id = ilinkUserId;
  }
  const apiBase = trimmedText(result.api_base);
  if (apiBase) {
    values.api_base = apiBase;
  }
};

const startWeixinQrFlow = async (scope: 'create' | 'edit', force = false) => {
  const isCreate = scope === 'create';
  if (isCreate && createForm.channel !== 'weixin') {
    return;
  }
  if (!isCreate && selectedAccount.value?.channel !== 'weixin') {
    return;
  }

  const values = isCreate ? createDynamicFields : editDynamicFields;
  const state = isCreate ? createWeixinQrState : editWeixinQrState;
  const accountId = isCreate ? '' : trimmedText(selectedAccount.value?.account_id);
  const apiBase = trimmedText(values.api_base) || state.apiBase || DEFAULT_WEIXIN_API_BASE;
  const botType = trimmedText(values.bot_type) || DEFAULT_WEIXIN_BOT_TYPE;

  state.loadingStart = true;
  state.message = '';
  try {
    const payload: Record<string, unknown> = {
      api_base: apiBase,
      bot_type: botType,
      force
    };
    if (accountId) {
      payload.account_id = accountId;
    }
    const { data } = await startWeixinQrLogin(payload);
    const result = isObjectRecord(data?.data) ? data.data : {};
    state.sessionKey = trimmedText(result.session_key);
    state.qrcode = trimmedText(result.qrcode);
    state.qrcodeUrl = trimmedText(result.qrcode_url);
    state.qrcodeOpenUrl = trimmedText(result.qrcode_open_url);
    state.botType = trimmedText(result.bot_type) || botType;
    state.apiBase = trimmedText(result.api_base) || apiBase;
    state.status = 'wait';
    state.message = t('channels.form.weixin.qrStartSuccess');

    if (!state.sessionKey || !resolveWeixinQrPreviewUrl(state)) {
      throw new Error(t('channels.form.weixin.qrInvalidResponse'));
    }
    if (!trimmedText(values.api_base)) {
      values.api_base = state.apiBase;
    }
    if (!trimmedText(values.bot_type)) {
      values.bot_type = botType;
    }
    ElMessage.success(t('channels.form.weixin.qrStartSuccess'));
    // Auto-poll after QR generation so users only need one click and one scan.
    void waitWeixinQrFlow(scope);
  } catch (error) {
    showApiError(error, t('channels.form.weixin.qrStartFailed'));
  } finally {
    state.loadingStart = false;
  }
};

const waitWeixinQrFlow = async (scope: 'create' | 'edit') => {
  const isCreate = scope === 'create';
  if (isCreate && createForm.channel !== 'weixin') {
    return;
  }
  if (!isCreate && selectedAccount.value?.channel !== 'weixin') {
    return;
  }

  const values = isCreate ? createDynamicFields : editDynamicFields;
  const state = isCreate ? createWeixinQrState : editWeixinQrState;
  if (!trimmedText(state.sessionKey)) {
    ElMessage.warning(t('channels.form.weixin.qrSessionMissing'));
    return;
  }
  const apiBase = trimmedText(values.api_base) || state.apiBase || DEFAULT_WEIXIN_API_BASE;

  state.loadingWait = true;
  state.message = t('channels.form.weixin.qrWaiting');
  try {
    const { data } = await waitWeixinQrLogin({
      session_key: state.sessionKey,
      api_base: apiBase,
      timeout_ms: 120000
    });
    const result = isObjectRecord(data?.data) ? data.data : {};
    const connected = result.connected === true;
    const status = trimmedText(result.status).toLowerCase();
    state.status = status || state.status || 'wait';
    state.message = trimmedText(result.message) || state.message;
    const responseApiBase = trimmedText(result.api_base);
    if (responseApiBase) {
      state.apiBase = responseApiBase;
      values.api_base = responseApiBase;
    }

    if (connected) {
      applyWeixinQrResultToFields(values, result);
      state.message = trimmedText(result.message) || t('channels.form.weixin.qrWaitSuccess');
      ElMessage.success(t('channels.form.weixin.qrWaitSuccess'));
      if (
        isCreate &&
        creating.value &&
        createForm.channel === 'weixin' &&
        !createWeixinAutoCreating.value &&
        !createSaving.value
      ) {
        createWeixinAutoCreating.value = true;
        try {
          await createAccount();
        } finally {
          createWeixinAutoCreating.value = false;
        }
      }
      return;
    }

    if (state.status === 'expired') {
      ElMessage.warning(t('channels.form.weixin.qrExpired'));
      return;
    }
  } catch (error) {
    showApiError(error, t('channels.form.weixin.qrWaitFailed'));
  } finally {
    state.loadingWait = false;
  }
};

const startCreateWeixinQr = async (force = false) => {
  await startWeixinQrFlow('create', force);
};

const refreshCreateWeixinQr = async () => {
  await startCreateWeixinQr(true);
};

const startEditWeixinQr = async () => {
  await startWeixinQrFlow('edit');
};

const validateWeixinNumericFields = (values: Record<string, string | boolean>): string | null => {
  const numericFields = [
    { key: 'poll_timeout_ms', labelKey: 'channels.form.weixin.pollTimeoutMs' },
    { key: 'api_timeout_ms', labelKey: 'channels.form.weixin.apiTimeoutMs' },
    { key: 'max_consecutive_failures', labelKey: 'channels.form.weixin.maxConsecutiveFailures' },
    { key: 'backoff_ms', labelKey: 'channels.form.weixin.backoffMs' }
  ];
  for (const field of numericFields) {
    const rawValue = trimmedText(values[field.key]);
    if (!rawValue) {
      continue;
    }
    const parsed = Number.parseInt(rawValue, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
      return t('channels.config.positiveIntegerRequired', { field: t(field.labelKey) });
    }
  }
  return null;
};

const ensureWeixinCredentialsReady = (
  values: Record<string, string | boolean>,
  secretFallback: Record<string, boolean> = {}
): boolean => {
  const hasBotToken = Boolean(trimmedText(values.bot_token)) || Boolean(secretFallback.bot_token);
  const hasIlinkBotId = Boolean(trimmedText(values.ilink_bot_id));
  if (hasBotToken && hasIlinkBotId) {
    return true;
  }
  ElMessage.warning(t('channels.form.weixin.qrSessionMissing'));
  return false;
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

const clearRuntimeLogTimer = () => {
  if (runtimeLogsPollTimer === null) {
    return;
  }
  clearTimeout(runtimeLogsPollTimer);
  runtimeLogsPollTimer = null;
};

const scheduleRuntimeLogPolling = () => {
  if (!mounted.value || disposed.value || !isPanelActive.value) {
    return;
  }
  clearRuntimeLogTimer();
  runtimeLogsPollTimer = setTimeout(() => {
    if (!mounted.value || disposed.value) {
      return;
    }
    void refreshRuntimeLogs(true);
  }, RUNTIME_LOG_POLL_INTERVAL_MS);
};

const normalizeRuntimeLog = (item: unknown, index: number): ChannelRuntimeLogItem | null => {
  const row = isObjectRecord(item) ? item : null;
  if (!row) {
    return null;
  }
  const channel = String(row.channel || '').trim().toLowerCase();
  if (!channel) {
    return null;
  }
  const accountId = String(row.account_id || '').trim();
  const ts = Number(row.ts || 0);
  return {
    id: String(row.id || `${channel}:${accountId}:${ts}:${index}`),
    ts: Number.isFinite(ts) ? ts : 0,
    level: String(row.level || 'info').trim().toLowerCase(),
    channel,
    account_id: accountId,
    event: String(row.event || '').trim().toLowerCase(),
    message: String(row.message || '').trim(),
    repeat_count: Math.max(1, Number(row.repeat_count || 1) || 1)
  };
};

const normalizeRuntimeStatus = (value: unknown): ChannelRuntimeLogStatus | null => {
  const row = isObjectRecord(value) ? value : null;
  if (!row) {
    return null;
  }
  const serverTs = Number(row.server_ts || 0);
  const ownedAccounts = Number(row.owned_accounts || 0);
  const scannedTotal = Number(row.scanned_total || 0);
  return {
    collector_alive: row.collector_alive !== false,
    server_ts: Number.isFinite(serverTs) ? serverTs : 0,
    owned_accounts: Number.isFinite(ownedAccounts) ? Math.max(0, ownedAccounts) : 0,
    scanned_total: Number.isFinite(scannedTotal) ? Math.max(0, scannedTotal) : 0
  };
};

const clearRuntimeLogsView = () => {
  runtimeLogsClearedAt.value = Date.now() / 1000;
};

const refreshRuntimeLogs = async (silent = false) => {
  const requestId = ++runtimeLogsRequestId;
  if (!silent) {
    runtimeLogsLoading.value = true;
  }
  try {
    const params: {
      limit: number;
      agent_id?: string;
      channel?: string;
      account_id?: string;
    } = {
      limit: 80
    };
    if (selectedAccount.value) {
      params.channel = selectedAccount.value.channel;
      params.account_id = selectedAccount.value.account_id;
    } else if (resolvedAgentId.value) {
      params.agent_id = resolvedAgentId.value;
    }
    const { data } = await listChannelRuntimeLogs(params);
    if (requestId !== runtimeLogsRequestId || disposed.value) {
      return;
    }
    const rows = Array.isArray(data?.data?.items) ? data.data.items : [];
    runtimeStatus.value = normalizeRuntimeStatus(data?.data?.status);
    runtimeLogs.value = rows
      .map((item, index) => normalizeRuntimeLog(item, index))
      .filter((item): item is ChannelRuntimeLogItem => Boolean(item));
    runtimeLogsError.value = '';
  } catch (error) {
    if (requestId !== runtimeLogsRequestId || disposed.value) {
      return;
    }
    runtimeLogs.value = [];
    runtimeStatus.value = null;
    runtimeLogsError.value = t('channels.runtime.loadFailed');
  } finally {
    if (requestId === runtimeLogsRequestId) {
      runtimeLogsLoading.value = false;
      scheduleRuntimeLogPolling();
    }
  }
};

const writeRuntimeProbe = async () => {
  runtimeProbeLoading.value = true;
  try {
    const payload: Record<string, string> = {};
    if (selectedAccount.value) {
      payload.channel = selectedAccount.value.channel;
      payload.account_id = selectedAccount.value.account_id;
    } else if (resolvedAgentId.value) {
      payload.agent_id = resolvedAgentId.value;
    }
    await writeChannelRuntimeProbe(payload);
    ElMessage.success(t('channels.runtime.probeSuccess'));
    await refreshRuntimeLogs(true);
  } catch (error) {
    showApiError(error, t('channels.runtime.probeFailed'));
  } finally {
    runtimeProbeLoading.value = false;
  }
};

const loadAccounts = async (preferred = undefined) => {
  const requestId = ++loadAccountsRequestId;
  loading.value = true;
  try {
    const [accountsResp, bindingsResp] = await Promise.all([
      listChannelAccounts(),
      resolvedAgentId.value ? listChannelBindings() : Promise.resolve({ data: null })
    ]);
    if (requestId !== loadAccountsRequestId || disposed.value) {
      return;
    }
    const data = accountsResp?.data;
    const payload = data?.data || {};
    const items = Array.isArray(payload.items) ? payload.items : [];
    const channels = Array.isArray(payload.supported_channels) ? payload.supported_channels : [];
    const bindingItems = Array.isArray(bindingsResp?.data?.data?.items)
      ? bindingsResp.data.data.items
      : [];

    supportedChannels.value = channels
      .map((item) => {
        const channel = String(item?.channel || '').trim().toLowerCase();
        if (!channel) {
          return null;
        }
        const displayName = String(item?.display_name || item?.displayName || '').trim();
        const description = String(item?.description || '').trim();
        const docsHint = String(item?.docs_hint || item?.docsHint || '').trim();
        return {
          channel,
          display_name: displayName || undefined,
          description: description || undefined,
          docs_hint: docsHint || undefined
        } as SupportedChannelItem;
      })
      .filter((item): item is SupportedChannelItem => Boolean(item));

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
    lastLoadedAgentKey = resolvedAgentId.value || '__default__';
  } catch (error) {
    if (requestId !== loadAccountsRequestId || disposed.value) {
      return;
    }
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
    if (requestId === loadAccountsRequestId) {
      loading.value = false;
    }
  }
};

const refreshAll = async () => {
  await loadAccounts();
  await refreshRuntimeLogs(true);
};

const startCreate = () => {
  creating.value = true;
  resetCreateForm();
  if (createForm.channel === 'weixin') {
    void startCreateWeixinQr();
  }
};

const cancelCreate = () => {
  creating.value = false;
  resetCreateForm();
};

const onCreateChannelChange = () => {
  applyCreateChannelDefaults();
  if (createForm.channel === 'weixin') {
    void startCreateWeixinQr();
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
    account_name: buildAutoAccountName(channel),
    enabled: true
  };
  if (resolvedAgentId.value) {
    payload.agent_id = resolvedAgentId.value;
  }

  const schema = schemaForChannel(channel);
  if (schema?.mode === 'weixin' && !createWeixinAdvancedEnabled.value) {
    if (!ensureWeixinCredentialsReady(createDynamicFields)) {
      return;
    }
  }
  const fieldError = validateChannelFields(schema, createDynamicFields);
  if (fieldError) {
    ElMessage.warning(fieldError);
    return;
  }
  if (channel === 'qqbot') {
    const qqCredentialError = validateQqbotCredentialFields(createDynamicFields);
    if (qqCredentialError) {
      ElMessage.warning(qqCredentialError);
      return;
    }
  }
  if (schema?.mode === 'weixin') {
    const weixinNumericError = validateWeixinNumericFields(createDynamicFields);
    if (weixinNumericError) {
      ElMessage.warning(weixinNumericError);
      return;
    }
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

  if (schema?.mode === 'weixin') {
    const apiBase = trimmedText(createDynamicFields.api_base);
    payload.weixin = {
      api_base: apiBase || undefined,
      cdn_base: trimmedText(createDynamicFields.cdn_base) || undefined,
      ilink_bot_id: trimmedText(createDynamicFields.ilink_bot_id),
      ilink_user_id: trimmedText(createDynamicFields.ilink_user_id) || undefined,
      bot_type: trimmedText(createDynamicFields.bot_type) || undefined,
      long_connection_enabled: Boolean(createDynamicFields.long_connection_enabled),
      allow_from: parseCommaSeparatedList(createDynamicFields.allow_from),
      poll_timeout_ms: parsePositiveInteger(createDynamicFields.poll_timeout_ms),
      api_timeout_ms: parsePositiveInteger(createDynamicFields.api_timeout_ms),
      max_consecutive_failures: parsePositiveInteger(createDynamicFields.max_consecutive_failures),
      backoff_ms: parsePositiveInteger(createDynamicFields.backoff_ms),
      route_tag: trimmedText(createDynamicFields.route_tag) || undefined,
      bot_token: trimmedText(createDynamicFields.bot_token)
    };
    payload.peer_kind = 'user';
    return submitCreate(payload);
  }

  const configPayload: Record<string, unknown> = {};
  if (schema?.mode === 'config') {
    mergeConfigObject(configPayload, buildStructuredConfigPatch(channel, createDynamicFields));
  }

  if (!Object.keys(configPayload).length) {
    ElMessage.warning(t('channels.config.jsonRequired'));
    return;
  }

  payload.config = configPayload;
  payload.peer_kind = USER_ONLY_CHANNELS.includes(channel) ? 'user' : 'group';
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
    createWeixinAutoCreating.value = false;
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
    enabled: account.active
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
  } else if (schema?.mode === 'weixin') {
    secretFallback.bot_token = account.weixinBotTokenSet;
  }
  if (schema?.mode === 'weixin' && !editWeixinAdvancedEnabled.value) {
    if (!ensureWeixinCredentialsReady(editDynamicFields, secretFallback)) {
      return;
    }
  }
  const fieldError = validateChannelFields(schema, editDynamicFields, secretFallback);
  if (fieldError) {
    ElMessage.warning(fieldError);
    return;
  }
  if (account.channel === 'qqbot') {
    const qqCredentialError = validateQqbotCredentialFields(editDynamicFields, secretFallback);
    if (qqCredentialError) {
      ElMessage.warning(qqCredentialError);
      return;
    }
  }
  if (schema?.mode === 'weixin') {
    const weixinNumericError = validateWeixinNumericFields(editDynamicFields);
    if (weixinNumericError) {
      ElMessage.warning(weixinNumericError);
      return;
    }
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

  if (schema?.mode === 'weixin') {
    const weixinPayload: NonNullable<ChannelAccountPayload['weixin']> = {
      api_base: trimmedText(editDynamicFields.api_base) || undefined,
      cdn_base: trimmedText(editDynamicFields.cdn_base) || undefined,
      ilink_bot_id: trimmedText(editDynamicFields.ilink_bot_id),
      ilink_user_id: trimmedText(editDynamicFields.ilink_user_id) || undefined,
      bot_type: trimmedText(editDynamicFields.bot_type) || undefined,
      long_connection_enabled: Boolean(editDynamicFields.long_connection_enabled),
      allow_from: parseCommaSeparatedList(editDynamicFields.allow_from),
      poll_timeout_ms: parsePositiveInteger(editDynamicFields.poll_timeout_ms),
      api_timeout_ms: parsePositiveInteger(editDynamicFields.api_timeout_ms),
      max_consecutive_failures: parsePositiveInteger(editDynamicFields.max_consecutive_failures),
      backoff_ms: parsePositiveInteger(editDynamicFields.backoff_ms),
      route_tag: trimmedText(editDynamicFields.route_tag) || undefined
    };
    const botToken = trimmedText(editDynamicFields.bot_token);
    if (botToken) {
      weixinPayload.bot_token = botToken;
    }
    payload.weixin = weixinPayload;
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

  if (!Object.keys(configPayload).length) {
    ElMessage.warning(t('channels.config.jsonRequired'));
    return;
  }

  payload.config = configPayload;
  payload.peer_kind = USER_ONLY_CHANNELS.includes(account.channel) ? 'user' : 'group';
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

watch(
  () => [resolvedAgentId.value, isPanelActive.value] as const,
  ([agentId, active], previous) => {
    if (!mounted.value || disposed.value) {
      return;
    }
    if (!active) {
      clearRuntimeLogTimer();
      return;
    }
    const agentKey = agentId || '__default__';
    const wasActive = previous?.[1] === true;
    if (!wasActive && lastLoadedAgentKey === agentKey) {
      void refreshRuntimeLogs(true);
      return;
    }
    permissionDenied.value = false;
    accounts.value = [];
    selectedKey.value = '';
    resetEditForm();
    void refreshAll();
  }
);

onMounted(() => {
  mounted.value = true;
  disposed.value = false;
  if (isPanelActive.value) {
    void refreshAll();
  }
});

onBeforeUnmount(() => {
  disposed.value = true;
  mounted.value = false;
  clearRuntimeLogTimer();
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

.channel-inline-options {
  grid-column: 1 / -1;
  display: inline-flex;
  align-items: center;
  gap: 14px;
  flex-wrap: wrap;
}

.channel-weixin-qr-panel {
  grid-column: 1 / -1;
  border: 1px dashed #d7d7d7;
  border-radius: 8px;
  background: #ffffff;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.channel-weixin-qr-preview {
  display: inline-flex;
  align-items: flex-start;
  gap: 10px;
  flex-wrap: wrap;
}

.channel-weixin-qr-image {
  width: 124px;
  height: 124px;
  border: 1px solid #e0e0e0;
  border-radius: 6px;
  object-fit: contain;
  background: #ffffff;
}

.channel-weixin-qr-link {
  font-size: 12px;
  color: var(--ui-accent);
  text-decoration: none;
  word-break: break-all;
}

.channel-weixin-qr-link:hover {
  text-decoration: underline;
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
  flex: 1;
  min-width: 0;
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.channel-account-status {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
  line-height: 1.2;
  white-space: nowrap;
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

.channel-runtime-log-card {
  border: 1px solid #e7e7e7;
  border-radius: 10px;
  background: #fafafa;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.channel-runtime-log-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.channel-runtime-log-status {
  margin-top: 4px;
  font-size: 11px;
  color: #6f6f6f;
}

.channel-runtime-log-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.channel-runtime-log-list {
  max-height: 220px;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-right: 2px;
}

.channel-runtime-log-item {
  border: 1px solid #e3e3e3;
  border-radius: 8px;
  background: #ffffff;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.channel-runtime-log-item--warn {
  border-color: #f0d8aa;
}

.channel-runtime-log-item--error {
  border-color: #efc0c0;
}

.channel-runtime-log-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 11px;
  color: #747474;
}

.channel-runtime-log-level {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 1px 7px;
  border-radius: 999px;
  border: 1px solid #d7d7d7;
  background: #f5f5f5;
  color: #5a5a5a;
}

.channel-runtime-log-item--warn .channel-runtime-log-level {
  border-color: #f0d8aa;
  background: #fff6e7;
  color: #9a6a1c;
}

.channel-runtime-log-item--error .channel-runtime-log-level {
  border-color: #efc0c0;
  background: #ffefef;
  color: #b43a3a;
}

.channel-runtime-log-repeat {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  padding: 1px 7px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.32);
  background: var(--ui-accent-soft);
  color: var(--ui-accent);
}

.channel-runtime-log-message {
  font-size: 12px;
  color: #2d2d2d;
  line-height: 1.4;
  white-space: pre-wrap;
  word-break: break-word;
}

.channel-runtime-log-error {
  font-size: 12px;
  color: #b43a3a;
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
