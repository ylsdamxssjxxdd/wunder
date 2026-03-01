<template>
  <div class="portal-shell">
    <UserTopbar :title="t('portal.title')" :subtitle="t('portal.subtitle')" :hide-chat="true" />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section portal-section--agents">
            <div class="agent-grid portal-agent-grid">
              <button class="agent-card agent-card--create" type="button" @click="openCreateDialog">
                <div class="agent-card-plus">+</div>
                <div class="agent-card-title">{{ t('portal.card.createTitle') }}</div>
                <div class="agent-card-desc">{{ t('portal.card.createDesc') }}</div>
              </button>
              <div
                class="agent-card agent-card--compact agent-card--default agent-card--clickable"
                role="button"
                tabindex="0"
                @mouseenter="queueAgentPrefetch(null)"
                @focus="queueAgentPrefetch(null)"
                @mouseleave="cancelAgentPrefetch(null)"
                @blur="cancelAgentPrefetch(null)"
                @mousedown="queueAgentPrefetch(null, true)"
                @touchstart.passive="queueAgentPrefetch(null, true)"
                @click="enterDefaultChat"
                @keydown.enter="enterDefaultChat"
              >
                <div class="agent-card-head">
                  <div
                    class="agent-card-avatar agent-card-avatar--robot"
                    :class="agentAvatarStatusClass(DEFAULT_AGENT_KEY)"
                    :title="agentStatusLabel(DEFAULT_AGENT_KEY)"
                    :aria-label="agentStatusLabel(DEFAULT_AGENT_KEY)"
                  >
                    <i class="fa-solid fa-robot agent-card-avatar-icon" aria-hidden="true"></i>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">{{ t('portal.card.defaultTitle') }}</div>
                    <div class="agent-card-desc">{{ t('portal.card.defaultDesc') }}</div>
                  </div>
                </div>
                <div class="agent-card-meta agent-card-meta--updated">
                  <span>{{ t('portal.card.defaultMeta') }}</span>
                  <span class="agent-card-meta-right">
                    <span
                      v-if="hasAgentCronTask(DEFAULT_AGENT_KEY)"
                      class="agent-card-cron-indicator"
                      :title="t('portal.agent.cronHint')"
                      :aria-label="t('portal.agent.cronHint')"
                    >
                      <i class="fa-solid fa-clock" aria-hidden="true"></i>
                    </span>
                    <span
                      v-for="item in resolveChannelIconsFor(DEFAULT_AGENT_KEY)"
                      :key="item.channel"
                      class="agent-card-channel-indicator"
                      :title="channelIndicatorTitle(item.label)"
                      :aria-label="channelIndicatorTitle(item.label)"
                    >
                      <i class="fa-solid" :class="item.icon" aria-hidden="true"></i>
                    </span>
                    <span class="agent-card-container-id">
                      {{ t('portal.agent.sandbox.option', { id: getAgentSandboxContainerId(null) }) }}
                    </span>
                  </span>
                </div>
              </div>
              <div v-if="showOwnedAgentLoading" class="agent-empty">{{ t('portal.section.loading') }}</div>
              <div
                v-else
                v-for="agent in filteredAgents"
                :key="agent.id"
                class="agent-card agent-card--compact agent-card--clickable"
                role="button"
                tabindex="0"
                @mouseenter="queueAgentPrefetch(agent)"
                @focus="queueAgentPrefetch(agent)"
                @mouseleave="cancelAgentPrefetch(agent)"
                @blur="cancelAgentPrefetch(agent)"
                @mousedown="queueAgentPrefetch(agent, true)"
                @touchstart.passive="queueAgentPrefetch(agent, true)"
                @click="enterAgent(agent)"
                @keydown.enter="enterAgent(agent)"
              >
                <div class="agent-card-head">
                  <div
                    class="agent-card-avatar agent-card-avatar--robot"
                    :class="agentAvatarStatusClass(agent.id)"
                    :title="agentStatusLabel(agent.id)"
                    :aria-label="agentStatusLabel(agent.id)"
                  >
                    <i class="fa-solid fa-robot agent-card-avatar-icon" aria-hidden="true"></i>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || t('portal.agent.noDesc') }}</div>
                  </div>
                </div>
                <div class="agent-card-meta agent-card-meta--updated">
                  <span>{{ formatTime(agent.updated_at) }}</span>
                  <span class="agent-card-meta-right">
                    <span
                      v-if="hasAgentCronTask(agent)"
                      class="agent-card-cron-indicator"
                      :title="t('portal.agent.cronHint')"
                      :aria-label="t('portal.agent.cronHint')"
                    >
                      <i class="fa-solid fa-clock" aria-hidden="true"></i>
                    </span>
                    <span
                      v-for="item in resolveChannelIconsFor(agent.id)"
                      :key="item.channel"
                      class="agent-card-channel-indicator"
                      :title="channelIndicatorTitle(item.label)"
                      :aria-label="channelIndicatorTitle(item.label)"
                    >
                      <i class="fa-solid" :class="item.icon" aria-hidden="true"></i>
                    </span>
                    <span class="agent-card-container-id">
                      {{ t('portal.agent.sandbox.option', { id: getAgentSandboxContainerId(agent) }) }}
                    </span>
                  </span>
                </div>
              </div>
            </div>
          </section>
          <section v-if="showSharedAgents" class="portal-section portal-section--shared">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">{{ t('portal.section.sharedAgents') }}</div>
              </div>
            </div>
            <div class="agent-grid portal-agent-grid">
              <div v-if="showSharedAgentLoading" class="agent-empty">{{ t('portal.section.loading') }}</div>
              <div
                v-else
                v-for="agent in filteredSharedAgents"
                :key="agent.id"
                class="agent-card agent-card--compact agent-card--clickable"
                role="button"
                tabindex="0"
                @mouseenter="queueAgentPrefetch(agent)"
                @focus="queueAgentPrefetch(agent)"
                @mouseleave="cancelAgentPrefetch(agent)"
                @blur="cancelAgentPrefetch(agent)"
                @mousedown="queueAgentPrefetch(agent, true)"
                @touchstart.passive="queueAgentPrefetch(agent, true)"
                @click="enterAgent(agent)"
                @keydown.enter="enterAgent(agent)"
              >
                <div class="agent-card-head">
                  <div
                    class="agent-card-avatar agent-card-avatar--robot"
                    :class="agentAvatarStatusClass(agent.id)"
                    :title="agentStatusLabel(agent.id)"
                    :aria-label="agentStatusLabel(agent.id)"
                  >
                    <i class="fa-solid fa-robot agent-card-avatar-icon" aria-hidden="true"></i>
                  </div>
                  <div class="agent-card-head-text">
                    <div class="agent-card-title">{{ agent.name }}</div>
                    <div class="agent-card-desc">{{ agent.description || t('portal.agent.noDesc') }}</div>
                  </div>
                </div>
                <div class="agent-card-meta agent-card-meta--updated">
                  <span>{{ formatTime(agent.updated_at) }}</span>
                  <span class="agent-card-meta-right">
                    <span
                      v-if="hasAgentCronTask(agent)"
                      class="agent-card-cron-indicator"
                      :title="t('portal.agent.cronHint')"
                      :aria-label="t('portal.agent.cronHint')"
                    >
                      <i class="fa-solid fa-clock" aria-hidden="true"></i>
                    </span>
                    <span
                      v-for="item in resolveChannelIconsFor(agent.id)"
                      :key="item.channel"
                      class="agent-card-channel-indicator"
                      :title="channelIndicatorTitle(item.label)"
                      :aria-label="channelIndicatorTitle(item.label)"
                    >
                      <i class="fa-solid" :class="item.icon" aria-hidden="true"></i>
                    </span>
                    <span class="agent-card-container-id">
                      {{ t('portal.agent.sandbox.option', { id: getAgentSandboxContainerId(agent) }) }}
                    </span>
                  </span>
                </div>
              </div>
            </div>
          </section>
          <section v-if="showMoreApps" class="portal-section portal-section--apps">
            <div class="portal-section-header portal-section-header--apps">
              <div class="portal-section-title">{{ t('portal.section.moreApps') }}</div>
            </div>
            <div class="agent-grid portal-agent-grid">
              <div v-if="externalLoading" class="agent-empty">{{ t('portal.section.loading') }}</div>
              <div v-else-if="!filteredExternalLinks.length" class="agent-empty">
                {{ normalizedQuery ? t('portal.external.searchEmpty') : t('portal.external.empty') }}
              </div>
              <template v-else>
                <div
                  v-for="link in filteredExternalLinks"
                  :key="link.link_id"
                  class="agent-card agent-card--compact agent-card--clickable"
                  role="button"
                  tabindex="0"
                  @click="openExternalApp(link)"
                  @keydown.enter="openExternalApp(link)"
                >
                  <div class="agent-card-head">
                    <div class="agent-card-avatar agent-card-avatar--idle agent-card-avatar--external" aria-hidden="true">
                      <i
                        class="agent-card-default-icon-svg"
                        :class="['fa-solid', resolveExternalIcon(link.icon)]"
                        :style="resolveExternalIconStyle(link.icon)"
                        aria-hidden="true"
                      ></i>
                    </div>
                    <div class="agent-card-head-text">
                      <div class="agent-card-title">{{ link.title }}</div>
                      <div class="agent-card-desc">{{ link.description || t('portal.agent.noDesc') }}</div>
                    </div>
                  </div>
                  <div class="agent-card-meta">
                    <span>{{ getExternalHost(link.url) }}</span>
                  </div>
                </div>
              </template>
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
        <div class="user-tools-header agent-editor-header">
          <div class="user-tools-title">{{ dialogTitle }}</div>
          <div class="agent-editor-header-actions">
            <div v-if="!editingId" class="agent-editor-copy">
              <span class="agent-editor-copy-label">{{ t('portal.agent.form.copyFrom') }}</span>
              <el-select
                v-model="form.copy_from_agent_id"
                class="agent-copy-select"
                clearable
                filterable
                size="small"
                :placeholder="t('portal.agent.form.copyFromPlaceholder')"
              >
                <el-option :label="t('portal.agent.form.copyFromNone')" value="" />
                <el-option
                  v-for="agent in agentCopyOptions"
                  :key="`copy-agent-${agent.id}`"
                  :label="formatAgentCopyLabel(agent)"
                  :value="agent.id"
                />
              </el-select>
            </div>
            <button class="icon-btn" type="button" @click="dialogVisible = false">&times;</button>
          </div>
        </div>
      </template>
      <div class="agent-editor-body">
        <el-form :model="form" label-position="top" class="agent-editor-form">
          <el-form-item class="agent-form-item agent-form-item--name" :label="t('portal.agent.form.name')">
            <el-input v-model="form.name" :placeholder="t('portal.agent.form.placeholder.name')" />
          </el-form-item>
          <el-form-item
            class="agent-form-item agent-form-item--description"
            :label="t('portal.agent.form.description')"
          >
            <el-input v-model="form.description" :placeholder="t('portal.agent.form.placeholder.description')" />
          </el-form-item>
          <el-form-item class="agent-form-item agent-form-item--prompt" :label="t('portal.agent.form.prompt')">
            <el-input
              v-model="form.system_prompt"
              type="textarea"
              :rows="8"
              :placeholder="t('portal.agent.form.placeholder.prompt')"
            />
          </el-form-item>
          <el-form-item class="agent-form-item agent-form-item--tools" :label="t('portal.agent.form.tools')">
            <div class="agent-tool-picker">
              <div v-if="toolLoading" class="agent-tool-loading">{{ t('portal.agent.tools.loading') }}</div>
              <el-checkbox-group v-else v-model="form.tool_names" class="agent-tool-groups">
                <div v-for="group in toolGroups" :key="group.label" class="agent-tool-group">
                  <div class="agent-tool-group-header">
                    <div class="agent-tool-group-title">{{ group.label }}</div>
                    <button
                      class="agent-tool-group-select"
                      type="button"
                      @click.stop="selectToolGroup(group)"
                    >
                      {{ isToolGroupFullySelected(group) ? t('portal.agent.tools.unselectAll') : t('portal.agent.tools.selectAll') }}
                    </button>
                  </div>
                  <div class="agent-tool-options">
                    <el-checkbox
                      v-for="option in group.options"
                      :key="option.value"
                      :value="option.value"
                    >
                      <span :title="option.description || option.label">{{ option.label }}</span>
                    </el-checkbox>
                  </div>
                </div>
              </el-checkbox-group>
              <div v-if="sharedToolsNotice" class="agent-editor-hint">
                {{ t('portal.agent.tools.notice') }}
              </div>
            </div>
          </el-form-item>
          <el-form-item class="agent-form-item agent-form-item--base" :label="t('portal.agent.form.base')">
            <div class="agent-basic-settings">
              <div class="agent-share-card agent-share-card--combined">
                <div class="agent-share-title">{{ t('portal.agent.share.title') }}</div>
                <div class="agent-share-row">
                  <el-switch v-model="form.is_shared" />
                  <span>{{ t('portal.agent.share.label') }}</span>
                </div>
                <div class="agent-share-row agent-share-row--sandbox">
                  <span>{{ t('portal.agent.sandbox.title') }}</span>
                  <el-select v-model="form.sandbox_container_id" size="small" class="agent-sandbox-select">
                    <el-option
                      v-for="id in sandboxContainerOptions"
                      :key="id"
                      :label="t('portal.agent.sandbox.option', { id })"
                      :value="id"
                    />
                  </el-select>
                </div>
                <div class="agent-share-row agent-share-row--sandbox">
                  <span>{{ t('portal.agent.permission.title') }}</span>
                  <el-select v-model="form.approval_mode" size="small" class="agent-sandbox-select">
                    <el-option
                      v-for="item in approvalModeOptions"
                      :key="item.value"
                      :label="item.label"
                      :value="item.value"
                    />
                  </el-select>
                </div>
                <div class="agent-editor-hint">{{ t('portal.agent.sandbox.hint') }}</div>
                <div class="agent-editor-hint">{{ t('portal.agent.permission.hint') }}</div>
              </div>
            </div>
          </el-form-item>
        </el-form>
      </div>
      <template #footer>
        <el-button @click="dialogVisible = false">{{ t('portal.agent.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveAgent">
          {{ t('portal.agent.save') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { listAgents as listAgentsApi, listRunningAgents } from '@/api/agents';
import { fetchExternalLinks } from '@/api/externalLinks';
import { fetchCronJobs } from '@/api/cron';
import { listChannelAccounts, listChannelBindings } from '@/api/channels';
import { fetchUserToolsCatalog } from '@/api/userTools';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { showApiError } from '@/utils/apiError';
import { resolveUserBasePath } from '@/utils/basePath';
import { prefetchWorkspaceTree } from '@/utils/workspaceTreeCache';

const router = useRouter();
const route = useRoute();
const authStore = useAuthStore();
const agentStore = useAgentStore();
const chatStore = useChatStore();
const { t } = useI18n();
const RUNNING_REFRESH_MS = 6000;
const DEFAULT_AGENT_KEY = '__default__';
const PORTAL_PREFETCH_HOVER_DELAY_MS = 120;
const PORTAL_PREFETCH_COOLDOWN_MS = 12 * 1000;
const DONE_ACK_CACHE_KEY = 'wunder.portal.done_ack';
const TRANSIENT_STATE_CACHE_KEY = 'wunder.portal.transient_state';
const AGENT_LIST_CACHE_KEY = 'wunder.portal.agent_list';
const RUNNING_STATE_CACHE_KEY = 'wunder.portal.running_state';
const AGENT_LIST_CACHE_TTL_MS = 10 * 60 * 1000;
const RUNNING_STATE_CACHE_TTL_MS = 5 * 60 * 1000;
const searchQuery = ref('');
const showSharedAgents = ref(false);
const showMoreApps = ref(false);
const dialogVisible = ref(false);
const saving = ref(false);
const editingId = ref('');
const toolCatalog = ref(null);
const toolLoading = ref(false);
const runningAgentIds = ref<string[]>([]);
const waitingAgentIds = ref<string[]>([]);
const externalLinks = ref<any[]>([]);
const externalLoading = ref(false);
const cronAgentIds = ref<Set<string>>(new Set());
type TransientAgentStateEntry = { state: 'done' | 'error'; until: number | null; signature: string };

const transientAgentStates = ref<Record<string, TransientAgentStateEntry>>(readTransientStateCache());
const acknowledgedDoneStateSignatures = ref<Record<string, string>>(readDoneAckCache());
const configuredChannelsByAgent = ref<Record<string, string[]>>({});
const agentCopyOptions = ref<Array<{ id: string; name: string }>>([]);
let runningTimer = null;
const agentPrefetchTimers = new Map<string, number>();
const agentPrefetchInFlight = new Map<string, Promise<void>>();
const agentPrefetchLastAt = new Map<string, number>();

type AgentListCachePayload = {
  cachedAt: number;
  owned: unknown[];
  shared: unknown[];
};

type RunningStateCachePayload = {
  cachedAt: number;
  running: string[];
  waiting: string[];
};

function readDoneAckCache(): Record<string, string> {
  if (typeof window === 'undefined') return {};
  try {
    const raw = window.sessionStorage.getItem(DONE_ACK_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return {};
    const output: Record<string, string> = {};
    Object.entries(parsed).forEach(([agentId, signature]) => {
      const key = String(agentId || '').trim();
      const value = String(signature || '').trim();
      if (!key || !value) return;
      output[key] = value;
    });
    return output;
  } catch (error) {
    return {};
  }
}

function writeDoneAckCache(value: Record<string, string>) {
  if (typeof window === 'undefined') return;
  try {
    window.sessionStorage.setItem(DONE_ACK_CACHE_KEY, JSON.stringify(value));
  } catch (error) {
    // Ignore quota/private-mode errors and keep runtime state only.
  }
}

function readTransientStateCache(): Record<string, TransientAgentStateEntry> {
  if (typeof window === 'undefined') return {};
  const now = Date.now();
  try {
    const raw = window.sessionStorage.getItem(TRANSIENT_STATE_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return {};
    const output: Record<string, TransientAgentStateEntry> = {};
    Object.entries(parsed as Record<string, { state?: string; until?: number; signature?: string }>).forEach(
      ([agentId, entry]) => {
        const key = String(agentId || '').trim();
        if (!key || !entry) return;
        const state = String(entry.state || '').trim().toLowerCase();
        const signature = String(entry.signature || '').trim();
        if (!signature) return;
        if (state === 'done') {
          output[key] = { state: 'done', until: null, signature };
          return;
        }
        if (state !== 'error') return;
        const until = Number(entry.until);
        if (!Number.isFinite(until) || until <= now) return;
        output[key] = { state: 'error', until, signature };
      }
    );
    return output;
  } catch (error) {
    return {};
  }
}

function writeTransientStateCache(value: Record<string, TransientAgentStateEntry>) {
  if (typeof window === 'undefined') return;
  try {
    window.sessionStorage.setItem(TRANSIENT_STATE_CACHE_KEY, JSON.stringify(value));
  } catch (error) {
    // Ignore quota/private-mode errors and keep runtime state only.
  }
}

function readAgentListCache(): AgentListCachePayload | null {
  if (typeof window === 'undefined') return null;
  const now = Date.now();
  try {
    const raw = window.sessionStorage.getItem(AGENT_LIST_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<AgentListCachePayload>;
    const cachedAt = Number(parsed?.cachedAt);
    if (!Number.isFinite(cachedAt) || now - cachedAt > AGENT_LIST_CACHE_TTL_MS) return null;
    const owned = Array.isArray(parsed?.owned) ? parsed.owned : [];
    const shared = Array.isArray(parsed?.shared) ? parsed.shared : [];
    return { cachedAt, owned, shared };
  } catch (error) {
    return null;
  }
}

function writeAgentListCache(owned: unknown[], shared: unknown[]) {
  if (typeof window === 'undefined') return;
  try {
    const payload: AgentListCachePayload = {
      cachedAt: Date.now(),
      owned: Array.isArray(owned) ? owned : [],
      shared: Array.isArray(shared) ? shared : []
    };
    window.sessionStorage.setItem(AGENT_LIST_CACHE_KEY, JSON.stringify(payload));
  } catch (error) {
    // Ignore quota/private-mode errors and keep runtime state only.
  }
}

function readRunningStateCache(): RunningStateCachePayload | null {
  if (typeof window === 'undefined') return null;
  const now = Date.now();
  try {
    const raw = window.sessionStorage.getItem(RUNNING_STATE_CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<RunningStateCachePayload>;
    const cachedAt = Number(parsed?.cachedAt);
    if (!Number.isFinite(cachedAt) || now - cachedAt > RUNNING_STATE_CACHE_TTL_MS) return null;
    const running = Array.isArray(parsed?.running)
      ? parsed.running.map((item) => String(item || '').trim()).filter(Boolean)
      : [];
    const waiting = Array.isArray(parsed?.waiting)
      ? parsed.waiting.map((item) => String(item || '').trim()).filter(Boolean)
      : [];
    return { cachedAt, running, waiting };
  } catch (error) {
    return null;
  }
}

function writeRunningStateCache(running: string[], waiting: string[]) {
  if (typeof window === 'undefined') return;
  try {
    const payload: RunningStateCachePayload = {
      cachedAt: Date.now(),
      running: Array.from(
        new Set(
          (Array.isArray(running) ? running : []).map((item) => String(item || '').trim()).filter(Boolean)
        )
      ),
      waiting: Array.from(
        new Set(
          (Array.isArray(waiting) ? waiting : []).map((item) => String(item || '').trim()).filter(Boolean)
        )
      )
    };
    window.sessionStorage.setItem(RUNNING_STATE_CACHE_KEY, JSON.stringify(payload));
  } catch (error) {
    // Ignore quota/private-mode errors and keep runtime state only.
  }
}

const hydrateAgentListFromCache = () => {
  if (agentStore.agents?.length || agentStore.sharedAgents?.length) return;
  const cached = readAgentListCache();
  if (!cached) return;
  const owned = Array.isArray(cached.owned) ? cached.owned : [];
  const shared = Array.isArray(cached.shared) ? cached.shared : [];
  agentStore.agents = owned;
  agentStore.sharedAgents = shared;
  agentStore.hydrateMap(owned, shared);
};

const hydrateRunningStateFromCache = () => {
  const cached = readRunningStateCache();
  if (!cached) return;
  runningAgentIds.value = cached.running;
  waitingAgentIds.value = cached.waiting;
};

hydrateAgentListFromCache();
hydrateRunningStateFromCache();

type AgentCardState = 'idle' | 'waiting' | 'running' | 'done' | 'error';

const TRANSIENT_ERROR_TTL_MS = 30000;

const CHANNEL_ICON_META = {
  feishu: { icon: 'fa-feather-pointed', labelKey: 'channels.provider.feishu' },
  whatsapp: { icon: 'fa-whatsapp', labelKey: 'channels.provider.whatsapp' },
  telegram: { icon: 'fa-telegram', labelKey: 'channels.provider.telegram' },
  wechat: { icon: 'fa-weixin', labelKey: 'channels.provider.wechat' },
  qqbot: { icon: 'fa-qq', labelKey: 'channels.provider.qqbot' }
};

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));
const approvalModeOptions = computed(() => [
  { value: 'suggest', label: t('portal.agent.permission.option.suggest') },
  { value: 'auto_edit', label: t('portal.agent.permission.option.auto_edit') },
  { value: 'full_auto', label: t('portal.agent.permission.option.full_auto') }
]);

const normalizeSandboxContainerId = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const normalizeApprovalMode = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return 'auto_edit';
};

const sortAgentsByContainerId = (list) =>
  (Array.isArray(list) ? list : [])
    .map((agent, index) => ({
      agent,
      index,
      sandboxContainerId: normalizeSandboxContainerId(agent?.sandbox_container_id)
    }))
    .sort((left, right) => {
      if (left.sandboxContainerId !== right.sandboxContainerId) {
        return left.sandboxContainerId - right.sandboxContainerId;
      }
      return left.index - right.index;
    })
    .map((item) => item.agent);

const getAgentSandboxContainerId = (agent) => normalizeSandboxContainerId(agent?.sandbox_container_id);

const hasAgentCronTask = (agent) => {
  const agentId =
    typeof agent === 'string'
      ? String(agent).trim()
      : String(agent?.id || '').trim();
  if (!agentId) return false;
  return cronAgentIds.value.has(agentId);
};

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  copy_from_agent_id: '',
  tool_names: [],
  system_prompt: '',
  sandbox_container_id: 1,
  approval_mode: 'auto_edit'
});

const basePath = computed(() => resolveUserBasePath(route.path));
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

const formatAgentCopyLabel = (agent) => {
  return String(agent?.name || '').trim() || String(agent?.id || '').trim();
};

const loadAgents = async () => {
  const payload = await agentStore.loadAgents();
  writeAgentListCache(payload?.owned || [], payload?.shared || []);
};

const loadAgentCopyOptions = async () => {
  try {
    const { data } = await listAgentsApi();
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    agentCopyOptions.value = items
      .map((item) => ({
        id: String(item?.id || '').trim(),
        name: String(item?.name || '').trim(),
      }))
      .filter((item) => item.id && item.name);
  } catch (error) {
    agentCopyOptions.value = [];
  }
};

const initPortal = async () => {
  try {
    if (!authStore.user) {
      await authStore.loadProfile();
    }
    const results = await Promise.allSettled([
      loadAgents(),
      loadExternalApps(),
      loadCronAgentIds(),
      loadRunningAgents(),
      loadConfiguredChannels()
    ]);
    const loadAgentsResult = results[0];
    const hasAnyAgents =
      (agentStore.agents?.length || 0) > 0 || (agentStore.sharedAgents?.length || 0) > 0;
    if (!hasAnyAgents && loadAgentsResult?.status === 'rejected') {
      showApiError(loadAgentsResult.reason, t('common.requestFailed'));
    }
  } catch (error) {
    showApiError(error, t('common.requestFailed'));
  }
};

onMounted(() => {
  void initPortal();
  runningTimer = window.setInterval(loadRunningAgents, RUNNING_REFRESH_MS);
});

onBeforeUnmount(() => {
  if (runningTimer) {
    clearInterval(runningTimer);
    runningTimer = null;
  }
  clearAgentPrefetchTimers();
  agentPrefetchInFlight.clear();
});

const agents = computed(() => agentStore.agents || []);
const sharedAgents = computed(() => agentStore.sharedAgents || []);
const agentLoading = computed(() => agentStore.loading);
const showOwnedAgentLoading = computed(() => agentLoading.value && agents.value.length === 0);
const showSharedAgentLoading = computed(() => agentLoading.value && sharedAgents.value.length === 0);
const filteredAgents = computed(() => {
  const query = normalizedQuery.value;
  const matched = query ? agents.value.filter((agent) => matchesQuery(agent, query)) : agents.value;
  return sortAgentsByContainerId(matched);
});
const filteredSharedAgents = computed(() => {
  const query = normalizedQuery.value;
  const matched = query ? sharedAgents.value.filter((agent) => matchesQuery(agent, query)) : sharedAgents.value;
  return sortAgentsByContainerId(matched);
});
const filteredExternalLinks = computed(() => {
  const query = normalizedQuery.value;
  if (!query) return externalLinks.value;
  return externalLinks.value.filter((link) => {
    const source = [link?.title, link?.description, link?.url]
      .filter(Boolean)
      .join(' ')
      .toLowerCase();
    return source.includes(query);
  });
});

const buildChannelIcons = (channels) => {
  const dedup = new Set();
  const icons = [];
  (Array.isArray(channels) ? channels : []).forEach((channel) => {
    const name = String(channel || '').trim().toLowerCase();
    if (!name || dedup.has(name)) return;
    dedup.add(name);
    const meta = CHANNEL_ICON_META[name] || { icon: 'fa-link', labelKey: '' };
    icons.push({
      channel: name,
      icon: meta.icon,
      label: meta.labelKey ? t(meta.labelKey) : name
    });
  });
  return icons;
};

const configuredChannelIconsByAgent = computed(() => {
  const output = {};
  const entries = configuredChannelsByAgent.value || {};
  Object.entries(entries).forEach(([agentId, channels]) => {
    output[agentId] = buildChannelIcons(channels);
  });
  return output;
});

const resolveChannelIconsFor = (agentId) => {
  const key = String(agentId || '').trim() || DEFAULT_AGENT_KEY;
  return configuredChannelIconsByAgent.value[key] || [];
};

const channelIndicatorTitle = (label) => t('portal.channel.configured', { channel: label });

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

const normalizeAgentKey = (agentId) => {
  const key = String(agentId || '').trim();
  return key || DEFAULT_AGENT_KEY;
};

const clearAgentPrefetchTimer = (agentKey) => {
  const timer = agentPrefetchTimers.get(agentKey);
  if (timer) {
    clearTimeout(timer);
    agentPrefetchTimers.delete(agentKey);
  }
};

const clearAgentPrefetchTimers = () => {
  agentPrefetchTimers.forEach((timer) => {
    clearTimeout(timer);
  });
  agentPrefetchTimers.clear();
};

const resolveAgentPrefetchMeta = (agent) => {
  const agentId = String(agent?.id || '').trim();
  return {
    agentId,
    agentKey: normalizeAgentKey(agentId)
  };
};

const runAgentPrefetch = (agentId, agentKey) => {
  const now = Date.now();
  const lastAt = agentPrefetchLastAt.get(agentKey) || 0;
  if (now - lastAt < PORTAL_PREFETCH_COOLDOWN_MS) {
    return agentPrefetchInFlight.get(agentKey) || Promise.resolve();
  }

  const existing = agentPrefetchInFlight.get(agentKey);
  if (existing) {
    return existing;
  }

  agentPrefetchLastAt.set(agentKey, now);
  const request = Promise.allSettled([
    chatStore.prefetchAgentSessions(agentId).then((sessions) => {
      const targetSessionId = chatStore.resolveInitialSessionId(agentId, sessions);
      if (!targetSessionId) return null;
      return chatStore.preloadSessionDetail(targetSessionId).catch(() => null);
    }),
    prefetchWorkspaceTree({
      agentId,
      path: '',
      sortBy: 'name',
      sortOrder: 'asc'
    })
  ])
    .then(() => undefined)
    .finally(() => {
      agentPrefetchInFlight.delete(agentKey);
    });

  agentPrefetchInFlight.set(agentKey, request);
  return request;
};

const queueAgentPrefetch = (agent, immediate = false) => {
  const { agentId, agentKey } = resolveAgentPrefetchMeta(agent);
  clearAgentPrefetchTimer(agentKey);
  if (immediate) {
    void runAgentPrefetch(agentId, agentKey);
    return;
  }
  const timer = window.setTimeout(() => {
    agentPrefetchTimers.delete(agentKey);
    void runAgentPrefetch(agentId, agentKey);
  }, PORTAL_PREFETCH_HOVER_DELAY_MS);
  agentPrefetchTimers.set(agentKey, timer);
};

const cancelAgentPrefetch = (agent) => {
  const { agentKey } = resolveAgentPrefetchMeta(agent);
  clearAgentPrefetchTimer(agentKey);
};

const cleanupTransientAgentStates = (now) => {
  const current = transientAgentStates.value || {};
  const next: Record<string, TransientAgentStateEntry> = {};
  Object.entries(current).forEach(([agentId, entry]) => {
    if (!entry) return;
    if (typeof entry.signature !== 'string' || !entry.signature.trim()) return;
    if (entry.state === 'done') {
      next[agentId] = { state: 'done', until: null, signature: entry.signature };
      return;
    }
    if (entry.state !== 'error') return;
    if (typeof entry.until !== 'number' || entry.until <= now) return;
    next[agentId] = { state: 'error', until: entry.until, signature: entry.signature };
  });
  transientAgentStates.value = next;
  writeTransientStateCache(next);
};

const resolveAgentCardState = (agentId): AgentCardState => {
  const key = normalizeAgentKey(agentId);
  if (isAgentWaiting(key)) return 'waiting';
  if (isAgentRunning(key)) return 'running';
  const transient = transientAgentStates.value[key];
  if (transient) {
    const acknowledgedSignature = acknowledgedDoneStateSignatures.value[key];
    if (transient.state === 'done' && acknowledgedSignature === transient.signature) {
      return 'idle';
    }
    if (transient.state === 'done') return 'done';
    if (typeof transient.until === 'number' && transient.until > Date.now()) return 'error';
  }
  return 'idle';
};

const agentAvatarStatusClass = (agentId) => `agent-card-avatar--${resolveAgentCardState(agentId)}`;

const agentStatusLabel = (agentId) => {
  const state = resolveAgentCardState(agentId);
  if (state === 'waiting') return t('portal.card.waiting');
  if (state === 'running') return t('portal.card.running');
  if (state === 'done') return t('portal.card.done');
  if (state === 'error') return t('portal.card.error');
  return t('portal.card.idle');
};

const acknowledgeAgentDoneState = (agentId) => {
  const key = normalizeAgentKey(agentId);
  const transient = transientAgentStates.value[key];
  if (!transient || transient.state !== 'done') return;
  const nextAcknowledgedDone = {
    ...acknowledgedDoneStateSignatures.value,
    [key]: transient.signature
  };
  const nextTransient = {
    ...transientAgentStates.value
  };
  delete nextTransient[key];
  transientAgentStates.value = nextTransient;
  acknowledgedDoneStateSignatures.value = nextAcknowledgedDone;
  writeDoneAckCache(nextAcknowledgedDone);
  writeTransientStateCache(nextTransient);
};

const dialogTitle = computed(() =>
  editingId.value ? t('portal.agent.editTitle') : t('portal.agent.createTitle')
);

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
    { label: t('portal.agent.tools.group.builtin'), options: normalizeOptions(payload.builtin_tools) },
    { label: t('portal.agent.tools.group.mcp'), options: normalizeOptions(payload.mcp_tools) },
    { label: t('portal.agent.tools.group.a2a'), options: normalizeOptions(payload.a2a_tools) },
    { label: t('portal.agent.tools.group.skills'), options: normalizeOptions(payload.skills) },
    { label: t('portal.agent.tools.group.knowledge'), options: normalizeOptions(payload.knowledge_tools) },
    { label: t('portal.agent.tools.group.user'), options: normalizeOptions(payload.user_tools) },
    { label: t('portal.agent.tools.group.shared'), options: normalizeOptions(sharedTools) }
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
  form.copy_from_agent_id = '';
  form.system_prompt = '';
  form.sandbox_container_id = 1;
  form.approval_mode = 'auto_edit';
  applyDefaultTools();
  editingId.value = '';
};

const normalizeHexColor = (value) => {
  const cleaned = String(value || '').trim();
  if (!cleaned) return '';
  const matched = cleaned.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!matched) return '';
  let hex = matched[1].toLowerCase();
  if (hex.length === 3) {
    hex = hex
      .split('')
      .map((part) => part + part)
      .join('');
  }
  return '#' + hex;
};

const resolveExternalIconConfig = (icon) => {
  const raw = String(icon || '').trim();
  if (!raw) {
    return { name: 'fa-globe', color: '' };
  }
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object') {
      const name = String(parsed?.name || '').trim();
      const match = name.split(/\s+/).find((part) => part.startsWith('fa-'));
      return {
        name: match || 'fa-globe',
        color: normalizeHexColor(parsed?.color)
      };
    }
  } catch (error) {
    // Fallback to plain icon name.
  }
  const match = raw.split(/\s+/).find((part) => part.startsWith('fa-'));
  return {
    name: match || 'fa-globe',
    color: ''
  };
};

const normalizeExternalLink = (item) => ({
  link_id: String(item?.link_id || '').trim(),
  title: String(item?.title || '').trim(),
  description: String(item?.description || '').trim(),
  url: String(item?.url || '').trim(),
  icon: String(item?.icon || '').trim(),
  sort_order: Number.isFinite(Number(item?.sort_order)) ? Number(item.sort_order) : 0
});

const resolveExternalIcon = (icon) => resolveExternalIconConfig(icon).name;

const resolveExternalIconStyle = (icon) => {
  const color = resolveExternalIconConfig(icon).color;
  return color ? { color } : {};
};

const getExternalHost = (url) => {
  const value = String(url || '').trim();
  if (!value) return '-';
  try {
    const parsed = new URL(value);
    return parsed.host || value;
  } catch (error) {
    return value;
  }
};

const loadCronAgentIds = async () => {
  try {
    const { data } = await fetchCronJobs();
    const jobs = Array.isArray(data?.data?.jobs) ? data.data.jobs : [];
    const ids = new Set<string>();
    jobs.forEach((job) => {
      const rawAgentId = String(job?.agent_id || '').trim();
      const sessionTarget = String(job?.session_target || '').trim().toLowerCase();
      const isDefaultJob =
        rawAgentId === '' ||
        sessionTarget === 'default' ||
        sessionTarget === 'system' ||
        sessionTarget === '__default__' ||
        job?.is_default === true;
      const agentId = isDefaultJob ? DEFAULT_AGENT_KEY : rawAgentId;
      if (agentId) {
        ids.add(agentId);
      }
    });
    cronAgentIds.value = ids;
  } catch (error) {
    cronAgentIds.value = new Set<string>();
  }
};

const loadExternalApps = async () => {
  externalLoading.value = true;
  try {
    const { data } = await fetchExternalLinks();
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    externalLinks.value = items
      .map(normalizeExternalLink)
      .filter((item) => item.link_id && item.title && item.url)
      .sort((left, right) => left.sort_order - right.sort_order);
  } catch (error) {
    externalLinks.value = [];
  } finally {
    externalLoading.value = false;
  }
};

const loadConfiguredChannels = async () => {
  try {
    const [accountsResp, bindingsResp] = await Promise.all([
      listChannelAccounts(),
      listChannelBindings()
    ]);
    const accountItems = Array.isArray(accountsResp?.data?.data?.items)
      ? accountsResp.data.data.items
      : [];
    const bindingItems = Array.isArray(bindingsResp?.data?.data?.items)
      ? bindingsResp.data.data.items
      : [];

    const accountStatus = new Map();
    accountItems.forEach((item) => {
      const channel = String(item?.channel || '').trim().toLowerCase();
      const accountId = String(item?.account_id || '').trim();
      if (!channel || !accountId) return;
      const configured = item?.meta?.configured === true;
      const active =
        item?.active === true ||
        String(item?.status || '').trim().toLowerCase() === 'active';
      accountStatus.set(`${channel}::${accountId}`, { channel, configured, active });
    });

    const channelMap = new Map();
    const boundKeys = new Set();

    bindingItems.forEach((binding) => {
      if (binding?.enabled !== true) return;
      const channel = String(binding?.channel || '').trim().toLowerCase();
      const accountId = String(binding?.account_id || '').trim();
      if (!channel || !accountId) return;
      const key = `${channel}::${accountId}`;
      const status = accountStatus.get(key);
      if (!status || !status.configured || !status.active) return;
      boundKeys.add(key);
      const agentId = String(binding?.agent_id || '').trim() || DEFAULT_AGENT_KEY;
      const set = channelMap.get(agentId) || new Set();
      set.add(channel);
      channelMap.set(agentId, set);
    });

    accountStatus.forEach((status, key) => {
      if (!status.configured || !status.active) return;
      if (boundKeys.has(key)) return;
      const set = channelMap.get(DEFAULT_AGENT_KEY) || new Set();
      set.add(status.channel);
      channelMap.set(DEFAULT_AGENT_KEY, set);
    });

    const output = {};
    channelMap.forEach((set, agentId) => {
      output[agentId] = Array.from(set);
    });
    configuredChannelsByAgent.value = output;
  } catch (error) {
    configuredChannelsByAgent.value = {};
  }
};

const loadCatalog = async () => {
  toolLoading.value = true;
  try {
    const { data } = await fetchUserToolsCatalog();
    toolCatalog.value = data?.data || null;
  } catch (error) {
    showApiError(error, t('portal.agent.tools.loadFailed'));
  } finally {
    toolLoading.value = false;
  }
};

const loadRunningAgents = async () => {
  try {
    const prevRunning = new Set(runningAgentIds.value);
    const prevWaiting = new Set(waitingAgentIds.value);
    const { data } = await listRunningAgents();
    const items = data?.data?.items || [];
    const running = new Set<string>();
    const waiting = new Set<string>();
    const done = new Set<string>();
    const failed = new Set<string>();
    const doneSignatures = new Map<string, string>();
    const failedSignatures = new Map<string, string>();
    items.forEach((item) => {
      const rawAgentId = String(item?.agent_id || '').trim();
      const agentId = rawAgentId || (item?.is_default ? DEFAULT_AGENT_KEY : '');
      if (!agentId) return;
      const state = String(item?.state || '').trim().toLowerCase();
      const signature = [
        state || 'unknown',
        String(item?.session_id || '').trim(),
        String(item?.updated_at || '').trim(),
        String(item?.expires_at || '').trim()
      ].join('|');
      const pending = item?.pending_question === true || state === 'waiting';
      const isError = state === 'error' || state === 'failed';
      const isDone = state === 'finished' || state === 'done' || state === 'completed';
      const isRunning =
        state === 'running' ||
        state === 'waiting' ||
        state === 'cancelling' ||
        (!state && String(item?.expires_at || '').trim());
      if (isError) {
        failed.add(agentId);
        failedSignatures.set(agentId, signature);
      } else if (isDone) {
        done.add(agentId);
        doneSignatures.set(agentId, signature);
      } else if (isRunning) {
        running.add(agentId);
      }
      if (pending) {
        waiting.add(agentId);
      }
    });
    runningAgentIds.value = Array.from(running);
    waitingAgentIds.value = Array.from(waiting);
    writeRunningStateCache(runningAgentIds.value, waitingAgentIds.value);

    const now = Date.now();
    cleanupTransientAgentStates(now);

    const nextTransient = { ...transientAgentStates.value };
    const nextAcknowledgedDone = { ...acknowledgedDoneStateSignatures.value };

    running.forEach((agentId) => {
      delete nextTransient[agentId];
      delete nextAcknowledgedDone[agentId];
    });
    waiting.forEach((agentId) => {
      delete nextTransient[agentId];
      delete nextAcknowledgedDone[agentId];
    });

    const setTransient = (agentId, state: 'done' | 'error', signature: string) => {
      if (!agentId) return;
      const nextSignature = signature.trim() || `${state}|${agentId}|${now}`;
      nextTransient[agentId] = {
        state,
        until: state === 'error' ? now + TRANSIENT_ERROR_TTL_MS : null,
        signature: nextSignature
      };
      if (state !== 'done') {
        delete nextAcknowledgedDone[agentId];
      } else if (nextAcknowledgedDone[agentId] && nextAcknowledgedDone[agentId] !== nextSignature) {
        delete nextAcknowledgedDone[agentId];
      }
    };

    done.forEach((agentId) => setTransient(agentId, 'done', doneSignatures.get(agentId) || ''));
    failed.forEach((agentId) => setTransient(agentId, 'error', failedSignatures.get(agentId) || ''));

    const prevActive = new Set([...prevRunning, ...prevWaiting]);
    prevActive.forEach((agentId) => {
      if (running.has(agentId) || waiting.has(agentId)) return;
      if (failed.has(agentId)) return;
      setTransient(agentId, 'done', `inferred|${agentId}|${now}`);
    });

    Object.entries(nextAcknowledgedDone).forEach(([agentId, signature]) => {
      const current = nextTransient[agentId];
      if (!current || current.state !== 'done' || current.signature !== signature) {
        delete nextAcknowledgedDone[agentId];
      }
    });

    transientAgentStates.value = nextTransient;
    acknowledgedDoneStateSignatures.value = nextAcknowledgedDone;
    writeTransientStateCache(nextTransient);
    writeDoneAckCache(nextAcknowledgedDone);
  } catch (error) {
    // Keep the last known state to avoid flickering to idle when the polling request fails.
  }
};


const openCreateDialog = async () => {
  if (!toolCatalog.value) {
    await loadCatalog();
  }
  await loadAgentCopyOptions();
  resetForm();
  dialogVisible.value = true;
};

const saveAgent = async () => {
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning(t('portal.agent.nameRequired'));
    return;
  }
  saving.value = true;
  try {
    const payload = {
      name,
      description: form.description || '',
      is_shared: Boolean(form.is_shared),
      copy_from_agent_id: String(form.copy_from_agent_id || '').trim(),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      system_prompt: form.system_prompt || '',
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    };
    if (!payload.copy_from_agent_id) {
      delete payload.copy_from_agent_id;
    }
    if (editingId.value) {
      delete payload.copy_from_agent_id;
      await agentStore.updateAgent(editingId.value, payload);
      ElMessage.success(t('portal.agent.updateSuccess'));
    } else {
      await agentStore.createAgent(payload);
      ElMessage.success(t('portal.agent.createSuccess'));
    }
    await loadAgentCopyOptions();
    dialogVisible.value = false;
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const enterAgent = (agent) => {
  const agentId = agent?.id;
  if (!agentId) return;
  queueAgentPrefetch(agent, true);
  acknowledgeAgentDoneState(agentId);
  router.push({
    path: `${basePath.value}/chat`,
    query: {
      agent_id: String(agentId),
      container_id: String(getAgentSandboxContainerId(agent))
    }
  });
};

const enterDefaultChat = () => {
  queueAgentPrefetch(null, true);
  acknowledgeAgentDoneState(DEFAULT_AGENT_KEY);
  router.push({
    path: `${basePath.value}/chat`,
    query: {
      entry: 'default',
      container_id: String(getAgentSandboxContainerId(null))
    }
  });
};

const toggleMoreApps = () => {
  showMoreApps.value = !showMoreApps.value;
};

const openExternalApp = (link) => {
  const linkId = String(link?.link_id || '').trim();
  if (!linkId) return;
  router.push(basePath.value + '/external/' + encodeURIComponent(linkId));
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
