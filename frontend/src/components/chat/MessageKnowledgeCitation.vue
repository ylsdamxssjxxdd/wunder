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
    class="message-knowledge-citation-dialog"
    :title="t('chat.knowledgeCitation.detailTitle')"
    width="680px"
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
  margin-top: 8px;
  border: 1px solid var(--hula-border, #dbe1ea);
  border-radius: 12px;
  padding: 8px;
  background: var(--hula-panel-bg, #f8fafc);
}

.message-knowledge-citation__title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--hula-text-secondary, #4b5563);
}

.message-knowledge-citation__list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.message-knowledge-citation__item {
  border: 1px solid var(--hula-border, #dbe1ea);
  border-radius: 10px;
  background: var(--hula-center-bg, #ffffff);
  color: inherit;
  padding: 6px 8px;
  max-width: 100%;
  text-align: left;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.message-knowledge-citation__item:hover {
  border-color: var(--ui-accent, #3b82f6);
}

.message-knowledge-citation__item-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--hula-text-primary, #111827);
}

.message-knowledge-citation__item-meta {
  font-size: 11px;
  line-height: 1.35;
  color: var(--hula-text-secondary, #4b5563);
  word-break: break-word;
}

.message-knowledge-citation-detail {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.message-knowledge-citation-detail__row {
  display: flex;
  gap: 8px;
  align-items: flex-start;
}

.message-knowledge-citation-detail__label {
  flex: 0 0 84px;
  font-size: 12px;
  color: var(--hula-text-secondary, #6b7280);
}

.message-knowledge-citation-detail__value {
  flex: 1;
  font-size: 13px;
  color: var(--hula-text-primary, #111827);
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
  border: 1px solid var(--hula-border, #dbe1ea);
  background: var(--hula-center-bg, #ffffff);
  max-height: 300px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.5;
}

.message-knowledge-citation-detail__raw summary {
  cursor: pointer;
  font-size: 12px;
  color: var(--hula-text-secondary, #4b5563);
}
</style>
