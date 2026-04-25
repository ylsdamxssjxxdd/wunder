import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { createServer } from 'vite';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const parseArgs = (argv) => {
  const parsed = {
    host: '',
    port: '',
    strictPort: false
  };

  for (let index = 0; index < argv.length; index += 1) {
    const current = argv[index];
    const next = argv[index + 1];

    if (current === '--host' && next) {
      parsed.host = next;
      index += 1;
      continue;
    }

    if (current.startsWith('--host=')) {
      parsed.host = current.slice('--host='.length);
      continue;
    }

    if (current === '--port' && next) {
      parsed.port = next;
      index += 1;
      continue;
    }

    if (current.startsWith('--port=')) {
      parsed.port = current.slice('--port='.length);
      continue;
    }

    if (current === '--strictPort') {
      parsed.strictPort = true;
    }
  }

  return parsed;
};

const parseBoolean = (value, fallback = false) => {
  if (value == null || value === '') {
    return fallback;
  }

  const normalized = String(value).trim().toLowerCase();
  if (['1', 'true', 'yes', 'on'].includes(normalized)) {
    return true;
  }
  if (['0', 'false', 'no', 'off'].includes(normalized)) {
    return false;
  }
  return fallback;
};

const parsePort = (value, fallback) => {
  const parsed = Number.parseInt(String(value || ''), 10);
  if (Number.isInteger(parsed) && parsed > 0) {
    return parsed;
  }
  return fallback;
};

const formatFallbackUrl = (host, port) => {
  if (!host || host === '0.0.0.0' || host === '::') {
    return `http://127.0.0.1:${port}/`;
  }
  return `http://${host}:${port}/`;
};

const args = parseArgs(process.argv.slice(2));
const host = args.host || process.env.FRONTEND_HOST || '0.0.0.0';
const port = parsePort(args.port || process.env.FRONTEND_PORT, 18002);
const strictPort = args.strictPort || parseBoolean(process.env.FRONTEND_STRICT_PORT, false);
const proxyTarget = process.env.VITE_DEV_PROXY_TARGET || 'http://127.0.0.1:18000';
const forceOptimizeDeps = parseBoolean(process.env.FRONTEND_VITE_FORCE_OPTIMIZE, true);

let viteServer;

const shutdown = async (signal) => {
  if (!viteServer) {
    process.exit(0);
    return;
  }

  console.log(`[frontend] received ${signal}, shutting down vite dev server...`);
  await viteServer.close();
  process.exit(0);
};

process.on('SIGINT', () => {
  void shutdown('SIGINT');
});

process.on('SIGTERM', () => {
  void shutdown('SIGTERM');
});

const main = async () => {
  // Keep readiness output explicit because Vite can stay silent under Docker/QEMU.
  viteServer = await createServer({
    configFile: path.resolve(__dirname, '..', 'vite.config.ts'),
    optimizeDeps: {
      force: forceOptimizeDeps
    },
    server: {
      host,
      port,
      strictPort
    }
  });

  await viteServer.listen();

  const localUrl = viteServer.resolvedUrls?.local?.[0] || formatFallbackUrl(host, port);
  const networkUrl = viteServer.resolvedUrls?.network?.[0];

  console.log(`[frontend] vite ready: ${localUrl}`);
  if (networkUrl && networkUrl !== localUrl) {
    console.log(`[frontend] vite network: ${networkUrl}`);
  }
  console.log(`[frontend] proxy target: ${proxyTarget}`);
  console.log(`[frontend] optimize deps force: ${forceOptimizeDeps ? 'on' : 'off'}`);
};

main().catch((error) => {
  console.error('[frontend] failed to start vite dev server');
  console.error(error);
  process.exit(1);
});
