import path from 'node:path';
import { existsSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';

import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vite';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoEnvDirCandidate = path.resolve(__dirname, '..');
const filesystemRoot = path.parse(repoEnvDirCandidate).root;
const envDir =
  process.env.VITE_ENV_DIR ||
  (repoEnvDirCandidate !== filesystemRoot &&
  (existsSync(path.join(repoEnvDirCandidate, '.env')) ||
    existsSync(path.join(repoEnvDirCandidate, '.env.example')))
    ? repoEnvDirCandidate
    : __dirname);
const devProxyTarget = process.env.VITE_DEV_PROXY_TARGET || 'http://127.0.0.1:18000';

const makeProxyRule = () => ({
  target: devProxyTarget,
  changeOrigin: true,
  ws: true,
  secure: false
});

const normalizePath = (id: string) => id.replace(/\\/g, '/');

const resolveManualChunk = (rawId: string) => {
  const id = normalizePath(rawId);
  if (!id.includes('/node_modules/')) {
    return undefined;
  }
  if (
    id.includes('/node_modules/vue/') ||
    id.includes('/node_modules/@vue/') ||
    id.includes('/node_modules/vue-router/') ||
    id.includes('/node_modules/pinia/')
  ) {
    return 'vendor-vue';
  }
  if (id.includes('/node_modules/element-plus/')) {
    return 'vendor-element-plus';
  }
  if (id.includes('/node_modules/@antv/')) {
    return 'vendor-antv';
  }
  if (id.includes('/node_modules/echarts/') || id.includes('/node_modules/zrender/')) {
    return 'vendor-echarts';
  }
  if (id.includes('/node_modules/three/') || id.includes('/node_modules/topojson-client/')) {
    return 'vendor-3d';
  }
  if (id.includes('/node_modules/markdown-it/')) {
    return 'vendor-markdown';
  }
  if (id.includes('/node_modules/axios/')) {
    return 'vendor-http';
  }
  return 'vendor-misc';
};

export default defineConfig({
  envDir,
  plugins: [vue()],
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          return resolveManualChunk(id);
        }
      }
    }
  },
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
