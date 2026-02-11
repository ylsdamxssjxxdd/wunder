import path from 'node:path';
import { fileURLToPath, URL } from 'node:url';

import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const devProxyTarget = process.env.VITE_DEV_PROXY_TARGET || 'http://127.0.0.1:18000';

const makeProxyRule = () => ({
  target: devProxyTarget,
  changeOrigin: true,
  ws: true,
  secure: false
});

export default defineConfig({
  envDir: path.resolve(__dirname, '..'),
  plugins: [vue()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  },
  server: {
    host: '0.0.0.0',
    port: 18001,
    proxy: {
      '/wunder': makeProxyRule(),
      '/a2a': makeProxyRule(),
      '/.well-known/agent-card.json': makeProxyRule()
    }
  }
});
