import { createApp } from 'vue';
import { createPinia } from 'pinia';
import 'element-plus/dist/index.css';
import 'katex/dist/katex.min.css';
import '@/vendor/fontawesome/css/fontawesome.min.css';
import '@/vendor/fontawesome/css/solid.min.css';
import '@/vendor/hula-icon.js';
import '@/styles/main.css';

import App from './App.vue';
import router from './router';
import { useThemeStore } from '@/stores/theme';
import { initI18n } from '@/i18n';
import { loadRuntimeConfig } from '@/config/runtime';
import {
  initDesktopRuntime,
  isDesktopModeEnabled,
  reportDesktopRendererStage,
  dismissDesktopStartupShell
} from '@/config/desktop';
import { installElementPlus } from '@/plugins/elementPlus';
import {
  clearAsyncComponentReloadMarker,
  reloadOnceForAsyncComponentFailure
} from '@/utils/asyncComponentRecovery';
import { installAuthSessionSync } from '@/utils/authSessionSync';
import { clearAllAccessTokens } from '@/utils/authTokenStorage';

const LEGACY_PERFORMANCE_STORAGE_KEYS = ['beeroom-performance-mode', 'wille-performance-mode'] as const;
const APP_VERSION_STORAGE_KEY = 'wunder_app_version';
const DESKTOP_RENDERER_FAILURE_ELEMENT_ID = 'wunder-desktop-renderer-failure';

type RendererFailurePayload = {
  source: string;
  message: string;
  stage?: string;
};

let rendererBootstrapStage = 'module-loaded';
let rendererFailureReported = false;
let rendererBlankCheckTimer: number | undefined;

function normalizeErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message || error.name || 'unknown error';
  }
  if (typeof error === 'string') {
    return error;
  }
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function reportRendererFailure(payload: RendererFailurePayload): void {
  if (rendererFailureReported && payload.source !== 'bootstrap') {
    return;
  }
  rendererFailureReported = true;
  reportDesktopRendererStage(payload.source === 'bootstrap' ? 'bootstrap-error' : 'renderer-error', {
    source: payload.source,
    message: payload.message,
    stage: payload.stage || rendererBootstrapStage,
    fatal: payload.source === 'bootstrap'
  });
}

function hasVisibleAppContent(): boolean {
  if (typeof document === 'undefined') {
    return true;
  }
  const appRoot = document.getElementById('app');
  if (!appRoot) {
    return false;
  }
  const text = String(appRoot.textContent || '').trim();
  return appRoot.childElementCount > 0 || Boolean(text);
}

function scheduleBlankRendererFallback(payload: RendererFailurePayload): void {
  if (typeof window === 'undefined' || !isDesktopModeEnabled()) {
    return;
  }
  if (rendererBlankCheckTimer !== undefined) {
    window.clearTimeout(rendererBlankCheckTimer);
  }
  rendererBlankCheckTimer = window.setTimeout(() => {
    rendererBlankCheckTimer = undefined;
    if (hasVisibleAppContent()) {
      return;
    }
    reportDesktopRendererStage('renderer-error', {
      source: payload.source,
      message: payload.message,
      stage: payload.stage || rendererBootstrapStage,
      fatal: true
    });
    renderDesktopFailurePage(payload);
  }, 250);
}

function renderDesktopFailurePage(payload: RendererFailurePayload): void {
  if (typeof document === 'undefined' || !isDesktopModeEnabled()) {
    return;
  }
  dismissDesktopStartupShell();
  const root = document.getElementById('app') || document.body;
  if (!root) {
    return;
  }
  const message = escapeHtml(payload.message || 'unknown error');
  const stage = escapeHtml(payload.stage || rendererBootstrapStage || 'unknown');
  const source = escapeHtml(payload.source || 'runtime');
  root.innerHTML = `
    <div id="${DESKTOP_RENDERER_FAILURE_ELEMENT_ID}">
      <style>
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} {
          min-height: 100vh;
          display: grid;
          place-items: center;
          padding: 24px;
          box-sizing: border-box;
          background: #f8fafc;
          color: #0f172a;
          font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
        }
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} .panel {
          width: min(720px, 100%);
          padding: 24px;
          border: 1px solid rgba(148, 163, 184, 0.45);
          border-radius: 10px;
          background: #fff;
          box-shadow: 0 18px 60px rgba(15, 23, 42, 0.16);
        }
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} h1 {
          margin: 0 0 10px;
          font-size: 18px;
          line-height: 1.35;
        }
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} p {
          margin: 0;
          color: #475569;
          font-size: 14px;
          line-height: 1.65;
        }
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} pre {
          margin: 14px 0 0;
          padding: 12px;
          border-radius: 8px;
          background: #f1f5f9;
          color: #334155;
          font-size: 12px;
          line-height: 1.55;
          white-space: pre-wrap;
          word-break: break-word;
        }
        #${DESKTOP_RENDERER_FAILURE_ELEMENT_ID} button {
          min-height: 36px;
          margin-top: 18px;
          padding: 0 14px;
          border: 1px solid #cbd5e1;
          border-radius: 8px;
          background: #0f172a;
          color: #fff;
          font: inherit;
          cursor: pointer;
        }
      </style>
      <div class="panel">
        <h1>Wunder Desktop recovered from a UI error</h1>
        <p>The window avoided a blank screen and kept the failure details visible.</p>
        <pre>source=${source}
stage=${stage}
message=${message}</pre>
        <button type="button">Reload desktop</button>
      </div>
    </div>
  `;
  root
    .querySelector('button')
    ?.addEventListener('click', () => window.location.reload(), { once: true });
}

function installRendererFailureHandlers(): void {
  if (typeof window === 'undefined') {
    return;
  }
  window.addEventListener('error', (event) => {
    const message = event.message || normalizeErrorMessage(event.error);
    const payload = { source: 'window-error', message, stage: rendererBootstrapStage };
    reportRendererFailure(payload);
    scheduleBlankRendererFallback(payload);
  });
  window.addEventListener('unhandledrejection', (event) => {
    const message = normalizeErrorMessage(event.reason);
    const payload = { source: 'unhandledrejection', message, stage: rendererBootstrapStage };
    reportRendererFailure(payload);
    scheduleBlankRendererFallback(payload);
  });
}

function applyAppVersionStorageMigration() {
  if (typeof window === 'undefined') {
    return;
  }
  const currentVersion = String(__WUNDER_APP_VERSION__ || '').trim();
  if (!currentVersion) {
    return;
  }
  let previousVersion = '';
  let canPersistVersion = true;
  try {
    previousVersion = String(window.localStorage.getItem(APP_VERSION_STORAGE_KEY) || '').trim();
  } catch {
    previousVersion = '';
    canPersistVersion = false;
  }
  if (!canPersistVersion) {
    return;
  }
  if (previousVersion === currentVersion) {
    return;
  }
  clearAllAccessTokens();
  try {
    window.localStorage.setItem(APP_VERSION_STORAGE_KEY, currentVersion);
  } catch {
    // ignore storage failures
  }
}

function clearLegacyPerformanceMode() {
  if (typeof window === 'undefined') {
    return;
  }
  for (const key of LEGACY_PERFORMANCE_STORAGE_KEYS) {
    window.localStorage.removeItem(key);
  }
  document.documentElement.removeAttribute('data-performance-mode');
}

const app = createApp(App);
const pinia = createPinia();
installRendererFailureHandlers();
app.config.errorHandler = (error, instance, info) => {
  const payload = {
    source: 'vue',
    message: normalizeErrorMessage(error),
    stage: info || rendererBootstrapStage
  };
  reportRendererFailure(payload);
  scheduleBlankRendererFallback(payload);
  console.error('[wunder-renderer][vue]', info, instance, error);
};
app.use(pinia);
applyAppVersionStorageMigration();
clearLegacyPerformanceMode();
useThemeStore(pinia);
installElementPlus(app);
app.use(router);
installAuthSessionSync(router);

if (import.meta.env.DEV && typeof window !== 'undefined') {
  // Recover from stale Vite optimized-deps/chunk URLs after hot updates or server cache invalidation.
  window.addEventListener('vite:preloadError', (event) => {
    event.preventDefault();
    reloadOnceForAsyncComponentFailure();
  });
}

const bootstrap = async () => {
  rendererBootstrapStage = 'bootstrap-start';
  reportDesktopRendererStage('bootstrap-start');
  await initDesktopRuntime();
  rendererBootstrapStage = 'desktop-runtime-ready';
  reportDesktopRendererStage('desktop-runtime-ready');
  await loadRuntimeConfig();
  rendererBootstrapStage = 'runtime-config-ready';
  reportDesktopRendererStage('runtime-config-ready');
  await initI18n();
  rendererBootstrapStage = 'i18n-ready';
  reportDesktopRendererStage('i18n-ready');
  // Resolve the initial async route before mounting the desktop chrome. This
  // keeps the startup shell in place instead of briefly rendering an empty
  // router-view while MessengerView is still loading.
  await router.isReady();
  rendererBootstrapStage = 'initial-route-ready';
  reportDesktopRendererStage('initial-route-ready');
  app.mount('#app');
  rendererBootstrapStage = 'app-mounted';
  reportDesktopRendererStage('app-mounted');
  clearAsyncComponentReloadMarker();
};

bootstrap().catch((error) => {
  const message = normalizeErrorMessage(error);
  reportRendererFailure({ source: 'bootstrap', message, stage: rendererBootstrapStage });
  renderDesktopFailurePage({ source: 'bootstrap', message, stage: rendererBootstrapStage });
  console.error('[wunder-renderer][bootstrap]', error);
});
