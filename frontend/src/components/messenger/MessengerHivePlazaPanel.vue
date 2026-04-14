<template>
  <div class="hive-plaza-panel">
    <section class="hive-plaza-hero">
      <div class="hive-plaza-hero-copy">
        <div class="hive-plaza-eyebrow">{{ t('plaza.title') }}</div>
        <h2 class="hive-plaza-hero-title">{{ t('plaza.heroTitle') }}</h2>
        <p class="hive-plaza-hero-desc">{{ t('plaza.heroDesc') }}</p>
      </div>
      <div class="hive-plaza-hero-actions">
        <button class="hive-plaza-primary-btn" type="button" @click="openPublishDialog">
          <i class="fa-solid fa-arrow-up-from-bracket" aria-hidden="true"></i>
          <span>{{ t('plaza.action.publish') }}</span>
        </button>
        <button class="hive-plaza-secondary-btn" type="button" :disabled="plazaStore.loading" @click="reload">
          <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
          <span>{{ t('common.refresh') }}</span>
        </button>
      </div>
      <div class="hive-plaza-stats">
        <div class="hive-plaza-stat-card">
          <span class="hive-plaza-stat-label">{{ t('plaza.stats.total') }}</span>
          <strong class="hive-plaza-stat-value">{{ totalCount }}</strong>
        </div>
        <div class="hive-plaza-stat-card">
          <span class="hive-plaza-stat-label">{{ t('plaza.stats.mine') }}</span>
          <strong class="hive-plaza-stat-value">{{ mineCount }}</strong>
        </div>
        <div class="hive-plaza-stat-card">
          <span class="hive-plaza-stat-label">{{ t('plaza.stats.remote') }}</span>
          <strong class="hive-plaza-stat-value">{{ remoteCount }}</strong>
        </div>
      </div>
    </section>

    <div class="hive-plaza-layout">
      <section class="hive-plaza-detail-card">
        <div v-if="selectedItem" class="hive-plaza-detail-shell">
          <div class="hive-plaza-detail-head">
            <AgentAvatar
              size="lg"
              state="idle"
              :icon="selectedItem.icon"
              :name="selectedItem.title"
              :title="selectedItem.title"
            />
            <div class="hive-plaza-detail-copy">
              <div class="hive-plaza-detail-badges">
                <span class="hive-plaza-kind-chip" :class="`is-${selectedItem.kind}`">
                  {{ resolveKindLabel(selectedItem.kind) }}
                </span>
                <span v-if="selectedItem.mine" class="hive-plaza-kind-chip is-mine">
                  {{ t('plaza.meta.mine') }}
                </span>
              </div>
              <h3 class="hive-plaza-detail-title">{{ selectedItem.title }}</h3>
              <p class="hive-plaza-detail-summary">
                {{ selectedItem.summary || t('common.noDescription') }}
              </p>
            </div>
          </div>

          <div class="hive-plaza-detail-grid">
            <div class="hive-plaza-detail-meta">
              <span class="hive-plaza-detail-label">{{ t('plaza.detail.owner') }}</span>
              <span class="hive-plaza-detail-value">{{ resolveOwnerLabel(selectedItem) }}</span>
            </div>
            <div class="hive-plaza-detail-meta">
              <span class="hive-plaza-detail-label">{{ t('plaza.detail.source') }}</span>
              <span class="hive-plaza-detail-value">{{ selectedItem.source_key || '-' }}</span>
            </div>
            <div class="hive-plaza-detail-meta">
              <span class="hive-plaza-detail-label">{{ t('plaza.detail.size') }}</span>
              <span class="hive-plaza-detail-value">{{ formatBytes(selectedItem.artifact_size_bytes) }}</span>
            </div>
            <div class="hive-plaza-detail-meta">
              <span class="hive-plaza-detail-label">{{ t('plaza.detail.updatedAt') }}</span>
              <span class="hive-plaza-detail-value">{{ formatTime(selectedItem.updated_at) || '-' }}</span>
            </div>
          </div>

          <div v-if="selectedItem.tags?.length" class="hive-plaza-tag-row">
            <span v-for="tag in selectedItem.tags" :key="tag" class="hive-plaza-tag">{{ tag }}</span>
          </div>

          <div class="hive-plaza-detail-actions">
            <button
              class="hive-plaza-primary-btn"
              type="button"
              :disabled="plazaStore.importingItemId === selectedItem.item_id"
              @click="importSelectedItem"
            >
              <i class="fa-solid fa-download" aria-hidden="true"></i>
              <span>{{ resolveImportActionLabel(selectedItem.kind) }}</span>
            </button>
            <button
              v-if="selectedItem.mine"
              class="hive-plaza-danger-btn"
              type="button"
              :disabled="plazaStore.deletingItemId === selectedItem.item_id"
              @click="removeSelectedItem"
            >
              <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
              <span>{{ t('plaza.action.unpublish') }}</span>
            </button>
          </div>
        </div>

        <div v-else class="hive-plaza-empty">
          <div class="hive-plaza-empty-icon">
            <i class="fa-solid fa-hexagon-nodes-bolt" aria-hidden="true"></i>
          </div>
          <div class="hive-plaza-empty-title">{{ t('plaza.detail.emptyTitle') }}</div>
          <div class="hive-plaza-empty-desc">{{ t('plaza.detail.emptyDesc') }}</div>
        </div>
      </section>

      <section class="hive-plaza-feed-card">
        <div class="hive-plaza-section-head">
          <div>
            <div class="hive-plaza-section-title">{{ t('plaza.feed.title') }}</div>
            <div class="hive-plaza-section-subtitle">{{ t('plaza.feed.subtitle') }}</div>
          </div>
        </div>

        <div v-if="plazaStore.loading && !recentItems.length" class="hive-plaza-feed-empty">
          {{ t('common.loading') }}
        </div>
        <div v-else-if="!recentItems.length" class="hive-plaza-feed-empty">
          {{ t('plaza.feed.empty') }}
        </div>
        <div v-else class="hive-plaza-feed-grid">
          <button
            v-for="item in recentItems"
            :key="item.item_id"
            class="hive-plaza-feed-item"
            :class="{ active: item.item_id === selectedItemIdInternal }"
            type="button"
            @click="selectItem(item.item_id)"
          >
            <div class="hive-plaza-feed-top">
              <span class="hive-plaza-feed-kind">{{ resolveKindLabel(item.kind) }}</span>
              <span class="hive-plaza-feed-time">{{ formatTime(item.updated_at) || '-' }}</span>
            </div>
            <div class="hive-plaza-feed-title">{{ item.title }}</div>
            <div class="hive-plaza-feed-summary">{{ item.summary || t('common.noDescription') }}</div>
            <div class="hive-plaza-feed-foot">
              <span>{{ resolveOwnerLabel(item) }}</span>
              <span>{{ formatBytes(item.artifact_size_bytes) }}</span>
            </div>
          </button>
        </div>
      </section>
    </div>

    <el-dialog
      v-model="publishDialogVisible"
      :title="t('plaza.publish.title')"
      width="620px"
      destroy-on-close
    >
      <div class="hive-plaza-publish">
        <div class="hive-plaza-publish-kind-row">
          <button
            v-for="kind in publishKinds"
            :key="kind"
            class="hive-plaza-kind-option"
            :class="{ active: publishForm.kind === kind }"
            type="button"
            @click="publishForm.kind = kind"
          >
            {{ resolveKindLabel(kind) }}
          </button>
        </div>

        <label class="hive-plaza-field">
          <span class="hive-plaza-field-label">{{ t('plaza.publish.source') }}</span>
          <select v-model="publishForm.source_key" class="hive-plaza-select">
            <option v-for="option in sourceOptions" :key="option.value" :value="option.value">
              {{ option.label }}
            </option>
          </select>
          <span v-if="!sourceOptions.length" class="hive-plaza-field-hint">
            {{ t('plaza.publish.sourceEmpty') }}
          </span>
        </label>

        <label class="hive-plaza-field">
          <span class="hive-plaza-field-label">{{ t('plaza.publish.name') }}</span>
          <input v-model="publishForm.title" class="hive-plaza-input" type="text" maxlength="80" />
        </label>

        <label class="hive-plaza-field">
          <span class="hive-plaza-field-label">{{ t('plaza.publish.summary') }}</span>
          <textarea
            v-model="publishForm.summary"
            class="hive-plaza-textarea"
            rows="4"
            maxlength="240"
          ></textarea>
        </label>
      </div>

      <template #footer>
        <div class="hive-plaza-dialog-actions">
          <button class="hive-plaza-secondary-btn" type="button" @click="publishDialogVisible = false">
            {{ t('common.cancel') }}
          </button>
          <button
            class="hive-plaza-primary-btn"
            type="button"
            :disabled="plazaStore.publishing || !publishForm.source_key"
            @click="submitPublish"
          >
            {{ t('plaza.publish.submit') }}
          </button>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import AgentAvatar from '@/components/messenger/AgentAvatar.vue';
import { fetchUserSkills } from '@/api/userTools';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useBeeroomStore } from '@/stores/beeroom';
import { usePlazaStore, type PlazaItem } from '@/stores/plaza';
import { showApiError } from '@/utils/apiError';
import { emitUserToolsUpdated } from '@/utils/userToolsEvents';
import { invalidateUserSkillsCache } from '@/utils/userToolsCache';

type PublishKind = 'hive_pack' | 'worker_card' | 'skill_pack';

type SourceOption = {
  value: string;
  label: string;
  title: string;
  summary: string;
};

const props = withDefaults(
  defineProps<{
    active?: boolean;
    selectedItemId?: string;
    currentUserId?: string;
  }>(),
  {
    active: false,
    selectedItemId: '',
    currentUserId: ''
  }
);

const emit = defineEmits<{
  (event: 'update:selectedItemId', value: string): void;
}>();

const { t } = useI18n();
const plazaStore = usePlazaStore();
const agentStore = useAgentStore();
const beeroomStore = useBeeroomStore();

const publishDialogVisible = ref(false);
const publishKinds: PublishKind[] = ['hive_pack', 'worker_card', 'skill_pack'];
const publishForm = reactive<{ kind: PublishKind; source_key: string; title: string; summary: string }>({
  kind: 'hive_pack',
  source_key: '',
  title: '',
  summary: ''
});
const customSkills = ref<Array<Record<string, unknown>>>([]);

const ownedAgentOptions = computed<SourceOption[]>(() => {
  const options: SourceOption[] = [
    {
      value: '__default__',
      label: t('messenger.defaultAgent'),
      title: t('messenger.defaultAgent'),
      summary: t('messenger.defaultAgentDesc')
    }
  ];
  (Array.isArray(agentStore.agents) ? agentStore.agents : []).forEach((agent) => {
    const id = String(agent?.id || '').trim();
    if (!id) return;
    options.push({
      value: id,
      label: String(agent?.name || id).trim(),
      title: String(agent?.name || id).trim(),
      summary: String(agent?.description || '').trim()
    });
  });
  return options;
});

const swarmOptions = computed<SourceOption[]>(() =>
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).map((group) => {
    const groupId = String(group?.group_id || group?.hive_id || '').trim();
    return {
      value: groupId,
      label: String(group?.name || groupId).trim(),
      title: String(group?.name || groupId).trim(),
      summary: String(group?.description || group?.mother_agent_name || '').trim()
    };
  }).filter((item) => item.value)
);

const skillOptions = computed<SourceOption[]>(() =>
  customSkills.value
    .map((skill) => ({
      value: String(skill?.name || '').trim(),
      label: String(skill?.name || '').trim(),
      title: String(skill?.name || '').trim(),
      summary: String(skill?.description || '').trim()
    }))
    .filter((item) => item.value)
);

const sourceOptions = computed<SourceOption[]>(() => {
  switch (publishForm.kind) {
    case 'worker_card':
      return ownedAgentOptions.value;
    case 'skill_pack':
      return skillOptions.value;
    default:
      return swarmOptions.value;
  }
});

const totalCount = computed(() => plazaStore.items.length);
const mineCount = computed(
  () => plazaStore.items.filter((item) => item.owner_user_id === String(props.currentUserId || '')).length
);
const remoteCount = computed(() => Math.max(0, totalCount.value - mineCount.value));
const selectedItemIdInternal = computed(() => String(props.selectedItemId || '').trim());
const selectedItem = computed<PlazaItem | null>(() => {
  const matched = plazaStore.items.find((item) => item.item_id === selectedItemIdInternal.value);
  return matched || plazaStore.items[0] || null;
});
const recentItems = computed(() => plazaStore.items.slice(0, 6));

const loadPublishSources = async () => {
  try {
    await Promise.allSettled([
      plazaStore.loadItems({ force: true }),
      beeroomStore.loadGroups(),
      agentStore.loadAgents()
    ]);
    const { data } = await fetchUserSkills();
    const skills = Array.isArray(data?.data?.skills) ? data.data.skills : [];
    customSkills.value = skills.filter((item) => String(item?.source || '').trim() === 'custom');
  } catch (error) {
    showApiError(error, t('plaza.publish.loadSkillsFailed'));
  }
};

const ensureSelectedItem = () => {
  if (selectedItemIdInternal.value && plazaStore.items.some((item) => item.item_id === selectedItemIdInternal.value)) {
    return;
  }
  emit('update:selectedItemId', plazaStore.items[0]?.item_id || '');
};

const syncPublishSource = () => {
  if (!sourceOptions.value.length) {
    publishForm.source_key = '';
    return;
  }
  const matched = sourceOptions.value.find((item) => item.value === publishForm.source_key) || sourceOptions.value[0];
  publishForm.source_key = matched.value;
  if (!String(publishForm.title || '').trim()) {
    publishForm.title = matched.title;
  }
  if (!String(publishForm.summary || '').trim()) {
    publishForm.summary = matched.summary;
  }
};

const reload = async () => {
  try {
    await plazaStore.loadItems({ force: true });
    ensureSelectedItem();
    ElMessage.success(t('common.refreshSuccess'));
  } catch (error) {
    showApiError(error, t('plaza.loadFailed'));
  }
};

const openPublishDialog = async () => {
  publishDialogVisible.value = true;
  publishForm.title = '';
  publishForm.summary = '';
  await loadPublishSources();
  syncPublishSource();
};

const selectItem = (itemId: string) => {
  emit('update:selectedItemId', String(itemId || '').trim());
};

const submitPublish = async () => {
  if (!publishForm.source_key) {
    ElMessage.warning(t('plaza.publish.sourceEmpty'));
    return;
  }
  try {
    const published = await plazaStore.publishItem({
      kind: publishForm.kind,
      source_key: publishForm.source_key,
      title: publishForm.title || undefined,
      summary: publishForm.summary || undefined
    });
    publishDialogVisible.value = false;
    await Promise.allSettled([plazaStore.loadItems({ force: true }), agentStore.loadAgents(), beeroomStore.loadGroups()]);
    if (published?.item_id) {
      selectItem(published.item_id);
    }
    ElMessage.success(t('plaza.publish.success'));
  } catch (error) {
    showApiError(error, t('plaza.publish.failed'));
  }
};

const importSelectedItem = async () => {
  if (!selectedItem.value) return;
  try {
    const result = await plazaStore.importItem(selectedItem.value.item_id);
    await Promise.allSettled([agentStore.loadAgents(), beeroomStore.loadGroups()]);
    if (selectedItem.value.kind === 'skill_pack') {
      invalidateUserSkillsCache();
      emitUserToolsUpdated({ scope: 'skills', action: 'import' });
    }
    const message = String(result?.message || '').trim() || t('plaza.import.success');
    ElMessage.success(message);
  } catch (error) {
    showApiError(error, t('plaza.import.failed'));
  }
};

const removeSelectedItem = async () => {
  if (!selectedItem.value?.mine) return;
  try {
    await ElMessageBox.confirm(t('plaza.delete.confirm', { title: selectedItem.value.title }), t('common.notice'), {
      type: 'warning'
    });
  } catch {
    return;
  }
  try {
    const removedId = selectedItem.value.item_id;
    await plazaStore.deleteItem(removedId);
    if (selectedItemIdInternal.value === removedId) {
      emit('update:selectedItemId', plazaStore.items.find((item) => item.item_id !== removedId)?.item_id || '');
    }
    ElMessage.success(t('plaza.delete.success'));
  } catch (error) {
    showApiError(error, t('plaza.delete.failed'));
  }
};

const resolveKindLabel = (kind: string) => t(`plaza.kind.${kind}`);

const resolveOwnerLabel = (item: PlazaItem) => {
  if (item.owner_user_id && item.owner_user_id === String(props.currentUserId || '')) {
    return t('plaza.meta.mine');
  }
  return item.owner_username || item.owner_user_id || '-';
};

const resolveImportActionLabel = (kind: string) => {
  if (kind === 'hive_pack') return t('plaza.action.importHive');
  if (kind === 'skill_pack') return t('plaza.action.importSkill');
  return t('plaza.action.importWorker');
};

const formatBytes = (value: unknown) => {
  const size = Number(value || 0);
  if (!Number.isFinite(size) || size <= 0) return t('plaza.meta.sizeUnknown');
  if (size >= 1024 * 1024) {
    return `${(size / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (size >= 1024) {
    return `${(size / 1024).toFixed(1)} KB`;
  }
  return `${Math.max(1, Math.round(size))} B`;
};

const formatTime = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) return '';
  const date = new Date(numeric * 1000);
  if (Number.isNaN(date.getTime())) return '';
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(
    date.getDate()
  ).padStart(2, '0')}`;
};

watch(
  () => props.active,
  (active) => {
    if (!active) return;
    void plazaStore.loadItems({ force: plazaStore.items.length === 0 }).then(ensureSelectedItem);
  },
  { immediate: true }
);

watch(() => plazaStore.items, ensureSelectedItem, { deep: true });

watch(
  () => publishForm.kind,
  () => {
    publishForm.source_key = '';
    publishForm.title = '';
    publishForm.summary = '';
    syncPublishSource();
  }
);

watch(sourceOptions, () => {
  syncPublishSource();
});

onMounted(() => {
  if (props.active) {
    void plazaStore.loadItems({ force: plazaStore.items.length === 0 }).then(ensureSelectedItem);
  }
});
</script>

<style scoped>
.hive-plaza-panel {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.hive-plaza-hero {
  position: relative;
  padding: 22px 24px;
  border-radius: 24px;
  background:
    radial-gradient(circle at top right, rgba(255, 206, 117, 0.34), transparent 34%),
    radial-gradient(circle at bottom left, rgba(247, 156, 62, 0.22), transparent 28%),
    linear-gradient(135deg, rgba(255, 248, 229, 0.98), rgba(255, 236, 196, 0.96));
  border: 1px solid rgba(223, 161, 72, 0.26);
  box-shadow: 0 18px 34px rgba(196, 145, 53, 0.12);
  display: grid;
  grid-template-columns: minmax(0, 1.6fr) auto;
  gap: 18px;
}

.hive-plaza-eyebrow {
  font-size: 12px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: #9a6a18;
  font-weight: 700;
}

.hive-plaza-hero-title {
  margin: 8px 0 10px;
  font-size: 28px;
  line-height: 1.1;
  color: #432b0b;
}

.hive-plaza-hero-desc {
  margin: 0;
  max-width: 720px;
  color: #6e5327;
  line-height: 1.7;
}

.hive-plaza-hero-actions {
  display: flex;
  align-items: flex-start;
  justify-content: flex-end;
  gap: 10px;
}

.hive-plaza-stats {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.hive-plaza-stat-card,
.hive-plaza-detail-card,
.hive-plaza-feed-card {
  border-radius: 22px;
  border: 1px solid rgba(15, 23, 42, 0.08);
  background: rgba(255, 255, 255, 0.92);
  box-shadow: 0 12px 28px rgba(15, 23, 42, 0.06);
}

.hive-plaza-stat-card {
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hive-plaza-stat-label {
  font-size: 12px;
  color: #8d6732;
}

.hive-plaza-stat-value {
  font-size: 24px;
  color: #3f2b11;
}

.hive-plaza-layout {
  display: grid;
  grid-template-columns: minmax(0, 1.1fr) minmax(320px, 0.9fr);
  gap: 18px;
}

.hive-plaza-detail-card,
.hive-plaza-feed-card {
  padding: 20px;
}

.hive-plaza-detail-shell {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.hive-plaza-detail-head {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.hive-plaza-detail-copy {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}

.hive-plaza-detail-badges,
.hive-plaza-tag-row,
.hive-plaza-detail-actions,
.hive-plaza-publish-kind-row,
.hive-plaza-dialog-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.hive-plaza-kind-chip,
.hive-plaza-tag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  padding: 5px 10px;
  font-size: 12px;
  font-weight: 700;
}

.hive-plaza-kind-chip {
  background: rgba(255, 214, 127, 0.22);
  color: #8a5a0a;
}

.hive-plaza-kind-chip.is-hive_pack {
  background: rgba(255, 196, 120, 0.24);
}

.hive-plaza-kind-chip.is-worker_card {
  background: rgba(114, 155, 255, 0.18);
  color: #2645a1;
}

.hive-plaza-kind-chip.is-skill_pack {
  background: rgba(65, 183, 136, 0.18);
  color: #136548;
}

.hive-plaza-kind-chip.is-mine,
.hive-plaza-tag {
  background: rgba(15, 23, 42, 0.06);
  color: #435068;
}

.hive-plaza-detail-title {
  margin: 0;
  font-size: 24px;
  color: #1f2937;
}

.hive-plaza-detail-summary {
  margin: 0;
  color: #536275;
  line-height: 1.7;
}

.hive-plaza-detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.hive-plaza-detail-meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 14px;
  border-radius: 16px;
  background: rgba(248, 250, 252, 0.9);
}

.hive-plaza-detail-label {
  font-size: 12px;
  color: #7a8797;
}

.hive-plaza-detail-value {
  color: #233146;
  font-weight: 600;
  word-break: break-all;
}

.hive-plaza-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.hive-plaza-section-title {
  font-size: 18px;
  font-weight: 700;
  color: #1f2937;
}

.hive-plaza-section-subtitle {
  margin-top: 4px;
  color: #66768a;
  font-size: 13px;
}

.hive-plaza-feed-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.hive-plaza-feed-item {
  text-align: left;
  padding: 14px;
  border-radius: 18px;
  border: 1px solid rgba(15, 23, 42, 0.08);
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.98), rgba(248, 250, 252, 0.96));
  cursor: pointer;
  transition: transform 0.18s ease, border-color 0.18s ease, box-shadow 0.18s ease;
}

.hive-plaza-feed-item:hover,
.hive-plaza-feed-item.active {
  transform: translateY(-1px);
  border-color: rgba(214, 143, 51, 0.34);
  box-shadow: 0 12px 22px rgba(196, 145, 53, 0.12);
}

.hive-plaza-feed-top,
.hive-plaza-feed-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  font-size: 12px;
  color: #7a8797;
}

.hive-plaza-feed-title {
  margin: 10px 0 8px;
  font-size: 16px;
  font-weight: 700;
  color: #1f2937;
}

.hive-plaza-feed-summary {
  min-height: 40px;
  color: #526070;
  line-height: 1.5;
}

.hive-plaza-feed-empty,
.hive-plaza-empty {
  min-height: 220px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 10px;
  color: #718096;
}

.hive-plaza-empty-icon {
  width: 56px;
  height: 56px;
  border-radius: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 214, 127, 0.2);
  color: #b87310;
  font-size: 22px;
}

.hive-plaza-empty-title {
  font-size: 18px;
  font-weight: 700;
  color: #314255;
}

.hive-plaza-empty-desc {
  max-width: 360px;
  text-align: center;
  line-height: 1.7;
}

.hive-plaza-primary-btn,
.hive-plaza-secondary-btn,
.hive-plaza-danger-btn,
.hive-plaza-kind-option {
  border: none;
  border-radius: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  font-weight: 700;
  cursor: pointer;
  transition: transform 0.16s ease, opacity 0.16s ease, box-shadow 0.16s ease;
}

.hive-plaza-primary-btn,
.hive-plaza-secondary-btn,
.hive-plaza-danger-btn {
  padding: 11px 15px;
}

.hive-plaza-primary-btn {
  background: linear-gradient(135deg, #f6a621, #e78b1e);
  color: #ffffff;
  box-shadow: 0 10px 20px rgba(231, 139, 30, 0.24);
}

.hive-plaza-secondary-btn {
  background: rgba(15, 23, 42, 0.06);
  color: #334155;
}

.hive-plaza-danger-btn {
  background: rgba(190, 24, 93, 0.12);
  color: #9d174d;
}

.hive-plaza-primary-btn:hover,
.hive-plaza-secondary-btn:hover,
.hive-plaza-danger-btn:hover,
.hive-plaza-kind-option:hover {
  transform: translateY(-1px);
}

.hive-plaza-primary-btn:disabled,
.hive-plaza-secondary-btn:disabled,
.hive-plaza-danger-btn:disabled {
  cursor: not-allowed;
  opacity: 0.6;
  transform: none;
}

.hive-plaza-publish {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.hive-plaza-kind-option {
  padding: 10px 14px;
  background: rgba(15, 23, 42, 0.06);
  color: #475569;
}

.hive-plaza-kind-option.active {
  background: rgba(231, 139, 30, 0.16);
  color: #9a5e11;
}

.hive-plaza-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.hive-plaza-field-label {
  font-size: 13px;
  font-weight: 700;
  color: #334155;
}

.hive-plaza-input,
.hive-plaza-textarea,
.hive-plaza-select {
  width: 100%;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.36);
  background: #ffffff;
  color: #1f2937;
  padding: 12px 14px;
  font: inherit;
  box-sizing: border-box;
}

.hive-plaza-textarea {
  resize: vertical;
  min-height: 108px;
}

.hive-plaza-field-hint {
  color: #8b97a7;
  font-size: 12px;
}

@media (max-width: 1180px) {
  .hive-plaza-layout,
  .hive-plaza-hero {
    grid-template-columns: minmax(0, 1fr);
  }

  .hive-plaza-feed-grid,
  .hive-plaza-detail-grid,
  .hive-plaza-stats {
    grid-template-columns: minmax(0, 1fr);
  }

  .hive-plaza-hero-actions {
    justify-content: flex-start;
  }
}
</style>
