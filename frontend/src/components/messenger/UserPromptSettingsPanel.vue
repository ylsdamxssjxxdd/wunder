<template>
  <div class="messenger-prompt-settings">
    <section class="messenger-settings-card">
      <div class="messenger-settings-head">
        <div>
          <div class="messenger-settings-title">{{ t('messenger.prompt.title') }}</div>
          <div class="messenger-settings-subtitle">{{ t('messenger.prompt.desc') }}</div>
        </div>
      </div>
      <div class="messenger-prompt-toolbar">
        <label class="messenger-prompt-field">
          <span>{{ t('messenger.prompt.pack') }}</span>
          <select :value="selectedPack" :disabled="metaLoading" @change="handlePackSelectChange">
            <option v-for="pack in packs" :key="pack.id" :value="pack.id">{{ pack.id }}</option>
          </select>
        </label>
        <button
          class="messenger-settings-action"
          type="button"
          :disabled="settingActive || selectedPack === activePack"
          @click="setActivePack"
        >
          {{ t('messenger.prompt.setActive') }}
        </button>
        <button class="messenger-settings-action ghost" type="button" :disabled="creatingPack" @click="createPack">
          {{ t('messenger.prompt.newPack') }}
        </button>
        <button
          class="messenger-settings-action danger ghost"
          type="button"
          :disabled="deletingPack || selectedPack === 'default'"
          @click="deletePack"
        >
          {{ t('messenger.prompt.deletePack') }}
        </button>
      </div>
      <div class="messenger-prompt-meta">
        <span class="messenger-kind-tag">
          {{ t('messenger.prompt.activeTag', { pack: activePack || 'default' }) }}
        </span>
        <span v-if="selectedPack === 'default' && defaultSyncPackId" class="messenger-kind-tag">
          {{ t('messenger.prompt.syncTag', { pack: defaultSyncPackId }) }}
        </span>
      </div>
    </section>

    <section class="messenger-settings-card messenger-prompt-main">
      <aside class="messenger-prompt-segments">
        <button
          v-for="segment in segments"
          :key="segment.key"
          class="messenger-prompt-segment-item"
          :class="{ active: selectedSegment === segment.key }"
          type="button"
          @click="selectSegment(segment.key)"
        >
          <strong>{{ resolveSegmentLabel(segment.key) }}</strong>
          <span>{{ segment.file || segment.key }}</span>
        </button>
      </aside>

      <div class="messenger-prompt-editor-wrap">
        <div class="messenger-prompt-editor-head">
          <div class="messenger-settings-title">{{ resolveSegmentLabel(selectedSegment) }}</div>
          <button
            class="messenger-settings-action"
            type="button"
            :disabled="savingFile || selectedPack === 'default'"
            @click="saveCurrentFile"
          >
            {{ savingFile ? t('common.loading') : t('common.save') }}
          </button>
        </div>
        <textarea
          v-model="editorContent"
          class="messenger-prompt-editor"
          :readonly="selectedPack === 'default'"
          spellcheck="false"
        ></textarea>
        <div class="messenger-prompt-status">{{ statusText }}</div>
      </div>
    </section>

    <section class="messenger-settings-card">
      <div class="messenger-settings-head">
        <div>
          <div class="messenger-settings-title">{{ t('messenger.prompt.previewTitle') }}</div>
          <div class="messenger-settings-subtitle">{{ t('messenger.prompt.previewHint') }}</div>
        </div>
        <button class="messenger-settings-action ghost" type="button" :disabled="previewLoading" @click="loadPreview">
          {{ previewLoading ? t('common.loading') : t('common.refresh') }}
        </button>
      </div>
      <pre class="messenger-prompt-preview">{{ previewPrompt }}</pre>
    </section>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  createUserPromptTemplatePack,
  deleteUserPromptTemplatePack,
  getUserPromptTemplateFile,
  listUserPromptTemplates,
  setUserPromptTemplateActive,
  updateUserPromptTemplateFile
} from '@/api/promptTemplates';
import { fetchRealtimeSystemPrompt } from '@/api/chat';
import { fetchUserToolsCatalog } from '@/api/userTools';
import { useI18n } from '@/i18n';

type PromptPack = {
  id: string;
  is_default?: boolean;
  readonly?: boolean;
};

type PromptSegment = {
  key: string;
  file: string;
};

type PromptTemplateStatus = {
  active?: string;
  packs?: PromptPack[];
  segments?: PromptSegment[];
  default_sync_pack_id?: string;
};

type PromptTemplateFile = {
  content?: string;
  fallback_used?: boolean;
  source_pack_id?: string;
};

const DEFAULT_SEGMENTS: PromptSegment[] = [
  { key: 'role', file: 'role.txt' },
  { key: 'engineering', file: 'engineering.txt' },
  { key: 'tools_protocol', file: 'tools_protocol.txt' },
  { key: 'skills_protocol', file: 'skills_protocol.txt' },
  { key: 'memory', file: 'memory.txt' },
  { key: 'extra', file: 'extra.txt' }
];

const { t, language } = useI18n();

const packs = ref<PromptPack[]>([]);
const segments = ref<PromptSegment[]>([...DEFAULT_SEGMENTS]);
const selectedPack = ref('default');
const activePack = ref('default');
const selectedSegment = ref(DEFAULT_SEGMENTS[0].key);
const defaultSyncPackId = ref('default');

const editorContent = ref('');
const loadedContent = ref('');
const statusText = ref('');
const previewPrompt = ref('');

const metaLoading = ref(false);
const fileLoading = ref(false);
const savingFile = ref(false);
const settingActive = ref(false);
const creatingPack = ref(false);
const deletingPack = ref(false);
const previewLoading = ref(false);
let fileLoadSequence = 0;

const resolveErrorMessage = (error: unknown, fallback: string) => {
  const detail = (error as { response?: { data?: { detail?: string } } })?.response?.data?.detail;
  if (detail && String(detail).trim()) {
    return String(detail);
  }
  const message = (error as { message?: string })?.message;
  if (message && String(message).trim()) {
    return String(message);
  }
  return fallback;
};

const resolveLocale = () => (String(language.value || '').toLowerCase().startsWith('en') ? 'en' : 'zh');

const resolveSegmentLabel = (key: string) => {
  switch (String(key || '').trim()) {
    case 'role':
      return t('messenger.prompt.file.role');
    case 'engineering':
      return t('messenger.prompt.file.engineering');
    case 'tools_protocol':
      return t('messenger.prompt.file.tools');
    case 'skills_protocol':
      return t('messenger.prompt.file.skills');
    case 'memory':
      return t('messenger.prompt.file.memory');
    case 'extra':
      return t('messenger.prompt.file.extra');
    default:
      return key || '-';
  }
};

const hasUnsavedChanges = () => editorContent.value !== loadedContent.value;

const isActionCanceled = (error: unknown) => {
  const message = String((error as { message?: string })?.message || '').toLowerCase();
  return message.includes('cancel') || message.includes('close');
};

const confirmDiscardChanges = async () => {
  if (!hasUnsavedChanges()) {
    return true;
  }
  try {
    await ElMessageBox.confirm(t('messenger.prompt.confirmDiscard'), t('common.notice'), {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
    return true;
  } catch {
    return false;
  }
};

const loadStatus = async () => {
  metaLoading.value = true;
  try {
    const result = await listUserPromptTemplates();
    const data = ((result?.data?.data || {}) as PromptTemplateStatus) || {};
    const nextPacks = Array.isArray(data.packs) && data.packs.length ? data.packs : [{ id: 'default' }];
    const nextSegments =
      Array.isArray(data.segments) && data.segments.length ? data.segments : [...DEFAULT_SEGMENTS];
    packs.value = nextPacks;
    segments.value = nextSegments;
    activePack.value = String(data.active || 'default').trim() || 'default';
    defaultSyncPackId.value = String(data.default_sync_pack_id || 'default').trim() || 'default';
    if (!packs.value.some((pack) => pack.id === selectedPack.value)) {
      selectedPack.value = activePack.value;
    }
    if (!segments.value.some((segment) => segment.key === selectedSegment.value)) {
      selectedSegment.value = segments.value[0]?.key || DEFAULT_SEGMENTS[0].key;
    }
  } finally {
    metaLoading.value = false;
  }
};

const loadFile = async () => {
  const currentSequence = ++fileLoadSequence;
  fileLoading.value = true;
  statusText.value = t('common.loading');
  try {
    const result = await getUserPromptTemplateFile({
      pack_id: selectedPack.value,
      key: selectedSegment.value,
      locale: resolveLocale()
    });
    const data = ((result?.data?.data || {}) as PromptTemplateFile) || {};
    if (currentSequence !== fileLoadSequence) {
      return;
    }
    const nextContent = String(data.content || '');
    editorContent.value = nextContent;
    loadedContent.value = nextContent;
    if (selectedPack.value === 'default') {
      statusText.value = t('messenger.prompt.readonlyDefault', {
        pack: String(data.source_pack_id || defaultSyncPackId.value || 'default')
      });
    } else if (data.fallback_used) {
      statusText.value = t('messenger.prompt.fallbackHint');
    } else {
      statusText.value = '';
    }
  } catch (error) {
    if (currentSequence !== fileLoadSequence) {
      return;
    }
    statusText.value = resolveErrorMessage(error, t('messenger.prompt.loadFailed'));
    editorContent.value = '';
    loadedContent.value = '';
  } finally {
    if (currentSequence === fileLoadSequence) {
      fileLoading.value = false;
    }
  }
};

const resolveEnabledToolOverrides = async () => {
  const result = await fetchUserToolsCatalog();
  const payload = (result?.data?.data || {}) as Record<string, unknown>;
  const groups = [
    payload.builtin_tools,
    payload.mcp_tools,
    payload.a2a_tools,
    payload.skills,
    payload.knowledge_tools,
    payload.user_tools,
    payload.shared_tools
  ];
  const overrides: Record<string, boolean> = {};
  for (const group of groups) {
    if (!Array.isArray(group)) {
      continue;
    }
    for (const item of group) {
      const name = String((item as { name?: string })?.name || '').trim();
      if (!name) {
        continue;
      }
      overrides[name] = true;
    }
  }
  return overrides;
};

const loadPreview = async () => {
  previewLoading.value = true;
  try {
    const payload: Record<string, unknown> = {};
    try {
      const toolOverrides = await resolveEnabledToolOverrides();
      if (Object.keys(toolOverrides).length) {
        payload.tool_overrides = toolOverrides;
      }
    } catch {
      // Fallback to backend default allowed-tool resolution.
    }
    const result = await fetchRealtimeSystemPrompt(payload);
    previewPrompt.value = String(result?.data?.data?.prompt || '');
  } catch (error) {
    previewPrompt.value = '';
    ElMessage.error(resolveErrorMessage(error, t('messenger.prompt.previewFailed')));
  } finally {
    previewLoading.value = false;
  }
};

const handlePackSelectChange = async (event: Event) => {
  const target = event.target as HTMLSelectElement | null;
  const nextPack = String(target?.value || '').trim() || 'default';
  if (nextPack === selectedPack.value) {
    return;
  }
  if (!(await confirmDiscardChanges())) {
    if (target) {
      target.value = selectedPack.value;
    }
    return;
  }
  selectedPack.value = nextPack;
  await loadFile();
};

const selectSegment = async (key: string) => {
  const next = String(key || '').trim();
  if (!next || next === selectedSegment.value) {
    return;
  }
  if (!(await confirmDiscardChanges())) {
    return;
  }
  selectedSegment.value = next;
  await loadFile();
};

const saveCurrentFile = async () => {
  if (selectedPack.value === 'default') {
    statusText.value = t('messenger.prompt.readonlyDefault', {
      pack: defaultSyncPackId.value || 'default'
    });
    return;
  }
  savingFile.value = true;
  try {
    await updateUserPromptTemplateFile({
      pack_id: selectedPack.value,
      key: selectedSegment.value,
      locale: resolveLocale(),
      content: editorContent.value
    });
    loadedContent.value = editorContent.value;
    statusText.value = t('messenger.prompt.saved');
    ElMessage.success(t('messenger.prompt.saved'));
    if (selectedPack.value === activePack.value) {
      await loadPreview();
    }
  } catch (error) {
    const message = resolveErrorMessage(error, t('messenger.prompt.saveFailed'));
    statusText.value = message;
    ElMessage.error(message);
  } finally {
    savingFile.value = false;
  }
};

const setActivePack = async () => {
  if (!selectedPack.value || selectedPack.value === activePack.value) {
    return;
  }
  settingActive.value = true;
  try {
    await setUserPromptTemplateActive({ active: selectedPack.value });
    activePack.value = selectedPack.value;
    ElMessage.success(t('messenger.prompt.activeUpdated', { pack: selectedPack.value }));
    await loadPreview();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error, t('messenger.prompt.activeUpdateFailed')));
  } finally {
    settingActive.value = false;
  }
};

const createPack = async () => {
  if (!(await confirmDiscardChanges())) {
    return;
  }
  let packId = '';
  try {
    const { value } = await ElMessageBox.prompt(t('messenger.prompt.newPackPrompt'), t('common.create'), {
      inputPattern: /^[A-Za-z0-9_-]{1,64}$/,
      inputErrorMessage: t('messenger.prompt.newPackPrompt'),
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
    packId = String(value || '').trim();
  } catch (error) {
    if (!isActionCanceled(error)) {
      ElMessage.error(resolveErrorMessage(error, t('messenger.prompt.packCreateFailed')));
    }
    return;
  }
  if (!packId || packId.toLowerCase() === 'default') {
    return;
  }
  creatingPack.value = true;
  try {
    await createUserPromptTemplatePack({ pack_id: packId, copy_from: 'default' });
    await loadStatus();
    selectedPack.value = packId;
    await loadFile();
    ElMessage.success(t('messenger.prompt.packCreated', { pack: packId }));
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error, t('messenger.prompt.packCreateFailed')));
  } finally {
    creatingPack.value = false;
  }
};

const deletePack = async () => {
  if (!selectedPack.value || selectedPack.value === 'default') {
    return;
  }
  try {
    await ElMessageBox.confirm(
      t('messenger.prompt.confirmDeletePack', { pack: selectedPack.value }),
      t('common.notice'),
      {
        type: 'warning',
        confirmButtonText: t('common.confirm'),
        cancelButtonText: t('common.cancel')
      }
    );
  } catch {
    return;
  }
  deletingPack.value = true;
  const deletedPack = selectedPack.value;
  try {
    await deleteUserPromptTemplatePack(deletedPack);
    await loadStatus();
    selectedPack.value = activePack.value || 'default';
    await loadFile();
    ElMessage.success(t('messenger.prompt.packDeleted', { pack: deletedPack }));
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error, t('messenger.prompt.packDeleteFailed')));
  } finally {
    deletingPack.value = false;
  }
};

onMounted(async () => {
  try {
    await loadStatus();
    await loadFile();
    await loadPreview();
  } catch (error) {
    statusText.value = resolveErrorMessage(error, t('messenger.prompt.loadFailed'));
  }
});
</script>

<style scoped>
.messenger-prompt-settings {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
}

.messenger-prompt-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: flex-end;
}

.messenger-prompt-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 180px;
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-prompt-field select {
  height: 34px;
  border-radius: 10px;
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  color: var(--hula-text);
  padding: 0 10px;
}

.messenger-prompt-meta {
  margin-top: 10px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.messenger-prompt-main {
  display: grid;
  grid-template-columns: minmax(180px, 220px) minmax(0, 1fr);
  gap: 10px;
  min-height: 360px;
  width: 100%;
  overflow: hidden;
}

.messenger-prompt-segments {
  border: 1px solid var(--hula-border);
  border-radius: 12px;
  background: var(--hula-panel-bg);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  overflow: auto;
  min-height: 0;
  min-width: 0;
}

.messenger-prompt-segment-item {
  border: 1px solid transparent;
  border-radius: 10px;
  background: transparent;
  color: var(--hula-text);
  text-align: left;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  cursor: pointer;
}

.messenger-prompt-segment-item span {
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-prompt-segment-item.active {
  border-color: rgba(var(--ui-accent-rgb), 0.28);
  background: rgba(var(--ui-accent-rgb), 0.12);
}

.messenger-prompt-editor-wrap {
  border: 1px solid var(--hula-border);
  border-radius: 12px;
  background: var(--hula-panel-bg);
  padding: 10px;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.messenger-prompt-editor-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}

.messenger-prompt-editor {
  flex: 1;
  min-height: 0;
  border-radius: 10px;
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  color: var(--hula-text);
  resize: vertical;
  padding: 10px 12px;
  line-height: 1.55;
  font-size: 13px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', monospace;
}

.messenger-prompt-editor:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb), 0.28);
  outline-offset: 1px;
}

.messenger-prompt-editor[readonly] {
  opacity: 0.9;
  cursor: not-allowed;
}

.messenger-prompt-status {
  margin-top: 8px;
  min-height: 20px;
  font-size: 12px;
  color: var(--hula-muted);
}

.messenger-prompt-preview {
  margin: 0;
  border: 1px solid var(--hula-border);
  border-radius: 10px;
  background: var(--hula-center-bg);
  color: var(--hula-text);
  padding: 12px;
  min-height: 240px;
  max-height: 420px;
  overflow: auto;
  white-space: pre-wrap;
  line-height: 1.6;
  font-size: 13px;
}

@media (max-width: 1200px) {
  .messenger-prompt-main {
    grid-template-columns: 1fr;
  }
}
</style>
