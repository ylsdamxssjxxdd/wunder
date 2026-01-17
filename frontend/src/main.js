import { createApp } from 'vue';
import { createPinia } from 'pinia';
import ElementPlus from 'element-plus';
import 'element-plus/dist/index.css';
import '@/styles/main.css';

import App from './App.vue';
import router from './router';
import { usePerformanceStore } from '@/stores/performance';

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
usePerformanceStore(pinia);
app.use(ElementPlus);
app.use(router);
app.mount('#app');
