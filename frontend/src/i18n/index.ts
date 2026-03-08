import { ref } from 'vue';

import zhCN from './messages/zh-CN';
import { resolveApiBase } from '@/config/runtime';

type LocaleMessages = Record<string, string>;

type LanguageAliases = Record<string, string>;
type LanguageLabels = Record<string, string>;

type SetLanguageOptions = {
  force?: boolean;
  persist?: boolean;
  emit?: boolean;
};

type I18nConfigPayload = {
  supported_languages?: string[];
  aliases?: Record<string, string>;
  labels?: Record<string, string>;
  default_language?: string;
};

const LOCALES: Record<string, LocaleMessages> = {
  'zh-CN': zhCN
};

const LOCALE_LOADERS: Record<string, () => Promise<LocaleMessages>> = {
  'en-US': () => import('./messages/en-US').then((module) => module.default)
};

const localeLoadTasks = new Map<string, Promise<LocaleMessages>>();

const DEFAULT_LANGUAGE_ALIASES: LanguageAliases = {
  zh: 'zh-CN',
  'zh-cn': 'zh-CN',
  'zh-hans': 'zh-CN',
  'zh-hant': 'zh-CN',
  en: 'en-US',
  'en-us': 'en-US',
  'en-gb': 'en-US'
};

const DEFAULT_LANGUAGE_LABELS: LanguageLabels = {
  'zh-CN': '\u7b80\u4f53\u4e2d\u6587',
  'en-US': 'English'
};

const STORAGE_KEY = 'wunder_language';

let defaultLanguage = 'zh-CN';
let supportedLanguages = Object.keys(LOCALES);
let languageAliases: LanguageAliases = { ...DEFAULT_LANGUAGE_ALIASES };
let languageLabels: LanguageLabels = { ...DEFAULT_LANGUAGE_LABELS };

const resolveLocale = (language: string): LocaleMessages => {
  if (language && LOCALES[language]) {
    return LOCALES[language];
  }
  if (defaultLanguage && LOCALES[defaultLanguage]) {
    return LOCALES[defaultLanguage];
  }
  const fallbackKey = Object.keys(LOCALES)[0];
  return fallbackKey ? LOCALES[fallbackKey] : {};
};


const ensureLocaleLoaded = async (language: string): Promise<void> => {
  const target = String(language || '').trim();
  if (!target || LOCALES[target]) {
    return;
  }
  const loader = LOCALE_LOADERS[target];
  if (!loader) {
    return;
  }
  let task = localeLoadTasks.get(target);
  if (!task) {
    task = loader()
      .then((messages) => {
        LOCALES[target] = messages || {};
        localeLoadTasks.delete(target);
        return LOCALES[target];
      })
      .catch((error) => {
        localeLoadTasks.delete(target);
        throw error;
      });
    localeLoadTasks.set(target, task);
  }
  await task;
};

const resolveLanguageCode = (raw: unknown): string => {
  const cleaned = String(raw || '').trim();
  if (!cleaned) {
    return '';
  }
  const lowered = cleaned.toLowerCase();
  const mapped = languageAliases[lowered];
  if (mapped) {
    return mapped;
  }
  if (supportedLanguages.includes(cleaned)) {
    return cleaned;
  }
  return (
    supportedLanguages.find((lang) => lang.toLowerCase() === lowered) ||
    languageAliases[cleaned] ||
    ''
  );
};

const formatMessage = (template: string, params: Record<string, unknown>): string => {
  if (!params || typeof params !== 'object') {
    return template;
  }
  return Object.keys(params).reduce(
    (result, key) =>
      result.replace(new RegExp(`\{${key}\}`, 'g'), String(params[key] ?? '')),
    template
  );
};

const resolveInitialLanguage = (): string => {
  const stored = resolveLanguageCode(localStorage.getItem(STORAGE_KEY));
  if (stored) {
    return stored;
  }
  const browser = resolveLanguageCode(
    (navigator.languages && navigator.languages[0]) || navigator.language
  );
  return browser || defaultLanguage;
};

const currentLanguage = ref(resolveInitialLanguage());

export const t = (key: string, params: Record<string, unknown> = {}): string => {
  const locale = resolveLocale(currentLanguage.value);
  const fallbackLocale = resolveLocale(defaultLanguage);
  const template = locale[key] || fallbackLocale[key] || key;
  return formatMessage(template, params);
};

export const getCurrentLanguage = (): string => currentLanguage.value;

export const getSupportedLanguages = (): string[] => [...supportedLanguages];

export const getLanguageLabel = (language: string): string => {
  const code = String(language || '').trim();
  const key = `language.${code}`;
  const locale = resolveLocale(currentLanguage.value);
  return locale[key] || languageLabels[code] || code;
};

export const setLanguage = async (language: unknown, options: SetLanguageOptions = {}): Promise<string> => {
  const next = resolveLanguageCode(language) || defaultLanguage;
  if (next === currentLanguage.value && !options.force) {
    return currentLanguage.value;
  }
  await ensureLocaleLoaded(next);
  currentLanguage.value = next;
  if (document?.documentElement) {
    document.documentElement.lang = currentLanguage.value;
  }
  if (options.persist !== false) {
    localStorage.setItem(STORAGE_KEY, currentLanguage.value);
  }
  if (typeof window !== 'undefined' && options.emit !== false) {
    window.dispatchEvent(
      new CustomEvent('wunder:language-changed', {
        detail: { language: currentLanguage.value }
      })
    );
  }
  return currentLanguage.value;
};

export const configureI18n = (config: I18nConfigPayload = {}): void => {
  const nextSupported = Array.isArray(config.supported_languages)
    ? config.supported_languages.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  supportedLanguages = nextSupported.length
    ? Array.from(new Set(nextSupported))
    : Object.keys(LOCALES);

  const mergedAliases: LanguageAliases = { ...DEFAULT_LANGUAGE_ALIASES };
  if (config && typeof config.aliases === 'object') {
    Object.entries(config.aliases).forEach(([key, value]) => {
      const aliasKey = String(key || '').trim().toLowerCase();
      const aliasValue = resolveLanguageCode(value);
      if (!aliasKey || !aliasValue) {
        return;
      }
      mergedAliases[aliasKey] = aliasValue;
    });
  }
  supportedLanguages.forEach((lang) => {
    mergedAliases[lang.toLowerCase()] = lang;
  });
  languageAliases = mergedAliases;

  if (config && typeof config.labels === 'object') {
    const nextLabels: LanguageLabels = { ...languageLabels };
    Object.entries(config.labels).forEach(([key, value]) => {
      const lang = String(key || '').trim();
      const label = String(value || '').trim();
      if (lang && label) {
        nextLabels[lang] = label;
      }
    });
    languageLabels = nextLabels;
  }

  const resolvedDefault = resolveLanguageCode(config.default_language);
  if (resolvedDefault) {
    defaultLanguage = resolvedDefault;
  }
};

const resolveI18nEndpoint = (): string => {
  const base = resolveApiBase();
  return `${base.replace(/\/+$/, '')}/i18n`;
};

const buildI18nRequestHeaders = (): HeadersInit | undefined => {
  try {
    const token = String(localStorage.getItem('access_token') || '').trim();
    if (!token) {
      return undefined;
    }
    return {
      Authorization: `Bearer ${token}`
    };
  } catch {
    return undefined;
  }
};

export const initI18n = async (): Promise<void> => {
  try {
    const response = await fetch(resolveI18nEndpoint(), {
      method: 'GET',
      headers: buildI18nRequestHeaders()
    });
    if (response.ok) {
      const payload = (await response.json()) as { data?: I18nConfigPayload } & Record<string, unknown>;
      configureI18n((payload.data as I18nConfigPayload) || (payload as I18nConfigPayload) || {});
    }
  } catch {
    // keep local defaults when remote i18n config is unavailable
  }
  const initialLanguage = resolveInitialLanguage();
  await Promise.all([ensureLocaleLoaded(defaultLanguage), ensureLocaleLoaded(initialLanguage)]);
  await setLanguage(initialLanguage, { force: true, persist: false, emit: false });
};

export const useI18n = () => ({
  t,
  language: currentLanguage,
  setLanguage,
  getSupportedLanguages,
  getLanguageLabel
});
