<template>
  <div class="hive-plaza-panel">
    <div v-if="plazaStore.error" class="hive-plaza-banner hive-plaza-banner--error">
      <i class="fa-solid fa-circle-exclamation" aria-hidden="true"></i>
      <span>{{ plazaStore.error }}</span>
    </div>

    <section class="hive-plaza-feed-shell">
      <div class="hive-plaza-toolbar">
        <div class="hive-plaza-toolbar-copy">
          <div class="hive-plaza-toolbar-title">{{ resolveKindLabel(browseKindInternal) }}</div>
          <div class="hive-plaza-toolbar-subtitle">{{ currentBrowseDescription }}</div>
        </div>
        <div class="hive-plaza-search-row">
          <label class="hive-plaza-search">
            <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
            <input
              v-model="panelKeyword"
              type="text"
              :placeholder="t('plaza.search.placeholder')"
              autocomplete="off"
              spellcheck="false"
            />
          </label>
          <div class="hive-plaza-search-actions">
            <button
              class="hive-plaza-secondary-btn hive-plaza-toolbar-btn"
              type="button"
              :disabled="plazaStore.publishing"
              @click="openPublishDialog"
            >
              <i class="fa-solid fa-arrow-up-from-bracket" aria-hidden="true"></i>
              <span>{{ t('plaza.action.publish') }}</span>
            </button>
            <button
              class="hive-plaza-secondary-btn hive-plaza-toolbar-btn"
              type="button"
              :disabled="plazaStore.loading"
              @click="reload"
            >
              <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
              <span>{{ t('common.refresh') }}</span>
            </button>
          </div>
        </div>
      </div>

      <div class="hive-plaza-toolbar-meta">
        <span>{{ t('plaza.search.resultCount', { count: filteredFeedItems.length }) }}</span>
        <span v-if="pageCount > 1">{{ t('plaza.pager.summary', { page: currentPageInternal, total: pageCount }) }}</span>
      </div>

      <div class="hive-plaza-results">
        <div v-if="plazaStore.loading && !filteredFeedItems.length" class="hive-plaza-empty">
          <div class="hive-plaza-empty-icon">
            <i class="fa-solid fa-spinner fa-spin" aria-hidden="true"></i>
          </div>
          <div class="hive-plaza-empty-title">{{ t('common.loading') }}</div>
        </div>
        <div v-else-if="!filteredFeedItems.length" class="hive-plaza-empty">
          <div class="hive-plaza-empty-icon">
            <i :class="hasSearchKeyword ? 'fa-solid fa-magnifying-glass' : 'fa-solid fa-store-slash'" aria-hidden="true"></i>
          </div>
          <div class="hive-plaza-empty-title">
            {{ hasSearchKeyword ? t('plaza.feed.emptyFiltered') : t('plaza.feed.empty') }}
          </div>
          <div class="hive-plaza-empty-desc">
            {{ hasSearchKeyword ? t('plaza.search.emptyHint') : currentBrowseDescription }}
          </div>
        </div>
        <div v-else class="hive-plaza-feed-grid">
          <button
            v-for="item in pagedFeedItems"
            :key="item.item_id"
            class="hive-plaza-feed-item"
            type="button"
            @click="selectItem(item.item_id)"
          >
            <div class="hive-plaza-feed-top">
              <span class="hive-plaza-kind-chip" :class="`is-${item.kind}`">
                {{ resolveKindLabel(item.kind) }}
              </span>
              <span class="hive-plaza-feed-time">{{ formatTime(item.updated_at) || '-' }}</span>
            </div>
            <div class="hive-plaza-feed-head">
              <AgentAvatar
                size="md"
                state="idle"
                :icon="resolvePlazaAvatarIcon(item)"
                :image-url="resolvePlazaAvatarImageUrl(item)"
                :name="item.title"
                :title="item.title"
              />
              <div class="hive-plaza-feed-copy">
                <div class="hive-plaza-feed-title">{{ item.title }}</div>
                <div class="hive-plaza-feed-owner">{{ resolveOwnerLabel(item) }}</div>
              </div>
            </div>
            <div class="hive-plaza-feed-summary">{{ item.summary || t('common.noDescription') }}</div>
            <div
              v-if="resolveFreshnessNotice(item)"
              class="hive-plaza-freshness"
              :class="`is-${item.freshness_status || 'current'}`"
            >
              <i class="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
              <span>{{ resolveFreshnessNotice(item) }}</span>
            </div>
            <div class="hive-plaza-feed-foot">
              <span>{{ formatBytes(item.artifact_size_bytes) }}</span>
              <span class="hive-plaza-feed-source">{{ item.source_key || '-' }}</span>
            </div>
          </button>
        </div>
      </div>

      <div v-if="pageCount > 1" class="hive-plaza-pager">
        <button
          class="hive-plaza-secondary-btn"
          type="button"
          :disabled="currentPageInternal <= 1"
          @click="goPrevPage"
        >
          <i class="fa-solid fa-chevron-left" aria-hidden="true"></i>
          <span>{{ t('plaza.pager.prev') }}</span>
        </button>
        <div class="hive-plaza-pager-summary">
          {{ t('plaza.pager.summary', { page: currentPageInternal, total: pageCount }) }}
        </div>
        <button
          class="hive-plaza-secondary-btn"
          type="button"
          :disabled="currentPageInternal >= pageCount"
          @click="goNextPage"
        >
          <span>{{ t('plaza.pager.next') }}</span>
          <i class="fa-solid fa-chevron-right" aria-hidden="true"></i>
        </button>
      </div>
    </section>

    <el-dialog
      v-model="detailDialogVisible"
      :title="selectedItem?.title || ''"
      width="720px"
      destroy-on-close
    >
      <section v-if="selectedItem" class="hive-plaza-detail-shell">
        <div class="hive-plaza-detail-badges">
          <span class="hive-plaza-kind-chip" :class="`is-${selectedItem.kind}`">
            {{ resolveKindLabel(selectedItem.kind) }}
          </span>
          <span v-if="selectedItem.mine" class="hive-plaza-kind-chip is-mine">
            {{ t('plaza.meta.mine') }}
          </span>
        </div>

        <div class="hive-plaza-detail-head">
          <AgentAvatar
            size="lg"
            state="idle"
            :icon="resolvePlazaAvatarIcon(selectedItem)"
            :image-url="resolvePlazaAvatarImageUrl(selectedItem)"
            :name="selectedItem.title"
            :title="selectedItem.title"
          />
          <div class="hive-plaza-detail-copy">
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

        <div
          v-if="resolveFreshnessNotice(selectedItem)"
          class="hive-plaza-detail-notice"
          :class="`is-${selectedItem.freshness_status || 'current'}`"
        >
          <i class="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
          <span>{{ resolveFreshnessNotice(selectedItem) }}</span>
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
      </section>
    </el-dialog>

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
import {
  clampPlazaPage,
  filterPlazaItemsByKeyword,
  isPublishableBeeroomGroup,
  isPublishableOwnedAgent,
  normalizePlazaBrowseKind,
  paginatePlazaItems,
  resolvePlazaPageCount,
  type PlazaBrowseKind
} from '@/components/messenger/hivePlazaPanelState';
import { showApiError } from '@/utils/apiError';
import { emitUserToolsUpdated } from '@/utils/userToolsEvents';
import { invalidateUserSkillsCache } from '@/utils/userToolsCache';

type PublishKind = PlazaBrowseKind;

type SourceOption = {
  value: string;
  label: string;
  title: string;
  summary: string;
};

const PAGE_SIZE = 8;

const props = withDefaults(
  defineProps<{
    active?: boolean;
    items?: PlazaItem[];
    browseKind?: PlazaBrowseKind;
    selectedItemId?: string;
    currentUserId?: string;
  }>(),
  {
    active: false,
    items: () => [],
    browseKind: 'hive_pack',
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
const hivePackAvatarImageUrl = `${import.meta.env.BASE_URL}beeroom.png`;

const publishDialogVisible = ref(false);
const panelKeyword = ref('');
const currentPage = ref(1);
const publishKinds: PublishKind[] = ['hive_pack', 'worker_card', 'skill_pack'];
const publishForm = reactive<{ kind: PublishKind; source_key: string; title: string; summary: string }>({
  kind: 'hive_pack',
  source_key: '',
  title: '',
  summary: ''
});
const customSkills = ref<Array<Record<string, unknown>>>([]);

const itemsInternal = computed(() => (Array.isArray(props.items) ? props.items : []));
const browseKindInternal = computed(() => normalizePlazaBrowseKind(props.browseKind));
const selectedItemIdInternal = computed(() => String(props.selectedItemId || '').trim());
const selectedItem = computed<PlazaItem | null>(
  () => itemsInternal.value.find((item) => item.item_id === selectedItemIdInternal.value) || null
);
const detailDialogVisible = computed({
  get: () => Boolean(selectedItem.value),
  set: (visible: boolean) => {
    if (!visible) {
      closeDetail();
    }
  }
});
const hasSearchKeyword = computed(() => Boolean(String(panelKeyword.value || '').trim()));
const currentBrowseDescription = computed(() => {
  if (browseKindInternal.value === 'worker_card') {
    return t('plaza.page.workerCardDesc');
  }
  if (browseKindInternal.value === 'skill_pack') {
    return t('plaza.page.skillPackDesc');
  }
  return t('plaza.page.hivePackDesc');
});
const filteredFeedItems = computed(() => filterPlazaItemsByKeyword(itemsInternal.value, panelKeyword.value));
const pageCount = computed(() => resolvePlazaPageCount(filteredFeedItems.value.length, PAGE_SIZE));
const currentPageInternal = computed(() => clampPlazaPage(currentPage.value, filteredFeedItems.value.length, PAGE_SIZE));
const pagedFeedItems = computed(() => paginatePlazaItems(filteredFeedItems.value, currentPageInternal.value, PAGE_SIZE));

const ownedAgentOptions = computed<SourceOption[]>(() =>
  (Array.isArray(agentStore.agents) ? agentStore.agents : [])
    .filter((agent) => isPublishableOwnedAgent(agent))
    .map((agent) => {
      const id = String(agent?.id || '').trim();
      const title = String(agent?.name || id).trim() || id;
      return {
        value: id,
        label: title,
        title,
        summary: String(agent?.description || '').trim()
      };
    })
);

const swarmOptions = computed<SourceOption[]>(() =>
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : [])
    .filter((group) => isPublishableBeeroomGroup(group))
    .map((group) => {
      const groupId = String(group?.group_id || group?.hive_id || '').trim();
      const title = String(group?.name || groupId).trim() || groupId;
      return {
        value: groupId,
        label: title,
        title,
        summary: String(group?.description || group?.mother_agent_name || '').trim()
      };
    })
);

const skillOptions = computed<SourceOption[]>(() =>
  customSkills.value
    .map((skill) => {
      const title = String(skill?.name || '').trim();
      return {
        value: title,
        label: title,
        title,
        summary: String(skill?.description || '').trim()
      };
    })
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

const loadPublishSources = async () => {
  try {
    await Promise.allSettled([beeroomStore.loadGroups(), agentStore.loadAgents()]);
    const { data } = await fetchUserSkills();
    const skills = Array.isArray(data?.data?.skills) ? data.data.skills : [];
    customSkills.value = skills.filter((item) => String(item?.source || '').trim() === 'custom');
  } catch (error) {
    showApiError(error, t('plaza.publish.loadSkillsFailed'));
  }
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
    ElMessage.success(t('common.refreshSuccess'));
  } catch (error) {
    showApiError(error, t('plaza.loadFailed'));
  }
};

const openPublishDialog = async () => {
  publishDialogVisible.value = true;
  publishForm.title = '';
  publishForm.summary = '';
  publishForm.kind = browseKindInternal.value;
  await loadPublishSources();
  syncPublishSource();
};

const selectItem = (itemId: string) => {
  emit('update:selectedItemId', String(itemId || '').trim());
};

const closeDetail = () => {
  emit('update:selectedItemId', '');
};

const goPrevPage = () => {
  currentPage.value = Math.max(1, currentPageInternal.value - 1);
};

const goNextPage = () => {
  currentPage.value = Math.min(pageCount.value, currentPageInternal.value + 1);
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
    if (published?.item_id && publishForm.kind === browseKindInternal.value) {
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
    await plazaStore.deleteItem(selectedItem.value.item_id);
    closeDetail();
    ElMessage.success(t('plaza.delete.success'));
  } catch (error) {
    showApiError(error, t('plaza.delete.failed'));
  }
};

const resolveKindLabel = (kind: string) => t(`plaza.kind.${kind}`);

const resolvePlazaAvatarImageUrl = (item: PlazaItem | null | undefined) =>
  item?.kind === 'hive_pack' ? hivePackAvatarImageUrl : '';

const resolvePlazaAvatarIcon = (item: PlazaItem | null | undefined) =>
  item?.kind === 'hive_pack' ? '' : item?.icon;

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

const resolveFreshnessNotice = (item: PlazaItem | null | undefined) => {
  const status = String(item?.freshness_status || 'current').trim();
  if (status === 'outdated') {
    return item?.mine ? t('plaza.freshness.outdatedMine') : t('plaza.freshness.outdatedRemote');
  }
  if (status === 'source_missing') {
    return item?.mine
      ? t('plaza.freshness.sourceMissingMine')
      : t('plaza.freshness.sourceMissingRemote');
  }
  return '';
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
    void plazaStore.loadItems({ force: plazaStore.items.length === 0 });
  },
  { immediate: true }
);

watch(panelKeyword, () => {
  currentPage.value = 1;
});

watch(
  () => browseKindInternal.value,
  () => {
    panelKeyword.value = '';
    currentPage.value = 1;
    closeDetail();
  }
);

watch(
  () => filteredFeedItems.value.length,
  (length) => {
    currentPage.value = clampPlazaPage(currentPage.value, length, PAGE_SIZE);
  }
);

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
    void plazaStore.loadItems({ force: plazaStore.items.length === 0 });
  }
});

defineExpose({
  openPublishDialog,
  reload
});
</script>

<style scoped>
.hive-plaza-panel {
  height: 100%;
  min-height: 100%;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.hive-plaza-banner,
.hive-plaza-feed-shell,
.hive-plaza-detail-shell {
  border-radius: 22px;
  border: 1px solid rgba(15, 23, 42, 0.08);
  background: rgba(255, 255, 255, 0.94);
  box-shadow: 0 14px 28px rgba(15, 23, 42, 0.06);
}

.hive-plaza-banner {
  padding: 12px 14px;
  display: flex;
  align-items: center;
  gap: 10px;
  color: #9f1239;
  background: rgba(255, 241, 242, 0.98);
  border-color: rgba(244, 114, 182, 0.22);
}

.hive-plaza-feed-shell {
  flex: 1;
  min-height: 0;
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.hive-plaza-toolbar {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 14px;
}

.hive-plaza-toolbar-copy {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hive-plaza-toolbar-title {
  font-size: 20px;
  font-weight: 700;
  color: #1f2937;
}

.hive-plaza-toolbar-subtitle {
  color: #64748b;
  line-height: 1.6;
}

.hive-plaza-search {
  min-width: 260px;
  max-width: 360px;
  display: flex;
  align-items: center;
  gap: 10px;
  border-radius: 16px;
  border: 1px solid rgba(223, 161, 72, 0.22);
  background: rgba(255, 250, 240, 0.96);
  padding: 0 14px;
  box-sizing: border-box;
}

.hive-plaza-search i {
  color: #a16207;
}

.hive-plaza-search input {
  width: 100%;
  min-height: 44px;
  border: none;
  outline: none;
  background: transparent;
  color: #1f2937;
  font: inherit;
}

.hive-plaza-search-row {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  min-width: 0;
  flex: 0 1 auto;
}

.hive-plaza-search-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.hive-plaza-toolbar-btn {
  min-height: 44px;
  white-space: nowrap;
}

.hive-plaza-toolbar-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  color: #7a8797;
  font-size: 13px;
}

.hive-plaza-results {
  flex: 1;
  min-height: 0;
  display: flex;
}

.hive-plaza-empty {
  flex: 1;
  min-height: 0;
  border-radius: 22px;
  border: 1px dashed rgba(223, 161, 72, 0.32);
  background: linear-gradient(180deg, rgba(255, 249, 237, 0.92), rgba(255, 255, 255, 0.96));
  display: flex;
  align-items: center;
  justify-content: center;
  flex-direction: column;
  gap: 10px;
  color: #718096;
  padding: 24px;
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
  max-width: 420px;
  text-align: center;
  line-height: 1.7;
}

.hive-plaza-feed-grid {
  width: 100%;
  display: grid;
  grid-template-columns: repeat(auto-fill, 280px);
  grid-auto-rows: 252px;
  gap: 16px;
  align-content: start;
  justify-content: start;
}

.hive-plaza-feed-item {
  width: 280px;
  height: 252px;
  padding: 16px;
  border-radius: 20px;
  border: 1px solid rgba(15, 23, 42, 0.08);
  background: rgba(255, 255, 255, 0.96);
  box-shadow: 0 12px 24px rgba(15, 23, 42, 0.06);
  display: flex;
  flex-direction: column;
  gap: 12px;
  text-align: left;
  cursor: pointer;
  transition: transform 0.18s ease, box-shadow 0.18s ease, border-color 0.18s ease;
}

.hive-plaza-feed-item:hover {
  transform: translateY(-2px);
  border-color: rgba(223, 161, 72, 0.32);
  box-shadow: 0 18px 32px rgba(223, 161, 72, 0.14);
}

.hive-plaza-feed-top,
.hive-plaza-feed-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.hive-plaza-feed-time,
.hive-plaza-feed-foot {
  font-size: 12px;
  color: #7a8797;
}

.hive-plaza-feed-head {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 12px;
  align-items: center;
}

.hive-plaza-feed-copy {
  min-width: 0;
}

.hive-plaza-feed-title {
  display: -webkit-box;
  overflow: hidden;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  font-size: 16px;
  font-weight: 700;
  color: #1f2937;
}

.hive-plaza-feed-owner {
  margin-top: 4px;
  display: -webkit-box;
  overflow: hidden;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  font-size: 13px;
  color: #7a8797;
}

.hive-plaza-feed-summary {
  flex: 1;
  min-height: 0;
  color: #526070;
  line-height: 1.6;
  display: -webkit-box;
  overflow: hidden;
  -webkit-line-clamp: 4;
  -webkit-box-orient: vertical;
}

.hive-plaza-freshness,
.hive-plaza-detail-notice {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 14px;
  font-size: 12px;
  line-height: 1.6;
  border: 1px solid rgba(245, 158, 11, 0.24);
  background: rgba(255, 247, 237, 0.92);
  color: #9a5e11;
}

.hive-plaza-freshness.is-source_missing,
.hive-plaza-detail-notice.is-source_missing {
  border-color: rgba(244, 63, 94, 0.2);
  background: rgba(255, 241, 242, 0.94);
  color: #be123c;
}

.hive-plaza-detail-badges,
.hive-plaza-tag-row,
.hive-plaza-detail-actions,
.hive-plaza-publish-kind-row,
.hive-plaza-dialog-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.hive-plaza-feed-source {
  max-width: 112px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.hive-plaza-kind-chip,
.hive-plaza-tag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 700;
}

.hive-plaza-kind-chip {
  background: rgba(15, 23, 42, 0.06);
  color: #475569;
}

.hive-plaza-kind-chip.is-hive_pack {
  background: rgba(252, 191, 73, 0.2);
  color: #a16207;
}

.hive-plaza-kind-chip.is-worker_card {
  background: rgba(251, 191, 36, 0.16);
  color: #a16207;
}

.hive-plaza-kind-chip.is-skill_pack {
  background: rgba(254, 240, 138, 0.26);
  color: #a16207;
}

.hive-plaza-kind-chip.is-mine,
.hive-plaza-tag {
  background: rgba(234, 179, 8, 0.12);
  color: #925b12;
}

.hive-plaza-pager {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
}

.hive-plaza-pager-summary {
  min-width: 88px;
  text-align: center;
  color: #64748b;
  font-size: 13px;
  font-weight: 600;
}

.hive-plaza-detail-shell {
  padding: 4px;
  border: none;
  background: transparent;
  box-shadow: none;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.hive-plaza-detail-head {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  gap: 16px;
  align-items: center;
}

.hive-plaza-detail-copy {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.hive-plaza-detail-title {
  margin: 0;
  font-size: 24px;
  color: #1f2937;
}

.hive-plaza-detail-summary {
  margin: 0;
  color: #526070;
  line-height: 1.7;
}

.hive-plaza-detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
}

.hive-plaza-detail-meta {
  padding: 14px 16px;
  border-radius: 16px;
  background: rgba(248, 250, 252, 0.92);
  border: 1px solid rgba(148, 163, 184, 0.16);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hive-plaza-detail-label {
  font-size: 12px;
  color: #7a8797;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}

.hive-plaza-detail-value {
  color: #1f2937;
  font-weight: 700;
  word-break: break-word;
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

@media (max-width: 900px) {
  .hive-plaza-toolbar {
    align-items: stretch;
    flex-direction: column;
  }

  .hive-plaza-search {
    min-width: 0;
    max-width: none;
  }

  .hive-plaza-search-row {
    width: 100%;
    flex-direction: column;
    align-items: stretch;
  }

  .hive-plaza-search-actions {
    width: 100%;
    justify-content: flex-end;
    flex-wrap: wrap;
  }

  .hive-plaza-feed-grid {
    grid-template-columns: minmax(0, 1fr);
    grid-auto-rows: auto;
  }

  .hive-plaza-feed-item {
    width: auto;
    height: auto;
    min-height: 240px;
  }

  .hive-plaza-detail-grid,
  .hive-plaza-detail-head {
    grid-template-columns: minmax(0, 1fr);
  }

  .hive-plaza-toolbar-meta,
  .hive-plaza-pager {
    flex-direction: column;
    align-items: stretch;
  }

  .hive-plaza-pager-summary {
    text-align: left;
  }
}
</style>
