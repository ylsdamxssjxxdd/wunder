import { createApp } from 'vue';
import { createPinia } from 'pinia';
import 'element-plus/dist/index.css';
import '@/vendor/fontawesome/css/fontawesome.min.css';
import '@/vendor/fontawesome/css/solid.min.css';
import '@/vendor/hula-icon.js';
import '@/styles/main.css';

import App from './App.vue';
import router from './router';
import { useThemeStore } from '@/stores/theme';
import { initI18n } from '@/i18n';
import { loadRuntimeConfig } from '@/config/runtime';
import { initDesktopRuntime, reportDesktopRendererStage } from '@/config/desktop';
import { installElementPlus } from '@/plugins/elementPlus';
import {
  clearAsyncComponentReloadMarker,
  reloadOnceForAsyncComponentFailure
} from '@/utils/asyncComponentRecovery';
import { installAuthSessionSync } from '@/utils/authSessionSync';
import { clearAllAccessTokens } from '@/utils/authTokenStorage';

const LEGACY_PERFORMANCE_STORAGE_KEYS = ['beeroom-performance-mode', 'wille-performance-mode'] as const;
const APP_VERSION_STORAGE_KEY = 'wunder_app_version';

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
  reportDesktopRendererStage('bootstrap-start');
  await initDesktopRuntime();
  reportDesktopRendererStage('desktop-runtime-ready');
  await loadRuntimeConfig();
  reportDesktopRendererStage('runtime-config-ready');
  await initI18n();
  reportDesktopRendererStage('i18n-ready');
  app.mount('#app');
  reportDesktopRendererStage('app-mounted');
  clearAsyncComponentReloadMarker();
};

bootstrap();
