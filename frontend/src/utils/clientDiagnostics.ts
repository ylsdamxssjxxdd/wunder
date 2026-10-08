/**
 * 客户端诊断导出（方案 §九：常规 / 帮助分类的「导出诊断」）。
 *
 * 只导出**浏览器本地可自证**的信息：版本、运行形态、语言/主题/字号偏好、
 * 当前设置分类、视口与 UA。不包含令牌、账号标识或任何服务端数据，
 * 避免诊断文件本身成为泄露面。
 */

import { APP_VERSION } from '@/config/appVersion';
import { saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';

export type ClientDiagnosticsContext = {
  language?: string;
  themePalette?: string;
  uiFontSize?: number;
  sendKey?: string;
  settingsCategory?: string;
  section?: string;
  authenticated?: boolean;
};

export type ClientDiagnosticsSnapshot = {
  generated_at: string;
  app_version: string;
  runtime: string;
  language: string;
  timezone: string;
  viewport: string;
  user_agent: string;
  preferences: {
    theme_palette: string;
    ui_font_size: number;
    send_key: string;
  };
  view: {
    settings_category: string;
    section: string;
  };
  session: {
    authenticated: boolean;
  };
};

export const buildClientDiagnostics = (
  context: ClientDiagnosticsContext = {}
): ClientDiagnosticsSnapshot => {
  const viewport =
    typeof window === 'undefined'
      ? ''
      : `${window.innerWidth}x${window.innerHeight}@${Number(window.devicePixelRatio || 1)}`;
  let timezone = '';
  try {
    timezone = String(Intl.DateTimeFormat().resolvedOptions().timeZone || '');
  } catch {
    timezone = '';
  }
  return {
    generated_at: new Date().toISOString(),
    app_version: String(APP_VERSION || ''),
    runtime: 'web',
    language: String(context.language || ''),
    timezone,
    viewport,
    user_agent: typeof navigator === 'undefined' ? '' : String(navigator.userAgent || ''),
    preferences: {
      theme_palette: String(context.themePalette || ''),
      ui_font_size: Number(context.uiFontSize || 0),
      send_key: String(context.sendKey || '')
    },
    view: {
      settings_category: String(context.settingsCategory || ''),
      section: String(context.section || '')
    },
    session: {
      authenticated: context.authenticated === true
    }
  };
};

export const exportClientDiagnostics = (context: ClientDiagnosticsContext = {}): string => {
  const snapshot = buildClientDiagnostics(context);
  const filename = `diagnostics-${snapshot.generated_at.replace(/[:.]/g, '-')}.json`;
  if (typeof window === 'undefined') {
    return filename;
  }
  const blob = new Blob([JSON.stringify(snapshot, null, 2)], {
    type: 'application/json;charset=utf-8'
  });
  const objectUrl = URL.createObjectURL(blob);
  saveObjectUrlAsFile(objectUrl, filename);
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
  return filename;
};
