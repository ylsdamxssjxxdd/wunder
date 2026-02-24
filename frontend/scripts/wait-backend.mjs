import http from 'node:http';
import https from 'node:https';

const targetText = process.env.VITE_DEV_PROXY_TARGET || 'http://wunder-server:18000';
const timeoutSeconds = Number.parseInt(
  process.env.FRONTEND_WAIT_BACKEND_TIMEOUT_S || '900',
  10
);
const intervalMs = Number.parseInt(
  process.env.FRONTEND_WAIT_BACKEND_INTERVAL_MS || '1000',
  10
);

let target;
try {
  target = new URL(targetText);
} catch (_error) {
  console.warn(`[frontend] invalid VITE_DEV_PROXY_TARGET: ${targetText}`);
  process.exit(0);
}

const healthUrl = new URL('/health', target);
const client = healthUrl.protocol === 'https:' ? https : http;
const deadline = Date.now() + Math.max(timeoutSeconds, 0) * 1000;
const delayMs = Math.max(intervalMs, 200);

const scheduleNext = () => {
  if (Date.now() > deadline) {
    console.warn(
      `[frontend] backend wait timeout (${timeoutSeconds}s), start vite anyway: ${healthUrl.href}`
    );
    process.exit(0);
  }
  setTimeout(ping, delayMs);
};

const ping = () => {
  const request = client.request(healthUrl, { method: 'GET' }, (response) => {
    response.resume();
    if ((response.statusCode ?? 0) >= 200 && (response.statusCode ?? 0) < 500) {
      console.log(`[frontend] backend ready: ${healthUrl.href}`);
      process.exit(0);
      return;
    }
    scheduleNext();
  });

  request.on('error', scheduleNext);
  request.setTimeout(5000, () => {
    request.destroy(new Error('timeout'));
  });
  request.end();
};

ping();
