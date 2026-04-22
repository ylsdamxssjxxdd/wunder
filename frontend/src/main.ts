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
import { initDesktopRuntime } from '@/config/desktop';
import { installElementPlus } from '@/plugins/elementPlus';
import { clearAsyncComponentReloadMarker } from '@/utils/asyncComponentRecovery';

const LEGACY_PERFORMANCE_STORAGE_KEYS = ['beeroom-performance-mode', 'wille-performance-mode'] as const;

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
clearLegacyPerformanceMode();
useThemeStore(pinia);
installElementPlus(app);
app.use(router);

if (import.meta.env.DEV && typeof window !== 'undefined') {
  // Recover from stale Vite optimized-deps/chunk URLs after hot updates or server cache invalidation.
  window.addEventListener('vite:preloadError', (event) => {
    event.preventDefault();
    window.location.reload();
  });
}

clearAsyncComponentReloadMarker();

const bootstrap = async () => {
  await initDesktopRuntime();
  await loadRuntimeConfig();
  await initI18n();
  app.mount('#app');
};

bootstrap();
