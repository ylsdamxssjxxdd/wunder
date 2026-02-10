type UnknownRecord = Record<string, unknown>;

type PromptSegmentState = {
  inSkills: boolean;
};

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const pickToolName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  const obj = asRecord(item);
  return String(obj.name || obj.tool_name || obj.toolName || obj.id || '');
};

const escapeHtml = (text: unknown): string =>
  String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

const normalizeToolNames = (list: unknown): string[] => {
  if (!Array.isArray(list)) {
    return [];
  }
  return list.map((item) => pickToolName(item).trim()).filter(Boolean);
};

const renderPromptSegmentWithSkills = (segment: string, segmentState: PromptSegmentState): string => {
  const skillHeaders = new Set(['[Mounted Skills]', '[\u5df2\u6302\u8f7d\u6280\u80fd]']);
  const lines = String(segment ?? '').split(/\r?\n/);
  const output = lines.map((line) => {
    const trimmed = line.trim();
    if (skillHeaders.has(trimmed)) {
      segmentState.inSkills = true;
      return escapeHtml(line);
    }
    if (trimmed.startsWith('[') && trimmed.endsWith(']') && !skillHeaders.has(trimmed)) {
      segmentState.inSkills = false;
      return escapeHtml(line);
    }
    if (segmentState.inSkills) {
      const match = line.match(/^(\s*-\s+)(.+)$/);
      if (match) {
        return `${escapeHtml(match[1])}<span class="skill-highlight">${escapeHtml(match[2])}</span>`;
      }
    }
    return escapeHtml(line);
  });
  return output.join('\n');
};

export const renderSystemPromptHighlight = (
  rawText: string,
  toolsPayload: UnknownRecord = {}
): string => {
  if (!rawText) {
    return '';
  }
  const builtinToolNames = new Set(
    normalizeToolNames(toolsPayload.builtin_tools || toolsPayload.builtinTools)
  );
  const knowledgeToolNames = new Set(
    normalizeToolNames(toolsPayload.knowledge_tools || toolsPayload.knowledgeTools)
  );
  const userToolNames = new Set(
    normalizeToolNames(toolsPayload.user_tools || toolsPayload.userTools)
  );
  const sharedToolNames = new Set(
    normalizeToolNames(toolsPayload.shared_tools || toolsPayload.sharedTools)
  );

  const startTag = '<tools>';
  const endTag = '</tools>';
  let output = '';
  let cursor = 0;
  const skillState: PromptSegmentState = { inSkills: false };

  while (true) {
    const start = rawText.indexOf(startTag, cursor);
    if (start < 0) {
      output += renderPromptSegmentWithSkills(rawText.slice(cursor), skillState);
      break;
    }
    const end = rawText.indexOf(endTag, start + startTag.length);
    if (end < 0) {
      output += renderPromptSegmentWithSkills(rawText.slice(cursor), skillState);
      break;
    }
    output += renderPromptSegmentWithSkills(rawText.slice(cursor, start), skillState);
    output += escapeHtml(startTag);
    const toolsContent = rawText.slice(start + startTag.length, end);
    const lines = toolsContent.split(/\r?\n/);
    const highlighted = lines
      .map((line) => {
        const match = line.match(/"name"\s*:\s*"([^"]+)"/);
        const escapedLine = escapeHtml(line);
        if (!match) {
          return escapedLine;
        }
        const escapedMatch = escapeHtml(match[0]);
        const escapedName = escapeHtml(match[1]);
        let highlightClass = 'tool-highlight';
        if (builtinToolNames.has(match[1])) {
          highlightClass = 'tool-highlight builtin';
        } else if (knowledgeToolNames.has(match[1])) {
          highlightClass = 'tool-highlight knowledge';
        } else if (userToolNames.has(match[1])) {
          highlightClass = 'tool-highlight user';
        } else if (sharedToolNames.has(match[1])) {
          highlightClass = 'tool-highlight shared';
        }
        const highlightedMatch = escapedMatch.replace(
          escapedName,
          `<span class="${highlightClass}">${escapedName}</span>`
        );
        return escapedLine.replace(escapedMatch, highlightedMatch);
      })
      .join('\n');
    output += highlighted;
    output += escapeHtml(endTag);
    cursor = end + endTag.length;
  }
  return output;
};

export { escapeHtml };
