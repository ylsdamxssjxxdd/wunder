<template>
  <div class="agent-memory-panel">
    <div class="agent-memory-toolbar">
      <div>
        <div class="agent-memory-title">{{ t('messenger.memory.title') }}</div>
        <div class="agent-memory-subtitle">{{ t('messenger.memory.subtitle') }}</div>
      </div>
      <div class="agent-memory-toolbar-actions">
        <button class="agent-memory-btn" type="button" :disabled="loading || saving || mutating" @click="loadData">
          {{ t('common.refresh') }}
        </button>
        <button
          class="agent-memory-icon-btn"
          type="button"
          :disabled="loading"
          :title="t('messenger.memory.hitsTitle')"
          :aria-label="t('messenger.memory.hitsTitle')"
          @click="hitsDialogVisible = true"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M10.5 4a6.5 6.5 0 1 0 4.07 11.57l3.93 3.93 1.41-1.41-3.93-3.93A6.5 6.5 0 0 0 10.5 4Zm0 2a4.5 4.5 0 1 1 0 9 4.5 4.5 0 0 1 0-9Zm7.5 1h2v2h-2V7Zm-1 4h3v2h-3v-2Zm-2 4h5v2h-5v-2Z"
            />
          </svg>
        </button>
        <button
          class="agent-memory-icon-btn"
          type="button"
          :disabled="loading"
          :title="t('messenger.memory.jobsTitle')"
          :aria-label="t('messenger.memory.jobsTitle')"
          @click="jobsDialogVisible = true"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path
              d="M7 3h10v3h3v14a2 2 0 0 1-2 2H7a3 3 0 0 1-3-3V6a3 3 0 0 1 3-3Zm8 1.5H7A1.5 1.5 0 0 0 5.5 6v13A1.5 1.5 0 0 0 7 20.5h11a.5.5 0 0 0 .5-.5V7.5H15V4.5Zm-6 5h6v1.75H9V9.5Zm0 4h6v1.75H9V13.5Zm0-8h4v1.75H9V5.5Z"
            />
          </svg>
        </button>
        <button
          class="agent-memory-btn agent-memory-btn--primary"
          type="button"
          :disabled="saving || mutating"
          @click="startCreate"
        >
          {{ t('messenger.memory.new') }}
        </button>
      </div>
    </div>

    <div class="agent-memory-overview">
      <div class="agent-memory-overview-main">
        <div class="agent-memory-overview-count">{{ filteredItems.length }}/{{ items.length }}</div>
        <div class="agent-memory-overview-label">{{ t('messenger.memory.visibleLabel') }}</div>
      </div>
      <div class="agent-memory-overview-stats">
        <div class="agent-memory-overview-item">
          <span>{{ t('messenger.memory.status.active') }}</span>
          <strong>{{ activeCount }}</strong>
        </div>
        <div class="agent-memory-overview-item">
          <span>{{ t('messenger.memory.status.superseded') }}</span>
          <strong>{{ supersededCount }}</strong>
        </div>
        <div class="agent-memory-overview-item">
          <span>{{ t('messenger.memory.status.invalidated') }}</span>
          <strong>{{ invalidatedCount }}</strong>
        </div>
        <div class="agent-memory-overview-item">
          <span>{{ t('messenger.memory.pinnedLabel') }}</span>
          <strong>{{ pinnedCount }}</strong>
        </div>
      </div>
      <div class="agent-memory-overview-note">{{ formatFragmentSource('auto_turn') }} {{ autoExtractedCount }}</div>
    </div>

    <div class="agent-memory-filters">
      <input v-model.trim="search" class="agent-memory-input agent-memory-search" :placeholder="t('messenger.memory.search')" />
      <select v-model="statusFilter" class="agent-memory-select">
        <option value="">{{ t('messenger.memory.filter.allStatus') }}</option>
        <option value="active">{{ t('messenger.memory.status.active') }}</option>
        <option value="superseded">{{ t('messenger.memory.status.superseded') }}</option>
        <option value="invalidated">{{ t('messenger.memory.status.invalidated') }}</option>
      </select>
      <select v-model="categoryFilter" class="agent-memory-select">
        <option value="">{{ t('messenger.memory.filter.allCategory') }}</option>
        <option v-for="item in categories" :key="item" :value="item">{{ item }}</option>
      </select>
    </div>

    <div v-if="errorMessage" class="agent-memory-error">{{ errorMessage }}</div>
    <div v-if="loading" class="agent-memory-empty">{{ t('common.loading') }}</div>
    <div v-else-if="!filteredItems.length" class="agent-memory-empty">{{ t('messenger.memory.empty') }}</div>
    <div v-else class="agent-memory-card-grid">
      <article
        v-for="item in filteredItems"
        :key="item.memory_id"
        class="agent-memory-card"
        :class="{
          'agent-memory-card--invalidated': isItemInvalidated(item),
          'agent-memory-card--superseded': isItemSuperseded(item)
        }"
        role="button"
        tabindex="0"
        @click="beginEdit(item.memory_id)"
        @keydown.enter.prevent="beginEdit(item.memory_id)"
        @keydown.space.prevent="beginEdit(item.memory_id)"
      >
        <div class="agent-memory-card-topline">
          <div class="agent-memory-card-topline-chips">
            <span v-if="item.pinned" class="agent-memory-chip agent-memory-chip--warn">{{ t('messenger.memory.pinTag') }}</span>
            <span v-if="isItemSuperseded(item)" class="agent-memory-chip">{{ t('messenger.memory.status.superseded') }}</span>
            <span v-else-if="isItemInvalidated(item)" class="agent-memory-chip agent-memory-chip--danger">
              {{ t('messenger.memory.invalidateTag') }}
            </span>
            <span class="agent-memory-chip">{{ item.category || '-' }}</span>
          </div>
          <span class="agent-memory-card-time">{{ formatFragmentTime(item.updated_at) }}</span>
        </div>

        <div class="agent-memory-card-head">
          <div class="agent-memory-card-head-main">
            <span class="agent-memory-card-title">{{ getMemoryTitle(item) }}</span>
          </div>
        </div>

        <div class="agent-memory-card-summary">{{ getMemoryPreview(item) }}</div>

        <div v-if="hasTaxonomy(item)" class="agent-memory-card-section-chips">
          <span v-for="tag in previewList(item.tags, 2)" :key="`${item.memory_id}-tag-${tag}`" class="agent-memory-chip">
            {{ tag }}
          </span>
          <span v-for="entity in previewList(item.entities, 2)" :key="`${item.memory_id}-entity-${entity}`" class="agent-memory-chip">
            {{ entity }}
          </span>
          <span
            v-if="remainingListCount(item.tags, 2) + remainingListCount(item.entities, 2)"
            class="agent-memory-chip"
          >
            +{{ remainingListCount(item.tags, 2) + remainingListCount(item.entities, 2) }}
          </span>
        </div>

        <div class="agent-memory-card-meta">
          <span class="agent-memory-card-meta-item">{{ formatFragmentSource(item.source_type) }}</span>
          <span class="agent-memory-card-meta-item">{{ t('messenger.memory.meta.hitCount', { count: Number(item.hit_count || 0) }) }}</span>
          <span v-if="describeItemRelation(item)" class="agent-memory-card-meta-item">{{ describeItemRelation(item) }}</span>
        </div>

        <div class="agent-memory-card-actions">
          <button class="agent-memory-card-action" type="button" :disabled="mutating || saving" @click.stop="togglePinned(item)">
            {{ item.pinned ? t('messenger.memory.action.unpin') : t('messenger.memory.action.pin') }}
          </button>
          <button
            class="agent-memory-card-action"
            type="button"
            :disabled="mutating || saving"
            @click.stop="toggleInvalidated(item)"
          >
            {{ isItemInvalidated(item) ? t('messenger.memory.action.restore') : t('messenger.memory.action.invalidate') }}
          </button>
          <button
            class="agent-memory-card-action agent-memory-card-action--danger"
            type="button"
            :disabled="mutating || saving"
            @click.stop="removeMemory(item.memory_id, item.title_l0)"
          >
            {{ t('common.delete') }}
          </button>
        </div>
      </article>
    </div>

    <el-dialog
      v-model="hitsDialogVisible"
      :title="t('messenger.memory.hitsTitle')"
      width="680px"
      top="6vh"
      append-to-body
      destroy-on-close
      class="messenger-dialog agent-memory-dialog agent-memory-dialog--insight"
    >
      <div v-if="!hits.length" class="agent-memory-empty agent-memory-empty--compact">{{ t('messenger.memory.hitsEmpty') }}</div>
      <div v-else class="agent-memory-hit-list agent-memory-hit-list--dialog">
        <article v-for="hit in hits" :key="hit.hit_id" class="agent-memory-hit-card">
          <div class="agent-memory-hit-head">
            <strong>{{ resolveHitTitle(hit.memory_id) }}</strong>
            <span>{{ formatHitTime(hit.created_at) }}</span>
          </div>
          <div class="agent-memory-hit-reason">{{ formatHitReason(hit.reason_json) }}</div>
          <div class="agent-memory-hit-score">{{ formatHitScoreLine(hit) }}</div>
        </article>
      </div>
    </el-dialog>

    <el-dialog
      v-model="jobsDialogVisible"
      :title="t('messenger.memory.jobsTitle')"
      width="680px"
      top="6vh"
      append-to-body
      destroy-on-close
      class="messenger-dialog agent-memory-dialog agent-memory-dialog--insight"
    >
      <div class="agent-memory-auto-extract-panel">
        <div class="agent-memory-auto-extract-copy">
          <div class="agent-memory-auto-extract-title">{{ t('messenger.memory.autoExtract.title') }}</div>
          <div class="agent-memory-auto-extract-hint">{{ t('messenger.memory.autoExtract.hint') }}</div>
        </div>
        <div class="agent-memory-auto-extract-switch">
          <span class="agent-memory-auto-extract-label">{{ t('messenger.memory.autoExtract.enable') }}</span>
          <el-switch
            :model-value="autoExtractEnabled"
            :loading="autoExtractSaving"
            :disabled="loading || autoExtractSaving"
            @change="handleAutoExtractChange"
          />
        </div>
      </div>
      <div class="agent-memory-dialog-divider"></div>
      <div v-if="!jobs.length" class="agent-memory-empty agent-memory-empty--compact">{{ t('messenger.memory.jobsEmpty') }}</div>
      <div v-else class="agent-memory-hit-list agent-memory-hit-list--dialog">
        <article v-for="job in jobs" :key="job.job_id" class="agent-memory-hit-card">
          <div class="agent-memory-hit-head">
            <strong>{{ formatJobType(job.job_type) }}</strong>
            <span class="agent-memory-chip" :class="formatJobStatusClass(job.status)">{{ formatJobStatus(job.status) }}</span>
          </div>
          <div class="agent-memory-hit-reason">{{ formatJobSummary(job) }}</div>
          <div class="agent-memory-hit-score">{{ formatJobMeta(job) }}</div>
        </article>
      </div>
    </el-dialog>

    <el-dialog
      v-model="dialogVisible"
      :title="dialogTitle"
      width="760px"
      top="4vh"
      append-to-body
      destroy-on-close
      class="messenger-dialog agent-memory-dialog"
      :close-on-click-modal="!saving"
      :close-on-press-escape="!saving"
      @closed="handleDialogClosed"
    >
      <div class="agent-memory-dialog-body">
        <div v-if="currentEditingItem" class="agent-memory-meta-row">
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.source') }}: {{ formatFragmentSource(currentEditingItem.source_type) }}</span>
          <span class="agent-memory-chip">{{ describeItemStatus(currentEditingItem) }}</span>
          <span class="agent-memory-chip">{{ describeItemTier(currentEditingItem) }}</span>
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.updatedAt') }}: {{ formatFragmentTime(currentEditingItem.updated_at) }}</span>
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.hitCount', { count: Number(currentEditingItem.hit_count || 0) }) }}</span>
        </div>
        <div v-if="currentEditingItem && describeItemRelation(currentEditingItem)" class="agent-memory-meta-row">
          <span class="agent-memory-chip">{{ describeItemRelation(currentEditingItem) }}</span>
        </div>
        <div v-if="dialogErrorMessage" class="agent-memory-error agent-memory-error--dialog">
          {{ dialogErrorMessage }}
        </div>

        <div class="agent-memory-editor-hint">{{ t('messenger.memory.editorHint') }}</div>

        <div class="agent-memory-grid agent-memory-grid--editor">
          <div class="agent-memory-field agent-memory-field--full">
            <label>{{ t('messenger.memory.field.title') }}</label>
            <input
              v-model.trim="editor.title_l0"
              class="agent-memory-input"
              :placeholder="t('messenger.memory.placeholder.title')"
            />
          </div>
          <div class="agent-memory-field agent-memory-field--full">
            <label>{{ t('messenger.memory.field.summary') }}</label>
            <textarea
              v-model.trim="editor.summary_l1"
              class="agent-memory-textarea"
              rows="4"
              :placeholder="t('messenger.memory.placeholder.summary')"
            ></textarea>
          </div>
          <div class="agent-memory-field agent-memory-field--full">
            <label>{{ t('messenger.memory.field.content') }}</label>
            <textarea
              v-model.trim="editor.content_l2"
              class="agent-memory-textarea"
              rows="8"
              :placeholder="t('messenger.memory.placeholder.content')"
            ></textarea>
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.category') }}</label>
            <input v-model.trim="editor.category" class="agent-memory-input" />
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.tags') }}</label>
            <input v-model.trim="editor.tagsText" class="agent-memory-input" :placeholder="t('messenger.memory.tagsPlaceholder')" />
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.entities') }}</label>
            <input v-model.trim="editor.entitiesText" class="agent-memory-input" :placeholder="t('messenger.memory.tagsPlaceholder')" />
          </div>
        </div>

        <div class="agent-memory-toggle-row agent-memory-toggle-row--dialog">
          <label><input v-model="editor.pinned" type="checkbox" /> {{ t('messenger.memory.pinTag') }}</label>
          <label><input v-model="editor.invalidated" type="checkbox" /> {{ t('messenger.memory.invalidateTag') }}</label>
        </div>
      </div>

      <template #footer>
        <div class="agent-memory-dialog-footer">
          <button
            v-if="editingId"
            class="agent-memory-btn agent-memory-btn--danger"
            type="button"
            :disabled="saving"
            @click="removeCurrent"
          >
            {{ t('common.delete') }}
          </button>
          <div class="agent-memory-dialog-footer-actions">
            <button class="agent-memory-btn" type="button" :disabled="saving" @click="dialogVisible = false">
              {{ t('common.cancel') }}
            </button>
            <button class="agent-memory-btn agent-memory-btn--primary" type="button" :disabled="saving" @click="saveCurrent">
              {{ saving ? t('common.loading') : t('common.save') }}
            </button>
          </div>
        </div>
      </template>
    </el-dialog>
  </div>
</template>
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import {
  createAgentMemory,
  deleteAgentMemory,
  invalidateAgentMemory,
  listAgentMemories,
  pinAgentMemory,
  updateAgentMemorySettings,
  updateAgentMemory
} from '@/api/memory';
import { useI18n } from '@/i18n';
import { resolveApiError } from '@/utils/apiError';
type MemoryItem = Record<string, any>;
type MemoryHit = Record<string, any>;
type MemoryJob = Record<string, any>;
type EditorState = {
  title_l0: string;
  summary_l1: string;
  content_l2: string;
  category: string;
  tagsText: string;
  entitiesText: string;
  pinned: boolean;
  invalidated: boolean;
};
const props = defineProps<{ agentId: string }>();
const { t } = useI18n();
const loading = ref(false);
const saving = ref(false);
const mutating = ref(false);
const errorMessage = ref('');
const dialogErrorMessage = ref('');
const items = ref<MemoryItem[]>([]);
const hits = ref<MemoryHit[]>([]);
const jobs = ref<MemoryJob[]>([]);
const search = ref('');
const categoryFilter = ref('');
const statusFilter = ref('');
const dialogVisible = ref(false);
const hitsDialogVisible = ref(false);
const jobsDialogVisible = ref(false);
const autoExtractEnabled = ref(false);
const autoExtractSaving = ref(false);
const editingId = ref('');
const editor = ref<EditorState>(createEmptyEditor());
const mounted = ref(false);
let disposed = false;
let requestToken = 0;
const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const requestAgentId = computed(() => normalizedAgentId.value || '__default__');
const filteredItems = computed(() => {
  const query = search.value.trim().toLowerCase();
  return items.value.filter((item) => {
    if (categoryFilter.value && item.category !== categoryFilter.value) return false;
    if (statusFilter.value && String(item.status || '') !== statusFilter.value) return false;
    if (!query) return true;
    return [
      item.title_l0,
      item.summary_l1,
      item.content_l2,
      item.fact_key,
      item.category,
      ...(item.tags || []),
      ...(item.entities || [])
    ]
      .join(' ')
      .toLowerCase()
      .includes(query);
  });
});
const categories = computed(() => [...new Set(items.value.map((item) => String(item.category || '').trim()).filter(Boolean))]);
const pinnedCount = computed(() => items.value.filter((item) => item.pinned).length);
const supersededCount = computed(() => items.value.filter((item) => isItemSuperseded(item)).length);
const invalidatedCount = computed(() => items.value.filter((item) => isItemInvalidated(item)).length);
const activeCount = computed(() => items.value.filter((item) => isItemActive(item)).length);
const autoExtractedCount = computed(() => items.value.filter((item) => normalizeSourceType(item.source_type) === 'auto_turn').length);
const currentEditingItem = computed(() => items.value.find((item) => item.memory_id === editingId.value) || null);
const dialogTitle = computed(() => (editingId.value ? `${t('common.edit')} - ${t('messenger.memory.title')}` : t('messenger.memory.new')));
const memoryTitleMap = computed(() => {
  const entries = items.value.map((item) => [String(item.memory_id || '').trim(), String(item.title_l0 || '')] as const);
  return new Map(entries.filter(([memoryId]) => Boolean(memoryId)));
});
function createEmptyEditor(): EditorState {
  return {
    title_l0: '',
    summary_l1: '',
    content_l2: '',
    category: 'tool-note',
    tagsText: '',
    entitiesText: '',
    pinned: false,
    invalidated: false
  };
}
function normalizeSourceType(value: unknown): string {
  return String(value || '').trim().replace(/-/g, '_') || 'manual';
}
function isItemSuperseded(item: MemoryItem | null | undefined): boolean {
  return String(item?.status || '') === 'superseded' || Boolean(item?.superseded_by_memory_id);
}
function isItemInvalidated(item: MemoryItem | null | undefined): boolean {
  return Boolean(item?.invalidated_at) || String(item?.status || '') === 'invalidated';
}
function isItemActive(item: MemoryItem | null | undefined): boolean {
  return !isItemInvalidated(item) && !isItemSuperseded(item);
}
function describeItemStatus(item: MemoryItem | null | undefined): string {
  if (isItemInvalidated(item)) return t('messenger.memory.status.invalidated');
  if (isItemSuperseded(item)) return t('messenger.memory.status.superseded');
  return t('messenger.memory.status.active');
}
function describeItemTier(item: MemoryItem | null | undefined): string {
  const tier = String(item?.tier || '').trim() || 'working';
  const key = `messenger.memory.tier.${tier}`;
  const translated = t(key);
  return translated === key ? tier : translated;
}
function resolveMemoryLabel(memoryId: unknown): string {
  const normalizedId = String(memoryId || '').trim();
  return String(memoryTitleMap.value.get(normalizedId) || normalizedId || t('messenger.memory.untitled'));
}
function describeItemRelation(item: MemoryItem | null | undefined): string {
  const supersededById = String(item?.superseded_by_memory_id || '').trim();
  if (supersededById) {
    return t('messenger.memory.meta.replacedBy', { name: resolveMemoryLabel(supersededById) });
  }
  const supersedesId = String(item?.supersedes_memory_id || '').trim();
  if (supersedesId) {
    return t('messenger.memory.meta.replaces', { name: resolveMemoryLabel(supersedesId) });
  }
  return '';
}
function normalizeStringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => String(item || '').trim()).filter(Boolean);
}
function previewList(value: unknown, limit = 3): string[] {
  return normalizeStringList(value).slice(0, limit);
}
function remainingListCount(value: unknown, limit = 3): number {
  return Math.max(normalizeStringList(value).length - limit, 0);
}
function hasTaxonomy(item: MemoryItem | null | undefined): boolean {
  return previewList(item?.tags).length > 0 || previewList(item?.entities).length > 0;
}
function syncEditorFromItem(item: MemoryItem | null | undefined): void {
  if (!item) {
    editor.value = createEmptyEditor();
    return;
  }
  editor.value = {
    title_l0: String(item.title_l0 || ''),
    summary_l1: String(item.summary_l1 || ''),
    content_l2: String(item.content_l2 || ''),
    category: String(item.category || 'tool-note'),
    tagsText: normalizeStringList(item.tags).join(', '),
    entitiesText: normalizeStringList(item.entities).join(', '),
    pinned: Boolean(item.pinned),
    invalidated: isItemInvalidated(item)
  };
}
async function loadData(): Promise<void> {
  const token = ++requestToken;
  loading.value = true;
  errorMessage.value = '';
  try {
    // Always include invalidated fragments so the card wall can review and restore them.
    const memoryRes = await listAgentMemories(requestAgentId.value, { limit: 200, include_invalidated: true });
    if (disposed || token !== requestToken) return;
    items.value = Array.isArray(memoryRes?.data?.data?.items) ? memoryRes.data.data.items : [];
    hits.value = Array.isArray(memoryRes?.data?.data?.recent_hits) ? memoryRes.data.data.recent_hits : [];
    jobs.value = Array.isArray(memoryRes?.data?.data?.recent_jobs) ? memoryRes.data.data.recent_jobs : [];
    autoExtractEnabled.value = Boolean(memoryRes?.data?.data?.settings?.auto_extract_enabled);
    if (editingId.value) {
      syncEditorFromItem(items.value.find((item) => item.memory_id === editingId.value) || null);
    }
  } catch (error: any) {
    if (disposed || token !== requestToken) return;
    errorMessage.value = resolveRequestError(error, 'common.loadFailed');
  } finally {
    if (!disposed && token === requestToken) loading.value = false;
  }
}
function startCreate(): void {
  errorMessage.value = '';
  dialogErrorMessage.value = '';
  editingId.value = '';
  editor.value = createEmptyEditor();
  dialogVisible.value = true;
}
function beginEdit(memoryId: string): void {
  errorMessage.value = '';
  dialogErrorMessage.value = '';
  editingId.value = memoryId;
  syncEditorFromItem(items.value.find((item) => item.memory_id === memoryId) || null);
  dialogVisible.value = true;
}
function handleDialogClosed(): void {
  dialogErrorMessage.value = '';
  saving.value = false;
  editingId.value = '';
  editor.value = createEmptyEditor();
}
function getMemoryTitle(item: MemoryItem | null | undefined): string {
  return String(item?.title_l0 || t('messenger.memory.untitled'));
}
function getMemoryPreview(item: MemoryItem | null | undefined): string {
  const title = getMemoryTitle(item);
  const summary = String(item?.summary_l1 || '').trim();
  const content = String(item?.content_l2 || '').trim();
  if (summary && summary !== title) return summary;
  if (content && content !== title) return content;
  return title;
}
function isActionCanceled(error: unknown): boolean {
  return error === 'cancel' || error === 'close' || error === 'dismiss';
}
function resolveRequestError(error: any, fallbackKey: string): string {
  return resolveApiError(error, t(fallbackKey)).message;
}
function splitTags(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}
async function handleAutoExtractChange(value: string | number | boolean): Promise<void> {
  if (autoExtractSaving.value) return;
  const nextValue = Boolean(value);
  const previousValue = autoExtractEnabled.value;
  autoExtractEnabled.value = nextValue;
  errorMessage.value = '';
  try {
    autoExtractSaving.value = true;
    const response = await updateAgentMemorySettings(requestAgentId.value, {
      auto_extract_enabled: nextValue
    });
    if (disposed) return;
    autoExtractEnabled.value = Boolean(response?.data?.data?.settings?.auto_extract_enabled);
    ElMessage.success(t('messenger.memory.autoExtract.saveSuccess'));
  } catch (error: any) {
    autoExtractEnabled.value = previousValue;
    const message = resolveRequestError(error, 'common.saveFailed');
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    if (!disposed) autoExtractSaving.value = false;
  }
}
function hasEditorContent(): boolean {
  return [editor.value.title_l0, editor.value.summary_l1, editor.value.content_l2].some(
    (value) => String(value || '').trim().length > 0
  );
}
async function saveCurrent(): Promise<void> {
  if (saving.value) return;
  dialogErrorMessage.value = '';
  errorMessage.value = '';
  if (!hasEditorContent()) {
    const message = t('error.content_required');
    dialogErrorMessage.value = message;
    ElMessage.warning(message);
    return;
  }
  const payload = {
    title_l0: editor.value.title_l0,
    summary_l1: editor.value.summary_l1,
    content_l2: editor.value.content_l2,
    category: editor.value.category,
    tags: splitTags(editor.value.tagsText),
    entities: splitTags(editor.value.entitiesText),
    pinned: editor.value.pinned,
    invalidated: editor.value.invalidated
  };
  saving.value = true;
  try {
    const response = editingId.value
      ? await updateAgentMemory(requestAgentId.value, editingId.value, payload)
      : await createAgentMemory(requestAgentId.value, payload);
    editingId.value = String(response?.data?.data?.item?.memory_id || editingId.value || '');
    dialogVisible.value = false;
    await loadData();
    ElMessage.success(t('messenger.memory.saveSuccess'));
  } catch (error: any) {
    const message = resolveRequestError(error, 'common.saveFailed');
    dialogErrorMessage.value = message;
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    saving.value = false;
  }
}
async function removeMemory(memoryId: string, title?: unknown): Promise<void> {
  if (mutating.value || saving.value || !memoryId) return;
  try {
    await ElMessageBox.confirm(
      t('messenger.memory.deleteConfirm', { name: String(title || t('messenger.memory.untitled')) }),
      t('common.notice'),
      { type: 'warning' }
    );
    mutating.value = true;
    await deleteAgentMemory(requestAgentId.value, memoryId);
    if (editingId.value === memoryId) {
      dialogVisible.value = false;
    }
    await loadData();
    ElMessage.success(t('messenger.memory.deleteSuccess'));
  } catch (error: any) {
    if (isActionCanceled(error)) return;
    const message = resolveRequestError(error, 'common.deleteFailed');
    dialogErrorMessage.value = message;
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    mutating.value = false;
  }
}
async function togglePinned(item: MemoryItem): Promise<void> {
  if (mutating.value || saving.value) return;
  errorMessage.value = '';
  try {
    mutating.value = true;
    await pinAgentMemory(requestAgentId.value, item.memory_id, !Boolean(item.pinned));
    await loadData();
    ElMessage.success(t('messenger.memory.updateSuccess'));
  } catch (error: any) {
    const message = resolveRequestError(error, 'common.saveFailed');
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    mutating.value = false;
  }
}
async function toggleInvalidated(item: MemoryItem): Promise<void> {
  if (mutating.value || saving.value) return;
  errorMessage.value = '';
  try {
    mutating.value = true;
    await invalidateAgentMemory(requestAgentId.value, item.memory_id, !isItemInvalidated(item));
    await loadData();
    ElMessage.success(t('messenger.memory.updateSuccess'));
  } catch (error: any) {
    const message = resolveRequestError(error, 'common.saveFailed');
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    mutating.value = false;
  }
}
async function removeCurrent(): Promise<void> {
  if (!editingId.value) return;
  await removeMemory(editingId.value, getMemoryTitle(currentEditingItem.value));
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
  return resolveMemoryLabel(memoryId);
}
function formatHitField(value: unknown): string {
  const field = String(value || '').trim();
  if (!field) return '';
  const key = `messenger.memory.hitField.${field}`;
  const translated = t(key);
  return translated === key ? field : translated;
}
function formatHitReason(reason: any): string {
  const terms = Array.isArray(reason?.matched_terms) ? reason.matched_terms.filter(Boolean).join(', ') : '';
  const fields = Array.isArray(reason?.matched_fields)
    ? reason.matched_fields.map((item: unknown) => formatHitField(item)).filter(Boolean).join(' / ')
    : '';
  if (terms && fields) {
    return t('messenger.memory.hitMatchedWithFields', { fields, terms });
  }
  if (terms) {
    return t('messenger.memory.hitMatched', { terms });
  }
  if (fields) {
    return t('messenger.memory.hitMatchedFields', { fields });
  }
  if (reason?.pinned) {
    return t('messenger.memory.hitPinned');
  }
  return t('messenger.memory.hitRecent');
}
function formatHitScoreLine(hit: MemoryHit): string {
  const semantic = Number(hit?.semantic_score || 0);
  if (semantic > 0) {
    return t('messenger.memory.hitScoreHybrid', {
      score: formatScore(hit?.final_score),
      lexical: formatScore(hit?.lexical_score),
      semantic: formatScore(hit?.semantic_score)
    });
  }
  return t('messenger.memory.hitScore', {
    score: formatScore(hit?.final_score),
    lexical: formatScore(hit?.lexical_score)
  });
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
function formatJobMeta(job: MemoryJob): string {
  return [
    t('messenger.memory.jobSession', { sessionId: job?.session_id || '-' }),
    t('messenger.memory.jobUpdatedAt', { time: formatFragmentTime(job?.updated_at) })
  ].join(' ? ');
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
    dialogVisible.value = false;
    hitsDialogVisible.value = false;
    jobsDialogVisible.value = false;
    dialogErrorMessage.value = '';
    saving.value = false;
    mutating.value = false;
    editingId.value = '';
    editor.value = createEmptyEditor();
    items.value = [];
    hits.value = [];
    jobs.value = [];
    autoExtractEnabled.value = false;
    autoExtractSaving.value = false;
    void loadData();
  }
);
</script>
<style scoped>
.agent-memory-panel {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.agent-memory-toolbar,
.agent-memory-card-head,
.agent-memory-card-topline,
.agent-memory-hit-head,
.agent-memory-dialog-footer,
.agent-memory-insight-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.agent-memory-toolbar-actions,
.agent-memory-card-topline-chips,
.agent-memory-card-meta,
.agent-memory-card-actions,
.agent-memory-toggle-row,
.agent-memory-meta-row,
.agent-memory-dialog-footer-actions,
.agent-memory-card-section-chips {
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
.agent-memory-card-time {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
}
.agent-memory-overview {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 16px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.2));
  border-radius: 18px;
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
}
.agent-memory-overview-main {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.agent-memory-overview-count {
  font-size: 30px;
  line-height: 1;
  font-weight: 800;
}
.agent-memory-overview-label,
.agent-memory-overview-note {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
}
.agent-memory-overview-stats {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.agent-memory-overview-item {
  display: inline-flex;
  align-items: baseline;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 12px;
  background: rgba(148, 163, 184, 0.08);
}
.agent-memory-overview-item strong {
  font-size: 15px;
}
.agent-memory-auto-extract-panel {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 16px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.22));
  border-radius: 16px;
  background: rgba(59, 130, 246, 0.04);
}
.agent-memory-auto-extract-copy {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}
.agent-memory-auto-extract-title {
  font-size: 14px;
  font-weight: 700;
}
.agent-memory-auto-extract-hint,
.agent-memory-auto-extract-label {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
  line-height: 1.6;
}
.agent-memory-auto-extract-switch {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}
.agent-memory-dialog-divider {
  height: 1px;
  margin: 14px 0 16px;
  background: var(--app-border-color, rgba(148, 163, 184, 0.16));
}
.agent-memory-icon-btn {
  width: 40px;
  height: 40px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: transparent;
  color: inherit;
  cursor: pointer;
  transition: border-color 0.16s ease, background-color 0.16s ease, transform 0.16s ease, opacity 0.16s ease;
}
.agent-memory-icon-btn svg {
  width: 18px;
  height: 18px;
  fill: currentColor;
}
.agent-memory-icon-btn:hover:not(:disabled) {
  border-color: var(--app-primary-color, #3b82f6);
  background: rgba(59, 130, 246, 0.08);
}
.agent-memory-icon-btn:active:not(:disabled) {
  transform: translateY(1px);
}
.agent-memory-icon-btn:disabled {
  opacity: 0.58;
  cursor: not-allowed;
}
.agent-memory-filters {
  display: grid;
  grid-template-columns: minmax(0, 1.6fr) repeat(2, minmax(180px, 0.7fr));
  gap: 12px;
}
.agent-memory-search {
  grid-column: 1 / 2;
}
.agent-memory-card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 14px;
}
.agent-memory-insight-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}
.agent-memory-insight-panel,
.agent-memory-card,
.agent-memory-hit-card {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
  border-radius: 18px;
}
.agent-memory-insight-panel {
  padding: 16px;
}
.agent-memory-card {
  padding: 16px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 220px;
  transition: border-color 0.18s ease, transform 0.18s ease, box-shadow 0.18s ease;
}
.agent-memory-card--invalidated {
  opacity: 0.86;
}
.agent-memory-card--superseded {
  border-style: dashed;
}
.agent-memory-card:hover {
  transform: translateY(-1px);
  border-color: var(--app-primary-color, #3b82f6);
  box-shadow: 0 12px 24px rgba(15, 23, 42, 0.08);
}
.agent-memory-card:focus-visible {
  outline: 2px solid var(--app-primary-color, #3b82f6);
  outline-offset: 2px;
}
.agent-memory-card-topline,
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
.agent-memory-card-title {
  font-weight: 700;
  line-height: 1.5;
  font-size: 15px;
}
.agent-memory-card-summary,
.agent-memory-hit-reason {
  font-size: 13px;
  color: var(--app-text-color, #0f172a);
  line-height: 1.7;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
.agent-memory-card-meta-item {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
}
.agent-memory-card-taxonomy {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.agent-memory-card-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.agent-memory-card-section-label {
  font-size: 12px;
  color: var(--app-text-muted, #64748b);
}
.agent-memory-card-actions {
  margin-top: auto;
  padding-top: 10px;
  border-top: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.16));
}
.agent-memory-card-action {
  border: 0;
  background: transparent;
  color: var(--app-primary-color, #3b82f6);
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
  border-radius: 10px;
  transition: background-color 0.16s ease, color 0.16s ease, transform 0.16s ease, opacity 0.16s ease;
}
.agent-memory-card-action:hover:not(:disabled) {
  background: rgba(59, 130, 246, 0.1);
}
.agent-memory-card-action:active:not(:disabled) {
  transform: translateY(1px);
}
.agent-memory-card-action:disabled {
  opacity: 0.52;
  cursor: not-allowed;
}
.agent-memory-card-action--danger:hover:not(:disabled) {
  background: rgba(239, 68, 68, 0.12);
}
.agent-memory-card-action--danger,
.agent-memory-btn--danger {
  color: #b91c1c;
}
.agent-memory-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.agent-memory-field--full {
  grid-column: 1 / -1;
}
.agent-memory-grid--editor {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 14px;
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
  transition: border-color 0.16s ease, box-shadow 0.16s ease, background-color 0.16s ease;
}
.agent-memory-input:focus,
.agent-memory-select:focus,
.agent-memory-textarea:focus {
  outline: none;
  border-color: var(--app-primary-color, #3b82f6);
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.14);
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
  transition: border-color 0.16s ease, background-color 0.16s ease, color 0.16s ease, box-shadow 0.16s ease,
    transform 0.16s ease, opacity 0.16s ease;
}
.agent-memory-btn:hover:not(:disabled) {
  border-color: var(--app-primary-color, #3b82f6);
  background: rgba(59, 130, 246, 0.08);
}
.agent-memory-btn:active:not(:disabled) {
  transform: translateY(1px);
}
.agent-memory-btn:disabled {
  opacity: 0.58;
  cursor: not-allowed;
  box-shadow: none;
}
.agent-memory-btn--primary {
  background: var(--app-primary-color, #3b82f6);
  color: #fff;
  border-color: var(--app-primary-color, #3b82f6);
}
.agent-memory-btn--primary:hover:not(:disabled) {
  background: var(--app-primary-color, #3b82f6);
  box-shadow: 0 10px 22px rgba(59, 130, 246, 0.22);
}
.agent-memory-chip {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 12px;
  background: rgba(148, 163, 184, 0.12);
}
.agent-memory-chip--stat {
  padding: 6px 10px;
}
.agent-memory-chip--mono {
  font-family: 'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace;
}
.agent-memory-chip--warn {
  background: rgba(245, 158, 11, 0.16);
  color: #b45309;
}
.agent-memory-chip--success {
  background: rgba(34, 197, 94, 0.16);
  color: #15803d;
}
.agent-memory-chip--danger {
  background: rgba(239, 68, 68, 0.16);
  color: #b91c1c;
}
.agent-memory-empty,
.agent-memory-error {
  padding: 12px;
  border-radius: 12px;
  background: rgba(148, 163, 184, 0.08);
  color: var(--app-text-muted, #64748b);
}
.agent-memory-empty--compact {
  margin-top: 12px;
}
.agent-memory-error {
  background: rgba(239, 68, 68, 0.12);
  color: #b91c1c;
}
.agent-memory-error--dialog {
  margin-bottom: 4px;
}
.agent-memory-hit-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 14px;
}
.agent-memory-hit-list--dialog {
  max-height: min(60vh, 640px);
  overflow-y: auto;
  margin-top: 0;
  padding-right: 4px;
}
.agent-memory-hit-card {
  padding: 12px;
}
.agent-memory-editor-hint {
  padding: 10px 12px;
  border-radius: 12px;
  background: rgba(59, 130, 246, 0.08);
  color: var(--app-text-muted, #475569);
  font-size: 12px;
  line-height: 1.6;
}
.agent-memory-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-height: calc(100vh - 220px);
  overflow-y: auto;
  padding: 2px 4px 2px 0;
}
.agent-memory-toggle-row--dialog {
  padding-top: 6px;
}
.agent-memory-dialog-footer {
  width: 100%;
  padding-top: 14px;
  border-top: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.18));
  align-items: flex-end;
}
:deep(.agent-memory-dialog.el-dialog) {
  width: min(720px, calc(100vw - 24px));
  max-width: calc(100vw - 24px);
  max-height: calc(100vh - 32px);
  margin: 24px auto 8px;
  overflow: hidden;
}
:deep(.agent-memory-dialog .el-dialog__body) {
  padding-top: 16px;
}
:deep(.agent-memory-dialog .el-dialog__footer) {
  padding-top: 0;
}
@media (max-width: 1100px) {
  .agent-memory-filters,
  .agent-memory-grid--editor {
    grid-template-columns: 1fr;
  }
  .agent-memory-overview {
    flex-direction: column;
    align-items: flex-start;
  }
  .agent-memory-search,
  .agent-memory-field--full {
    grid-column: auto;
  }
}
</style>
