<template>
  <div class="agent-memory-panel">
    <div class="agent-memory-sidebar">
      <div class="agent-memory-toolbar">
        <div>
          <div class="agent-memory-title">{{ t('messenger.memory.title') }}</div>
          <div class="agent-memory-subtitle">{{ t('messenger.memory.subtitle') }}</div>
        </div>
        <div class="agent-memory-toolbar-actions">
          <button class="agent-memory-btn" type="button" @click="loadData">{{ t('common.refresh') }}</button>
          <button class="agent-memory-btn agent-memory-btn--primary" type="button" @click="startCreate">
            {{ t('messenger.memory.new') }}
          </button>
        </div>
      </div>

      <div class="agent-memory-filters">
        <input v-model.trim="search" class="agent-memory-input" :placeholder="t('messenger.memory.search')" />
        <select v-model="statusFilter" class="agent-memory-select">
          <option value="">{{ t('messenger.memory.filter.allStatus') }}</option>
          <option value="active">{{ t('messenger.memory.status.active') }}</option>
          <option value="invalidated">{{ t('messenger.memory.status.invalidated') }}</option>
        </select>
        <select v-model="categoryFilter" class="agent-memory-select">
          <option value="">{{ t('messenger.memory.filter.allCategory') }}</option>
          <option v-for="item in categories" :key="item" :value="item">{{ item }}</option>
        </select>
      </div>

      <div class="agent-memory-stats">
        <span>{{ t('messenger.memory.total', { count: filteredItems.length }) }}</span>
        <span>{{ t('messenger.memory.pinned', { count: pinnedCount }) }}</span>
        <span>{{ t('messenger.memory.confirmed', { count: confirmedCount }) }}</span>
      </div>

      <div v-if="errorMessage" class="agent-memory-error">{{ errorMessage }}</div>
      <div v-if="loading" class="agent-memory-empty">{{ t('common.loading') }}</div>
      <div v-else-if="!filteredItems.length" class="agent-memory-empty">{{ t('messenger.memory.empty') }}</div>
      <div v-else class="agent-memory-list">
        <article
          v-for="item in filteredItems"
          :key="item.memory_id"
          class="agent-memory-card"
          :class="{ active: item.memory_id === selectedId }"
          role="button"
          tabindex="0"
          @click="selectItem(item.memory_id)"
          @keydown.enter.prevent="selectItem(item.memory_id)"
          @keydown.space.prevent="selectItem(item.memory_id)"
        >
          <div class="agent-memory-card-head">
            <div class="agent-memory-card-head-main">
              <span class="agent-memory-card-title">{{ item.title_l0 || t('messenger.memory.untitled') }}</span>
              <span v-if="item.pinned" class="agent-memory-chip agent-memory-chip--warn">{{ t('messenger.memory.pinTag') }}</span>
              <span v-if="item.confirmed_by_user" class="agent-memory-chip agent-memory-chip--success">{{ t('messenger.memory.confirmTag') }}</span>
            </div>
            <div class="agent-memory-card-actions">
              <button class="agent-memory-card-action" type="button" @click.stop="beginEdit(item.memory_id)">{{ t('common.edit') }}</button>
              <button class="agent-memory-card-action agent-memory-card-action--danger" type="button" @click.stop="removeMemory(item.memory_id, item.title_l0)">{{ t('common.delete') }}</button>
            </div>
          </div>
          <div class="agent-memory-card-summary">{{ item.summary_l1 || item.content_l2 }}</div>
          <div class="agent-memory-card-meta">
            <span class="agent-memory-chip">{{ item.category || '-' }}</span>
            <span class="agent-memory-chip">{{ formatFragmentSource(item.source_type) }}</span>
            <span class="agent-memory-chip" :class="item.status === 'invalidated' ? 'agent-memory-chip--danger' : ''">{{ item.status || 'active' }}</span>
            <span class="agent-memory-chip">{{ t('messenger.memory.meta.hitCount', { count: Number(item.hit_count || 0) }) }}</span>
          </div>
          <div class="agent-memory-card-actions agent-memory-card-actions--secondary">
            <button class="agent-memory-card-action" type="button" @click.stop="togglePinned(item)">
              {{ item.pinned ? t('messenger.memory.action.unpin') : t('messenger.memory.action.pin') }}
            </button>
            <button class="agent-memory-card-action" type="button" @click.stop="toggleConfirmed(item)">
              {{ item.confirmed_by_user ? t('messenger.memory.action.unconfirm') : t('messenger.memory.action.confirm') }}
            </button>
            <button class="agent-memory-card-action" type="button" @click.stop="toggleInvalidated(item)">
              {{ String(item.status || '') === 'invalidated' || item.invalidated_at ? t('messenger.memory.action.restore') : t('messenger.memory.action.invalidate') }}
            </button>
          </div>
        </article>
      </div>
    </div>

    <div class="agent-memory-detail">
      <div class="agent-memory-editor">
        <div class="agent-memory-editor-head">
          <div>
            <div class="agent-memory-title">{{ isCreating ? t('messenger.memory.new') : t('messenger.memory.detail') }}</div>
            <div class="agent-memory-subtitle">{{ t('messenger.memory.detailSubtitle') }}</div>
          </div>
          <div class="agent-memory-toolbar-actions">
            <button v-if="!isCreating" class="agent-memory-btn" type="button" @click="removeCurrent">
              {{ t('common.delete') }}
            </button>
            <button class="agent-memory-btn agent-memory-btn--primary" type="button" @click="saveCurrent">
              {{ t('common.save') }}
            </button>
          </div>
        </div>

        <label class="agent-memory-field">
          <span>{{ t('messenger.memory.field.title') }}</span>
          <input v-model.trim="editor.title_l0" class="agent-memory-input" />
        </label>
        <label class="agent-memory-field">
          <span>{{ t('messenger.memory.field.summary') }}</span>
          <textarea v-model.trim="editor.summary_l1" class="agent-memory-textarea" rows="4"></textarea>
        </label>
        <label class="agent-memory-field">
          <span>{{ t('messenger.memory.field.content') }}</span>
          <textarea v-model.trim="editor.content_l2" class="agent-memory-textarea" rows="8"></textarea>
        </label>

        <div class="agent-memory-grid">
          <label class="agent-memory-field">
            <span>{{ t('messenger.memory.field.category') }}</span>
            <input v-model.trim="editor.category" class="agent-memory-input" />
          </label>
          <label class="agent-memory-field">
            <span>{{ t('messenger.memory.field.factKey') }}</span>
            <input v-model.trim="editor.fact_key" class="agent-memory-input" />
          </label>
          <label class="agent-memory-field">
            <span>{{ t('messenger.memory.field.tags') }}</span>
            <input v-model.trim="editor.tagsText" class="agent-memory-input" :placeholder="t('messenger.memory.tagsPlaceholder')" />
          </label>
          <label class="agent-memory-field">
            <span>{{ t('messenger.memory.field.entities') }}</span>
            <input v-model.trim="editor.entitiesText" class="agent-memory-input" :placeholder="t('messenger.memory.tagsPlaceholder')" />
          </label>
        </div>

        <div class="agent-memory-toggle-row">
          <label><input v-model="editor.pinned" type="checkbox" /> {{ t('messenger.memory.pinTag') }}</label>
          <label><input v-model="editor.confirmed_by_user" type="checkbox" /> {{ t('messenger.memory.confirmTag') }}</label>
          <label><input v-model="editor.invalidated" type="checkbox" /> {{ t('messenger.memory.invalidateTag') }}</label>
        </div>

        <div v-if="selectedItem" class="agent-memory-meta-row">
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.source') }}: {{ formatFragmentSource(selectedItem.source_type) }}</span>
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.updatedAt') }}: {{ formatFragmentTime(selectedItem.updated_at) }}</span>
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.hitCount', { count: Number(selectedItem.hit_count || 0) }) }}</span>
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.accessCount', { count: Number(selectedItem.access_count || 0) }) }}</span>
        </div>
      </div>

      <div class="agent-memory-hits">
        <div class="agent-memory-title">{{ t('messenger.memory.hitsTitle') }}</div>
        <div v-if="!hits.length" class="agent-memory-empty">{{ t('messenger.memory.hitsEmpty') }}</div>
        <div v-else class="agent-memory-hit-list">
          <article v-for="hit in hits" :key="hit.hit_id" class="agent-memory-hit-card">
            <div class="agent-memory-hit-head">
              <strong>{{ resolveHitTitle(hit.memory_id) }}</strong>
              <span>{{ formatHitTime(hit.created_at) }}</span>
            </div>
            <div class="agent-memory-hit-reason">{{ formatHitReason(hit.reason_json) }}</div>
            <div class="agent-memory-hit-score">
              score {{ formatScore(hit.final_score) }} · lexical {{ formatScore(hit.lexical_score) }}
            </div>
          </article>
        </div>
      </div>

      <div class="agent-memory-hits">
        <div class="agent-memory-title">{{ t('messenger.memory.jobsTitle') }}</div>
        <div v-if="!jobs.length" class="agent-memory-empty">{{ t('messenger.memory.jobsEmpty') }}</div>
        <div v-else class="agent-memory-hit-list">
          <article v-for="job in jobs" :key="job.job_id" class="agent-memory-hit-card">
            <div class="agent-memory-hit-head">
              <strong>{{ formatJobType(job.job_type) }}</strong>
              <span class="agent-memory-chip" :class="formatJobStatusClass(job.status)">{{ formatJobStatus(job.status) }}</span>
            </div>
            <div class="agent-memory-hit-reason">{{ formatJobSummary(job) }}</div>
            <div class="agent-memory-hit-score">
              {{ t('messenger.memory.jobSession', { sessionId: job.session_id || '-' }) }} · {{ t('messenger.memory.jobUpdatedAt', { time: formatFragmentTime(job.updated_at) }) }}
            </div>
          </article>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { ElMessage, ElMessageBox } from 'element-plus';

import {
  confirmAgentMemory,
  createAgentMemory,
  deleteAgentMemory,
  invalidateAgentMemory,
  listAgentMemories,
  pinAgentMemory,
  updateAgentMemory
} from '@/api/memory';
import { useI18n } from '@/i18n';

type MemoryItem = Record<string, any>;
type MemoryHit = Record<string, any>;
type MemoryJob = Record<string, any>;
type EditorState = {
  title_l0: string;
  summary_l1: string;
  content_l2: string;
  category: string;
  fact_key: string;
  tagsText: string;
  entitiesText: string;
  pinned: boolean;
  confirmed_by_user: boolean;
  invalidated: boolean;
};

const props = defineProps<{ agentId: string }>();
const { t } = useI18n();
const loading = ref(false);
const errorMessage = ref('');
const items = ref<MemoryItem[]>([]);
const hits = ref<MemoryHit[]>([]);
const jobs = ref<MemoryJob[]>([]);
const search = ref('');
const categoryFilter = ref('');
const statusFilter = ref('');
const selectedId = ref('');
const editor = ref<EditorState>(createEmptyEditor());
const mounted = ref(false);
let disposed = false;
let requestToken = 0;

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const filteredItems = computed(() => {
  const query = search.value.trim().toLowerCase();
  return items.value.filter((item) => {
    if (categoryFilter.value && item.category !== categoryFilter.value) return false;
    if (statusFilter.value && item.status !== statusFilter.value) return false;
    if (!query) return true;
    return [item.title_l0, item.summary_l1, item.content_l2, item.fact_key, ...(item.tags || [])]
      .join(' ')
      .toLowerCase()
      .includes(query);
  });
});
const categories = computed(() => [...new Set(items.value.map((item) => String(item.category || '').trim()).filter(Boolean))]);
const pinnedCount = computed(() => items.value.filter((item) => item.pinned).length);
const confirmedCount = computed(() => items.value.filter((item) => item.confirmed_by_user).length);
const isCreating = computed(() => !selectedId.value);
const selectedItem = computed(() => pickSelected());
const memoryTitleMap = computed(() => {
  const entries = items.value.map((item) => [String(item.memory_id || '').trim(), String(item.title_l0 || '')] as const);
  return new Map(entries.filter(([memoryId]) => Boolean(memoryId)));
});

function createEmptyEditor(): EditorState {
  return {
    title_l0: '',
    summary_l1: '',
    content_l2: '',
    category: 'session_summary',
    fact_key: '',
    tagsText: '',
    entitiesText: '',
    pinned: false,
    confirmed_by_user: false,
    invalidated: false
  };
}

function pickSelected(): MemoryItem | null {
  return items.value.find((item) => item.memory_id === selectedId.value) || null;
}

function syncEditorFromSelected(): void {
  const item = pickSelected();
  if (!item) {
    editor.value = createEmptyEditor();
    return;
  }
  editor.value = {
    title_l0: String(item.title_l0 || ''),
    summary_l1: String(item.summary_l1 || ''),
    content_l2: String(item.content_l2 || ''),
    category: String(item.category || 'session_summary'),
    fact_key: String(item.fact_key || ''),
    tagsText: Array.isArray(item.tags) ? item.tags.join(', ') : '',
    entitiesText: Array.isArray(item.entities) ? item.entities.join(', ') : '',
    pinned: Boolean(item.pinned),
    confirmed_by_user: Boolean(item.confirmed_by_user),
    invalidated: Boolean(item.invalidated_at) || String(item.status || '') === 'invalidated'
  };
}

async function loadData(): Promise<void> {
  if (!normalizedAgentId.value) return;
  const token = ++requestToken;
  loading.value = true;
  errorMessage.value = '';
  try {
    const memoryRes = await listAgentMemories(normalizedAgentId.value, { limit: 200 });
    if (disposed || token !== requestToken) return;
    items.value = Array.isArray(memoryRes?.data?.data?.items) ? memoryRes.data.data.items : [];
    hits.value = Array.isArray(memoryRes?.data?.data?.recent_hits) ? memoryRes.data.data.recent_hits : [];
    jobs.value = Array.isArray(memoryRes?.data?.data?.recent_jobs) ? memoryRes.data.data.recent_jobs : [];
    if (!selectedId.value || !items.value.some((item) => item.memory_id === selectedId.value)) {
      selectedId.value = items.value[0]?.memory_id || '';
    }
    syncEditorFromSelected();
  } catch (error: any) {
    if (disposed || token !== requestToken) return;
    errorMessage.value = String(error?.response?.data?.error || error?.message || t('common.loadFailed'));
  } finally {
    if (!disposed && token === requestToken) loading.value = false;
  }
}

function startCreate(): void {
  selectedId.value = '';
  editor.value = createEmptyEditor();
}

function selectItem(memoryId: string): void {
  selectedId.value = memoryId;
  syncEditorFromSelected();
}

function getMemoryTitle(item: MemoryItem | null | undefined): string {
  return String(item?.title_l0 || t('messenger.memory.untitled'));
}

function isActionCanceled(error: unknown): boolean {
  return error === 'cancel' || error === 'close' || error === 'dismiss';
}

function resolveRequestError(error: any, fallbackKey: string): string {
  return String(error?.response?.data?.error || error?.message || t(fallbackKey));
}

function beginEdit(memoryId: string): void {
  selectItem(memoryId);
}

function splitTags(value: string): string[] {
  return value
    .split(/[，,\n]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

async function saveCurrent(): Promise<void> {
  if (!normalizedAgentId.value) return;
  const payload = {
    title_l0: editor.value.title_l0,
    summary_l1: editor.value.summary_l1,
    content_l2: editor.value.content_l2,
    category: editor.value.category,
    fact_key: editor.value.fact_key,
    tags: splitTags(editor.value.tagsText),
    entities: splitTags(editor.value.entitiesText),
    pinned: editor.value.pinned,
    confirmed_by_user: editor.value.confirmed_by_user,
    invalidated: editor.value.invalidated
  };
  try {
    const response = selectedId.value
      ? await updateAgentMemory(normalizedAgentId.value, selectedId.value, payload)
      : await createAgentMemory(normalizedAgentId.value, payload);
    selectedId.value = String(response?.data?.data?.item?.memory_id || selectedId.value || '');
    await loadData();
    ElMessage.success(t('messenger.memory.saveSuccess'));
  } catch (error: any) {
    errorMessage.value = resolveRequestError(error, 'common.saveFailed');
  }
}

async function removeMemory(memoryId: string, title?: unknown): Promise<void> {
  if (!normalizedAgentId.value || !memoryId) return;
  try {
    await ElMessageBox.confirm(
      t('messenger.memory.deleteConfirm', { name: String(title || t('messenger.memory.untitled')) }),
      t('common.notice'),
      { type: 'warning' }
    );
    await deleteAgentMemory(normalizedAgentId.value, memoryId);
    if (selectedId.value === memoryId) {
      selectedId.value = '';
    }
    await loadData();
    ElMessage.success(t('messenger.memory.deleteSuccess'));
  } catch (error: any) {
    if (isActionCanceled(error)) return;
    errorMessage.value = resolveRequestError(error, 'common.deleteFailed');
  }
}

async function togglePinned(item: MemoryItem): Promise<void> {
  if (!normalizedAgentId.value) return;
  try {
    await pinAgentMemory(normalizedAgentId.value, item.memory_id, !Boolean(item.pinned));
    await loadData();
    ElMessage.success(t('messenger.memory.updateSuccess'));
  } catch (error: any) {
    errorMessage.value = resolveRequestError(error, 'common.saveFailed');
  }
}

async function toggleConfirmed(item: MemoryItem): Promise<void> {
  if (!normalizedAgentId.value) return;
  try {
    await confirmAgentMemory(normalizedAgentId.value, item.memory_id, !Boolean(item.confirmed_by_user));
    await loadData();
    ElMessage.success(t('messenger.memory.updateSuccess'));
  } catch (error: any) {
    errorMessage.value = resolveRequestError(error, 'common.saveFailed');
  }
}

async function toggleInvalidated(item: MemoryItem): Promise<void> {
  if (!normalizedAgentId.value) return;
  const nextValue = !(Boolean(item.invalidated_at) || String(item.status || '') === 'invalidated');
  try {
    await invalidateAgentMemory(normalizedAgentId.value, item.memory_id, nextValue);
    await loadData();
    ElMessage.success(t('messenger.memory.updateSuccess'));
  } catch (error: any) {
    errorMessage.value = resolveRequestError(error, 'common.saveFailed');
  }
}

async function removeCurrent(): Promise<void> {
  if (!selectedId.value) return;
  await removeMemory(selectedId.value, getMemoryTitle(selectedItem.value));
}

function formatScore(value: unknown): string {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric.toFixed(2) : '0.00';
}

function formatHitTime(value: unknown): string {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return '-';
  return new Date(numeric * 1000).toLocaleString();
}

function formatFragmentSource(value: unknown): string {
  const source = String(value || '').trim();
  if (!source) return 'manual';
  const normalizedSource = source.replace(/-/g, '_');
  const key = `messenger.memory.sourceType.${normalizedSource}`;
  const translated = t(key);
  return translated === key ? source.replace(/[-_]/g, ' ') : translated;
}

function formatFragmentTime(value: unknown): string {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return '-';
  return new Date(numeric * 1000).toLocaleString();
}

function resolveHitTitle(memoryId: unknown): string {
  const normalizedId = String(memoryId || '').trim();
  return String(memoryTitleMap.value.get(normalizedId) || normalizedId || t('messenger.memory.untitled'));
}

function formatHitReason(reason: any): string {
  const terms = Array.isArray(reason?.matched_terms) ? reason.matched_terms.filter(Boolean).join(', ') : '';
  if (terms) {
    return t('messenger.memory.hitMatched', { terms });
  }
  return t('messenger.memory.hitRecent');
}

function formatJobType(value: unknown): string {
  const jobType = String(value || '').trim();
  if (!jobType) return '-';
  const key = `messenger.memory.jobType.${jobType.replace(/-/g, '_')}`;
  const translated = t(key);
  return translated === key ? jobType.replace(/[-_]/g, ' ') : translated;
}

function formatJobStatus(value: unknown): string {
  const status = String(value || '').trim();
  if (!status) return '-';
  const key = `messenger.memory.jobStatus.${status.replace(/-/g, '_')}`;
  const translated = t(key);
  return translated === key ? status.replace(/[-_]/g, ' ') : translated;
}

function formatJobStatusClass(value: unknown): string {
  const status = String(value || '').trim();
  if (status === 'completed') return 'agent-memory-chip--success';
  if (status === 'failed') return 'agent-memory-chip--danger';
  if (status === 'running') return 'agent-memory-chip--warn';
  return '';
}

function formatJobSummary(job: MemoryJob): string {
  return String(job?.result_summary || job?.error_message || t('messenger.memory.jobSummaryEmpty'));
}

onMounted(() => {
  mounted.value = true;
  void loadData();
});

onBeforeUnmount(() => {
  disposed = true;
});

watch(
  () => props.agentId,
  (value, previousValue) => {
    if (!mounted.value || disposed || String(value || '') === String(previousValue || '')) return;
    selectedId.value = '';
    editor.value = createEmptyEditor();
    void loadData();
  }
);
</script>

<style scoped>
.agent-memory-panel {
  display: grid;
  grid-template-columns: minmax(320px, 420px) minmax(0, 1fr);
  gap: 20px;
  min-height: 620px;
}
.agent-memory-sidebar,
.agent-memory-detail,
.agent-memory-editor,
.agent-memory-hits,
.agent-memory-card,
.agent-memory-hit-card {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
  border-radius: 18px;
}
.agent-memory-sidebar,
.agent-memory-detail {
  padding: 16px;
}
.agent-memory-toolbar,
.agent-memory-editor-head,
.agent-memory-card-head,
.agent-memory-hit-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.agent-memory-toolbar-actions,
.agent-memory-toggle-row,
.agent-memory-card-meta,
.agent-memory-stats,
.agent-memory-meta-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.agent-memory-title {
  font-size: 16px;
  font-weight: 700;
}
.agent-memory-subtitle,
.agent-memory-hit-score,
.agent-memory-stats {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
}
.agent-memory-filters,
.agent-memory-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px;
  margin: 14px 0;
}
.agent-memory-list,
.agent-memory-hit-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 14px;
}
.agent-memory-card {
  width: 100%;
  padding: 14px;
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.agent-memory-card:focus-visible {
  outline: 2px solid var(--app-primary-color, #3b82f6);
  outline-offset: 2px;
}
.agent-memory-card.active {
  border-color: var(--app-primary-color, #3b82f6);
  box-shadow: 0 0 0 1px var(--app-primary-color, #3b82f6) inset;
}
.agent-memory-card-head {
  align-items: flex-start;
}
.agent-memory-card-head-main {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  min-width: 0;
}
.agent-memory-card-actions {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  flex-wrap: wrap;
}
.agent-memory-card-actions--secondary {
  padding-top: 4px;
  border-top: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.16));
}
.agent-memory-card-action {
  border: 0;
  background: transparent;
  color: var(--app-primary-color, #3b82f6);
  padding: 0;
  font-size: 12px;
  cursor: pointer;
}
.agent-memory-card-action--danger {
  color: #b91c1c;
}
.agent-memory-card-title {
  font-weight: 700;
}
.agent-memory-card-summary,
.agent-memory-hit-reason {
  margin-top: 8px;
  font-size: 13px;
  color: var(--app-text-color, #e2e8f0);
  line-height: 1.6;
}
.agent-memory-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 14px;
}
.agent-memory-input,
.agent-memory-select,
.agent-memory-textarea {
  width: 100%;
  border-radius: 12px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: var(--app-surface-bg, rgba(255, 255, 255, 0.9));
  color: inherit;
  padding: 10px 12px;
}
.agent-memory-textarea {
  resize: vertical;
}
.agent-memory-btn {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: transparent;
  color: inherit;
  border-radius: 12px;
  padding: 8px 12px;
  cursor: pointer;
}
.agent-memory-btn--primary {
  background: var(--app-primary-color, #3b82f6);
  color: #fff;
  border-color: var(--app-primary-color, #3b82f6);
}
.agent-memory-chip {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 12px;
  background: rgba(148, 163, 184, 0.12);
}
.agent-memory-chip--warn { background: rgba(245, 158, 11, 0.16); color: #b45309; }
.agent-memory-chip--success { background: rgba(34, 197, 94, 0.16); color: #15803d; }
.agent-memory-chip--danger { background: rgba(239, 68, 68, 0.16); color: #b91c1c; }
.agent-memory-error,
.agent-memory-empty {
  margin-top: 14px;
  padding: 12px;
  border-radius: 12px;
  background: rgba(148, 163, 184, 0.08);
  color: var(--app-text-muted, #64748b);
}
.agent-memory-error {
  background: rgba(239, 68, 68, 0.12);
  color: #b91c1c;
}
.agent-memory-editor,
.agent-memory-hits {
  padding: 16px;
}
.agent-memory-hits {
  margin-top: 16px;
}
.agent-memory-hit-card {
  padding: 12px;
}
@media (max-width: 1280px) {
  .agent-memory-panel {
    grid-template-columns: 1fr;
  }
}
</style>
