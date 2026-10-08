import type { Extension } from '@codemirror/state';
import { markdown } from '@codemirror/lang-markdown';
import { python } from '@codemirror/lang-python';
import { javascript } from '@codemirror/lang-javascript';
import { json } from '@codemirror/lang-json';
import { html } from '@codemirror/lang-html';
import { css } from '@codemirror/lang-css';
import { sql } from '@codemirror/lang-sql';
import { xml } from '@codemirror/lang-xml';
import { yaml } from '@codemirror/lang-yaml';
import { cpp } from '@codemirror/lang-cpp';
import { java } from '@codemirror/lang-java';
import { rust } from '@codemirror/lang-rust';
import { php } from '@codemirror/lang-php';

const MARKDOWN_EXTENSIONS = new Set(['md', 'markdown', 'mdown', 'mkd', 'mkdn']);
const JAVASCRIPT_EXTENSIONS = new Set(['js', 'jsx', 'mjs', 'cjs']);
const TYPESCRIPT_EXTENSIONS = new Set(['ts', 'tsx', 'mts', 'cts']);
const HTML_EXTENSIONS = new Set(['html', 'htm', 'xhtml', 'vue', 'astro', 'svelte']);
const CSS_EXTENSIONS = new Set(['css', 'scss', 'sass', 'less']);
const YAML_EXTENSIONS = new Set(['yaml', 'yml']);
const CPP_EXTENSIONS = new Set(['c', 'cc', 'cpp', 'cxx', 'h', 'hh', 'hpp', 'hxx']);
const SHELL_EXTENSIONS = new Set(['sh', 'bash', 'zsh', 'fish', 'bat', 'cmd', 'ps1']);
const XML_EXTENSIONS = new Set(['xml', 'xsd', 'xsl', 'xslt', 'svg']);

const FALLBACK_TEXT_EXTENSIONS = new Set([
  'txt',
  'log',
  'ini',
  'cfg',
  'conf',
  'toml',
  'properties',
  'env',
  'gitignore',
  'dockerfile'
]);

export const resolveCodeMirrorLanguageExtension = (sourcePath = ''): Extension => {
  const extension = extractExtension(sourcePath);

  if (MARKDOWN_EXTENSIONS.has(extension)) return markdown();
  if (extension === 'py' || extension === 'pyi' || extension === 'pyw') return python();
  if (JAVASCRIPT_EXTENSIONS.has(extension)) return javascript({ jsx: true });
  if (TYPESCRIPT_EXTENSIONS.has(extension)) return javascript({ typescript: true, jsx: extension.endsWith('x') });
  if (extension === 'json' || extension === 'jsonl' || extension === 'json5') return json();
  if (HTML_EXTENSIONS.has(extension)) return html();
  if (CSS_EXTENSIONS.has(extension)) return css();
  if (extension === 'sql') return sql();
  if (YAML_EXTENSIONS.has(extension)) return yaml();
  if (XML_EXTENSIONS.has(extension)) return xml();
  if (CPP_EXTENSIONS.has(extension)) return cpp();
  if (extension === 'java' || extension === 'gradle') return java();
  if (extension === 'rs') return rust();
  if (extension === 'php') return php();
  if (SHELL_EXTENSIONS.has(extension)) return [];
  if (FALLBACK_TEXT_EXTENSIONS.has(extension)) return [];
  return [];
};

const extractExtension = (sourcePath = ''): string => {
  const raw = String(sourcePath || '').trim().toLowerCase();
  if (!raw) return '';
  const base = raw.split('?')[0].split('#')[0];
  const name = base.split('/').pop() || '';
  const dotIndex = name.lastIndexOf('.');
  if (dotIndex <= 0 || dotIndex >= name.length - 1) {
    return name;
  }
  return name.slice(dotIndex + 1);
};
