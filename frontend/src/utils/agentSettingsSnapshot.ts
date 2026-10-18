const DESKTOP_CONTROL_TOOL_NAME = '\u684c\u9762\u63a7\u5236';
const DESKTOP_CONTROLLER_LEGACY_TOOL_NAME = '\u684c\u9762\u63a7\u5236\u5668';
const DESKTOP_MONITOR_LEGACY_TOOL_NAME = '\u684c\u9762\u76d1\u89c6\u5668';
const WEB_FETCH_TOOL_NAME = '\u7f51\u9875\u6293\u53d6';
const WEB_SEARCH_TOOL_NAME = '\u7f51\u9875\u641c\u7d22';
const BROWSER_GROUP_TOOL_NAME = '\u6d4f\u89c8\u5668';

const DESKTOP_CONTROL_CANONICAL = DESKTOP_CONTROL_TOOL_NAME;
const WEB_FETCH_CANONICAL = WEB_FETCH_TOOL_NAME;
const WEB_SEARCH_CANONICAL = WEB_SEARCH_TOOL_NAME;

const normalizeText = (value: unknown): string => String(value || '').trim();
const normalizeAlias = (value: unknown): string =>
  normalizeText(value).toLowerCase().replace(/[\s-]+/g, '_');

// 桌面控制工具已合并控制器与监视器：旧工具名一并归一到同一规范名，保证历史选择可迁移。
export const resolveDesktopToolKind = (value: unknown): 'control' | '' => {
  const text = normalizeText(value);
  const normalized = text.toLowerCase();
  if (
    text === DESKTOP_CONTROL_TOOL_NAME ||
    text === DESKTOP_CONTROLLER_LEGACY_TOOL_NAME ||
    text === DESKTOP_MONITOR_LEGACY_TOOL_NAME ||
    normalized === 'desktop_control' ||
    normalized === 'desktop control' ||
    normalized === 'desktop' ||
    normalized === 'desktop_controller' ||
    normalized === 'desktop controller' ||
    normalized === 'controller' ||
    normalized === 'desktop_monitor' ||
    normalized === 'desktop monitor' ||
    normalized === 'monitor'
  ) {
    return 'control';
  }
  return '';
};

export const resolveWebToolKind = (value: unknown): 'fetch' | 'search' | '' => {
  const text = normalizeText(value);
  const alias = normalizeAlias(text);
  if (text === WEB_FETCH_TOOL_NAME || alias === 'web_fetch' || alias === 'webfetch') {
    return 'fetch';
  }
  if (text === WEB_SEARCH_TOOL_NAME || alias === 'web_search' || alias === 'websearch') {
    return 'search';
  }
  return '';
};

// 浏览器工具面在用户侧只有一条聚合条目：provider 的 browser_* 成员与英文别名
// 都归一到同一个规范名，历史选择记录也能落在同一条目上。
const isBrowserFamilyToolName = (value: unknown): boolean => {
  const text = normalizeText(value);
  if (!text) return false;
  if (text === BROWSER_GROUP_TOOL_NAME) return true;
  const alias = normalizeAlias(text);
  return alias === 'browser' || alias.startsWith('browser_');
};

export const canonicalizeAgentToolName = (value: unknown): string => {
  const text = normalizeText(value);
  if (isBrowserFamilyToolName(text)) {
    return BROWSER_GROUP_TOOL_NAME;
  }
  if (resolveDesktopToolKind(text)) {
    return DESKTOP_CONTROL_CANONICAL;
  }
  const webKind = resolveWebToolKind(text);
  if (webKind === 'fetch') {
    return WEB_FETCH_CANONICAL;
  }
  if (webKind === 'search') {
    return WEB_SEARCH_CANONICAL;
  }
  return text;
};

export const normalizeAgentToolNamesForSettings = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const output: string[] = [];
  for (const item of value) {
    const name = canonicalizeAgentToolName(item);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    output.push(name);
  }
  return output;
};

export const normalizeAgentToolNamesForSettingsSnapshot = (value: unknown): string[] =>
  normalizeAgentToolNamesForSettings(value).sort();
