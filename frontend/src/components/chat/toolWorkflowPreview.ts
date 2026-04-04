type UnknownObject = Record<string, unknown>;

const PREVIEW_BLOCK_LIMIT = 4;
const PREVIEW_TEXT_LINES = 10;
const PREVIEW_TEXT_CHARS = 1600;
const PREVIEW_JSON_LINES = 14;
const PREVIEW_JSON_CHARS = 2000;
const PREVIEW_STRING_LIMIT = 360;
const PREVIEW_ARRAY_LIMIT = 6;
const PREVIEW_OBJECT_LIMIT = 12;
const PREVIEW_DEPTH_LIMIT = 3;

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim();
    }
  }
  return '';
};

const truncateSingleLine = (text: string, maxLength = PREVIEW_STRING_LIMIT): string => {
  const normalized = String(text || '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, maxLength)}...`;
};

const buildTextPreview = (
  text: string,
  maxLines = PREVIEW_TEXT_LINES,
  maxChars = PREVIEW_TEXT_CHARS,
  indent = '  '
): string => {
  const normalized = String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
  if (!normalized) return '';

  const chars = Array.from(normalized);
  let clipped = normalized;
  if (chars.length > maxChars) {
    const headChars = Math.max(1, Math.floor(maxChars * 0.62));
    const tailChars = Math.max(1, maxChars - headChars);
    const omittedChars = Math.max(chars.length - headChars - tailChars, 0);
    clipped = `${chars.slice(0, headChars).join('')}\n... (${omittedChars} chars omitted)\n${chars
      .slice(chars.length - tailChars)
      .join('')}`;
  }

  const lines = clipped.split('\n');
  const keepLines = Math.max(maxLines, 1);
  let lineText = clipped;
  if (lines.length > keepLines) {
    const headLines = Math.max(1, Math.floor(keepLines * 0.62));
    const tailLines = Math.max(1, keepLines - headLines);
    const omittedLines = Math.max(lines.length - headLines - tailLines, 0);
    lineText = [
      ...lines.slice(0, headLines),
      `... (${omittedLines} lines omitted)`,
      ...lines.slice(lines.length - tailLines)
    ].join('\n');
  }
  const rows = lineText.split('\n').map((line, index) => (index === 0 ? line : `${indent}${line}`));
  return rows.join('\n');
};

const looksBinaryText = (value: string): boolean => {
  if (value.length < 240) return false;
  if (/\s/.test(value)) return false;
  return /^[A-Za-z0-9+/=]+$/.test(value);
};

const compactPreviewValue = (value: unknown, depth = 0): unknown => {
  if (depth >= PREVIEW_DEPTH_LIMIT) {
    if (typeof value === 'string') return truncateSingleLine(value);
    if (Array.isArray(value)) return `[${value.length} items]`;
    if (asObject(value)) return '[object]';
    return value ?? null;
  }
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') {
    if (looksBinaryText(value)) return `[binary ${value.length} chars]`;
    return truncateSingleLine(value);
  }
  if (typeof value === 'number' || typeof value === 'boolean') return value;
  if (Array.isArray(value)) {
    const items = value.slice(0, PREVIEW_ARRAY_LIMIT).map((item) => compactPreviewValue(item, depth + 1));
    if (value.length > items.length) {
      items.push(`... (+${value.length - items.length} items)`);
    }
    return items;
  }
  const obj = asObject(value);
  if (obj) {
    const keys = Object.keys(obj);
    const output: UnknownObject = {};
    keys.slice(0, PREVIEW_OBJECT_LIMIT).forEach((key) => {
      output[key] = compactPreviewValue(obj[key], depth + 1);
    });
    if (keys.length > PREVIEW_OBJECT_LIMIT) {
      output.__more__ = `+${keys.length - PREVIEW_OBJECT_LIMIT} keys`;
    }
    return output;
  }
  return String(value);
};

const buildJsonPreview = (value: unknown, maxLines = PREVIEW_JSON_LINES, maxChars = PREVIEW_JSON_CHARS): string => {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return buildTextPreview(value, maxLines, maxChars, '  ');
  try {
    const compacted = compactPreviewValue(value);
    return buildTextPreview(JSON.stringify(compacted, null, 2), maxLines, maxChars, '  ');
  } catch {
    return buildTextPreview(String(value), maxLines, maxChars, '  ');
  }
};

const formatContentBlock = (block: unknown): string => {
  if (typeof block === 'string') {
    return buildTextPreview(block, PREVIEW_TEXT_LINES, PREVIEW_TEXT_CHARS, '  ');
  }
  const obj = asObject(block);
  if (!obj) return '';
  const type = String(obj.type || '').trim().toLowerCase();
  const text = pickString(obj.text, obj.content, obj.value);
  if (text) {
    const preview = buildTextPreview(text, PREVIEW_TEXT_LINES, PREVIEW_TEXT_CHARS, '  ');
    if (!preview) return '';
    return type && type !== 'text' ? `${type}\n${preview}` : preview;
  }
  if (type === 'image') {
    const mime = pickString(obj.mimeType, obj.mime_type);
    const size = typeof obj.data === 'string' ? obj.data.length : 0;
    const meta = [mime && `mime=${mime}`, size ? `chars=${size}` : ''].filter(Boolean).join(' ');
    return meta ? `[image ${meta}]` : '[image]';
  }
  if (type === 'resource') {
    const uri = pickString(obj.uri, obj.url, obj.name);
    return uri ? `[resource ${uri}]` : '[resource]';
  }
  if (type) {
    const preview = buildJsonPreview(obj, 8, 900);
    return preview ? `${type}\n${preview}` : `[${type}]`;
  }
  const preview = buildJsonPreview(obj, 8, 900);
  return preview || '';
};

const buildContentBlocksPreview = (content: unknown): string => {
  if (!Array.isArray(content) || content.length === 0) return '';
  const blocks: string[] = [];
  content.slice(0, PREVIEW_BLOCK_LIMIT).forEach((block) => {
    const preview = formatContentBlock(block);
    if (preview) blocks.push(preview);
  });
  if (content.length > PREVIEW_BLOCK_LIMIT) {
    blocks.push(`... (+${content.length - PREVIEW_BLOCK_LIMIT} blocks)`);
  }
  return blocks.join('\n\n');
};

const extractStructuredContent = (dataObject: UnknownObject | null, resultObject: UnknownObject | null): unknown =>
  dataObject?.structured_content ??
  dataObject?.structuredContent ??
  resultObject?.structured_content ??
  resultObject?.structuredContent ??
  null;

const pickTextCandidate = (dataObject: UnknownObject | null, resultObject: UnknownObject | null): string =>
  pickString(
    dataObject?.text,
    typeof dataObject?.content === 'string' ? dataObject?.content : '',
    dataObject?.message,
    dataObject?.answer,
    dataObject?.output,
    dataObject?.result,
    dataObject?.detail,
    resultObject?.text,
    typeof resultObject?.content === 'string' ? resultObject?.content : '',
    resultObject?.message,
    resultObject?.answer,
    resultObject?.output,
    resultObject?.result,
    resultObject?.detail
  );

export const buildToolResultPreview = (
  dataObject: UnknownObject | null,
  resultObject: UnknownObject | null
): string => {
  if (!dataObject && !resultObject) return '';

  const blocks: string[] = [];
  const contentPreview = buildContentBlocksPreview(dataObject?.content ?? resultObject?.content);
  if (contentPreview) {
    blocks.push(contentPreview);
  }

  const structured = extractStructuredContent(dataObject, resultObject);
  if (structured !== null) {
    const preview = buildJsonPreview(structured, 10, 1400);
    if (preview) {
      blocks.push(`structured_content\n${preview}`);
    }
  }

  if (blocks.length === 0) {
    const text = pickTextCandidate(dataObject, resultObject);
    const preview = text ? buildTextPreview(text, PREVIEW_TEXT_LINES, PREVIEW_TEXT_CHARS, '  ') : '';
    if (preview) blocks.push(preview);
  }

  if (blocks.length === 0) {
    const preview = buildJsonPreview(dataObject ?? resultObject, PREVIEW_JSON_LINES, PREVIEW_JSON_CHARS);
    if (preview) blocks.push(preview);
  }

  return blocks.join('\n\n');
};
