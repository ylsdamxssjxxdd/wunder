const DESKTOP_CONTROLLER_TOOL_NAME = '\u684c\u9762\u63a7\u5236\u5668';
const DESKTOP_MONITOR_TOOL_NAME = '\u684c\u9762\u76d1\u89c6\u5668';
const WEB_FETCH_TOOL_NAME = '\u7f51\u9875\u6293\u53d6';
const WEB_SEARCH_TOOL_NAME = '\u7f51\u9875\u641c\u7d22';

const DESKTOP_CONTROLLER_CANONICAL = DESKTOP_CONTROLLER_TOOL_NAME;
const DESKTOP_MONITOR_CANONICAL = DESKTOP_MONITOR_TOOL_NAME;
const WEB_FETCH_CANONICAL = WEB_FETCH_TOOL_NAME;
const WEB_SEARCH_CANONICAL = WEB_SEARCH_TOOL_NAME;

const normalizeText = (value: unknown): string => String(value || '').trim();
const normalizeAlias = (value: unknown): string =>
  normalizeText(value).toLowerCase().replace(/[\s-]+/g, '_');

export const resolveDesktopToolKind = (value: unknown): 'controller' | 'monitor' | '' => {
  const text = normalizeText(value);
  const normalized = text.toLowerCase();
  if (
    text === DESKTOP_CONTROLLER_TOOL_NAME ||
    normalized === 'desktop_controller' ||
    normalized === 'desktop controller' ||
    normalized === 'controller'
  ) {
    return 'controller';
  }
  if (
    text === DESKTOP_MONITOR_TOOL_NAME ||
    normalized === 'desktop_monitor' ||
    normalized === 'desktop monitor' ||
    normalized === 'monitor'
  ) {
    return 'monitor';
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

export const canonicalizeAgentToolName = (value: unknown): string => {
  const text = normalizeText(value);
  const desktopKind = resolveDesktopToolKind(text);
  if (desktopKind === 'controller') {
    return DESKTOP_CONTROLLER_CANONICAL;
  }
  if (desktopKind === 'monitor') {
    return DESKTOP_MONITOR_CANONICAL;
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
