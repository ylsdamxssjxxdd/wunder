// 系统提示词高亮工具：仅输出已转义的 HTML，避免 XSS 风险
const escapeHtml = (text) =>
  String(text ?? '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

// 规范化工具名称列表，兼容字符串/对象
const normalizeToolNames = (list) => {
  if (!Array.isArray(list)) {
    return [];
  }
  return list
    .map((item) => {
      if (!item) return '';
      if (typeof item === 'string') return item;
      return item.name || item.tool_name || item.toolName || item.id || '';
    })
    .map((name) => String(name).trim())
    .filter(Boolean);
};

// 渲染系统提示词非 tools 段落，识别技能区并高亮技能条目
const renderPromptSegmentWithSkills = (segment, segmentState) => {
  const skillHeaders = new Set(['[Mounted Skills]', '[已挂载技能]']);
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

// 渲染系统提示词，按 Wunder 的规则高亮 tools 段内工具名称与技能名称
export const renderSystemPromptHighlight = (rawText, toolsPayload = {}) => {
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
  const skillState = { inSkills: false };

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
