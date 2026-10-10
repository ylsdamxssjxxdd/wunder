<template>
  <div class="messenger-settings-frame-category messenger-settings-devices" data-testid="settings-category-devices">
    <section class="messenger-settings-card">
      <div class="messenger-settings-group-head messenger-settings-group-head--row">
        <div>
          <div class="messenger-settings-title">{{ t('interlink.devices.title') }}</div>
          <div class="messenger-settings-subtitle">{{ t('interlink.devices.hint') }}</div>
        </div>
        <div class="messenger-settings-page-row-actions">
          <span class="interlink-devices-aggregate">
            {{ t('interlink.devices.aggregate', { online: onlineCount, total: totalCount }) }}
          </span>
          <button
            class="messenger-settings-action ghost"
            type="button"
            :disabled="loading"
            @click="handleRefresh"
          >
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ t('common.refresh') }}</span>
          </button>
        </div>
      </div>

      <div v-if="loading && !deviceNodes.length" class="messenger-list-empty">
        {{ t('common.loading') }}
      </div>
      <div v-else-if="error && !deviceNodes.length" class="messenger-list-empty is-error">
        {{ error || t('interlink.devices.loadFailed') }}
      </div>
      <div v-else-if="!deviceNodes.length" class="messenger-list-empty">
        {{ t('interlink.devices.empty') }}
      </div>

      <div v-else class="interlink-device-grid">
        <article
          v-for="node in deviceNodes"
          :key="node.node_id"
          class="interlink-device-card"
          :class="{ 'is-offline': !isReachable(node) }"
        >
          <header class="interlink-device-card-head">
            <span class="interlink-status-dot" :class="statusDotClass(node.status)" aria-hidden="true"></span>
            <i :class="nodeTypeIcon(node.node_type)" aria-hidden="true"></i>
            <strong class="interlink-device-card-name" :title="node.label || node.node_id">
              {{ node.label || node.node_id }}
            </strong>
            <span class="interlink-device-card-badge">{{ t(nodeTypeLabelKey(node.node_type)) }}</span>
            <span class="interlink-device-card-status">{{ t(statusLabelKey(node.status)) }}</span>
          </header>

          <dl class="interlink-device-card-meta">
            <div class="interlink-device-meta-row">
              <dt>{{ t('interlink.devices.lastActive') }}</dt>
              <dd>{{ lastActiveLabel(node) }}</dd>
            </div>
            <div class="interlink-device-meta-row">
              <dt>{{ t('interlink.devices.shadow') }}</dt>
              <dd>
                {{ shadowLabel(node) }}
                <span v-if="node.shadow_revision > 0" class="interlink-device-shadow-rev">
                  r{{ node.shadow_revision }}
                </span>
              </dd>
            </div>
            <div class="interlink-device-meta-row">
              <dt>{{ t('interlink.devices.capabilities') }}</dt>
              <dd :title="node.capabilities.join(', ')">{{ capabilityLabel(node) }}</dd>
            </div>
          </dl>

          <div class="interlink-device-card-actions">
            <label class="interlink-device-switch">
              <span>{{ t('interlink.devices.killSwitch') }}</span>
              <el-switch
                :model-value="node.interlink_enabled"
                :loading="busyIds.has(node.node_id)"
                :disabled="busyIds.has(node.node_id)"
                :aria-label="t('interlink.devices.killSwitch')"
                @update:model-value="toggleEnabled(node)"
              />
            </label>
            <button
              class="messenger-settings-action ghost"
              type="button"
              :disabled="busyIds.has(node.node_id)"
              @click="openRemoteWorkspace(node)"
            >
              {{ t('interlink.devices.openWorkspace') }}
            </button>
            <button
              class="messenger-settings-action ghost is-danger"
              type="button"
              :disabled="busyIds.has(node.node_id) || node.shadow_revision <= 0"
              @click="purgeShadow(node)"
            >
              {{ t('interlink.devices.purgeShadow') }}
            </button>
          </div>
        </article>
      </div>

      <button
        v-if="hasMore"
        class="interlink-devices-more"
        type="button"
        :disabled="loading"
        @click="loadMore"
      >
        {{ t('interlink.devices.loadMore') }}
      </button>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onActivated, onDeactivated, reactive, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import {
  purgeInterlinkShadow,
  setInterlinkNodeEnabled
} from '@/api/interlink';
import type { InterlinkNode } from '@/api/interlink';
import { useI18n } from '@/i18n';
import { resolveApiError } from '@/utils/apiError';
import { confirmWithFallback } from '@/utils/confirm';
import { resolveUserBasePath } from '@/utils/basePath';
import type { MessengerControllerContext } from '@/views/messenger/controller/messengerControllerContext';
import {
  isNodeReachable,
  minutesSince,
  nodeTypeIcon,
  nodeTypeLabelKey,
  statusDotClass,
  statusLabelKey
} from '@/views/messenger/interlink/interlinkNodeModel';
import { useInterlinkNodes } from '@/views/messenger/interlink/useInterlinkNodes';

// 分类页统一由 MessengerSettingsPage 注入 controller；本页只依赖互通节点目录，不用控制器状态。
defineProps<{ controller: MessengerControllerContext }>();

const { t } = useI18n();
const route = useRoute();
const router = useRouter();

/** KeepAlive 停用（切到别的分类）时停掉轮询，节点目录不在前台就不该打服务端。 */
const isActive = ref(true);
onActivated(() => {
  isActive.value = true;
});
onDeactivated(() => {
  isActive.value = false;
});

const nodes = useInterlinkNodes({ active: isActive });

const deviceNodes = nodes.deviceNodes;
const loading = nodes.loading;
const error = nodes.error;
const hasMore = nodes.hasMore;
const onlineCount = nodes.onlineCount;
const totalCount = nodes.total;

const busyIds = reactive(new Set<string>());

const isReachable = (node: InterlinkNode): boolean => isNodeReachable(node);

const lastActiveLabel = (node: InterlinkNode): string => {
  const minutes = minutesSince(node.last_seen_at);
  if (minutes === null) return t('interlink.devices.neverSeen');
  return t('interlink.devices.minutesAgo', { minutes });
};

const shadowSyncedAt = (node: InterlinkNode): unknown => {
  const meta = (node.meta || {}) as Record<string, unknown>;
  const nested = (meta.shadow || {}) as Record<string, unknown>;
  return meta.shadow_synced_at ?? meta.synced_at ?? nested.synced_at ?? '';
};

const shadowLabel = (node: InterlinkNode): string => {
  if (!node.shadow_revision) return t('interlink.devices.shadowEmpty');
  const minutes = minutesSince(shadowSyncedAt(node));
  if (minutes === null) return t('interlink.devices.shadowUpdated');
  return t('interlink.devices.minutesAgo', { minutes });
};
const capabilityLabel = (node: InterlinkNode): string => {
  const caps = node.capabilities || [];
  if (!caps.length) return t('interlink.devices.capabilitiesReadonly');
  return t('interlink.devices.capabilitiesCount', { count: caps.length });
};

const handleRefresh = (): void => {
  nodes.refresh();
};

const loadMore = (): void => {
  nodes.loadMore();
};

/**
 * kill switch：关闭前必须二次确认（关掉后服务端拒绝隧道并在 30s 内拆链，
 * 重开要等设备下一次心跳带确认，§5.3）；开启是低风险动作，直接执行。
 */
const toggleEnabled = async (node: InterlinkNode): Promise<void> => {
  const next = node.interlink_enabled !== true;
  if (!next) {
    const confirmed = await confirmWithFallback(
      t('interlink.devices.killSwitchConfirmBody', { name: node.label || node.node_id }),
      t('interlink.devices.killSwitchConfirmTitle'),
      {
        type: 'warning',
        confirmButtonText: t('interlink.devices.killSwitchConfirmOk'),
        cancelButtonText: t('common.cancel')
      }
    );
    if (!confirmed) return;
  }
  if (busyIds.has(node.node_id)) return;
  busyIds.add(node.node_id);
  try {
    const outcome = await setInterlinkNodeEnabled(node.node_id, next);
    node.interlink_enabled = outcome.interlink_enabled;
    ElMessage.success(
      next ? t('interlink.devices.killSwitchOn') : t('interlink.devices.killSwitchOff')
    );
  } catch (caught) {
    ElMessage.error(resolveApiError(caught, t('interlink.devices.actionFailed')).message);
  } finally {
    busyIds.delete(node.node_id);
  }
};

const purgeShadow = async (node: InterlinkNode): Promise<void> => {
  const confirmed = await confirmWithFallback(
    t('interlink.devices.purgeConfirmBody', { name: node.label || node.node_id }),
    t('interlink.devices.purgeConfirmTitle'),
    {
      type: 'warning',
      confirmButtonText: t('interlink.devices.purgeConfirmOk'),
      cancelButtonText: t('common.cancel')
    }
  );
  if (!confirmed || busyIds.has(node.node_id)) return;
  busyIds.add(node.node_id);
  try {
    await purgeInterlinkShadow(node.node_id);
    node.shadow_revision = 0;
    ElMessage.success(t('interlink.devices.purgeDone'));
  } catch (caught) {
    ElMessage.error(resolveApiError(caught, t('interlink.devices.actionFailed')).message);
  } finally {
    busyIds.delete(node.node_id);
  }
};

/** §5.3 卡片入口：进入 §6.3 的远程工作区，节点目标写在 URL 上（可分享，接收者须同账号）。 */
const openRemoteWorkspace = (node: InterlinkNode): void => {
  const path = `${resolveUserBasePath(route.path || '')}/chat`;
  void router
    .push({
      path,
      query: { ...route.query, section: 'messages', node: `device:${node.node_id}` }
    })
    .catch(() => undefined);
};
</script>
