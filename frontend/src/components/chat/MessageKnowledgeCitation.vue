<template>
  <div v-if="references.length" class="message-knowledge-citation">
    <div class="message-knowledge-citation__title">
      <i class="fa-solid fa-bookmark" aria-hidden="true"></i>
      <span>{{ t('chat.knowledgeCitation.title', { count: references.length }) }}</span>
    </div>

    <div class="message-knowledge-citation__list">
      <button
        v-for="item in references"
        :key="item.key"
        class="message-knowledge-citation__item"
        type="button"
        :title="t('chat.knowledgeCitation.openDetail')"
        @click="openDetail(item)"
      >
        <span class="message-knowledge-citation__item-title">{{ item.title }}</span>
        <span class="message-knowledge-citation__item-meta">{{ item.meta }}</span>
      </button>
    </div>
  </div>

  <el-dialog
    v-model="detailVisible"
    class="messenger-dialog message-knowledge-citation-dialog"
    :title="t('chat.knowledgeCitation.detailTitle')"
    width="680px"
    append-to-body
    destroy-on-close
  >
    <template v-if="activeReference">
      <div class="message-knowledge-citation-detail">
        <div class="message-knowledge-citation-detail__row">
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.base') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.baseName }}</span>
        </div>
        <div class="message-knowledge-citation-detail__row">
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.source') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.sourceName }}</span>
        </div>
        <div class="message-knowledge-citation-detail__row">
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.typeLabel') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.typeLabel }}</span>
        </div>
        <div
          v-if="activeReference.keyword"
          class="message-knowledge-citation-detail__row"
        >
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.keyword') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.keyword }}</span>
        </div>
        <div
          v-if="activeReference.scoreText"
          class="message-knowledge-citation-detail__row"
        >
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.score') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.scoreText }}</span>
        </div>
        <div
          v-if="activeReference.reason"
          class="message-knowledge-citation-detail__row"
        >
          <span class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.reason') }}</span>
          <span class="message-knowledge-citation-detail__value">{{ activeReference.reason }}</span>
        </div>
        <div class="message-knowledge-citation-detail__section">
          <div class="message-knowledge-citation-detail__label">{{ t('chat.knowledgeCitation.content') }}</div>
          <pre class="message-knowledge-citation-detail__content">{{
            activeReference.content || t('chat.knowledgeCitation.noContent')
          }}</pre>
        </div>

        <details class="message-knowledge-citation-detail__raw">
          <summary>{{ t('chat.knowledgeCitation.raw') }}</summary>
          <pre>{{ activeReference.raw }}</pre>
        </details>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';

type WorkflowItem = {
  id?: string | number;
  detail?: unknown;
  eventType?: string;
  toolName?: string;
};

type UnknownObject = Record<string, unknown>;

type KnowledgeReference = {
  key: string;
  baseName: string;
  sourceName: string;
  typeLabel: string;
  title: string;
  meta: string;
  keyword: string;
  scoreText: string;
  reason: string;
  content: string;
  raw: string;
};

type Props = {
  items?: WorkflowItem[];
};

const props = withDefaults(defineProps<Props>(), {
  items: () => []
});

const { t } = useI18n();

const detailVisible = ref(false);
const activeReference = ref<KnowledgeReference | null>(null);

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const asArray = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim();
    }
  }
  return '';
};

const pickScore = (value: unknown): number | null => {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return value;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const pickPositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const parseDetailObject = (detail: unknown): UnknownObject | null => {
  if (!detail) return null;
  if (typeof detail === 'object' && !Array.isArray(detail)) {
    return detail as UnknownObject;
  }
  if (typeof detail !== 'string') return null;
  const trimmed = detail.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) {
    return null;
  }
  try {
    return asObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

const hasKnowledgeDocumentShape = (value: unknown): boolean => {
  const doc = asObject(value);
  if (!doc) return false;
  return (
    'code' in doc ||
    'section_path' in doc ||
    'knowledge_base' in doc ||
    'doc_id' in doc ||
    'chunk_index' in doc
  );
};

// Tool payloads may be nested differently across stream/runtime versions.
const resolveKnowledgePayload = (detailObject: UnknownObject): UnknownObject | null => {
  const candidates = [detailObject, asObject(detailObject.data), asObject(detailObject.result)];
  for (const candidate of candidates) {
    if (!candidate) continue;
    const documents = asArray(candidate.documents);
    if (!documents.length) continue;
    const baseName = pickString(
      candidate.knowledge_base,
      candidate.knowledgeBase,
      candidate.base,
      candidate.base_name,
      candidate.baseName
    );
    const vectorFlag =
      candidate.vector === true ||
      String(candidate.base_type || candidate.baseType || '')
        .trim()
        .toLowerCase() === 'vector';
    if (baseName || vectorFlag || hasKnowledgeDocumentShape(documents[0])) {
      return candidate;
    }
  }
  return null;
};

const references = computed<KnowledgeReference[]>(() => {
  const output: KnowledgeReference[] = [];
  const dedupe = new Set<string>();
  const items = Array.isArray(props.items) ? props.items : [];

  items.forEach((item, itemIndex) => {
    if (!item) return;
    const eventType = String(item.eventType || '').trim();
    if (eventType && eventType !== 'tool_result') return;

    const detailObject = parseDetailObject(item.detail);
    if (!detailObject) return;
    const payload = resolveKnowledgePayload(detailObject);
    if (!payload) return;

    const documents = asArray(payload.documents);
    if (!documents.length) return;

    const baseName =
      pickString(
        payload.knowledge_base,
        payload.knowledgeBase,
        payload.base,
        payload.base_name,
        payload.baseName
      ) ||
      pickString(item.toolName) ||
      t('knowledge.name.unnamed');

    const payloadVector =
      payload.vector === true ||
      String(payload.base_type || payload.baseType || '')
        .trim()
        .toLowerCase() === 'vector';

    documents.forEach((entry, docIndex) => {
      const doc = asObject(entry);
      if (!doc) return;

      const chunkIndex = pickPositiveInteger(doc.chunk_index ?? doc.chunkIndex);
      const inferredVector =
        payloadVector ||
        chunkIndex !== null ||
        'doc_id' in doc ||
        'start' in doc ||
        'end' in doc;
      const typeLabel = inferredVector
        ? t('chat.knowledgeCitation.type.vector')
        : t('chat.knowledgeCitation.type.literal');

      const sourceName =
        pickString(doc.name, doc.document, doc.doc_name, doc.doc_id, doc.code) ||
        t('chat.knowledgeCitation.unnamedSource');
      const title =
        inferredVector && chunkIndex !== null
          ? `${sourceName} #${chunkIndex + 1}`
          : sourceName;

      const sectionPath = asArray(doc.section_path)
        .map((part) => String(part || '').trim())
        .filter(Boolean)
        .join(' / ');
      const keyword = pickString(doc.keyword, payload.keyword);
      const scoreValue = pickScore(doc.score);
      const scoreText = scoreValue === null ? '' : scoreValue.toFixed(3);
      const reason = pickString(doc.reason);
      const content = pickString(doc.content, doc.text, doc.preview);

      const metaParts: string[] = [
        `${t('chat.knowledgeCitation.base')}: ${baseName}`,
        typeLabel
      ];
      if (sectionPath) {
        metaParts.push(sectionPath);
      }
      if (keyword) {
        metaParts.push(`${t('chat.knowledgeCitation.keyword')}: ${keyword}`);
      }
      if (scoreText) {
        metaParts.push(`${t('chat.knowledgeCitation.score')}: ${scoreText}`);
      }

      const dedupeKey = [
        baseName,
        inferredVector ? 'vector' : 'literal',
        String(doc.doc_id || ''),
        String(doc.code || ''),
        String(doc.document || ''),
        String(doc.name || ''),
        String(chunkIndex ?? ''),
        sectionPath
      ].join('::');
      if (dedupe.has(dedupeKey)) return;
      dedupe.add(dedupeKey);

      const raw = JSON.stringify(
        {
          knowledge_base: baseName,
          base_type: inferredVector ? 'vector' : 'literal',
          ...doc
        },
        null,
        2
      );

      output.push({
        key: `${itemIndex}-${docIndex}-${dedupeKey}`,
        baseName,
        sourceName,
        typeLabel,
        title,
        meta: metaParts.join(' · '),
        keyword,
        scoreText,
        reason,
        content,
        raw
      });
    });
  });

  return output;
});

const openDetail = (item: KnowledgeReference) => {
  activeReference.value = item;
  detailVisible.value = true;
};
</script>

<style scoped>
.message-knowledge-citation {
  --message-knowledge-citation-border: var(--chat-border, var(--hula-border, #dbe1ea));
  --message-knowledge-citation-surface: var(--chat-panel, var(--hula-center-bg, #f8fafc));
  --message-knowledge-citation-surface-strong: var(--chat-card, var(--hula-main-bg, #ffffff));
  --message-knowledge-citation-text: var(--chat-text, var(--hula-text, #111827));
  --message-knowledge-citation-muted: var(--chat-muted, var(--hula-muted, #4b5563));
  --message-knowledge-citation-accent: var(--chat-primary, var(--ui-accent, #3b82f6));
  margin-top: 10px;
  border: 1px solid var(--message-knowledge-citation-border);
  border-radius: 14px;
  padding: 10px;
  background:
    linear-gradient(180deg, rgba(var(--ui-accent-rgb), 0.08), rgba(var(--ui-accent-rgb), 0.03)),
    var(--message-knowledge-citation-surface);
  box-shadow: 0 10px 24px rgba(15, 23, 42, 0.08);
  color: var(--message-knowledge-citation-text);
}

.message-knowledge-citation__title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--message-knowledge-citation-muted);
}

.message-knowledge-citation__list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.message-knowledge-citation__item {
  flex: 0 1 280px;
  width: min(100%, 280px);
  border: 1px solid var(--message-knowledge-citation-border);
  border-radius: 10px;
  background: var(--message-knowledge-citation-surface-strong);
  color: inherit;
  padding: 8px 10px;
  max-width: min(100%, 280px);
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 2px;
  appearance: none;
  transition:
    border-color 0.18s ease,
    background-color 0.18s ease,
    transform 0.18s ease,
    box-shadow 0.18s ease;
}

.message-knowledge-citation__item:hover,
.message-knowledge-citation__item:focus-visible {
  border-color: var(--message-knowledge-citation-accent);
  transform: translateY(-1px);
  box-shadow: 0 8px 18px rgba(15, 23, 42, 0.12);
  outline: none;
}

.message-knowledge-citation__item-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--message-knowledge-citation-text);
}

.message-knowledge-citation__item-meta {
  font-size: 11px;
  line-height: 1.35;
  color: var(--message-knowledge-citation-muted);
  word-break: break-word;
}

.message-knowledge-citation-dialog {
  --message-knowledge-citation-dialog-bg: #ffffff;
  --message-knowledge-citation-dialog-surface: #f8fafc;
  --message-knowledge-citation-dialog-border: #dbe1ea;
  --message-knowledge-citation-dialog-text: #111827;
  --message-knowledge-citation-dialog-muted: #6b7280;
  --el-dialog-bg-color: var(--message-knowledge-citation-dialog-bg);
  --el-text-color-primary: var(--message-knowledge-citation-dialog-text);
  --el-text-color-regular: var(--message-knowledge-citation-dialog-text);
  --el-text-color-secondary: var(--message-knowledge-citation-dialog-muted);
  --el-border-color: var(--message-knowledge-citation-dialog-border);
}

.message-knowledge-citation-dialog.el-dialog {
  width: min(680px, calc(100vw - 24px)) !important;
  max-width: calc(100vw - 24px);
  border-radius: 16px;
  border: 1px solid var(--message-knowledge-citation-dialog-border);
  background: var(--message-knowledge-citation-dialog-bg);
  color: var(--message-knowledge-citation-dialog-text);
  box-shadow: 0 18px 44px rgba(15, 23, 42, 0.18);
}

.message-knowledge-citation-dialog .el-dialog__header {
  border-bottom: 1px solid var(--message-knowledge-citation-dialog-border);
}

.message-knowledge-citation-dialog .el-dialog__body {
  max-height: min(72vh, 680px);
  overflow: auto;
  color: var(--message-knowledge-citation-dialog-text);
}

.message-knowledge-citation-detail {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.message-knowledge-citation-detail__row {
  display: grid;
  grid-template-columns: 84px minmax(0, 1fr);
  gap: 10px;
  align-items: flex-start;
}

.message-knowledge-citation-detail__label {
  font-size: 12px;
  color: var(--message-knowledge-citation-dialog-muted);
}

.message-knowledge-citation-detail__value {
  font-size: 13px;
  color: var(--message-knowledge-citation-dialog-text);
  word-break: break-word;
}

.message-knowledge-citation-detail__section {
  margin-top: 2px;
}

.message-knowledge-citation-detail__content,
.message-knowledge-citation-detail__raw pre {
  margin: 6px 0 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--message-knowledge-citation-dialog-border);
  background: var(--message-knowledge-citation-dialog-surface);
  max-height: 300px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.5;
  color: var(--message-knowledge-citation-dialog-text);
}

.message-knowledge-citation-detail__raw summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--message-knowledge-citation-dialog-muted);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .message-knowledge-citation) {
  --message-knowledge-citation-border: rgba(var(--ui-accent-rgb), 0.22);
  --message-knowledge-citation-surface:
    linear-gradient(180deg, rgba(12, 22, 38, 0.96), rgba(8, 15, 28, 0.98));
  --message-knowledge-citation-surface-strong:
    linear-gradient(180deg, rgba(16, 31, 53, 0.96), rgba(10, 20, 35, 0.98));
  --message-knowledge-citation-text: #e6f2ff;
  --message-knowledge-citation-muted: #8aa1c0;
  --message-knowledge-citation-accent: #58d0ff;
  box-shadow:
    0 16px 34px rgba(2, 8, 20, 0.26),
    inset 0 0 0 1px rgba(255, 255, 255, 0.03);
}

:global(:root[data-user-theme='dark'][data-user-accent='tech-blue'] .message-knowledge-citation-dialog.el-dialog) {
  --message-knowledge-citation-dialog-bg:
    linear-gradient(180deg, rgba(12, 22, 38, 0.98), rgba(8, 15, 28, 0.98));
  --message-knowledge-citation-dialog-surface:
    linear-gradient(180deg, rgba(16, 31, 53, 0.92), rgba(10, 20, 35, 0.96));
  --message-knowledge-citation-dialog-border: rgba(var(--ui-accent-rgb), 0.2);
  --message-knowledge-citation-dialog-text: #e6f2ff;
  --message-knowledge-citation-dialog-muted: #8aa1c0;
  box-shadow:
    0 20px 46px rgba(2, 8, 20, 0.32),
    inset 0 0 0 1px rgba(255, 255, 255, 0.03);
}

@media (max-width: 640px) {
  .message-knowledge-citation-detail__row {
    grid-template-columns: minmax(0, 1fr);
    gap: 4px;
  }
}
</style>
