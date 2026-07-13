import { defineConfig } from '@playwright/test';

const baseURL = process.env.BEEROOM_REAL_BASE_URL || 'http://127.0.0.1:18001';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: 'beeroom-real-service.spec.ts',
  timeout: 180_000,
  expect: {
    timeout: 15_000
  },
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL,
    viewport: { width: 1600, height: 1000 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure'
  }
});
