import { defineConfig } from '@playwright/test';

const PORT = Number(process.env.PLAYWRIGHT_VITE_PORT || 4174);
const HOST = process.env.PLAYWRIGHT_VITE_HOST || '127.0.0.1';
const baseURL = `http://${HOST}:${PORT}`;

export default defineConfig({
  testDir: './tests/e2e',
  timeout: 45_000,
  expect: {
    timeout: 8_000
  },
  fullyParallel: true,
  reporter: [['list']],
  use: {
    baseURL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure'
  },
  webServer: {
    command: `node ../node_modules/vite/bin/vite.js --host ${HOST} --port ${PORT}`,
    url: `${baseURL}/__e2e/beeroom-harness`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000
  }
});
