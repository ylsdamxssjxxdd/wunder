import MarkdownIt from 'markdown-it';
import katex from 'katex';
import {
  isImagePath,
  normalizeWorkspaceBareRelativePath,
  parseWorkspaceResourceUrl
} from '@/utils/workspaceResources';
import {
  decodeWorkspaceResourceLabel,
  extractWorkspaceResourceExtension,
  normalizeWorkspacePreviewFilename,
  resolveWorkspaceFileCardIconPath
} from '@/utils/workspaceResourceMetadata';

type WorkspacePathResolver = (rawPath: string) => string;
type MarkdownRenderEnv = {
  resolveWorkspacePath?: WorkspacePathResolver;
  workspacePreviewMode?: 'download' | 'preview';
};
export type MarkdownRenderOptions = {
  labels?: Record<string, string>;
  resolveWorkspacePath?: WorkspacePathResolver;
  workspacePreviewMode?: 'download' | 'preview';
};

// Rendering is synchronous; each request supplies its own locale-dependent labels.
let renderLabels: Record<string, string> = {};
const t = (key: string) => renderLabels[key] || key;

const BROKEN_TABLE_DIVIDER_REGEX = /^[\s|:-]+$/;
const MATH_RENDER_CACHE_LIMIT = 240;
const MATH_RENDER_MAX_LENGTH = 6000;
const mathRenderCache = new Map<string, string>();

const KATEX_RENDER_OPTIONS = Object.freeze({
  throwOnError: false,
  strict: 'ignore',
  trust: false,
  output: 'html'
});

// 统一的 Markdown 渲染器：禁用原始 HTML，启用自动换行与链接识别
const markdown = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true
});

markdown.block.ruler.after('fence', 'math_block', mathBlockRule, {
  alt: ['paragraph', 'reference', 'blockquote', 'list']
});
markdown.inline.ruler.before('text', 'math_inline', mathInlineRule);
markdown.renderer.rules.math_inline = (tokens, idx) => {
  const token = tokens[idx];
  return renderMath(token.content || '', Boolean(token.meta?.displayMode));
};
markdown.renderer.rules.math_block = (tokens, idx) => renderMath(tokens[idx].content || '', true);

function mathBlockRule(state: any, startLine: number, endLine: number, silent: boolean) {
  const start = state.bMarks[startLine] + state.tShift[startLine];
  const max = state.eMarks[startLine];
  const firstLine = state.src.slice(start, max);
  const trimmed = firstLine.trim();
  const marker = trimmed.startsWith('$$') ? '$$' : trimmed.startsWith('\\[') ? '\\[' : '';
  const closeMarker = marker === '$$' ? '$$' : marker === '\\[' ? '\\]' : '';
  if (!marker) {
    return false;
  }

  const firstContent = trimmed.slice(marker.length);
  const singleLineClose = findBlockMathClose(firstContent, closeMarker);
  if (singleLineClose >= 0) {
    const rest = firstContent.slice(singleLineClose + closeMarker.length).trim();
    if (rest) {
      return false;
    }
    if (!silent) {
      const token = state.push('math_block', 'math', 0);
      token.block = true;
      token.content = firstContent.slice(0, singleLineClose).trim();
      token.map = [startLine, startLine + 1];
    }
    state.line = startLine + 1;
    return true;
  }

  let nextLine = startLine;
  const lines = [firstContent];
  while (++nextLine < endLine) {
    const lineStart = state.bMarks[nextLine] + state.tShift[nextLine];
    const lineMax = state.eMarks[nextLine];
    const line = state.src.slice(lineStart, lineMax);
    const closeIndex = findBlockMathClose(line, closeMarker);
    if (closeIndex >= 0) {
      const rest = line.slice(closeIndex + closeMarker.length).trim();
      if (rest) {
        return false;
      }
      lines.push(line.slice(0, closeIndex));
      if (!silent) {
        const token = state.push('math_block', 'math', 0);
        token.block = true;
        token.content = lines.join('\n').trim();
        token.map = [startLine, nextLine + 1];
      }
      state.line = nextLine + 1;
      return true;
    }
    lines.push(line);
  }
  return false;
}

function mathInlineRule(state: any, silent: boolean) {
  const start = state.pos;
  const source = state.src;
  const marker = source.startsWith('\\(', start) ? '\\(' : source[start] === '$' ? '$' : '';
  if (!marker) {
    return false;
  }
  if (marker === '$' && !isValidInlineDollarStart(source, start)) {
    return false;
  }

  const closeMarker = marker === '$' ? '$' : '\\)';
  const contentStart = start + marker.length;
  const closeIndex = findInlineMathClose(source, contentStart, closeMarker);
  if (closeIndex < 0) {
    return false;
  }
  const content = source.slice(contentStart, closeIndex);
  if (!content.trim()) {
    return false;
  }
  if (marker === '$' && !isValidInlineDollarEnd(source, closeIndex)) {
    return false;
  }

  if (!silent) {
    const token = state.push('math_inline', 'math', 0);
    token.content = content.trim();
    token.meta = { displayMode: false };
  }
  state.pos = closeIndex + closeMarker.length;
  return true;
}

function findBlockMathClose(content: string, closeMarker: string): number {
  if (!content) {
    return -1;
  }
  return content.indexOf(closeMarker);
}

function findInlineMathClose(source: string, start: number, closeMarker: string): number {
  for (let index = start; index < source.length; index += 1) {
    if (source[index] === '\n') {
      return -1;
    }
    if (closeMarker === '$' && source[index] === '$' && !isEscaped(source, index)) {
      return index;
    }
    if (closeMarker === '\\)' && source.startsWith('\\)', index)) {
      return index;
    }
  }
  return -1;
}

function isValidInlineDollarStart(source: string, index: number): boolean {
  if (source.startsWith('$$', index)) {
    return false;
  }
  if (isEscaped(source, index)) {
    return false;
  }
  const next = source[index + 1] || '';
  if (!next || /\s|\d/.test(next)) {
    return false;
  }
  return true;
}

function isValidInlineDollarEnd(source: string, index: number): boolean {
  const previous = source[index - 1] || '';
  const next = source[index + 1] || '';
  if (!previous || /\s/.test(previous)) {
    return false;
  }
  if (next && /[A-Za-z0-9_]/.test(next)) {
    return false;
  }
  return true;
}

function isEscaped(source: string, index: number): boolean {
  let count = 0;
  for (let cursor = index - 1; cursor >= 0 && source[cursor] === '\\'; cursor -= 1) {
    count += 1;
  }
  return count % 2 === 1;
}

function renderMath(content = '', displayMode = false): string {
  const source = String(content || '').trim();
  if (!source) {
    return '';
  }
  if (source.length > MATH_RENDER_MAX_LENGTH) {
    return `<code>${escapeHtml(source)}</code>`;
  }
  const cacheKey = `${displayMode ? 'block' : 'inline'}:${source}`;
  const cached = mathRenderCache.get(cacheKey);
  if (cached) {
    return cached;
  }

  let html = '';
  try {
    html = katex.renderToString(source, {
      ...KATEX_RENDER_OPTIONS,
      displayMode
    });
  } catch {
    html = `<code>${escapeHtml(source)}</code>`;
  }

  const wrapped = displayMode
    ? `<div class="ai-math-block">${html}</div>`
    : `<span class="ai-math-inline">${html}</span>`;
  mathRenderCache.set(cacheKey, wrapped);
  if (mathRenderCache.size > MATH_RENDER_CACHE_LIMIT) {
    mathRenderCache.delete(mathRenderCache.keys().next().value);
  }
  return wrapped;
}

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

const defaultImageRenderer =
  markdown.renderer.rules.image ||
  ((tokens, idx, options, env, slf) => slf.renderToken(tokens, idx, options, env, slf));
const defaultLinkOpenRenderer =
  markdown.renderer.rules.link_open ||
  ((tokens, idx, options, env, slf) => slf.renderToken(tokens, idx, options, env, slf));

const parseMarkdownWorkspaceResource = (raw: string, env?: MarkdownRenderEnv) => {
  const direct = parseWorkspaceResourceUrl(raw);
  if (direct) return direct;
  const resolver = env?.resolveWorkspacePath;
  if (typeof resolver !== 'function') return null;
  const resolved = resolver(raw);
  if (!resolved) return null;
  return parseWorkspaceResourceUrl(resolved);
};

const buildWorkspaceFallbackText = (text: string) =>
  `<span class="ai-resource-fallback">${escapeHtml(text)}</span>`;

const buildExternalMarkdownImage = (src: string, alt: string) => {
  const fallbackText = `![${alt}](${src})`;
  const safeSrc = escapeHtml(src);
  const safeAlt = escapeHtml(alt);
  const displayAlt = safeAlt || 'image';
  const downloadLabel = resolveWorkspaceResourceActionLabel();
  return `
    <div class="ai-external-image-card" data-external-image-src="${safeSrc}" data-external-image-alt="${displayAlt}" data-markdown-fallback="${escapeHtml(fallbackText)}">
      <div class="ai-external-image-header">
        <span class="ai-external-image-name">${displayAlt}</span>
        <button class="ai-external-image-btn" type="button" data-external-image-action="download">${downloadLabel}</button>
      </div>
      <div class="ai-external-image-body">
        <img class="ai-external-image-preview" src="${safeSrc}" alt="${displayAlt}" loading="lazy" />
      </div>
    </div>
  `;
};

markdown.renderer.rules.image = (tokens, idx, options, env, slf) => {
  const token = tokens[idx];
  const src = token.attrGet('src') || '';
  const isBareRelative = Boolean(normalizeWorkspaceBareRelativePath(src));
  const resource = parseMarkdownWorkspaceResource(src, env as MarkdownRenderEnv);
  if (!resource) {
    const alt = token.content || token.attrGet('alt') || 'image';
    if (isBareRelative) {
      return buildWorkspaceFallbackText(`![${alt}](${src})`);
    }
    return buildExternalMarkdownImage(src, alt);
  }
  const alt = token.content || token.attrGet('alt') || resource.filename || 'image';
  const kind = isImagePath(resource.filename || resource.relativePath) ? 'image' : 'file';
  const fallback = isBareRelative ? `![${alt}](${src})` : '';
  return buildWorkspaceResourceCard(resource.publicPath, alt, resource.filename, kind, fallback);
};

markdown.renderer.rules.link_open = (tokens, idx, options, env, slf) => {
  const token = tokens[idx];
  const href = token.attrGet('href') || '';
  const resource = parseMarkdownWorkspaceResource(href, env as MarkdownRenderEnv);
  if (resource) {
    const existingClass = token.attrGet('class');
    token.attrSet('class', existingClass ? `${existingClass} ai-resource-link` : 'ai-resource-link');
    token.attrSet('data-workspace-path', resource.publicPath);
    token.attrSet('data-workspace-link', 'true');
    token.attrSet('href', '#');
  }
  return defaultLinkOpenRenderer(tokens, idx, options, env, slf);
};

markdown.core.ruler.after('inline', 'workspace_resource_links', (state) => {
  state.tokens.forEach((blockToken) => {
    if (blockToken.type !== 'inline' || !Array.isArray(blockToken.children)) return;
    const children = blockToken.children;
    const nextChildren = [];
    for (let i = 0; i < children.length; i += 1) {
      const token = children[i];
      if (token.type === 'link_open') {
        const href = token.attrGet('href') || '';
        const resource = parseMarkdownWorkspaceResource(href, state.env as MarkdownRenderEnv);
        const isBareRelative = Boolean(normalizeWorkspaceBareRelativePath(href));
        if (resource && !isImagePath(resource.filename || resource.relativePath)) {
          let label = '';
          let j = i + 1;
          for (; j < children.length; j += 1) {
            if (children[j].type === 'link_close') break;
            if (children[j].type === 'text' || children[j].type === 'code_inline') {
              label += children[j].content;
            }
          }
          const displayLabel = label.trim() || resource.filename || 'resource';
          const htmlToken = new state.Token('html_inline', '', 0);
          const fallback = isBareRelative ? `[${displayLabel}](${href})` : '';
          htmlToken.content = buildWorkspaceResourceCard(
            resource.publicPath,
            displayLabel,
            resource.filename,
            'file',
            fallback
          );
          nextChildren.push(htmlToken);
          i = j;
          continue;
        }
        if (!resource && isBareRelative) {
          let label = '';
          let j = i + 1;
          for (; j < children.length; j += 1) {
            if (children[j].type === 'link_close') break;
            if (children[j].type === 'text' || children[j].type === 'code_inline') {
              label += children[j].content;
            }
          }
          const displayLabel = label.trim() || href;
          const htmlToken = new state.Token('html_inline', '', 0);
          htmlToken.content = buildWorkspaceFallbackText(`[${displayLabel}](${href})`);
          nextChildren.push(htmlToken);
          i = j;
          continue;
        }
      }
      nextChildren.push(token);
    }
    blockToken.children = nextChildren;
  });
});

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
  ['sql', 'sql'],
  ['slint', 'slint'],
  ['rs', 'rust'],
  ['rust', 'rust'],
  ['golang', 'go'],
  ['go', 'go'],
  ['java', 'java'],
  ['kt', 'kotlin'],
  ['kts', 'kotlin'],
  ['kotlin', 'kotlin'],
  ['swift', 'swift'],
  ['c', 'c'],
  ['h', 'c'],
  ['cc', 'cpp'],
  ['cxx', 'cpp'],
  ['cpp', 'cpp'],
  ['c++', 'cpp'],
  ['hpp', 'cpp'],
  ['cs', 'csharp'],
  ['c#', 'csharp'],
  ['csharp', 'csharp'],
  ['php', 'php'],
  ['rb', 'ruby'],
  ['ruby', 'ruby'],
  ['scala', 'scala'],
  ['lua', 'lua'],
  ['dart', 'dart'],
  ['yml', 'yaml'],
  ['yaml', 'yaml'],
  ['toml', 'toml'],
  ['ini', 'ini'],
  ['conf', 'ini'],
  ['xml', 'xml'],
  ['svg', 'xml'],
  ['html', 'html'],
  ['htm', 'html'],
  ['vue', 'html'],
  ['css', 'css'],
  ['scss', 'css'],
  ['less', 'css']
]);
const CODE_HIGHLIGHT_LANGS = new Set([
  'javascript', 'typescript', 'json', 'python', 'shell', 'sql', 'slint',
  'rust', 'go', 'java', 'kotlin', 'swift', 'c', 'cpp', 'csharp', 'php',
  'ruby', 'scala', 'lua', 'dart', 'yaml', 'toml', 'ini', 'xml', 'html', 'css'
]);
const HIGHLIGHT_CACHE_LIMIT = 120;
const HIGHLIGHT_CACHE_MAX_LENGTH = 8000;
const CODE_BLOCK_MAX_LENGTH = 20000;
const CODE_BLOCK_MAX_LINES = 400;
const NUMBER_TOKEN_REGEX = /^-?\d/;
/** Hex colour literals (`#f8fafc`) belong to the number bucket, not the comment one. */
const COLOR_LITERAL_REGEX = /^#[0-9a-fA-F]{3,8}$/;
/** Languages whose `#` starts a comment, so a hex literal never appears there. */
const HASH_COMMENT_LANGS = new Set(['python', 'shell', 'ruby', 'php', 'yaml', 'toml', 'ini']);
/** In C and C++ a `#` line is a preprocessor directive, not a comment. */
const PREPROCESSOR_LANGS = new Set(['c', 'cpp']);
const PREPROCESSOR_REGEX = /^#\s*(?:include|define|undef|ifdef|ifndef|endif|if|else|elif|pragma|error|warning)\b/;
/**
 * Line comment markers per language.
 *
 * A shared heuristic cannot decide this: `;` opens an INI comment but ends a
 * statement in half the languages here, and `#` is a comment in YAML yet a
 * colour literal in Slint. Each language declares its own markers instead.
 */
const LINE_COMMENT_PREFIXES: Record<string, string[]> = {
  javascript: ['//'],
  typescript: ['//'],
  json: [],
  python: ['#'],
  shell: ['#'],
  sql: ['--'],
  slint: ['//'],
  rust: ['//'],
  go: ['//'],
  java: ['//'],
  kotlin: ['//'],
  swift: ['//'],
  c: ['//'],
  cpp: ['//'],
  csharp: ['//'],
  php: ['//', '#'],
  ruby: ['#'],
  scala: ['//'],
  lua: ['--'],
  dart: ['//'],
  yaml: ['#'],
  toml: ['#'],
  ini: ['#', ';'],
  xml: [],
  html: [],
  css: []
};
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
const SLINT_KEYWORDS = [
  // `in-out` must precede `in`: an alternation matches the leftmost branch that
  // succeeds, so listing the short keyword first would split the compound one.
  'in-out',
  'in',
  'out',
  'component',
  'inherits',
  'export',
  'import',
  'from',
  'struct',
  'enum',
  'global',
  'singleton',
  'callback',
  'property',
  'function',
  'return',
  'if',
  'else',
  'for',
  'while',
  'break',
  'continue',
  'states',
  'transitions',
  'animate',
  'duration',
  'easing',
  'private',
  'pure',
  'init',
  'parent',
  'root',
  'self',
  'true',
  'false'
];
const RUST_KEYWORDS = [
  'as',
  'async',
  'await',
  'break',
  'const',
  'continue',
  'crate',
  'dyn',
  'else',
  'enum',
  'extern',
  'false',
  'fn',
  'for',
  'if',
  'impl',
  'in',
  'let',
  'loop',
  'match',
  'mod',
  'move',
  'mut',
  'pub',
  'ref',
  'return',
  'self',
  'Self',
  'static',
  'struct',
  'super',
  'trait',
  'true',
  'type',
  'unsafe',
  'use',
  'where',
  'while',
  // Primitive and prelude types carry as much meaning as keywords here.
  'i8',
  'i16',
  'i32',
  'i64',
  'i128',
  'isize',
  'u8',
  'u16',
  'u32',
  'u64',
  'u128',
  'usize',
  'f32',
  'f64',
  'bool',
  'char',
  'str',
  'String',
  'Vec',
  'Option',
  'Result',
  'Box',
  'HashMap'
];
const GO_KEYWORDS = [
  'break',
  'case',
  'chan',
  'const',
  'continue',
  'default',
  'defer',
  'else',
  'fallthrough',
  'for',
  'func',
  'go',
  'goto',
  'if',
  'import',
  'interface',
  'map',
  'package',
  'range',
  'return',
  'select',
  'struct',
  'switch',
  'type',
  'var',
  'nil',
  'true',
  'false',
  // Builtin types and the functions that show up in nearly every snippet.
  'string',
  'int',
  'int8',
  'int16',
  'int32',
  'int64',
  'uint',
  'uint8',
  'uint32',
  'uint64',
  'float32',
  'float64',
  'bool',
  'byte',
  'rune',
  'error',
  'make',
  'new',
  'len',
  'cap',
  'append',
  'panic',
  'recover'
];
const JAVA_KEYWORDS = [
  'abstract',
  'assert',
  'boolean',
  'break',
  'byte',
  'case',
  'catch',
  'char',
  'class',
  'const',
  'continue',
  'default',
  'do',
  'double',
  'else',
  'enum',
  'extends',
  'final',
  'finally',
  'float',
  'for',
  'if',
  'implements',
  'import',
  'instanceof',
  'int',
  'interface',
  'long',
  'native',
  'new',
  'package',
  'private',
  'protected',
  'public',
  'return',
  'short',
  'static',
  'super',
  'switch',
  'synchronized',
  'this',
  'throw',
  'throws',
  'transient',
  'try',
  'void',
  'volatile',
  'while',
  'record',
  'sealed',
  'var',
  'true',
  'false',
  'null'
];
const KOTLIN_KEYWORDS = [
  'as',
  'break',
  'class',
  'continue',
  'do',
  'else',
  'false',
  'for',
  'fun',
  'if',
  'in',
  'interface',
  'is',
  'null',
  'object',
  'package',
  'return',
  'super',
  'this',
  'throw',
  'true',
  'try',
  'typealias',
  'val',
  'var',
  'when',
  'while',
  'by',
  'catch',
  'constructor',
  'finally',
  'get',
  'import',
  'init',
  'set',
  'companion',
  'data',
  'enum',
  'inner',
  'internal',
  'lateinit',
  'open',
  'operator',
  'out',
  'override',
  'private',
  'protected',
  'public',
  'sealed',
  'suspend',
  'inline',
  'reified'
];
const SWIFT_KEYWORDS = [
  'as',
  'associatedtype',
  'break',
  'case',
  'catch',
  'class',
  'continue',
  'default',
  'defer',
  'deinit',
  'do',
  'else',
  'enum',
  'extension',
  'fallthrough',
  'false',
  'for',
  'func',
  'guard',
  'if',
  'import',
  'in',
  'init',
  'inout',
  'internal',
  'is',
  'let',
  'nil',
  'open',
  'operator',
  'private',
  'protocol',
  'public',
  'repeat',
  'rethrows',
  'return',
  'self',
  'Self',
  'static',
  'struct',
  'subscript',
  'super',
  'switch',
  'throw',
  'throws',
  'true',
  'try',
  'typealias',
  'var',
  'where',
  'while',
  'weak',
  'unowned'
];
const C_KEYWORDS = [
  'auto',
  'break',
  'case',
  'char',
  'const',
  'continue',
  'default',
  'do',
  'double',
  'else',
  'enum',
  'extern',
  'float',
  'for',
  'goto',
  'if',
  'inline',
  'int',
  'long',
  'register',
  'restrict',
  'return',
  'short',
  'signed',
  'sizeof',
  'static',
  'struct',
  'switch',
  'typedef',
  'union',
  'unsigned',
  'void',
  'volatile',
  'while'
];
const CPP_KEYWORDS = [
  ...C_KEYWORDS,
  'bool',
  'catch',
  'class',
  'constexpr',
  'consteval',
  'decltype',
  'delete',
  'explicit',
  'export',
  'friend',
  'mutable',
  'namespace',
  'new',
  'noexcept',
  'nullptr',
  'operator',
  'override',
  'private',
  'protected',
  'public',
  'template',
  'this',
  'throw',
  'try',
  'typename',
  'using',
  'virtual',
  'concept',
  'requires',
  'true',
  'false'
];
const CSHARP_KEYWORDS = [
  'abstract',
  'as',
  'async',
  'await',
  'base',
  'bool',
  'break',
  'byte',
  'case',
  'catch',
  'char',
  'checked',
  'class',
  'const',
  'continue',
  'decimal',
  'default',
  'delegate',
  'do',
  'double',
  'else',
  'enum',
  'event',
  'explicit',
  'extern',
  'false',
  'finally',
  'fixed',
  'float',
  'for',
  'foreach',
  'get',
  'goto',
  'if',
  'implicit',
  'in',
  'int',
  'interface',
  'internal',
  'is',
  'lock',
  'long',
  'namespace',
  'new',
  'null',
  'object',
  'operator',
  'out',
  'override',
  'params',
  'private',
  'protected',
  'public',
  'readonly',
  'record',
  'ref',
  'return',
  'sbyte',
  'sealed',
  'set',
  'short',
  'sizeof',
  'stackalloc',
  'static',
  'string',
  'struct',
  'switch',
  'this',
  'throw',
  'true',
  'try',
  'typeof',
  'uint',
  'ulong',
  'unchecked',
  'unsafe',
  'ushort',
  'using',
  'var',
  'virtual',
  'void',
  'volatile',
  'while'
];
const PHP_KEYWORDS = [
  'abstract',
  'array',
  'as',
  'break',
  'callable',
  'case',
  'catch',
  'class',
  'clone',
  'const',
  'continue',
  'declare',
  'default',
  'do',
  'echo',
  'else',
  'elseif',
  'empty',
  'endfor',
  'endforeach',
  'endif',
  'endswitch',
  'endwhile',
  'enum',
  'extends',
  'final',
  'finally',
  'fn',
  'for',
  'foreach',
  'function',
  'global',
  'if',
  'implements',
  'include',
  'include_once',
  'instanceof',
  'insteadof',
  'interface',
  'isset',
  'list',
  'match',
  'namespace',
  'new',
  'print',
  'private',
  'protected',
  'public',
  'readonly',
  'require',
  'require_once',
  'return',
  'static',
  'switch',
  'throw',
  'trait',
  'try',
  'unset',
  'use',
  'var',
  'while',
  'yield',
  'true',
  'false',
  'null'
];
const RUBY_KEYWORDS = [
  'alias',
  'and',
  'begin',
  'break',
  'case',
  'class',
  'def',
  'do',
  'else',
  'elsif',
  'end',
  'ensure',
  'false',
  'for',
  'if',
  'in',
  'module',
  'next',
  'nil',
  'not',
  'or',
  'redo',
  'rescue',
  'retry',
  'return',
  'self',
  'super',
  'then',
  'true',
  'undef',
  'unless',
  'until',
  'when',
  'while',
  'yield',
  'attr_accessor',
  'attr_reader',
  'attr_writer',
  'require',
  'require_relative',
  'lambda',
  'proc'
];
const SCALA_KEYWORDS = [
  'abstract',
  'case',
  'catch',
  'class',
  'def',
  'do',
  'else',
  'extends',
  'false',
  'final',
  'finally',
  'for',
  'if',
  'implicit',
  'import',
  'lazy',
  'match',
  'new',
  'null',
  'object',
  'override',
  'package',
  'private',
  'protected',
  'return',
  'sealed',
  'super',
  'this',
  'throw',
  'trait',
  'true',
  'try',
  'type',
  'val',
  'var',
  'while',
  'with',
  'yield',
  'given',
  'enum',
  'extension',
  'derives'
];
const LUA_KEYWORDS = [
  'and',
  'break',
  'do',
  'else',
  'elseif',
  'end',
  'false',
  'for',
  'function',
  'goto',
  'if',
  'in',
  'local',
  'nil',
  'not',
  'or',
  'repeat',
  'return',
  'then',
  'true',
  'until',
  'while'
];
const DART_KEYWORDS = [
  'abstract',
  'as',
  'async',
  'await',
  'break',
  'case',
  'catch',
  'class',
  'const',
  'continue',
  'covariant',
  'default',
  'deferred',
  'do',
  'dynamic',
  'else',
  'enum',
  'export',
  'extends',
  'extension',
  'external',
  'factory',
  'false',
  'final',
  'finally',
  'for',
  'get',
  'hide',
  'if',
  'implements',
  'import',
  'in',
  'interface',
  'is',
  'late',
  'library',
  'mixin',
  'new',
  'null',
  'on',
  'operator',
  'part',
  'required',
  'rethrow',
  'return',
  'set',
  'show',
  'static',
  'super',
  'switch',
  'sync',
  'this',
  'throw',
  'true',
  'try',
  'typedef',
  'var',
  'void',
  'while',
  'with',
  'yield'
];
const YAML_KEYWORDS = ['true', 'false', 'null', 'yes', 'no', 'on', 'off'];
const TOML_KEYWORDS = ['true', 'false'];
/** Markup languages carry no keywords: tags and property names come from `extras`. */
const MARKUP_KEYWORDS: string[] = [];

const CODE_KEYWORDS: Record<string, Set<string>> = {
  javascript: new Set(SCRIPT_KEYWORDS),
  typescript: new Set(SCRIPT_KEYWORDS),
  python: new Set(PYTHON_KEYWORDS.map((item) => item.toLowerCase())),
  shell: new Set(SHELL_KEYWORDS),
  sql: new Set(SQL_KEYWORDS),
  json: new Set(JSON_KEYWORDS),
  slint: new Set(SLINT_KEYWORDS),
  rust: new Set(RUST_KEYWORDS),
  go: new Set(GO_KEYWORDS),
  java: new Set(JAVA_KEYWORDS),
  kotlin: new Set(KOTLIN_KEYWORDS),
  swift: new Set(SWIFT_KEYWORDS),
  c: new Set(C_KEYWORDS),
  cpp: new Set(CPP_KEYWORDS),
  csharp: new Set(CSHARP_KEYWORDS),
  php: new Set(PHP_KEYWORDS),
  ruby: new Set(RUBY_KEYWORDS),
  scala: new Set(SCALA_KEYWORDS),
  lua: new Set(LUA_KEYWORDS),
  dart: new Set(DART_KEYWORDS),
  yaml: new Set(YAML_KEYWORDS),
  toml: new Set(TOML_KEYWORDS),
  ini: new Set(TOML_KEYWORDS),
  xml: new Set(MARKUP_KEYWORDS),
  html: new Set(MARKUP_KEYWORDS),
  css: new Set(MARKUP_KEYWORDS)
};

/**
 * One lexer per language, declared as data instead of a hand-built regex.
 *
 * Branch order is fixed and matters: comments first so a `#` or `//` inside a
 * string is never taken for one, then strings, numbers, language extras, and
 * finally keywords.
 */
type TokenPattern = {
  blockComment?: string;
  lineComments?: string[];
  strings?: string[];
  numbers?: string;
  extras?: string[];
  keywords: string[];
  caseInsensitive?: boolean;
};
const C_BLOCK_COMMENT = '\\/\\*[\\s\\S]*?\\*\\/';
const XML_BLOCK_COMMENT = '<!--[\\s\\S]*?-->';
const DEFAULT_STRINGS = ['"(?:\\\\.|[^"\\\\])*"', "'(?:\\\\.|[^'\\\\])*'"];
const DEFAULT_NUMBERS = '\\b-?\\d+(?:\\.\\d+)?\\b';
/** Dimensions keep their unit: `12px`, `200ms`, `50%`. */
const SIZED_NUMBERS = '\\b-?\\d+(?:\\.\\d+)?(?:px|pt|rem|em|%|s|ms|deg|fr|vh|vw)?';
const COLOR_LITERAL = '#[0-9a-fA-F]{3,8}';

function buildTokenRegex(pattern: TokenPattern): RegExp {
  const branches: string[] = [];
  if (pattern.blockComment) branches.push(pattern.blockComment);
  for (const marker of pattern.lineComments ?? []) branches.push(`${marker}[^\\n]*`);
  branches.push(...(pattern.strings ?? DEFAULT_STRINGS));
  branches.push(pattern.numbers ?? DEFAULT_NUMBERS);
  branches.push(...(pattern.extras ?? []));
  if (pattern.keywords.length) branches.push(`\\b(?:${pattern.keywords.join('|')})\\b`);
  return new RegExp(branches.join('|'), pattern.caseInsensitive ? 'gi' : 'g');
}

const SCRIPT_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  strings: ['`(?:\\\\.|[^`])*`', ...DEFAULT_STRINGS],
  keywords: SCRIPT_KEYWORDS
});
const PYTHON_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['#'],
  // Triple quotes precede the plain ones so a docstring is captured whole.
  strings: ['"""[\\s\\S]*?"""', "'''[\\s\\S]*?'''", ...DEFAULT_STRINGS],
  keywords: PYTHON_KEYWORDS
});
const SHELL_TOKEN_REGEX = buildTokenRegex({ lineComments: ['#'], keywords: SHELL_KEYWORDS });
const SQL_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['--'],
  keywords: SQL_KEYWORDS,
  caseInsensitive: true
});
const JSON_TOKEN_REGEX = buildTokenRegex({
  strings: ['"(?:\\\\.|[^"\\\\])*"'],
  numbers: '\\b-?\\d+(?:\\.\\d+)?(?:[eE][+-]?\\d+)?\\b',
  keywords: JSON_KEYWORDS
});
const SLINT_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  strings: ['"(?:\\\\.|[^"\\\\])*"'],
  numbers: SIZED_NUMBERS,
  extras: [COLOR_LITERAL, '<[A-Za-z_][A-Za-z0-9_]*>', '@[A-Za-z_][A-Za-z0-9_]*'],
  keywords: SLINT_KEYWORDS
});
const RUST_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  numbers: '\\b-?\\d+(?:\\.\\d+)?(?:[iuf](?:8|16|32|64|128|size))?\\b',
  extras: ['#\\[[^\\]]*\\]', '\\b[a-z_][a-zA-Z0-9_]*!'],
  keywords: RUST_KEYWORDS
});
const GO_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  strings: ['`(?:[^`])*`', ...DEFAULT_STRINGS],
  keywords: GO_KEYWORDS
});
const JAVA_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  keywords: JAVA_KEYWORDS
});
const KOTLIN_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  keywords: KOTLIN_KEYWORDS
});
const SWIFT_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  keywords: SWIFT_KEYWORDS
});
const C_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  extras: ['#\\s*[a-z]+'],
  keywords: C_KEYWORDS
});
const CPP_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  extras: ['#\\s*[a-z]+'],
  keywords: CPP_KEYWORDS
});
const CSHARP_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  strings: ['\\$"(?:\\\\.|[^"\\\\])*"', ...DEFAULT_STRINGS],
  keywords: CSHARP_KEYWORDS
});
const PHP_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/', '#'],
  extras: ['<\\?php', '\\$[A-Za-z_][A-Za-z0-9_]*'],
  keywords: PHP_KEYWORDS
});
const RUBY_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['#'],
  extras: ['@[A-Za-z_][A-Za-z0-9_]*', ':[A-Za-z_][A-Za-z0-9_]*'],
  keywords: RUBY_KEYWORDS
});
const SCALA_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  keywords: SCALA_KEYWORDS
});
const LUA_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['--'],
  extras: ['--\\[\\[[\\s\\S]*?\\]\\]'],
  keywords: LUA_KEYWORDS
});
const DART_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  lineComments: ['\\/\\/'],
  keywords: DART_KEYWORDS
});
const YAML_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['#'],
  extras: ['[A-Za-z_][\\w.-]*(?=\\s*:)'],
  keywords: YAML_KEYWORDS
});
const TOML_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['#'],
  extras: ['\\[[A-Za-z_][\\w.-]*\\]'],
  keywords: TOML_KEYWORDS
});
const INI_TOKEN_REGEX = buildTokenRegex({
  lineComments: ['#', ';'],
  extras: ['\\[[A-Za-z_][\\w.-]*\\]'],
  keywords: TOML_KEYWORDS
});
const XML_TOKEN_REGEX = buildTokenRegex({
  blockComment: XML_BLOCK_COMMENT,
  extras: ['<\\/?[A-Za-z_][\\w:.-]*', '\\/?>'],
  keywords: MARKUP_KEYWORDS
});
const CSS_TOKEN_REGEX = buildTokenRegex({
  blockComment: C_BLOCK_COMMENT,
  numbers: SIZED_NUMBERS,
  extras: [COLOR_LITERAL, '@[a-z-]+', '[a-zA-Z-]+(?=\\s*:)'],
  keywords: MARKUP_KEYWORDS
});
const TOKEN_REGEX: Record<string, RegExp> = {
  javascript: SCRIPT_TOKEN_REGEX,
  typescript: SCRIPT_TOKEN_REGEX,
  python: PYTHON_TOKEN_REGEX,
  shell: SHELL_TOKEN_REGEX,
  sql: SQL_TOKEN_REGEX,
  json: JSON_TOKEN_REGEX,
  slint: SLINT_TOKEN_REGEX,
  rust: RUST_TOKEN_REGEX,
  go: GO_TOKEN_REGEX,
  java: JAVA_TOKEN_REGEX,
  kotlin: KOTLIN_TOKEN_REGEX,
  swift: SWIFT_TOKEN_REGEX,
  c: C_TOKEN_REGEX,
  cpp: CPP_TOKEN_REGEX,
  csharp: CSHARP_TOKEN_REGEX,
  php: PHP_TOKEN_REGEX,
  ruby: RUBY_TOKEN_REGEX,
  scala: SCALA_TOKEN_REGEX,
  lua: LUA_TOKEN_REGEX,
  dart: DART_TOKEN_REGEX,
  yaml: YAML_TOKEN_REGEX,
  toml: TOML_TOKEN_REGEX,
  ini: INI_TOKEN_REGEX,
  xml: XML_TOKEN_REGEX,
  html: XML_TOKEN_REGEX,
  css: CSS_TOKEN_REGEX
};

const highlightCache = new Map();

const CODE_COPY_ICON = `
  <i class="fa-solid fa-copy ai-code-copy-icon" aria-hidden="true"></i>
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
export function renderMarkdown(content = '', options: MarkdownRenderOptions = {}) {
  if (!content) return '';
  renderLabels = options.labels || {};
  const normalizedContent = normalizeMarkdownForRender(String(content));
  const env: MarkdownRenderEnv | undefined =
    typeof options.resolveWorkspacePath === 'function' || options.workspacePreviewMode
      ? {
          ...(typeof options.resolveWorkspacePath === 'function'
            ? { resolveWorkspacePath: options.resolveWorkspacePath }
            : {}),
          ...(options.workspacePreviewMode ? { workspacePreviewMode: options.workspacePreviewMode } : {})
        }
      : undefined;
  return markdown.render(normalizedContent, env);
}

function normalizeMarkdownForRender(content = '') {
  if (!content) return '';
  return preserveMalformedMarkdownTables(content.replace(/\r\n/g, '\n'));
}

function preserveMalformedMarkdownTables(content = '') {
  if (!content.includes('|')) return content;
  const lines = content.split('\n');
  let activeFence = '';
  for (let index = 0; index < lines.length; index += 1) {
    const trimmed = lines[index].trim();
    const fenceMatch = trimmed.match(/^([`~]{3,})/);
    if (fenceMatch) {
      const marker = fenceMatch[1];
      if (!activeFence) {
        activeFence = marker;
      } else if (marker[0] === activeFence[0] && marker.length >= activeFence.length) {
        activeFence = '';
      }
      continue;
    }
    if (activeFence || !isMalformedMarkdownTableStart(lines, index)) continue;
    let end = index + 1;
    while (end + 1 < lines.length && looksLikeMarkdownTableContinuation(lines[end + 1])) {
      end += 1;
    }
    for (let row = index; row <= end; row += 1) {
      lines[row] = escapeMarkdownLiteralLine(lines[row]);
    }
    index = end;
  }
  return lines.join('\n');
}

function isMalformedMarkdownTableStart(lines: string[], index: number) {
  const headerRow = String(lines[index] || '').trim();
  if (!looksLikeMarkdownTableRow(headerRow)) return false;
  const previousLine = String(lines[index - 1] || '').trim();
  if (looksLikeDividerRow(previousLine) || looksLikeMarkdownTableRow(previousLine)) {
    return false;
  }
  const headerCells = splitTableRow(headerRow);
  if (headerCells.length < 2 || headerCells.every((cell) => !cell)) return false;
  const nextLine = String(lines[index + 1] || '').trim();
  if (!nextLine) return false;
  if (looksLikeDividerRow(nextLine)) {
    return splitTableRow(nextLine).length !== headerCells.length;
  }
  return looksLikeMarkdownTableRow(nextLine);
}

function looksLikeMarkdownTableRow(row = '') {
  const trimmed = row.trim();
  if (!trimmed.includes('|')) return false;
  const pipeCount = (trimmed.match(/\|/g) || []).length;
  if (pipeCount < 2 && !(trimmed.startsWith('|') && trimmed.endsWith('|'))) return false;
  if (looksLikeDividerRow(row)) return false;
  return splitTableRow(row).length >= 2;
}

function looksLikeDividerRow(row = '') {
  const trimmed = row.trim();
  const pipeCount = (trimmed.match(/\|/g) || []).length;
  if (
    !trimmed ||
    !trimmed.includes('|') ||
    !trimmed.includes('-') ||
    (pipeCount < 2 && !(trimmed.startsWith('|') && trimmed.endsWith('|')))
  ) {
    return false;
  }
  return BROKEN_TABLE_DIVIDER_REGEX.test(trimmed);
}

function looksLikeMarkdownTableContinuation(row = '') {
  const trimmed = row.trim();
  if (!trimmed) return false;
  return looksLikeDividerRow(trimmed) || looksLikeMarkdownTableRow(trimmed);
}

function escapeMarkdownLiteralLine(line = '') {
  return String(line || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/\\/g, '\\\\')
    .replace(/([`*_{}\[\]()#+.!|~-])/g, '\\$1');
}

const resolveWorkspaceResourceActionLabel = (): string =>
  t('resource.download');

function buildWorkspaceResourceCard(publicPath, label, filename, kind = 'file', fallbackText = '') {
  const title = decodeWorkspaceResourceLabel(label);
  const fallback = decodeWorkspaceResourceLabel(filename);
  const displayName = normalizeWorkspacePreviewFilename(title, fallback);
  const safeName = escapeHtml(displayName);
  const safePath = escapeHtml(publicPath);
  const safeKind = kind === 'image' ? 'image' : 'file';
  const metaText = title && fallback && title !== fallback ? escapeHtml(fallback) : '';
  const metaInline = metaText ? `<span class="ai-resource-meta-inline">${metaText}</span>` : '';
  const fileExt = extractWorkspaceResourceExtension(fallback || displayName);
  const downloadLabel = resolveWorkspaceResourceActionLabel();
  const previewLabel = escapeHtml(t('workspace.preview.dialogTitle'));
  const fileActionLabel = escapeHtml(`${previewLabel} ${displayName}`);
  const fileDownloadLabel = escapeHtml(`${downloadLabel} ${displayName}`);
  const fileIcon = resolveWorkspaceFileCardIconPath(fallback || displayName);
  const fileBadge = fileExt ? fileExt.toUpperCase() : 'FILE';
  const imageLoadingLabel = escapeHtml(t('chat.resourceImageLoading'));
  const safeFallbackText = fallbackText ? escapeHtml(fallbackText) : '';
  const fallbackAttr = safeFallbackText ? ` data-workspace-fallback="${safeFallbackText}"` : '';
  const header = `
    <div class="ai-resource-header">
      <div class="ai-resource-title">
        <span class="ai-resource-name" title="${safeName}">${safeName}</span>
        ${metaText ? `<span class="ai-resource-meta-inline" title="${metaText}">${metaText}</span>` : ''}
      </div>
    </div>
  `;
  const fileBody = `
    <div class="ai-resource-body ai-resource-file">
      <div class="ai-resource-file-row">
        <button
          class="ai-resource-file-icon"
          type="button"
          data-workspace-action="preview"
          title="${fileActionLabel}"
          aria-label="${fileActionLabel}"
        >
          <img class="ai-resource-file-icon-img" src="${fileIcon}" alt="${escapeHtml(fileBadge)}" aria-hidden="true" />
        </button>
        <button
          class="ai-resource-file-download"
          type="button"
          data-workspace-action="download"
          title="${fileDownloadLabel}"
          aria-label="${fileDownloadLabel}"
        >
          <i class="fa-solid fa-download" aria-hidden="true"></i>
        </button>
      </div>
    </div>
  `;
  const imageBody = `
    <div class="ai-resource-body">
      <div class="ai-resource-status" data-loading-label="${imageLoadingLabel}"></div>
      <img class="ai-resource-preview" alt="${safeName}" />
    </div>
  `;
  return `
    <div class="ai-resource-card ai-resource-${safeKind}" data-workspace-kind="${safeKind}" data-workspace-path="${safePath}" data-workspace-action="preview"${fallbackAttr}>
      ${header}
      ${safeKind === 'image' ? imageBody : fileBody}
    </div>
  `;
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
        <button class="ai-code-copy" type="button" aria-label="${t('chat.code.copy')}" title="${t('chat.code.copy')}">
          ${CODE_COPY_ICON}
          <span>${t('common.copy')}</span>
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
  // Slint colours read as data, not as the line comment other languages use.
  if (!HASH_COMMENT_LANGS.has(lang) && COLOR_LITERAL_REGEX.test(token)) {
    return 'number';
  }
  if (PREPROCESSOR_LANGS.has(lang) && PREPROCESSOR_REGEX.test(token)) {
    return 'keyword';
  }
  if (token.startsWith('/*') || token.startsWith('<!--')) {
    return 'comment';
  }
  const lineComments = LINE_COMMENT_PREFIXES[lang];
  if (lineComments?.some((prefix) => token.startsWith(prefix))) {
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
