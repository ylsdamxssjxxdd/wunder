import { ref } from 'vue';

import enUS from './messages/en-US';
import zhCN from './messages/zh-CN';
import { resolveApiBase } from '@/config/runtime';

const LOCALES = {
  'zh-CN': zhCN,
  'en-US': enUS
};

const DEFAULT_LANGUAGE_ALIASES = {
  zh: 'zh-CN',
  'zh-cn': 'zh-CN',
  'zh-hans': 'zh-CN',
  'zh-hant': 'zh-CN',
  en: 'en-US',
  'en-us': 'en-US',
  'en-gb': 'en-US'
};

const DEFAULT_LANGUAGE_LABELS = {
  'zh-CN': '简体中文',
  'en-US': 'English'
};

const STORAGE_KEY = 'wunder_language';

let defaultLanguage = 'zh-CN';
let supportedLanguages = Object.keys(LOCALES);
let languageAliases = { ...DEFAULT_LANGUAGE_ALIASES };
let languageLabels = { ...DEFAULT_LANGUAGE_LABELS };

const resolveLocale = (language) => {
  if (language && LOCALES[language]) {
    return LOCALES[language];
  }
  if (defaultLanguage && LOCALES[defaultLanguage]) {
    return LOCALES[defaultLanguage];
  }
  const fallbackKey = Object.keys(LOCALES)[0];
  return fallbackKey ? LOCALES[fallbackKey] : {};
};

const resolveLanguageCode = (raw) => {
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

const formatMessage = (template, params) => {
  if (!params || typeof params !== 'object') {
    return template;
  }
  return Object.keys(params).reduce(
    (result, key) =>
      result.replace(new RegExp(`\\{${key}\\}`, 'g'), String(params[key] ?? '')),
    template
  );
};

const resolveInitialLanguage = () => {
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

export const t = (key, params = {}) => {
  const locale = resolveLocale(currentLanguage.value);
  const fallbackLocale = resolveLocale(defaultLanguage);
  const template = locale[key] || fallbackLocale[key] || key;
  return formatMessage(template, params);
};

export const getCurrentLanguage = () => currentLanguage.value;

export const getSupportedLanguages = () => [...supportedLanguages];

export const getLanguageLabel = (language) => {
  const code = String(language || '').trim();
  const key = `language.${code}`;
  const locale = resolveLocale(currentLanguage.value);
  return locale[key] || languageLabels[code] || code;
};

export const setLanguage = (language, options = {}) => {
  const next = resolveLanguageCode(language) || defaultLanguage;
  if (next === currentLanguage.value && !options.force) {
    return currentLanguage.value;
  }
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

export const configureI18n = (config = {}) => {
  const nextSupported = Array.isArray(config.supported_languages)
    ? config.supported_languages.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  supportedLanguages = nextSupported.length
    ? Array.from(new Set(nextSupported))
    : Object.keys(LOCALES);

  const mergedAliases = { ...DEFAULT_LANGUAGE_ALIASES };
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
    const nextLabels = { ...languageLabels };
    Object.entries(config.labels).forEach(([key, value]) => {
      const lang = String(key || '').trim();
      const label = String(value || '').trim();
      if (lang && label) {
        nextLabels[lang] = label;
      }
    });
    languageLabels = nextLabels;
  }

  const resolvedDefault = resolveLanguageCode(config?.default_language);
  if (resolvedDefault) {
    defaultLanguage = resolvedDefault;
  }
};

const resolveI18nEndpoint = () => {
  const base = resolveApiBase();
  return `${base.replace(/\/+$/, '')}/i18n`;
};

export const initI18n = async () => {
  try {
    const response = await fetch(resolveI18nEndpoint(), { method: 'GET' });
    if (response.ok) {
      const payload = await response.json();
      configureI18n(payload?.data || payload || {});
    }
  } catch (error) {
    // 保持本地默认配置即可
  }
  setLanguage(resolveInitialLanguage(), { force: true, persist: false, emit: false });
};

export const useI18n = () => ({
  t,
  language: currentLanguage,
  setLanguage,
  getSupportedLanguages,
  getLanguageLabel
});
