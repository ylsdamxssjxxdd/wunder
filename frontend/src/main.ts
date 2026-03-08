import { createApp } from 'vue';
import { createPinia } from 'pinia';
import 'element-plus/dist/index.css';
import '@/vendor/fontawesome/css/fontawesome.min.css';
import '@/vendor/fontawesome/css/solid.min.css';
import '@/vendor/hula-icon.js';
import '@/styles/main.css';

import App from './App.vue';
import router from './router';
import { usePerformanceStore } from '@/stores/performance';
import { useThemeStore } from '@/stores/theme';
import { initI18n } from '@/i18n';
import { loadRuntimeConfig } from '@/config/runtime';
import { initDesktopRuntime } from '@/config/desktop';
import { installElementPlus } from '@/plugins/elementPlus';

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
usePerformanceStore(pinia);
useThemeStore(pinia);
installElementPlus(app);
app.use(router);

const bootstrap = async () => {
  await initDesktopRuntime();
  await loadRuntimeConfig();
  await initI18n();
  app.mount('#app');
};

bootstrap();
