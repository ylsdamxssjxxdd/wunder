import type { ToolWorkflowStructuredGroup, ToolWorkflowStructuredMetric, ToolWorkflowStructuredView } from './toolWorkflowTypes';

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
  const meta = asObject(dataObject.meta);
  const metaFiles = Array.isArray(meta?.files)
    ? (meta.files.map((item) => asObject(item)).filter(Boolean) as UnknownObject[])
    : [];

  if (!sections.length && !metaFiles.length) return null;

  const fileMetaByPath = new Map<string, UnknownObject>();
  metaFiles.forEach((item) => {
    const path = pickString(item.path);
    if (path) fileMetaByPath.set(path, item);
  });

  // Group file reads by file path so users can scan file-by-file instead of parsing one large blob.
  const groups: ToolWorkflowStructuredGroup[] = sections.slice(0, READ_FILE_LIMIT).map((section, index) => {
    const fileMeta = fileMetaByPath.get(section.path);
    const readLines = toInt(fileMeta?.read_lines);
    const totalLines = toInt(fileMeta?.total_lines);
    const binary = fileMeta?.binary === true;
    const metaText = binary
      ? t('chat.toolWorkflow.detail.binary')
      : readLines > 0 && totalLines > 0
        ? `${readLines}/${totalLines}`
        : totalLines > 0
          ? `${totalLines}`
          : '';
    return {
      key: `read-${index}`,
      rows: [
        {
          key: `read-row-${index}`,
          title: section.path || '(unknown)',
          meta: metaText,
          body: binary ? '' : truncateText(section.body),
          mono: true
        }
      ]
    };
  });

  if (!groups.length && metaFiles.length) {
    metaFiles.slice(0, READ_FILE_LIMIT).forEach((item, index) => {
      const path = pickString(item.path);
      const readLines = toInt(item.read_lines);
      const totalLines = toInt(item.total_lines);
      const binary = item.binary === true;
      groups.push({
        key: `read-meta-${index}`,
        rows: [
          {
            key: `read-meta-row-${index}`,
            title: path || '(unknown)',
            meta: binary
              ? t('chat.toolWorkflow.detail.binary')
              : readLines > 0 && totalLines > 0
                ? `${readLines}/${totalLines}`
                : totalLines > 0
                  ? `${totalLines}`
                  : ''
          }
        ]
      });
    });
  }

  const metrics = [
    buildMetric('files', t('chat.toolWorkflow.detail.files'), metaFiles.length || sections.length)
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  return {
    variant: 'read',
    metrics,
    groups
  };
};

const buildListStructuredView = (
  dataObject: UnknownObject,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const items = Array.isArray(dataObject.items) ? dataObject.items : [];
  const rows = items
    .map((item, index) => String(item || '').trim())
    .filter(Boolean)
    .slice(0, LIST_ITEM_LIMIT)
    .map((title, index) => ({
      key: `list-row-${index}`,
      title,
      mono: true
    }));
  if (!rows.length) return null;
  return {
    variant: 'list',
    metrics: [
      buildMetric('items', t('chat.toolWorkflow.detail.items'), rows.length)
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
  const rawHits = Array.isArray(dataObject.hits) && dataObject.hits.length > 0
    ? dataObject.hits
    : Array.isArray(dataObject.matches)
      ? dataObject.matches
      : [];
  const hits = rawHits
    .map(parseSearchHit)
    .filter(Boolean)
    .slice(0, SEARCH_HIT_LIMIT) as SearchHit[];
  if (!hits.length) return null;

  const grouped = new Map<string, SearchHit[]>();
  hits.forEach((hit) => {
    const key = hit.path || '(matches)';
    if (!grouped.has(key)) grouped.set(key, []);
    grouped.get(key)?.push(hit);
  });

  const groups = Array.from(grouped.entries())
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
    }));

  const metrics = [
    buildMetric('hits', t('chat.toolWorkflow.detail.hits'), hits.length),
    buildMetric('scanned', t('chat.toolWorkflow.detail.scannedFiles'), toInt(dataObject.scanned_files))
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  return {
    variant: 'search',
    metrics,
    groups
  };
};

const buildWriteStructuredView = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const firstResult = Array.isArray(dataObject.results)
    ? (dataObject.results.find((item) => asObject(item)) as UnknownObject | undefined)
    : undefined;
  const path = pickString(
    firstResult?.path,
    firstResult?.file,
    firstResult?.file_path,
    dataObject.path,
    dataObject.file,
    dataObject.file_path,
    resultObject?.path,
    resultObject?.file,
    resultObject?.file_path,
    dataObject.target
  );
  if (!path) return null;
  const bytes = toInt(
    firstResult?.bytes,
    firstResult?.written_bytes,
    dataObject.bytes,
    dataObject.written_bytes,
    resultObject?.bytes,
    resultObject?.written_bytes
  );
  return {
    variant: 'write',
    metrics: [
      buildMetric('bytes', t('chat.toolWorkflow.detail.bytes'), bytes > 0 ? bytes : '')
    ].filter(Boolean) as ToolWorkflowStructuredMetric[],
    groups: [
      {
        key: 'write',
        rows: [
          {
            key: 'write-row',
            title: path,
            mono: true
          }
        ]
      }
    ]
  };
};

export const buildStructuredToolResultView = (
  toolName: string,
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  t: Translate
): ToolWorkflowStructuredView | null => {
  if (!dataObject) return null;
  if (isReadFileTool(toolName)) return buildReadStructuredView(dataObject, t);
  if (isListFilesTool(toolName)) return buildListStructuredView(dataObject, t);
  if (isSearchContentTool(toolName)) return buildSearchStructuredView(dataObject, t);
  if (isWriteFileTool(toolName)) return buildWriteStructuredView(resultObject, dataObject, t);
  return null;
};

export const buildStructuredToolResultNote = (
  toolName: string,
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  t: Translate
): string => {
  if (!dataObject) return '';
  if (isReadFileTool(toolName)) {
    const meta = asObject(dataObject.meta);
    const metaFiles = Array.isArray(meta?.files) ? meta.files.length : 0;
    const sections = parseReadSections(pickString(dataObject.content));
    const count = metaFiles || sections.length;
    return count > 0 ? `${t('chat.toolWorkflow.detail.files')} ${count}` : '';
  }
  if (isListFilesTool(toolName)) {
    const count = Array.isArray(dataObject.items) ? dataObject.items.length : 0;
    return count > 0 ? `${t('chat.toolWorkflow.detail.items')} ${count}` : '';
  }
  if (isSearchContentTool(toolName)) {
    const count = Array.isArray(dataObject.hits)
      ? dataObject.hits.length
      : Array.isArray(dataObject.matches)
        ? dataObject.matches.length
        : 0;
    return count > 0 ? `${t('chat.toolWorkflow.detail.hits')} ${count}` : '';
  }
  if (isWriteFileTool(toolName)) {
    const firstResult = Array.isArray(dataObject.results)
      ? (dataObject.results.find((item) => asObject(item)) as UnknownObject | undefined)
      : undefined;
    const bytes = toInt(
      firstResult?.bytes,
      firstResult?.written_bytes,
      dataObject.bytes,
      dataObject.written_bytes,
      resultObject?.bytes,
      resultObject?.written_bytes
    );
    return bytes > 0 ? `${t('chat.toolWorkflow.detail.bytes')} ${bytes}` : '';
  }
  return '';
};
