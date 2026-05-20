<template>
  <div class="desktop-system-settings-shell">
    <DesktopRuntimeSettingsPanel v-if="showRuntimePanel" />

    <section
      v-if="showLanPanel"
      class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--stack"
    >
      <div class="desktop-system-settings-head">
        <div>
          <div class="messenger-settings-title">{{ t('desktop.system.lan.title') }}</div>
          <div class="messenger-settings-subtitle">{{ t('desktop.system.lan.hint') }}</div>
        </div>
        <span class="desktop-system-settings-remote-state" :class="{ connected: lanMeshEnabled }">
          {{ lanMeshEnabled ? t('desktop.system.lan.enabled') : t('desktop.system.lan.disabled') }}
        </span>
      </div>

      <div v-if="loading" class="desktop-system-settings-loading">
        {{ t('common.loading') }}
      </div>

      <div class="desktop-system-settings-section desktop-system-settings-form-grid">
        <label class="desktop-system-settings-field desktop-system-settings-field--switch">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.enabledSwitch') }}</span>
          <el-switch v-model="lanMeshEnabled" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.peerId') }}</span>
          <el-input v-model="lanPeerId" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.displayName') }}</span>
          <el-input v-model="lanDisplayName" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.listenHost') }}</span>
          <el-input v-model="lanListenHost" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.listenPort') }}</span>
          <el-input v-model="lanListenPort" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.discoveryPort') }}</span>
          <el-input v-model="lanDiscoveryPort" :disabled="loading || savingLan" />
        </label>
        <label class="desktop-system-settings-field desktop-system-settings-field--full">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.allowSubnets') }}</span>
          <el-input
            v-model="lanAllowSubnetsText"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            :disabled="loading || savingLan"
            :placeholder="t('desktop.system.lan.cidrPlaceholder')"
          />
        </label>
        <label class="desktop-system-settings-field desktop-system-settings-field--full">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.denySubnets') }}</span>
          <el-input
            v-model="lanDenySubnetsText"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            :disabled="loading || savingLan"
            :placeholder="t('desktop.system.lan.cidrPlaceholder')"
          />
        </label>
        <label class="desktop-system-settings-field desktop-system-settings-field--full">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.peerBlacklist') }}</span>
          <el-input
            v-model="lanPeerBlacklistText"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            :disabled="loading || savingLan"
            :placeholder="t('desktop.system.lan.peerBlacklistPlaceholder')"
          />
        </label>
        <label class="desktop-system-settings-field desktop-system-settings-field--full">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.sharedSecret') }}</span>
          <el-input v-model="lanSharedSecret" show-password :disabled="loading || savingLan" />
        </label>
        <div class="desktop-system-settings-actions desktop-system-settings-field--full">
          <el-button
            class="desktop-system-settings-btn desktop-system-settings-btn--primary"
            :loading="savingLan"
            :disabled="loading"
            @click="saveLanSettings"
          >
            {{ t('desktop.common.save') }}
          </el-button>
          <el-button
            class="desktop-system-settings-btn"
            :loading="loadingLanPeers"
            :disabled="loading"
            @click="refreshLanPeers"
          >
            {{ t('desktop.system.lan.refreshPeers') }}
          </el-button>
        </div>
      </div>

      <div class="desktop-system-settings-section">
        <div class="desktop-system-settings-section-title">
          <i class="fa-solid fa-sitemap" aria-hidden="true"></i>
          <span>{{ t('desktop.system.lan.peerListTitle') }}</span>
        </div>
        <div v-if="lanPeers.length" class="desktop-system-settings-peer-list">
          <div
            v-for="peer in lanPeers"
            :key="`${peer.peer_id || ''}-${peer.lan_ip || ''}-${peer.listen_port || ''}`"
            class="desktop-system-settings-peer-item"
          >
            <div class="desktop-system-settings-peer-item-head">
              <span class="desktop-system-settings-peer-item-name">
                {{ peer.display_name || peer.user_id || peer.peer_id }}
              </span>
              <span class="desktop-system-settings-peer-item-id">{{ peer.peer_id }}</span>
            </div>
            <div class="desktop-system-settings-peer-item-meta">
              {{ peer.lan_ip }}:{{ peer.listen_port }}
            </div>
          </div>
        </div>
        <div v-else class="desktop-system-settings-empty">{{ t('desktop.system.lan.peerListEmpty') }}</div>
      </div>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import {
  fetchDesktopSettings,
  listDesktopLanPeers,
  updateDesktopSettings,
  type DesktopLanMeshSettings,
  type DesktopLanPeer
} from '@/api/desktop';
import { useI18n } from '@/i18n';
import DesktopRuntimeSettingsPanel from '@/components/messenger/DesktopRuntimeSettingsPanel.vue';

const props = withDefaults(
  defineProps<{
    panel?: 'system' | 'lan' | 'all';
  }>(),
  {
    panel: 'lan'
  }
);

const { t } = useI18n();

const loading = ref(false);
const savingLan = ref(false);
const loadingLanPeers = ref(false);
const lanMeshEnabled = ref(false);
const lanPeerId = ref('');
const lanDisplayName = ref('');
const lanListenHost = ref('0.0.0.0');
const lanListenPort = ref('18661');
const lanDiscoveryPort = ref('18662');
const lanAllowSubnetsText = ref('');
const lanDenySubnetsText = ref('');
const lanPeerBlacklistText = ref('');
const lanSharedSecret = ref('');
const lanPeers = ref<DesktopLanPeer[]>([]);

const showRuntimePanel = computed(() => props.panel === 'all' || props.panel === 'system');
const showLanPanel = computed(() => props.panel === 'all' || props.panel === 'lan');

const normalizeLineList = (value: string): string[] =>
  String(value || '')
    .split(/\r?\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);

const formatLineList = (items: unknown): string =>
  Array.isArray(items)
    ? items
        .map((item) => String(item || '').trim())
        .filter(Boolean)
        .join('\n')
    : '';

const parsePositiveInt = (value: string, fallback: number): number => {
  const parsed = Number.parseInt(String(value || '').trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
};

const applyLanSettings = (lanMesh: DesktopLanMeshSettings | Record<string, unknown> | undefined) => {
  const data = (lanMesh || {}) as Record<string, unknown>;
  lanMeshEnabled.value = data.enabled === true;
  lanPeerId.value = String(data.peer_id || '').trim();
  lanDisplayName.value = String(data.display_name || '').trim();
  lanListenHost.value = String(data.listen_host || '0.0.0.0').trim() || '0.0.0.0';
  lanListenPort.value = String(data.listen_port ?? 18661);
  lanDiscoveryPort.value = String(data.discovery_port ?? 18662);
  lanAllowSubnetsText.value = formatLineList(data.allow_subnets);
  lanDenySubnetsText.value = formatLineList(data.deny_subnets);
  lanPeerBlacklistText.value = formatLineList(data.peer_blacklist);
  lanSharedSecret.value = String(data.shared_secret || '');
};

const buildLanSettingsPayload = (): DesktopLanMeshSettings => ({
  enabled: lanMeshEnabled.value,
  peer_id: lanPeerId.value.trim(),
  display_name: lanDisplayName.value.trim(),
  listen_host: lanListenHost.value.trim() || '0.0.0.0',
  listen_port: parsePositiveInt(lanListenPort.value, 18661),
  discovery_port: parsePositiveInt(lanDiscoveryPort.value, 18662),
  discovery_interval_ms: 2500,
  peer_ttl_ms: 15000,
  allow_subnets: normalizeLineList(lanAllowSubnetsText.value),
  deny_subnets: normalizeLineList(lanDenySubnetsText.value),
  peer_blacklist: normalizeLineList(lanPeerBlacklistText.value),
  shared_secret: lanSharedSecret.value,
  max_inbound_dedup: 4096,
  relay_http_fallback: true,
  peer_ws_path: '/wunder/desktop/lan/ws',
  peer_http_path: '/wunder/desktop/lan/envelope'
});

const refreshLanPeers = async () => {
  loadingLanPeers.value = true;
  try {
    const response = await listDesktopLanPeers();
    const items = (response?.data?.data?.items || []) as DesktopLanPeer[];
    lanPeers.value = Array.isArray(items) ? items : [];
  } catch (error) {
    console.error(error);
    lanPeers.value = [];
  } finally {
    loadingLanPeers.value = false;
  }
};

const loadLanSettings = async () => {
  if (!showLanPanel.value) return;
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    applyLanSettings(data.lan_mesh as DesktopLanMeshSettings | undefined);
    await refreshLanPeers();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveLanSettings = async () => {
  savingLan.value = true;
  try {
    const response = await updateDesktopSettings({
      lan_mesh: buildLanSettingsPayload()
    });
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    applyLanSettings(data.lan_mesh as DesktopLanMeshSettings | undefined);
    await refreshLanPeers();
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingLan.value = false;
  }
};

watch(
  showLanPanel,
  (enabled) => {
    if (enabled) {
      void loadLanSettings();
    }
  },
  { immediate: true }
);
</script>

<style scoped>
.desktop-system-settings-shell {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 320px;
}

.desktop-system-settings-panel {
  display: grid;
  gap: 14px;
  min-height: 0;
}

.desktop-system-settings-panel--stack {
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
}

.desktop-system-settings-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.desktop-system-settings-loading {
  border: 1px dashed var(--portal-border);
  border-radius: 10px;
  color: var(--portal-muted);
  font-size: 12px;
  padding: 10px 12px;
}

.desktop-system-settings-section {
  border: 1px solid #d8dee8;
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: grid;
  gap: 12px;
}

.desktop-system-settings-section-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-system-settings-section-title i {
  color: var(--portal-muted);
}

.desktop-system-settings-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.desktop-system-settings-btn {
  border-radius: 9px;
  border: 1px solid #d8dce4;
  background: #ffffff;
  color: #4b5563;
  font-weight: 600;
  box-shadow: none;
  transition: border-color 0.15s ease, color 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-btn:hover:not(:disabled) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  color: var(--ui-accent-deep);
  background: var(--ui-accent-soft-2);
}

.desktop-system-settings-btn--primary {
  border-color: transparent;
  background: var(--ui-accent);
  color: #ffffff;
}

.desktop-system-settings-btn--primary:hover:not(:disabled) {
  border-color: transparent;
  background: var(--ui-accent-hover);
  color: #ffffff;
}

.desktop-system-settings-field-label {
  color: var(--portal-text);
  font-size: 12px;
}

.desktop-system-settings-form-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.desktop-system-settings-field {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-settings-field--switch {
  align-content: start;
}

.desktop-system-settings-field--full {
  grid-column: 1 / -1;
}

.desktop-system-settings-empty {
  border: 1px dashed var(--portal-border);
  border-radius: 10px;
  color: var(--portal-muted);
  font-size: 12px;
  text-align: center;
  padding: 16px 10px;
}

.desktop-system-settings-remote-state {
  border: 1px solid var(--portal-border);
  border-radius: 999px;
  color: var(--portal-muted);
  font-size: 12px;
  line-height: 1;
  padding: 7px 10px;
}

.desktop-system-settings-remote-state.connected {
  border-color: rgba(22, 163, 74, 0.32);
  background: rgba(22, 163, 74, 0.08);
  color: #16a34a;
}

.desktop-system-settings-peer-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: min(42vh, 360px);
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
}

.desktop-system-settings-peer-item {
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-surface, rgba(255, 255, 255, 0.9));
  padding: 10px;
  display: grid;
  gap: 6px;
}

.desktop-system-settings-peer-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.desktop-system-settings-peer-item-name {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  color: var(--portal-text);
  font-size: 12px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-peer-item-id {
  border: 1px solid var(--portal-border);
  border-radius: 999px;
  color: var(--portal-muted);
  flex-shrink: 0;
  font-size: 11px;
  padding: 1px 8px;
}

.desktop-system-settings-peer-item-meta {
  color: var(--portal-muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-peer-list::-webkit-scrollbar {
  width: 8px;
}

.desktop-system-settings-peer-list::-webkit-scrollbar-thumb {
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.45);
}

.desktop-system-settings-peer-list::-webkit-scrollbar-track {
  background: transparent;
}

.desktop-system-settings-panel :deep(.el-input__wrapper),
.desktop-system-settings-panel :deep(.el-select__wrapper) {
  background: #ffffff;
  border: 1px solid #d8dce4;
  border-radius: 10px;
  box-shadow: none;
  min-height: 36px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-panel :deep(.el-input__wrapper:hover),
.desktop-system-settings-panel :deep(.el-select__wrapper:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  box-shadow: none;
}

.desktop-system-settings-panel :deep(.el-input__wrapper.is-focus),
.desktop-system-settings-panel :deep(.el-select__wrapper.is-focused) {
  border-color: rgba(var(--ui-accent-rgb), 0.62);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.14);
}

.desktop-system-settings-panel :deep(.el-textarea__inner) {
  background: #ffffff;
  border: 1px solid #d8dce4;
  border-radius: 10px;
  min-height: 64px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-panel :deep(.el-textarea__inner:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
}

.desktop-system-settings-panel :deep(.el-textarea__inner:focus) {
  border-color: rgba(var(--ui-accent-rgb), 0.62);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.14);
}

.desktop-system-settings-panel :deep(.el-input__inner),
.desktop-system-settings-panel :deep(.el-select__selected-item),
.desktop-system-settings-panel :deep(.el-select__placeholder) {
  color: #374151;
  outline: none !important;
  box-shadow: none !important;
}

.desktop-system-settings-panel :deep(input:focus),
.desktop-system-settings-panel :deep(input:focus-visible),
.desktop-system-settings-panel :deep(textarea:focus),
.desktop-system-settings-panel :deep(textarea:focus-visible),
.desktop-system-settings-panel :deep(select:focus),
.desktop-system-settings-panel :deep(select:focus-visible) {
  outline: none !important;
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .desktop-system-settings-btn) {
  border-color: var(--tech-blue-border);
  background: linear-gradient(180deg, rgba(16, 28, 47, 0.96), rgba(11, 20, 35, 0.94));
  color: var(--tech-blue-text);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.03);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .desktop-system-settings-btn:hover:not(:disabled)) {
  border-color: var(--tech-blue-border-strong);
  background: rgba(var(--ui-accent-rgb), 0.14);
  color: #eef9ff;
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .desktop-system-settings-btn--primary) {
  border-color: rgba(var(--ui-accent-rgb), 0.6);
  background: linear-gradient(
    180deg,
    rgba(var(--ui-accent-rgb), 0.88),
    rgba(var(--ui-accent-secondary-rgb), 0.78)
  );
  color: #f8fcff;
  box-shadow:
    0 12px 26px rgba(8, 24, 44, 0.28),
    inset 0 0 0 1px rgba(255, 255, 255, 0.14);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .desktop-system-settings-btn--primary:hover:not(:disabled)) {
  border-color: rgba(var(--ui-accent-rgb), 0.76);
  background: linear-gradient(
    180deg,
    rgba(var(--ui-accent-rgb), 0.96),
    rgba(var(--ui-accent-secondary-rgb), 0.86)
  );
  color: #ffffff;
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-input__wrapper),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__wrapper),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-textarea__inner) {
  border-color: var(--tech-blue-border);
  background: linear-gradient(180deg, rgba(12, 22, 38, 0.96), rgba(9, 17, 30, 0.94));
  color: var(--tech-blue-text);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.02);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-input__wrapper:hover),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__wrapper:hover),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-textarea__inner:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.46);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-input__wrapper.is-focus),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__wrapper.is-focused),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-textarea__inner:focus) {
  border-color: var(--tech-blue-border-strong);
  box-shadow:
    0 0 0 2px rgba(var(--ui-accent-rgb), 0.2),
    inset 0 0 0 1px rgba(var(--ui-accent-rgb), 0.22);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-input__inner),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__selected-item),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__placeholder),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-textarea__inner) {
  color: var(--tech-blue-text);
}

:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-input__inner::placeholder),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-textarea__inner::placeholder),
:global(:root[data-user-accent='tech-blue'] .desktop-system-settings-panel .el-select__placeholder) {
  color: var(--tech-blue-muted);
}

@media (max-width: 900px) {
  .desktop-system-settings-form-grid {
    grid-template-columns: 1fr;
  }
}
</style>
