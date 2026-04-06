<template>
  <div class="messenger-prompt-settings">
    <section class="messenger-settings-card">
      <div class="messenger-prompt-toolbar">
        <label class="messenger-prompt-field">
          <span>{{ t('messenger.prompt.pack') }}</span>
          <select :value="selectedPack" :disabled="metaLoading" @change="handlePackSelectChange">
            <option v-for="pack in packs" :key="pack.id" :value="pack.id">
              {{ resolvePackLabel(pack.id, pack) }}
            </option>
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
          :disabled="deletingPack || selectedPackReadonly"
          @click="deletePack"
        >
          {{ t('messenger.prompt.deletePack') }}
        </button>
      </div>
      <div class="messenger-prompt-meta">
        <span class="messenger-kind-tag">
          {{ t('messenger.prompt.activeTag', { pack: resolvePackLabel(activePack, activePackMeta) }) }}
        </span>
        <span v-if="selectedPackMeta?.is_system_language_default" class="messenger-kind-tag">
          {{ t('messenger.prompt.systemLanguageDefaultTag') }}
        </span>
        <span v-if="selectedPackBuiltin && defaultSyncPackId" class="messenger-kind-tag">
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
            :disabled="savingFile || selectedPackReadonly"
            @click="saveCurrentFile"
          >
            {{ savingFile ? t('common.loading') : t('common.save') }}
          </button>
        </div>
        <textarea
          v-model="editorContent"
          class="messenger-prompt-editor"
          :readonly="selectedPackReadonly"
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

    <HoneycombWaitingOverlay
      :visible="Boolean(promptSettingsWaitingState)"
      :title="promptSettingsWaitingState?.title || t('messenger.waiting.title')"
      :target-name="promptSettingsWaitingState?.targetName || ''"
      :phase-label="promptSettingsWaitingState?.phaseLabel || ''"
      :summary-label="promptSettingsWaitingState?.summaryLabel || ''"
      :progress="promptSettingsWaitingState?.progress ?? 0"
      :teleport-to-body="false"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
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
import HoneycombWaitingOverlay from '@/components/common/HoneycombWaitingOverlay.vue';
import { useI18n } from '@/i18n';

type PromptPack = {
  id: string;
  is_default?: boolean;
  readonly?: boolean;
  builtin?: boolean;
  locale?: string;
  is_system_language_default?: boolean;
  sync_pack_id?: string;
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

const DEFAULT_PACK_ID = 'default';
const DEFAULT_ZH_PACK_ID = 'default-zh';
const DEFAULT_EN_PACK_ID = 'default-en';
const BUILTIN_PACK_IDS = new Set([DEFAULT_PACK_ID, DEFAULT_ZH_PACK_ID, DEFAULT_EN_PACK_ID]);

const { t, language } = useI18n();

function resolveLocale() {
  return String(language.value || '').toLowerCase().startsWith('en') ? 'en' : 'zh';
}

function resolveSystemLanguageBuiltinPackId() {
  return resolveLocale() === 'en' ? DEFAULT_EN_PACK_ID : DEFAULT_ZH_PACK_ID;
}

const packs = ref<PromptPack[]>([]);
const segments = ref<PromptSegment[]>([...DEFAULT_SEGMENTS]);
const selectedPack = ref(resolveSystemLanguageBuiltinPackId());
const activePack = ref(resolveSystemLanguageBuiltinPackId());
const selectedSegment = ref(DEFAULT_SEGMENTS[0].key);
const defaultSyncPackId = ref(DEFAULT_PACK_ID);

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

type PromptSettingsWaitingState = {
  title: string;
  targetName: string;
  phaseLabel: string;
  summaryLabel: string;
  progress: number;
};

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

const packMap = computed(() => {
  const next = new Map<string, PromptPack>();
  for (const pack of packs.value) {
    next.set(String(pack.id || '').trim(), pack);
  }
  return next;
});

const selectedPackMeta = computed(() => packMap.value.get(selectedPack.value) || null);
const activePackMeta = computed(() => packMap.value.get(activePack.value) || null);

const isBuiltinPack = (packId: string, pack?: PromptPack | null) => {
  if (pack?.builtin) {
    return true;
  }
  return BUILTIN_PACK_IDS.has(String(pack?.id || packId || '').trim().toLowerCase());
};

const isReadonlyPack = (pack?: PromptPack | null, packId = '') =>
  Boolean(pack?.readonly || isBuiltinPack(packId, pack));

const selectedPackBuiltin = computed(() => isBuiltinPack(selectedPack.value, selectedPackMeta.value));
const selectedPackReadonly = computed(() => isReadonlyPack(selectedPackMeta.value, selectedPack.value));
const promptSettingsWaitingState = computed<PromptSettingsWaitingState | null>(() => {
  const targetName = resolvePackLabel(selectedPack.value, selectedPackMeta.value);
  if (metaLoading.value) {
    return {
      title: t('messenger.waiting.title'),
      targetName,
      phaseLabel: t('messenger.waiting.phase.preparing'),
      summaryLabel: t('messenger.waiting.summary.promptSettings'),
      progress: 24
    };
  }
  if (fileLoading.value) {
    return {
      title: t('messenger.waiting.title'),
      targetName,
      phaseLabel: t('messenger.waiting.phase.loading'),
      summaryLabel: t('messenger.waiting.summary.promptSettings'),
      progress: 52
    };
  }
  return null;
});

const resolvePackLocale = (packId: string, pack?: PromptPack | null) => {
  const normalizedPackId = String(packId || '').trim().toLowerCase();
  const locale = String(pack?.locale || '').trim().toLowerCase();
  if (normalizedPackId === DEFAULT_PACK_ID) {
    return resolveLocale();
  }
  if (locale.startsWith('en') || normalizedPackId === DEFAULT_EN_PACK_ID) {
    return 'en';
  }
  if (locale.startsWith('zh') || normalizedPackId === DEFAULT_ZH_PACK_ID) {
    return 'zh';
  }
  return resolveLocale();
};

const resolvePackLabel = (packId: string, pack?: PromptPack | null) => {
  const normalizedPackId = String(packId || '').trim();
  if (!normalizedPackId) {
    return '-';
  }
  const meta = pack || packMap.value.get(normalizedPackId) || null;
  if (isBuiltinPack(normalizedPackId, meta)) {
    return resolvePackLocale(normalizedPackId, meta) === 'en'
      ? t('messenger.prompt.defaultPackEn')
      : t('messenger.prompt.defaultPackZh');
  }
  return normalizedPackId;
};

const normalizeSegmentKey = (value: unknown) =>
  String(value || '')
    .trim()
    .replace(/\.txt$/i, '')
    .replace(/\s+/g, '_')
    .toLowerCase();

const normalizeSegments = (value: unknown): PromptSegment[] => {
  if (!Array.isArray(value) || !value.length) {
    return [...DEFAULT_SEGMENTS];
  }
  const normalized: PromptSegment[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const raw = (item || {}) as Record<string, unknown>;
    const key = normalizeSegmentKey(raw.key || raw.id || raw.name || raw.segment || raw.file);
    if (!key || seen.has(key)) {
      continue;
    }
    const file = String(raw.file || `${key}.txt`).trim() || `${key}.txt`;
    normalized.push({ key, file });
    seen.add(key);
  }
  return normalized.length ? normalized : [...DEFAULT_SEGMENTS];
};

const resolveTemplateContent = (value: unknown) => {
  const payload = (value || {}) as Record<string, unknown>;
  const candidates = [payload.content, payload.text, payload.value, payload.prompt];
  for (const candidate of candidates) {
    if (typeof candidate === 'string') {
      return candidate;
    }
  }
  return '';
};

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
    const hadPacks = packs.value.length > 0;
    const fallbackPackId = resolveSystemLanguageBuiltinPackId();
    const nextPacks =
      Array.isArray(data.packs) && data.packs.length
        ? data.packs
        : [
            {
              id: fallbackPackId,
              builtin: true,
              readonly: true,
              locale: resolveLocale(),
              is_system_language_default: true
            }
          ];
    const nextSegments = normalizeSegments(data.segments);
    packs.value = nextPacks;
    segments.value = nextSegments;
    activePack.value = String(data.active || fallbackPackId).trim() || fallbackPackId;
    defaultSyncPackId.value = String(data.default_sync_pack_id || DEFAULT_PACK_ID).trim() || DEFAULT_PACK_ID;
    if (!hadPacks || !packs.value.some((pack) => pack.id === selectedPack.value)) {
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
    const nextContent = resolveTemplateContent(data);
    editorContent.value = nextContent;
    loadedContent.value = nextContent;
    if (selectedPackReadonly.value) {
      statusText.value = t('messenger.prompt.readonlyDefault', {
        pack: String(data.source_pack_id || defaultSyncPackId.value || DEFAULT_PACK_ID)
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
  const overrides: string[] = [];
  const seen = new Set<string>();
  for (const group of groups) {
    if (!Array.isArray(group)) {
      continue;
    }
    for (const item of group) {
      const name = String((item as { name?: string })?.name || '').trim();
      if (!name) {
        continue;
      }
      if (seen.has(name)) {
        continue;
      }
      seen.add(name);
      overrides.push(name);
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
      if (toolOverrides.length) {
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
  const nextPack = String(target?.value || '').trim() || resolveSystemLanguageBuiltinPackId();
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
  if (selectedPackReadonly.value) {
    statusText.value = t('messenger.prompt.readonlyDefault', {
      pack: defaultSyncPackId.value || DEFAULT_PACK_ID
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
    ElMessage.success(
      t('messenger.prompt.activeUpdated', {
        pack: resolvePackLabel(activePack.value, activePackMeta.value)
      })
    );
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
  if (!packId || BUILTIN_PACK_IDS.has(packId.toLowerCase())) {
    return;
  }
  creatingPack.value = true;
  try {
    await createUserPromptTemplatePack({
      pack_id: packId,
      copy_from: selectedPack.value || activePack.value || resolveSystemLanguageBuiltinPackId()
    });
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
  if (!selectedPack.value || selectedPackReadonly.value) {
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
    selectedPack.value = activePack.value || resolveSystemLanguageBuiltinPackId();
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
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 0;
  min-height: 360px;
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
  min-height: 420px;
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
  min-height: 280px;
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
