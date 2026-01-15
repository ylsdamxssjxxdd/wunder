import MarkdownIt from 'markdown-it';

// 统一的 Markdown 渲染器：禁用原始 HTML，启用自动换行与链接识别
const markdown = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true
});

// 为所有 Markdown 表格包裹滚动容器，复刻参考项目的工业风表格样式
const defaultTableOpenRenderer =
  markdown.renderer.rules.table_open ||
  ((tokens, idx, options, env, slf) => slf.renderToken(tokens, idx, options));
const defaultTableCloseRenderer =
  markdown.renderer.rules.table_close ||
  ((tokens, idx, options, env, slf) => slf.renderToken(tokens, idx, options));

markdown.renderer.rules.table_open = (tokens, idx, options, env, slf) =>
  `<div class="ai-rich-table">${defaultTableOpenRenderer(tokens, idx, options, env, slf)}`;
markdown.renderer.rules.table_close = (tokens, idx, options, env, slf) =>
  `${defaultTableCloseRenderer(tokens, idx, options, env, slf)}</div>`;

const TABLE_LANG_HINTS = new Set(['table', 'tables', 'tab', 'markdown', 'md', 'grid', 'pipe', 'csv']);
const CODE_LANG_ALIASES = new Map([
  ['js', 'javascript'],
  ['jsx', 'javascript'],
  ['javascript', 'javascript'],
  ['ts', 'typescript'],
  ['tsx', 'typescript'],
  ['typescript', 'typescript'],
  ['json', 'json'],
  ['py', 'python'],
  ['python', 'python'],
  ['bash', 'shell'],
  ['sh', 'shell'],
  ['shell', 'shell'],
  ['zsh', 'shell'],
  ['sql', 'sql']
]);
const CODE_HIGHLIGHT_LANGS = new Set(['javascript', 'typescript', 'json', 'python', 'shell', 'sql']);
const HIGHLIGHT_CACHE_LIMIT = 120;
const HIGHLIGHT_CACHE_MAX_LENGTH = 8000;
const CODE_BLOCK_MAX_LENGTH = 20000;
const CODE_BLOCK_MAX_LINES = 400;
const NUMBER_TOKEN_REGEX = /^-?\\d/;

const SCRIPT_KEYWORDS = [
  'await',
  'async',
  'break',
  'case',
  'catch',
  'class',
  'const',
  'continue',
  'debugger',
  'default',
  'delete',
  'do',
  'else',
  'export',
  'extends',
  'finally',
  'for',
  'function',
  'if',
  'import',
  'in',
  'instanceof',
  'let',
  'new',
  'return',
  'super',
  'switch',
  'this',
  'throw',
  'try',
  'typeof',
  'var',
  'void',
  'while',
  'with',
  'yield',
  'enum',
  'implements',
  'interface',
  'private',
  'protected',
  'public',
  'readonly',
  'static',
  'abstract',
  'declare',
  'namespace',
  'module',
  'type',
  'keyof',
  'infer',
  'unknown',
  'never',
  'as',
  'satisfies'
];
const PYTHON_KEYWORDS = [
  'and',
  'as',
  'assert',
  'async',
  'await',
  'break',
  'class',
  'continue',
  'def',
  'del',
  'elif',
  'else',
  'except',
  'False',
  'finally',
  'for',
  'from',
  'global',
  'if',
  'import',
  'in',
  'is',
  'lambda',
  'None',
  'nonlocal',
  'not',
  'or',
  'pass',
  'raise',
  'return',
  'True',
  'try',
  'while',
  'with',
  'yield'
];
const SHELL_KEYWORDS = [
  'case',
  'do',
  'done',
  'elif',
  'else',
  'esac',
  'fi',
  'for',
  'function',
  'if',
  'in',
  'select',
  'then',
  'time',
  'until',
  'while'
];
const SQL_KEYWORDS = [
  'select',
  'from',
  'where',
  'insert',
  'into',
  'update',
  'delete',
  'join',
  'inner',
  'left',
  'right',
  'full',
  'on',
  'group',
  'by',
  'order',
  'having',
  'limit',
  'offset',
  'values',
  'set',
  'create',
  'table',
  'alter',
  'drop',
  'and',
  'or',
  'not',
  'null',
  'is',
  'as',
  'distinct',
  'union',
  'all'
];
const JSON_KEYWORDS = ['true', 'false', 'null'];

const CODE_KEYWORDS = {
  javascript: new Set(SCRIPT_KEYWORDS),
  typescript: new Set(SCRIPT_KEYWORDS),
  python: new Set(PYTHON_KEYWORDS.map((item) => item.toLowerCase())),
  shell: new Set(SHELL_KEYWORDS),
  sql: new Set(SQL_KEYWORDS),
  json: new Set(JSON_KEYWORDS)
};

const SCRIPT_TOKEN_REGEX = new RegExp(
  [
    '\\/\\*[\\s\\S]*?\\*\\/',
    '\\/\\/[^\\n]*',
    '`(?:\\\\.|[^`])*`',
    '"(?:\\\\.|[^"\\\\])*"',
    "'(?:\\\\.|[^'\\\\])*'",
    '\\b\\d+(?:\\.\\d+)?\\b',
    `\\b(?:${SCRIPT_KEYWORDS.join('|')})\\b`
  ].join('|'),
  'g'
);
const PYTHON_TOKEN_REGEX = new RegExp(
  [
    '"""[\\s\\S]*?"""',
    "'''[\\s\\S]*?'''",
    '#[^\\n]*',
    '"(?:\\\\.|[^"\\\\])*"',
    "'(?:\\\\.|[^'\\\\])*'",
    '\\b\\d+(?:\\.\\d+)?\\b',
    `\\b(?:${PYTHON_KEYWORDS.join('|')})\\b`
  ].join('|'),
  'g'
);
const SHELL_TOKEN_REGEX = new RegExp(
  [
    '#[^\\n]*',
    '"(?:\\\\.|[^"\\\\])*"',
    "'(?:\\\\.|[^'\\\\])*'",
    '\\b\\d+(?:\\.\\d+)?\\b',
    `\\b(?:${SHELL_KEYWORDS.join('|')})\\b`
  ].join('|'),
  'g'
);
const SQL_TOKEN_REGEX = new RegExp(
  [
    '\\/\\*[\\s\\S]*?\\*\\/',
    '--[^\\n]*',
    '"(?:\\\\.|[^"\\\\])*"',
    "'(?:\\\\.|[^'\\\\])*'",
    '\\b\\d+(?:\\.\\d+)?\\b',
    `\\b(?:${SQL_KEYWORDS.join('|')})\\b`
  ].join('|'),
  'gi'
);
const JSON_TOKEN_REGEX = new RegExp(
  [
    '"(?:\\\\.|[^"\\\\])*"',
    '\\b-?\\d+(?:\\.\\d+)?(?:[eE][+-]?\\d+)?\\b',
    `\\b(?:${JSON_KEYWORDS.join('|')})\\b`
  ].join('|'),
  'g'
);
const TOKEN_REGEX = {
  javascript: SCRIPT_TOKEN_REGEX,
  typescript: SCRIPT_TOKEN_REGEX,
  python: PYTHON_TOKEN_REGEX,
  shell: SHELL_TOKEN_REGEX,
  sql: SQL_TOKEN_REGEX,
  json: JSON_TOKEN_REGEX
};

const highlightCache = new Map();

const CODE_COPY_ICON = `
  <svg class="ai-code-copy-icon" viewBox="0 0 24 24" aria-hidden="true">
    <rect x="9" y="9" width="10" height="10" rx="2" />
    <path d="M7 15H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h7a2 2 0 0 1 2 2v1" />
  </svg>
`;

markdown.renderer.rules.fence = (tokens, idx, options, env, slf) => {
  const token = tokens[idx];
  const info = (token.info || '').trim();
  const content = token.content || '';
  const loweredInfo = info.toLowerCase();
  if (shouldRenderAsTable(loweredInfo, content)) {
    const tableHtml = buildTableHtml(content);
    if (tableHtml) {
      return tableHtml;
    }
  }
  return renderCodeBlock(content, info);
};

/**
 * 将 AI 输出的 Markdown 文本转为安全 HTML，供 v-html 渲染
 * @param {string} content 原始 Markdown 内容
 * @returns {string} 渲染后的 HTML
 */
export function renderMarkdown(content = '') {
  if (!content) return '';
  return markdown.render(String(content));
}

function renderCodeBlock(content = '', info = '') {
  const rawLang = String(info || '').trim();
  const normalizedLang = normalizeLanguage(rawLang);
  const safeLang = sanitizeLanguage(normalizedLang);
  const displayLang = sanitizeLanguage(rawLang.split(/\s+/)[0] || '');
  const highlighted = highlightCode(content, normalizedLang);
  const langLabel = displayLang ? `<span class="ai-code-lang">${escapeHtml(displayLang)}</span>` : '';
  const codeClass = safeLang ? ` class="language-${safeLang}"` : '';
  return `
    <div class="ai-code-block">
      <div class="ai-code-header">
        ${langLabel}
        <button class="ai-code-copy" type="button" aria-label="复制代码" title="复制代码">
          ${CODE_COPY_ICON}
          <span>复制</span>
        </button>
      </div>
      <pre><code${codeClass}>${highlighted}</code></pre>
    </div>
  `;
}

function normalizeLanguage(info = '') {
  const trimmed = String(info || '').trim();
  if (!trimmed) return '';
  const token = trimmed.split(/\s+/)[0].toLowerCase();
  return CODE_LANG_ALIASES.get(token) || token;
}

function sanitizeLanguage(lang = '') {
  return String(lang || '').toLowerCase().replace(/[^a-z0-9_+-]/g, '');
}

function highlightCode(code = '', lang = '') {
  if (!code) return '';
  const normalizedLang = normalizeLanguage(lang);
  if (!normalizedLang || !CODE_HIGHLIGHT_LANGS.has(normalizedLang)) {
    return escapeHtml(code);
  }
  if (code.length > CODE_BLOCK_MAX_LENGTH) {
    return escapeHtml(code);
  }
  const lines = code.split('\n').length;
  if (lines > CODE_BLOCK_MAX_LINES) {
    return escapeHtml(code);
  }
  const cacheKey = `${normalizedLang}:${code}`;
  if (code.length <= HIGHLIGHT_CACHE_MAX_LENGTH) {
    const cached = highlightCache.get(cacheKey);
    if (cached) return cached;
  }
  const regex = TOKEN_REGEX[normalizedLang];
  if (!regex) return escapeHtml(code);
  regex.lastIndex = 0;
  let result = '';
  let lastIndex = 0;
  for (const match of code.matchAll(regex)) {
    const index = match.index ?? 0;
    const token = match[0] || '';
    result += escapeHtml(code.slice(lastIndex, index));
    const tokenType = classifyToken(token, normalizedLang);
    if (tokenType) {
      result += `<span class="token-${tokenType}">${escapeHtml(token)}</span>`;
    } else {
      result += escapeHtml(token);
    }
    lastIndex = index + token.length;
  }
  result += escapeHtml(code.slice(lastIndex));
  if (code.length <= HIGHLIGHT_CACHE_MAX_LENGTH) {
    highlightCache.set(cacheKey, result);
    if (highlightCache.size > HIGHLIGHT_CACHE_LIMIT) {
      highlightCache.delete(highlightCache.keys().next().value);
    }
  }
  return result;
}

function classifyToken(token = '', lang = '') {
  if (
    token.startsWith('/*') ||
    token.startsWith('//') ||
    token.startsWith('#') ||
    token.startsWith('--')
  ) {
    return 'comment';
  }
  const firstChar = token[0];
  if (firstChar === '"' || firstChar === "'" || firstChar === '`') {
    return 'string';
  }
  if (NUMBER_TOKEN_REGEX.test(token)) {
    return 'number';
  }
  if (lang === 'json') {
    return 'keyword';
  }
  const keywordSet = CODE_KEYWORDS[lang];
  if (keywordSet && keywordSet.has(token.toLowerCase())) {
    return 'keyword';
  }
  return 'keyword';
}

function shouldRenderAsTable(info = '', content = '') {
  if (TABLE_LANG_HINTS.has(info)) return true;
  return looksLikeMarkdownTable(content);
}

function looksLikeMarkdownTable(content = '') {
  const lines = content
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length < 2) return false;
  const hasPipes = lines[0].includes('|');
  const dividerLooksLikeTable = /^[:\\-\\|\\s]+$/.test(lines[1] || '');
  return hasPipes && dividerLooksLikeTable;
}

function buildTableHtml(content = '') {
  const lines = content
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length < 2) return '';
  const headerCells = splitTableRow(lines[0]);
  if (!headerCells.length) return '';
  let bodyLines = lines.slice(1);
  let alignments = new Array(headerCells.length).fill('left');
  if (bodyLines.length && isDividerRow(bodyLines[0])) {
    alignments = parseAlignments(bodyLines[0], headerCells.length);
    bodyLines = bodyLines.slice(1);
  }
  const bodyHtml = bodyLines
    .map((row) => {
      const cells = splitTableRow(row);
      if (!cells.length) return '';
      const normalized = headerCells.map((_, idx) => escapeHtml(cells[idx] || ''));
      const cellsHtml = normalized
        .map((cell, idx) => `<td style="text-align:${alignments[idx] || 'left'};">${cell || '&nbsp;'}</td>`)
        .join('');
      return `<tr>${cellsHtml}</tr>`;
    })
    .filter(Boolean)
    .join('');
  if (!bodyHtml) return '';
  const headerHtml = headerCells
    .map((cell, idx) => `<th style="text-align:${alignments[idx] || 'left'};">${escapeHtml(cell)}</th>`)
    .join('');
  return `
    <div class="ai-rich-table">
      <table>
        <thead><tr>${headerHtml}</tr></thead>
        <tbody>${bodyHtml}</tbody>
      </table>
    </div>
  `;
}

function splitTableRow(row = '') {
  return row
    .replace(/^\\|/, '')
    .replace(/\\|$/, '')
    .split('|')
    .map((cell) => cell.trim());
}

function isDividerRow(row = '') {
  return /^[:\\-\\|\\s]+$/.test(row);
}

function parseAlignments(row = '', columnCount = 0) {
  const tokens = splitTableRow(row);
  return new Array(columnCount).fill('left').map((_, index) => {
    const token = tokens[index] || '';
    const startsWithColon = token.trim().startsWith(':');
    const endsWithColon = token.trim().endsWith(':');
    if (startsWithColon && endsWithColon) return 'center';
    if (endsWithColon) return 'right';
    return 'left';
  });
}

function escapeHtml(str = '') {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}
