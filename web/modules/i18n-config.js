import { configureI18n } from "./i18n.js?v=20260110-04";

// 统一拉取后端 i18n 配置，保持语言设置一致
const resolveI18nEndpoint = (apiBase) => {
  const base = String(apiBase || "").trim();
  if (!base) {
    return "/wunder/i18n";
  }
  const trimmed = base.replace(/\/+$/, "");
  const normalized = trimmed.endsWith("/wunder") ? trimmed : `${trimmed}/wunder`;
  return `${normalized}/i18n`;
};

export const loadI18nConfig = async (options = {}) => {
  const endpoint = resolveI18nEndpoint(options.apiBase);
  const headers = new Headers();
  const apiKey = String(options.apiKey || "").trim();
  if (apiKey) {
    headers.set("X-API-Key", apiKey);
  }
  const language = String(options.language || "").trim();
  if (language) {
    headers.set("X-Wunder-Language", language);
  }
  try {
    const response = await fetch(endpoint, { headers });
    if (!response.ok) {
      throw new Error(`i18n request failed: ${response.status}`);
    }
    const data = await response.json();
    configureI18n(data);
    return data;
  } catch (error) {
    configureI18n();
    return null;
  }
};
