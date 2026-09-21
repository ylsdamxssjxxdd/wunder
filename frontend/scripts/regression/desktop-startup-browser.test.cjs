const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { chromium } = require('@playwright/test');

const root = path.resolve(process.argv[2] || 'dist-desktop');

test('built desktop entry paints before importing the app and can recover from a failed chunk', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    for (const fail of [false, true]) {
      const page = await browser.newPage();
      await page.addInitScript(() => {
        window.wunderDesktop = {
          reportRendererStage: () => Promise.resolve(true)
        };
      });
      let appRequestObserved = false;
      await page.route('http://desktop.test/**', async (route) => {
        const pathname = new URL(route.request().url()).pathname;
        if (/\/main-[^/]+\.js$/.test(pathname)) {
          appRequestObserved = true;
          const timing = await page.evaluate(() => ({
            paint: performance.getEntriesByName('first-contentful-paint')[0]?.startTime,
            release: performance.getEntriesByName('desktop-post-first-frame')[0]?.startTime,
            visible: getComputedStyle(document.getElementById('wunder-desktop-startup-shell')).display
          }));
          assert.equal(timing.visible, 'grid');
          assert.equal(typeof timing.paint, 'number');
          assert.ok(timing.release - timing.paint >= 10, JSON.stringify(timing));
          await route.fulfill({
            status: fail ? 503 : 200,
            contentType: 'text/javascript',
            body: fail ? '' : 'window.applicationImported = true;'
          });
          return;
        }
        const file = path.resolve(root, pathname === '/' ? 'index.html' : `.${pathname}`);
        if (!file.startsWith(`${root}${path.sep}`) || !fs.existsSync(file)) {
          await route.fulfill({ status: 404, body: '' });
          return;
        }
        const contentType = file.endsWith('.html') ? 'text/html'
          : file.endsWith('.js') ? 'text/javascript'
          : file.endsWith('.css') ? 'text/css' : 'application/octet-stream';
        await route.fulfill({ contentType, body: fs.readFileSync(file) });
      });
      await page.goto('http://desktop.test/');
      if (fail) {
        await page.getByRole('button', { name: 'Reload' }).waitFor();
        assert.match(await page.locator('.wunder-startup-shell__hint').textContent(), /Unable to load/);
      } else {
        await page.waitForFunction(() => window.applicationImported === true);
      }
      assert.equal(appRequestObserved, true);
      await page.close();
    }
  } finally {
    await browser.close();
  }
});

test('Tauri startup shell paints before IPC and retries a failed runtime', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.addInitScript(() => {
      window.startupCalls = [];
      window.__TAURI_INTERNALS__ = {
        invoke: async (command) => {
          window.startupCalls.push({
            command,
            at: performance.now(),
            paint: performance.getEntriesByName('first-contentful-paint')[0]?.startTime
          });
          throw new Error('unavailable');
        }
      };
    });
    await page.route('http://desktop.test/**', (route) => route.fulfill({
      contentType: 'text/html',
      body: fs.readFileSync(path.resolve(__dirname, '../../../crates/wunder-desktop/startup.html'))
    }));
    await page.goto('http://desktop.test/');
    await page.getByRole('button', { name: 'Retry' }).waitFor();
    await page.getByRole('button', { name: 'Retry' }).click();
    await page.waitForFunction(() => window.startupCalls.length === 2);
    const calls = await page.evaluate(() => window.startupCalls);
    assert.deepEqual(calls.map((call) => call.command), ['desktop_startup_ready', 'desktop_startup_ready']);
    assert.ok(calls[0].at - calls[0].paint >= 10, JSON.stringify(calls));
  } finally {
    await browser.close();
  }
});
