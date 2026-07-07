import type { ToolWorkflowStructuredGroup, ToolWorkflowStructuredMetric, ToolWorkflowStructuredView } from './toolWorkflowTypes';
import { normalizeToolResultDataObject } from './toolWorkflowResultPayload';

type UnknownObject = Record<string, unknown>;

type Translate = (key: string, params?: Record<string, unknown>) => string;

type ReadSection = {
  path: string;
  body: string;
};

type SearchHit = {
  path: string;
  line: number | null;
  content: string;
};

const READ_FILE_LIMIT = 8;
const LIST_ITEM_LIMIT = 80;
const SEARCH_GROUP_LIMIT = 8;
const SEARCH_HIT_LIMIT = 24;
const DATABASE_ROW_LIMIT = 12;
const DATABASE_CELL_LIMIT = 160;
const KNOWLEDGE_CHUNK_LIMIT = 8;
const SNIPPET_MAX_CHARS = 1400;

const asObject = (value: unknown): UnknownObject | null =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as UnknownObject) : null;

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
};

const toInt = (...values: unknown[]): number => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      return Math.floor(value);
    }
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed) && parsed >= 0) return parsed;
    }
  }
  return 0;
};

const parseJsonlRows = (value: unknown): string[] => {
  if (typeof value !== 'string') return [];
  return value
    .replace(/\r\n/g, '\n')
    .replace(/\r/g, '\n')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
};

const normalizeListFileItems = (
  items: unknown[]
): { rows: string[]; omittedItems: number } => {
  const rows: string[] = [];
  let omittedItems = 0;
  for (const item of items) {
    const obj = asObject(item);
    if (obj) {
      // Tool-result truncation can inject marker objects into arrays; convert markers to omission counts.
      const isTruncationMarker =
        Object.prototype.hasOwnProperty.call(obj, 'truncated_items') ||
        Object.prototype.hasOwnProperty.call(obj, 'omitted_items') ||
        obj.__truncated === true;
      if (isTruncationMarker) {
        omittedItems += toInt(obj.truncated_items, obj.omitted_items, obj.__omitted_items);
        continue;
      }
      const pathLike = pickString(obj.path, obj.file, obj.file_path, obj.name, obj.title);
      if (pathLike) {
        rows.push(pathLike);
        continue;
      }
      const serialized = JSON.stringify(obj);
      if (serialized && serialized !== '{}') {
        rows.push(serialized);
      }
      continue;
    }
    const text = String(item ?? '').trim();
    if (text) rows.push(text);
  }
  return { rows, omittedItems };
};

const toOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      return Math.floor(value);
    }
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed) && parsed >= 0) return parsed;
    }
  }
  return null;
};

const truncateText = (value: string, maxChars = SNIPPET_MAX_CHARS): string => {
  const normalized = String(value || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
  if (!normalized) return '';
  const chars = Array.from(normalized);
  if (chars.length <= maxChars) return normalized;
  const headChars = Math.max(1, Math.floor(maxChars * 0.62));
  const tailChars = Math.max(1, maxChars - headChars);
  const omittedChars = Math.max(chars.length - headChars - tailChars, 0);
  return `${chars.slice(0, headChars).join('')}\n... (${omittedChars} chars omitted)\n${chars
    .slice(chars.length - tailChars)
    .join('')}`;
};

const normalizeToolName = (toolName: string): string => String(toolName || '').trim().toLowerCase();

const isReadFileTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'read_file' || toolName.includes('读取文件');
};

const isListFilesTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'list_files' || toolName.includes('列出文件');
};

const isSearchContentTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'search_content' || toolName.includes('搜索内容');
};

const isWriteFileTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'write_file' || toolName.includes('写入文件');
};

const isDatabaseQueryTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return (
    normalized === 'db_query' ||
    normalized.startsWith('db_query_') ||
    normalized.endsWith('@db_query') ||
    normalized.includes('@db_query_')
  );
};

const isKnowledgeQueryTool = (toolName: string): boolean => {
  const normalized = normalizeToolName(toolName);
  return (
    normalized === 'kb_query' ||
    normalized.startsWith('kb_query_') ||
    normalized.endsWith('@kb_query') ||
    normalized.includes('@kb_query_')
  );
};

const buildMetric = (
  key: string,
  label: string,
  value: unknown,
  tone: ToolWorkflowStructuredMetric['tone'] = 'default'
): ToolWorkflowStructuredMetric | null => {
  const text = String(value ?? '').trim();
  if (!text) return null;
  return { key, label, value: text, tone };
};

const parseReadSections = (content: string): ReadSection[] => {
  const normalized = String(content || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (!normalized.trim()) return [];
  const chunks = normalized.split(/\n(?=>>> )/g);
  const sections: ReadSection[] = [];
  chunks.forEach((chunk) => {
    const text = chunk.trim();
    if (!text.startsWith('>>> ')) return;
    const firstBreak = text.indexOf('\n');
    if (firstBreak < 0) {
      sections.push({ path: text.slice(4).trim(), body: '' });
      return;
    }
    sections.push({
      path: text.slice(4, firstBreak).trim(),
      body: text.slice(firstBreak + 1).trim()
    });
  });
  return sections;
};

const buildReadStructuredView = (
  dataObject: UnknownObject,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const content = pickString(dataObject.content);
  const sections = parseReadSections(content);

  if (!sections.length) return null;

  const groups: ToolWorkflowStructuredGroup[] = sections.slice(0, READ_FILE_LIMIT).map((section, index) => {
    return {
      key: `read-${index}`,
      rows: [
        {
          key: `read-row-${index}`,
          title: '',
          body: truncateText(section.body),
          mono: true
        }
      ]
    };
  });

  return {
    variant: 'read',
    metrics: [],
    groups
  };
};

const buildListStructuredView = (
  dataObject: UnknownObject,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const items = Array.isArray(dataObject.items)
    ? dataObject.items
    : parseJsonlRows(dataObject.items_jsonl);
  const normalized = normalizeListFileItems(items);
  const declaredCount = toInt(dataObject.items_count);
  const hiddenByCount =
    declaredCount > normalized.rows.length + normalized.omittedItems
      ? declaredCount - normalized.rows.length - normalized.omittedItems
      : 0;
  const rows: ToolWorkflowStructuredGroup['rows'] = normalized.rows
    .slice(0, LIST_ITEM_LIMIT)
    .map((title, index) => ({
      key: `list-row-${index}`,
      title,
      mono: true
    }));
  if (normalized.omittedItems > 0 || hiddenByCount > 0) {
    const omitted = normalized.omittedItems + hiddenByCount;
    rows.push({
      key: 'list-omitted-items',
      title: `... (+${omitted} items omitted)`,
      mono: true,
      tone: 'warning'
    });
  }
  if (!rows.length) return null;
  const itemCount = declaredCount || normalized.rows.length + normalized.omittedItems + hiddenByCount;
  return {
    variant: 'list',
    metrics: [
      buildMetric('items', t('chat.toolWorkflow.detail.items'), itemCount)
    ].filter(Boolean) as ToolWorkflowStructuredMetric[],
    groups: [{ key: 'list', rows }]
  };
};

const parseSearchHit = (value: unknown): SearchHit | null => {
  const obj = asObject(value);
  if (obj) {
    return {
      path: pickString(obj.path),
      line: toInt(obj.line) || null,
      content: pickString(obj.content)
    };
  }
  const text = String(value || '').trim();
  if (!text) return null;
  const match = text.match(/^(.+?):(\d+):(.*)$/);
  if (!match) {
    return { path: '', line: null, content: text };
  }
  return {
    path: match[1].trim(),
    line: Number.parseInt(match[2], 10) || null,
    content: match[3].trim()
  };
};

const buildSearchStructuredView = (
  dataObject: UnknownObject,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const summary = asObject(dataObject.summary);
  const rawHits =
    Array.isArray(dataObject.hits) && dataObject.hits.length > 0
      ? dataObject.hits
      : Array.isArray(dataObject.matches) && dataObject.matches.length > 0
        ? dataObject.matches
        : parseJsonlRows(dataObject.hits_jsonl).length > 0
          ? parseJsonlRows(dataObject.hits_jsonl)
          : parseJsonlRows(dataObject.matches_jsonl);
  const hits = rawHits
    .map(parseSearchHit)
    .filter(Boolean)
    .slice(0, SEARCH_HIT_LIMIT) as SearchHit[];

  const groups: ToolWorkflowStructuredGroup[] = [];
  if (hits.length) {
    const grouped = new Map<string, SearchHit[]>();
    hits.forEach((hit) => {
      const key = hit.path || '(matches)';
      if (!grouped.has(key)) grouped.set(key, []);
      grouped.get(key)?.push(hit);
    });

    groups.push(
      ...Array.from(grouped.entries())
        .slice(0, SEARCH_GROUP_LIMIT)
        .map(([path, groupHits], index) => ({
          key: `search-${index}`,
          title: path,
          rows: groupHits.map((hit, rowIndex) => ({
            key: `search-row-${index}-${rowIndex}`,
            title: hit.line !== null ? `#${hit.line}` : path,
            body: truncateText(hit.content, 600),
            mono: true
          }))
        }))
    );
  }

  const infoRows: ToolWorkflowStructuredGroup['rows'] = [];
  const scopeNote = pickString(dataObject.scope_note);
  const nextHint = pickString(summary?.next_hint);
  if (!hits.length && scopeNote) {
    infoRows.push({
      key: 'search-scope-note',
      title: scopeNote,
      tone: 'warning'
    });
  }
  if (nextHint) {
    infoRows.push({
      key: 'search-next-hint',
      title: nextHint,
      tone: 'warning'
    });
  }
  if (infoRows.length) {
    groups.push({
      key: 'search-info',
      rows: infoRows
    });
  }

  const metrics = [
    buildMetric(
      'hits',
      t('chat.toolWorkflow.detail.hits'),
      toInt(
        dataObject.returned_match_count,
        dataObject.hits_count,
        dataObject.matches_count
      ) || hits.length
    ),
    buildMetric('scanned', t('chat.toolWorkflow.detail.scannedFiles'), toInt(dataObject.scanned_files))
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  if (!groups.length) return null;
  return {
    variant: 'search',
    metrics,
    groups
  };
};

const buildWriteStructuredView = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject,
  t: Translate,
  callArgs: UnknownObject | null = null
): ToolWorkflowStructuredView | null => {
  const firstResult = Array.isArray(dataObject.results)
    ? (dataObject.results.find((item) => asObject(item)) as UnknownObject | undefined)
    : undefined;
  const preview = truncateText(
    pickString(
      callArgs?.content,
      callArgs?.text,
      callArgs?.input,
      callArgs?.body,
      firstResult?.content_preview,
      firstResult?.content,
      firstResult?.text,
      firstResult?.preview,
      dataObject.content_preview,
      dataObject.content,
      dataObject.text,
      dataObject.preview,
      resultObject?.content_preview,
      resultObject?.content,
      resultObject?.text,
      resultObject?.preview
    )
  );
  if (!preview) return null;
  return {
    variant: 'write',
    metrics: [],
    groups: [
      {
        key: 'write',
        rows: [
          {
            key: 'write-row',
            title: '',
            body: preview,
            mono: true
          }
        ]
      }
    ]
  };
};

const parseJsonValue = (value: string): unknown => {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
};

const parseObjectJsonlRows = (value: unknown): UnknownObject[] => {
  if (typeof value !== 'string') return [];
  return parseJsonlRows(value)
    .map(parseJsonValue)
    .map(asObject)
    .filter(Boolean) as UnknownObject[];
};

const parseColumnNames = (value: unknown): string[] => {
  if (Array.isArray(value)) {
    return value
      .map((item) => pickString(item))
      .filter(Boolean);
  }
  if (typeof value === 'string') {
    const parsed = parseJsonValue(value);
    if (Array.isArray(parsed)) {
      return parsed.map((item) => pickString(item)).filter(Boolean);
    }
    return value
      .split(/[,\n\r\t|]+/g)
      .map((item) => item.trim())
      .filter(Boolean);
  }
  return [];
};

const formatDbCellValue = (value: unknown): string => {
  if (value === null || value === undefined) return 'null';
  if (typeof value === 'string') return truncateText(value, DATABASE_CELL_LIMIT);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return truncateText(JSON.stringify(value), DATABASE_CELL_LIMIT);
  } catch {
    return truncateText(String(value), DATABASE_CELL_LIMIT);
  }
};

const buildDatabaseStructuredView = (
  dataObject: UnknownObject,
  t: Translate,
  callArgs: UnknownObject | null
): ToolWorkflowStructuredView | null => {
  const rows = Array.isArray(dataObject.rows)
    ? (dataObject.rows.map(asObject).filter(Boolean) as UnknownObject[])
    : parseObjectJsonlRows(dataObject.rows_jsonl);
  const columns = parseColumnNames(dataObject.columns ?? dataObject.columns_jsonl);
  const declaredRows = toInt(dataObject.row_count, dataObject.rows_count);
  const declaredColumns = toInt(dataObject.columns_count);
  const table = pickString(dataObject.table, callArgs?.table);
  const sql = pickString(dataObject.sql, callArgs?.sql);
  const query = pickString(callArgs?.query);
  const elapsed = pickString(dataObject.elapsed_ms);
  const truncated = dataObject.truncated === true;

  const metrics = [
    buildMetric('rows', t('chat.toolWorkflow.detail.rows'), declaredRows || rows.length),
    buildMetric('columns', t('chat.toolWorkflow.detail.columns'), declaredColumns || columns.length),
    buildMetric('table', t('chat.toolWorkflow.detail.table'), table),
    buildMetric('elapsed', t('chat.toolWorkflow.detail.elapsed'), elapsed ? `${elapsed}ms` : ''),
    buildMetric(
      'truncated',
      t('chat.toolWorkflow.detail.truncated'),
      truncated ? t('common.yes') : '',
      'warning'
    )
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  const infoRows: ToolWorkflowStructuredGroup['rows'] = [];
  if (query) {
    infoRows.push({
      key: 'db-query',
      title: `${t('chat.toolWorkflow.detail.query')}: ${truncateText(query, 360)}`
    });
  }
  if (sql) {
    infoRows.push({
      key: 'db-sql',
      title: t('chat.toolWorkflow.detail.sql'),
      body: truncateText(sql, 900),
      mono: true
    });
  }

  const rowViews: ToolWorkflowStructuredGroup['rows'] = rows.slice(0, DATABASE_ROW_LIMIT).map((row, index) => {
    const keys = columns.length > 0 ? columns : Object.keys(row);
    const parts = keys
      .map((key) => `${key}: ${formatDbCellValue(row[key])}`)
      .filter(Boolean);
    return {
      key: `db-row-${index}`,
      title: `${t('chat.toolWorkflow.detail.row')} ${index + 1}`,
      body: parts.join('\n'),
      mono: true
    };
  });

  if (declaredRows > rowViews.length) {
    rowViews.push({
      key: 'db-omitted-rows',
      title: `... (+${declaredRows - rowViews.length} rows omitted)`,
      tone: 'warning'
    });
  }

  const groups: ToolWorkflowStructuredGroup[] = [];
  if (infoRows.length) groups.push({ key: 'db-info', rows: infoRows });
  if (rowViews.length) {
    groups.push({
      key: 'db-rows',
      title: t('chat.toolWorkflow.detail.rows'),
      rows: rowViews
    });
  }

  if (!metrics.length && !groups.length) return null;
  return {
    variant: 'database',
    metrics,
    groups
  };
};

const buildKnowledgeStructuredView = (
  dataObject: UnknownObject,
  t: Translate,
  callArgs: UnknownObject | null
): ToolWorkflowStructuredView | null => {
  const chunks = Array.isArray(dataObject.chunks)
    ? (dataObject.chunks.map(asObject).filter(Boolean) as UnknownObject[])
    : parseObjectJsonlRows(dataObject.chunks_jsonl);
  const documents = Array.isArray(dataObject.documents)
    ? (dataObject.documents.map(asObject).filter(Boolean) as UnknownObject[])
    : parseObjectJsonlRows(dataObject.documents_jsonl);
  const total = toInt(dataObject.total, dataObject.chunks_count);
  const elapsed = pickString(dataObject.elapsed_ms);
  const query = pickString(callArgs?.query, callArgs?.question);

  const metrics = [
    buildMetric('hits', t('chat.toolWorkflow.detail.hits'), total || chunks.length),
    buildMetric('documents', t('chat.toolWorkflow.detail.documents'), documents.length),
    buildMetric('elapsed', t('chat.toolWorkflow.detail.elapsed'), elapsed ? `${elapsed}ms` : '')
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  const groups: ToolWorkflowStructuredGroup[] = [];
  if (query) {
    groups.push({
      key: 'kb-query',
      rows: [
        {
          key: 'kb-query-row',
          title: `${t('chat.toolWorkflow.detail.query')}: ${truncateText(query, 360)}`
        }
      ]
    });
  }

  if (chunks.length) {
    groups.push({
      key: 'kb-chunks',
      title: t('chat.toolWorkflow.detail.hits'),
      rows: chunks.slice(0, KNOWLEDGE_CHUNK_LIMIT).map((chunk, index) => {
        const documentName = pickString(chunk.document_name, chunk.document, chunk.title);
        const score = pickString(chunk.similarity, chunk.score);
        return {
          key: `kb-chunk-${index}`,
          title: documentName || `${t('chat.toolWorkflow.detail.hit')} ${index + 1}`,
          meta: score ? `score ${score}` : '',
          body: truncateText(
            pickString(chunk.highlight, chunk.content, chunk.text, chunk.answer),
            900
          )
        };
      })
    });
  }

  if (documents.length) {
    groups.push({
      key: 'kb-documents',
      title: t('chat.toolWorkflow.detail.documents'),
      rows: documents.slice(0, KNOWLEDGE_CHUNK_LIMIT).map((document, index) => ({
        key: `kb-document-${index}`,
        title: pickString(document.name, document.title, document.id) || `${t('chat.toolWorkflow.detail.document')} ${index + 1}`,
        meta: pickString(document.count)
      }))
    });
  }

  if (!metrics.length && !groups.length) return null;
  return {
    variant: 'knowledge',
    metrics,
    groups
  };
};

export const buildStructuredToolResultView = (
  toolName: string,
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  t: Translate,
  callArgs: UnknownObject | null = null
): ToolWorkflowStructuredView | null => {
  const normalizedDataObject = normalizeToolResultDataObject(dataObject);
  if (!normalizedDataObject) return null;
  if (isReadFileTool(toolName)) return buildReadStructuredView(normalizedDataObject, t);
  if (isListFilesTool(toolName)) return buildListStructuredView(normalizedDataObject, t);
  if (isSearchContentTool(toolName)) return buildSearchStructuredView(normalizedDataObject, t);
  if (isWriteFileTool(toolName)) return buildWriteStructuredView(resultObject, normalizedDataObject, t, callArgs);
  if (isDatabaseQueryTool(toolName)) return buildDatabaseStructuredView(normalizedDataObject, t, callArgs);
  if (isKnowledgeQueryTool(toolName)) return buildKnowledgeStructuredView(normalizedDataObject, t, callArgs);
  return null;
};

export const buildStructuredToolResultNote = (
  toolName: string,
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  t: Translate
): string => {
  const normalizedDataObject = normalizeToolResultDataObject(dataObject);
  if (!normalizedDataObject) return '';
  if (isReadFileTool(toolName)) {
    const meta = asObject(normalizedDataObject.meta);
    const metaFiles = Array.isArray(meta?.files) ? meta.files.length : 0;
    const sections = parseReadSections(pickString(normalizedDataObject.content));
    const count = metaFiles || sections.length;
    return count > 0 ? `${t('chat.toolWorkflow.detail.files')} ${count}` : '';
  }
  if (isListFilesTool(toolName)) {
    const count =
      toInt(normalizedDataObject.items_count)
      || (Array.isArray(normalizedDataObject.items)
        ? normalizedDataObject.items.length
        : parseJsonlRows(normalizedDataObject.items_jsonl).length);
    return count > 0 ? `${t('chat.toolWorkflow.detail.items')} ${count}` : '';
  }
  if (isSearchContentTool(toolName)) {
    const count =
      toInt(normalizedDataObject.returned_match_count, normalizedDataObject.hits_count, normalizedDataObject.matches_count)
      || (Array.isArray(normalizedDataObject.hits)
        ? normalizedDataObject.hits.length
        : Array.isArray(normalizedDataObject.matches)
          ? normalizedDataObject.matches.length
          : parseJsonlRows(normalizedDataObject.hits_jsonl).length || parseJsonlRows(normalizedDataObject.matches_jsonl).length);
    const scanned = toInt(normalizedDataObject.scanned_files);
    if (count > 0 && scanned > 0) {
      return `${t('chat.toolWorkflow.detail.hits')} ${count} · ${t('chat.toolWorkflow.detail.scannedFiles')} ${scanned}`;
    }
    if (count > 0) return `${t('chat.toolWorkflow.detail.hits')} ${count}`;
    if (scanned > 0) return `${t('chat.toolWorkflow.detail.scannedFiles')} ${scanned}`;
    return '';
  }
  if (isWriteFileTool(toolName)) {
    return '';
  }
  return '';
};
