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
          class="agent-memory-btn"
          type="button"
          :disabled="loading || saving || mutating || !items.length"
          @click="openReplicateDialog"
        >
          {{ t('messenger.memory.replicate') }}
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

    <div class="agent-memory-filters">
      <input
        v-model.trim="search"
        class="agent-memory-input agent-memory-search"
        :placeholder="t('messenger.memory.search')"
      />
      <select v-model="tagFilter" class="agent-memory-select">
        <option value="">{{ t('messenger.memory.filter.allTag') }}</option>
        <option v-for="item in tags" :key="item" :value="item">{{ item }}</option>
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
        :class="{ 'agent-memory-card--superseded': isItemSuperseded(item) }"
        role="button"
        tabindex="0"
        @click="beginEdit(item.memory_id)"
        @keydown.enter.prevent="beginEdit(item.memory_id)"
        @keydown.space.prevent="beginEdit(item.memory_id)"
      >
        <div class="agent-memory-card-topline">
          <div class="agent-memory-card-topline-chips">
            <span class="agent-memory-chip">{{ resolveMemoryTag(item) || '-' }}</span>
            <span v-if="isItemSuperseded(item)" class="agent-memory-chip">{{ t('messenger.memory.status.superseded') }}</span>
          </div>
          <span class="agent-memory-card-time">{{ formatFragmentTime(item.updated_at) }}</span>
        </div>

        <div class="agent-memory-card-head">
          <div class="agent-memory-card-head-main">
            <span class="agent-memory-card-title">{{ getMemoryTitle(item) }}</span>
          </div>
        </div>

        <div class="agent-memory-card-summary">{{ getMemoryPreview(item) }}</div>

        <div class="agent-memory-card-meta">
          <span class="agent-memory-card-meta-item">{{ formatFragmentSource(item.source_type) }}</span>
          <span v-if="describeItemRelation(item)" class="agent-memory-card-meta-item">{{ describeItemRelation(item) }}</span>
        </div>
      </article>
    </div>

    <el-dialog
      v-model="replicateDialogVisible"
      :title="t('messenger.memory.replicateTitle')"
      width="560px"
      top="8vh"
      append-to-body
      destroy-on-close
      class="messenger-dialog agent-memory-dialog"
      :close-on-click-modal="!replicateSaving"
      :close-on-press-escape="!replicateSaving"
      @closed="handleReplicateDialogClosed"
    >
      <div class="agent-memory-dialog-body">
        <div class="agent-memory-editor-hint">{{ t('messenger.memory.replicateHint') }}</div>
        <div v-if="replicateErrorMessage" class="agent-memory-error agent-memory-error--dialog">
          {{ replicateErrorMessage }}
        </div>

        <div class="agent-memory-field">
          <label>{{ t('messenger.memory.replicateTarget') }}</label>
          <select v-model="replicateTargetId" class="agent-memory-select" :disabled="replicateLoading || replicateSaving">
            <option value="">{{ t('messenger.memory.replicatePlaceholder') }}</option>
            <option v-for="target in replicateTargets" :key="target.id" :value="target.id">
              {{ target.name }}
            </option>
          </select>
        </div>

        <div v-if="replicateLoading" class="agent-memory-empty agent-memory-empty--compact">{{ t('common.loading') }}</div>
        <div v-else-if="!replicateTargets.length" class="agent-memory-empty agent-memory-empty--compact">
          {{ t('messenger.memory.replicateNoAgents') }}
        </div>
      </div>

      <template #footer>
        <div class="agent-memory-dialog-footer agent-memory-dialog-footer--end">
          <div class="agent-memory-dialog-footer-actions">
            <button class="agent-memory-btn" type="button" :disabled="replicateSaving" @click="replicateDialogVisible = false">
              {{ t('common.cancel') }}
            </button>
            <button
              class="agent-memory-btn agent-memory-btn--primary"
              type="button"
              :disabled="replicateSaving || replicateLoading || !replicateTargets.length"
              @click="submitReplicate"
            >
              {{ replicateSaving ? t('common.loading') : t('messenger.memory.replicate') }}
            </button>
          </div>
        </div>
      </template>
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
          <span class="agent-memory-chip">{{ t('messenger.memory.meta.updatedAt') }}: {{ formatFragmentTime(currentEditingItem.updated_at) }}</span>
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
            <label>{{ t('messenger.memory.field.indexTitle') }}</label>
            <input
              v-model.trim="editor.title_l0"
              class="agent-memory-input"
              :placeholder="t('messenger.memory.placeholder.title')"
            />
          </div>
          <div class="agent-memory-field agent-memory-field--full">
            <label>{{ t('messenger.memory.field.contentDetail') }}</label>
            <textarea
              v-model.trim="editor.content_l2"
              class="agent-memory-textarea"
              rows="9"
              :placeholder="t('messenger.memory.placeholder.content')"
            ></textarea>
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.memoryTag') }}</label>
            <input v-model.trim="editor.tag" class="agent-memory-input" />
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.relatedMemoryId') }}</label>
            <input
              v-model.trim="editor.relatedMemoryId"
              class="agent-memory-input"
              :placeholder="t('messenger.memory.placeholder.relatedMemoryId')"
            />
          </div>
          <div class="agent-memory-field">
            <label>{{ t('messenger.memory.field.memoryTime') }}</label>
            <input v-model="editor.validFromText" class="agent-memory-input" type="datetime-local" />
          </div>
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
import { createAgentMemory, deleteAgentMemory, listAgentMemories, replicateAgentMemories, updateAgentMemory } from '@/api/memory';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { resolveApiError } from '@/utils/apiError';

type MemoryItem = Record<string, any>;

type EditorState = {
  title_l0: string;
  content_l2: string;
  tag: string;
  relatedMemoryId: string;
  validFromText: string;
};

type MigrationTarget = {
  id: string;
  name: string;
};

const props = withDefaults(
  defineProps<{
    agentId: string;
    active?: boolean;
  }>(),
  {
    active: true
  }
);

const { t } = useI18n();
const agentStore = useAgentStore();
const isPanelActive = computed(() => props.active !== false);
const loading = ref(false);
const saving = ref(false);
const mutating = ref(false);
const replicateLoading = ref(false);
const replicateSaving = ref(false);
const errorMessage = ref('');
const dialogErrorMessage = ref('');
const replicateErrorMessage = ref('');
const items = ref<MemoryItem[]>([]);
const search = ref('');
const tagFilter = ref('');
const dialogVisible = ref(false);
const replicateDialogVisible = ref(false);
const replicateTargetId = ref('');
const editingId = ref('');
const editor = ref<EditorState>(createEmptyEditor());
const mounted = ref(false);

let disposed = false;
let requestToken = 0;
let lastLoadedAgentKey = '';

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const requestAgentId = computed(() => normalizedAgentId.value || '__default__');
const currentEditingItem = computed(() => items.value.find((item) => item.memory_id === editingId.value) || null);
const dialogTitle = computed(() => (editingId.value ? `${t('common.edit')} - ${t('messenger.memory.title')}` : t('messenger.memory.new')));

const tags = computed(() =>
  [...new Set(items.value.map((item) => resolveMemoryTag(item)).filter(Boolean))]
);

const memoryTitleMap = computed(() => {
  const entries = items.value.map((item) => [String(item.memory_id || '').trim(), String(item.title_l0 || '')] as const);
  return new Map(entries.filter(([memoryId]) => Boolean(memoryId)));
});

const filteredItems = computed(() => {
  const query = search.value.trim().toLowerCase();
  return items.value
    .filter((item) => {
      if (tagFilter.value && resolveMemoryTag(item) !== tagFilter.value) return false;
      if (!query) return true;
      return [
        item.memory_id,
        item.title_l0,
        item.content_l2,
        resolveMemoryTag(item),
        item.supersedes_memory_id,
        item.superseded_by_memory_id
      ]
        .join(' ')
        .toLowerCase()
        .includes(query);
    })
    .slice()
    .sort((left, right) => {
      const updatedDiff = Number(right?.updated_at || 0) - Number(left?.updated_at || 0);
      if (updatedDiff !== 0) return updatedDiff;
      return String(left?.memory_id || '').localeCompare(String(right?.memory_id || ''));
    });
});

const replicateTargets = computed<MigrationTarget[]>(() => {
  const byId = new Map<string, MigrationTarget>();
  const collect = (records: unknown[]) => {
    records.forEach((item) => {
      if (!item || typeof item !== 'object') return;
      const record = item as Record<string, unknown>;
      const id = String(record.id || '').trim();
      if (!id || id === '__default__' || id === requestAgentId.value) return;
      byId.set(id, {
        id,
        name: String(record.name || id).trim() || id
      });
    });
  };
  collect(Array.isArray(agentStore.agents) ? agentStore.agents : []);
  collect(Array.isArray(agentStore.sharedAgents) ? agentStore.sharedAgents : []);
  return Array.from(byId.values()).sort((left, right) => left.name.localeCompare(right.name));
});

function createEmptyEditor(): EditorState {
  return {
    title_l0: '',
    content_l2: '',
    tag: 'tool-note',
    relatedMemoryId: '',
    validFromText: ''
  };
}

function isItemSuperseded(item: MemoryItem | null | undefined): boolean {
  return String(item?.status || '') === 'superseded' || Boolean(item?.superseded_by_memory_id);
}

function describeItemStatus(item: MemoryItem | null | undefined): string {
  if (isItemSuperseded(item)) return t('messenger.memory.status.superseded');
  return t('messenger.memory.status.active');
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

function syncEditorFromItem(item: MemoryItem | null | undefined): void {
  if (!item) {
    editor.value = createEmptyEditor();
    return;
  }
  editor.value = {
    title_l0: String(item.title_l0 || ''),
    content_l2: String(item.content_l2 || ''),
    tag: resolveMemoryTag(item) || 'tool-note',
    relatedMemoryId: String(item.supersedes_memory_id || ''),
    validFromText: formatDateTimeLocalInput(item.valid_from)
  };
}

async function loadData(): Promise<void> {
  const token = ++requestToken;
  loading.value = true;
  errorMessage.value = '';
  try {
    const memoryRes = await listAgentMemories(requestAgentId.value, { limit: 200 });
    if (disposed || token !== requestToken) return;
    items.value = Array.isArray(memoryRes?.data?.data?.items) ? memoryRes.data.data.items : [];
    lastLoadedAgentKey = requestAgentId.value;
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

function handleReplicateDialogClosed(): void {
  replicateErrorMessage.value = '';
  replicateSaving.value = false;
  replicateTargetId.value = '';
}

function resolveMemoryTag(item: MemoryItem | null | undefined): string {
  return String(item?.tag || item?.category || '').trim();
}

function getMemoryTitle(item: MemoryItem | null | undefined): string {
  return String(item?.title_l0 || t('messenger.memory.untitled'));
}

function getMemoryPreview(item: MemoryItem | null | undefined): string {
  const content = String(item?.content_l2 || '').replace(/\s+/g, ' ').trim();
  if (!content) return getMemoryTitle(item);
  return content.length > 140 ? `${content.slice(0, 140)}...` : content;
}

function isActionCanceled(error: unknown): boolean {
  return error === 'cancel' || error === 'close' || error === 'dismiss';
}

function resolveRequestError(error: any, fallbackKey: string): string {
  return resolveApiError(error, t(fallbackKey)).message;
}

function formatDateTimeLocalInput(value: unknown): string {
  const numeric = Number(value);
  if (!Number.isFinite(numeric) || numeric <= 0) return '';
  const local = new Date(numeric * 1000 - new Date().getTimezoneOffset() * 60_000);
  return local.toISOString().slice(0, 16);
}

function parseDateTimeLocalInput(value: string): number | null {
  const text = String(value || '').trim();
  if (!text) return null;
  const numeric = new Date(text).getTime();
  if (!Number.isFinite(numeric) || numeric <= 0) return null;
  return Math.floor(numeric / 1000);
}

async function saveCurrent(): Promise<void> {
  if (saving.value) return;
  dialogErrorMessage.value = '';
  errorMessage.value = '';
  if (!String(editor.value.content_l2 || '').trim()) {
    const message = t('error.content_required');
    dialogErrorMessage.value = message;
    ElMessage.warning(message);
    return;
  }

  const payload = {
    title_l0: String(editor.value.title_l0 || '').trim() || undefined,
    content_l2: String(editor.value.content_l2 || '').trim(),
    tag: String(editor.value.tag || '').trim() || undefined,
    supersedes_memory_id: String(editor.value.relatedMemoryId || '').trim() || undefined,
    valid_from: parseDateTimeLocalInput(editor.value.validFromText)
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

async function removeCurrent(): Promise<void> {
  if (!editingId.value) return;
  await removeMemory(editingId.value, getMemoryTitle(currentEditingItem.value));
}

async function openReplicateDialog(): Promise<void> {
  if (!items.value.length) {
    ElMessage.warning(t('messenger.memory.replicateNoSource'));
    return;
  }
  replicateErrorMessage.value = '';
  replicateLoading.value = true;
  try {
    await agentStore.loadAgents();
    replicateTargetId.value = replicateTargets.value[0]?.id || '';
    replicateDialogVisible.value = true;
  } catch (error: any) {
    replicateErrorMessage.value = resolveRequestError(error, 'common.loadFailed');
    ElMessage.error(replicateErrorMessage.value);
  } finally {
    replicateLoading.value = false;
  }
}

async function submitReplicate(): Promise<void> {
  if (replicateSaving.value) return;
  if (!replicateTargetId.value) {
    const message = t('messenger.memory.replicateEmptyTarget');
    replicateErrorMessage.value = message;
    ElMessage.warning(message);
    return;
  }

  const targetName =
    replicateTargets.value.find((item) => item.id === replicateTargetId.value)?.name || replicateTargetId.value;

  try {
    await ElMessageBox.confirm(
      t('messenger.memory.replicateConfirm', { name: targetName }),
      t('common.notice'),
      { type: 'warning' }
    );
    replicateSaving.value = true;
    replicateErrorMessage.value = '';
    errorMessage.value = '';
    await replicateAgentMemories(requestAgentId.value, {
      target_agent_id: replicateTargetId.value,
      overwrite: true
    });
    replicateDialogVisible.value = false;
    ElMessage.success(t('messenger.memory.replicateSuccess', { name: targetName }));
  } catch (error: any) {
    if (isActionCanceled(error)) return;
    const message = resolveRequestError(error, 'common.saveFailed');
    replicateErrorMessage.value = message;
    errorMessage.value = message;
    ElMessage.error(message);
  } finally {
    replicateSaving.value = false;
  }
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

onMounted(() => {
  mounted.value = true;
  if (isPanelActive.value) {
    void loadData();
  }
});

onBeforeUnmount(() => {
  disposed = true;
});

watch(
  () => [requestAgentId.value, isPanelActive.value] as const,
  ([agentKey, active], previous) => {
    if (!mounted.value || disposed || !active) return;
    const wasActive = previous?.[1] === true;
    if (!wasActive && lastLoadedAgentKey === agentKey) return;
    if (agentKey === previous?.[0] && wasActive) return;
    dialogVisible.value = false;
    replicateDialogVisible.value = false;
    dialogErrorMessage.value = '';
    replicateErrorMessage.value = '';
    saving.value = false;
    mutating.value = false;
    replicateSaving.value = false;
    replicateLoading.value = false;
    editingId.value = '';
    replicateTargetId.value = '';
    editor.value = createEmptyEditor();
    items.value = [];
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
.agent-memory-dialog-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.agent-memory-toolbar-actions,
.agent-memory-card-topline-chips,
.agent-memory-card-meta,
.agent-memory-meta-row,
.agent-memory-dialog-footer-actions {
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
.agent-memory-card-time {
  color: var(--app-text-muted, #64748b);
  font-size: 12px;
}

.agent-memory-filters {
  display: grid;
  grid-template-columns: minmax(0, 1.6fr) minmax(180px, 0.7fr);
  gap: 12px;
}

.agent-memory-search {
  grid-column: 1 / 2;
}

.agent-memory-card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 10px;
}

.agent-memory-card {
  padding: 12px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 8px;
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
  border-radius: 16px;
  transition: border-color 0.18s ease, transform 0.18s ease, box-shadow 0.18s ease;
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
  gap: 6px;
  flex-wrap: wrap;
  min-width: 0;
}

.agent-memory-card-title {
  font-weight: 700;
  line-height: 1.35;
  font-size: 14px;
}

.agent-memory-card-summary {
  font-size: 12px;
  color: var(--app-text-color, #0f172a);
  line-height: 1.55;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.agent-memory-card-meta-item {
  color: var(--app-text-muted, #64748b);
  font-size: 11px;
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

.agent-memory-btn--danger {
  color: #b91c1c;
}

.agent-memory-chip {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 2px 8px;
  font-size: 12px;
  background: rgba(148, 163, 184, 0.12);
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

.agent-memory-dialog-footer {
  width: 100%;
  padding-top: 14px;
  border-top: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.18));
  align-items: flex-end;
}

.agent-memory-dialog-footer--end {
  justify-content: flex-end;
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

  .agent-memory-search,
  .agent-memory-field--full {
    grid-column: auto;
  }
}
</style>
