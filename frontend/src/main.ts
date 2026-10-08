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
import { installElementPlus } from '@/plugins/elementPlus';
import {
  clearAsyncComponentReloadMarker,
  reloadOnceForAsyncComponentFailure
} from '@/utils/asyncComponentRecovery';
import { installAuthSessionSync } from '@/utils/authSessionSync';
import { clearAllAccessTokens } from '@/utils/authTokenStorage';

const LEGACY_PERFORMANCE_STORAGE_KEYS = ['beeroom-performance-mode', 'wille-performance-mode'] as const;
const APP_VERSION_STORAGE_KEY = 'wunder_app_version';

type RendererFailurePayload = {
  source: string;
  message: string;
  stage?: string;
};

let rendererBootstrapStage = 'module-loaded';

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

function reportRendererFailure(payload: RendererFailurePayload): void {
  console.error('[wunder-renderer]', payload.source, payload.stage || rendererBootstrapStage, payload.message);
}

function installRendererFailureHandlers(): void {
  if (typeof window === 'undefined') {
    return;
  }
  window.addEventListener('error', (event) => {
    reportRendererFailure({
      source: 'window-error',
      message: event.message || normalizeErrorMessage(event.error),
      stage: rendererBootstrapStage
    });
  });
  window.addEventListener('unhandledrejection', (event) => {
    reportRendererFailure({
      source: 'unhandledrejection',
      message: normalizeErrorMessage(event.reason),
      stage: rendererBootstrapStage
    });
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
  reportRendererFailure({
    source: 'vue',
    message: normalizeErrorMessage(error),
    stage: info || rendererBootstrapStage
  });
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
  await loadRuntimeConfig();
  rendererBootstrapStage = 'runtime-config-ready';
  await initI18n();
  rendererBootstrapStage = 'i18n-ready';
  // Resolve the initial async route before mounting so the first paint is the
  // real page instead of an empty router-view while MessengerView loads.
  await router.isReady();
  rendererBootstrapStage = 'initial-route-ready';
  app.mount('#app');
  rendererBootstrapStage = 'app-mounted';
  clearAsyncComponentReloadMarker();
};

bootstrap().catch((error) => {
  reportRendererFailure({ source: 'bootstrap', message: normalizeErrorMessage(error), stage: rendererBootstrapStage });
  console.error('[wunder-renderer][bootstrap]', error);
});
