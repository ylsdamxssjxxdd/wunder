import { spawn } from 'node:child_process';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '..', '..');
const frontendRoot = path.resolve(__dirname, '..');
const distRoot = path.join(frontendRoot, 'dist');
const tempDistRoot = path.join(frontendRoot, 'dist.__docker_tmp');
const viteCacheRoots = [
  path.join(repoRoot, 'node_modules', '.vite'),
  path.join(frontendRoot, 'node_modules', '.vite')
];

const log = (message) => {
  console.log(`[frontend][docker] ${message}`);
};

const hasPath = async (targetPath) => {
  try {
    await fs.access(targetPath);
    return true;
  } catch (_error) {
    return false;
  }
};

const run = (command, args, options = {}) =>
  new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      stdio: 'inherit',
      ...options
    });

    child.on('error', reject);
    child.on('exit', (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(
        new Error(
          `${command} ${args.join(' ')} failed with ${signal ? `signal ${signal}` : `exit code ${code}`}`
        )
      );
    });
  });

const ensureDependencies = async () => {
  const viteBins = [
    path.join(repoRoot, 'node_modules', '.bin', 'vite'),
    path.join(frontendRoot, 'node_modules', '.bin', 'vite')
  ];
  const viteReady = (await Promise.all(viteBins.map((candidate) => hasPath(candidate)))).some(Boolean);
  if (viteReady) {
    return;
  }

  log('installing workspace dependencies');
  await run('npm', ['ci', '--workspace', 'wunder-frontend', '--include-workspace-root=false'], {
    cwd: repoRoot
  });
};

const clearCaches = async () => {
  await Promise.all(viteCacheRoots.map((targetPath) => fs.rm(targetPath, { recursive: true, force: true })));
  await fs.rm(tempDistRoot, { recursive: true, force: true });
};

const buildTempDist = async () => {
  log('building static assets into temporary dist');
  await run(
    'npm',
    ['run', 'build', '--workspace', 'wunder-frontend', '--', '--outDir', 'dist.__docker_tmp'],
    {
      cwd: repoRoot
    }
  );
};

const copyTree = async (sourceRoot, targetRoot, { skipIndex = false } = {}) => {
  const entries = await fs.readdir(sourceRoot, { withFileTypes: true });

  await fs.mkdir(targetRoot, { recursive: true });

  for (const entry of entries) {
    if (skipIndex && entry.name === 'index.html') {
      continue;
    }

    const sourcePath = path.join(sourceRoot, entry.name);
    const targetPath = path.join(targetRoot, entry.name);

    if (entry.isDirectory()) {
      const stat = await fs.lstat(targetPath).catch(() => null);
      if (stat && !stat.isDirectory()) {
        await fs.rm(targetPath, { recursive: true, force: true });
      }
      await copyTree(sourcePath, targetPath);
      continue;
    }

    const stat = await fs.lstat(targetPath).catch(() => null);
    if (stat?.isDirectory()) {
      await fs.rm(targetPath, { recursive: true, force: true });
    }
    await fs.copyFile(sourcePath, targetPath);
  }
};

const removeStaleEntries = async (sourceRoot, targetRoot) => {
  const sourceEntries = new Map(
    (await fs.readdir(sourceRoot, { withFileTypes: true })).map((entry) => [entry.name, entry])
  );
  const targetEntries = await fs.readdir(targetRoot, { withFileTypes: true }).catch(() => []);

  for (const entry of targetEntries) {
    const sourceEntry = sourceEntries.get(entry.name);
    const targetPath = path.join(targetRoot, entry.name);

    if (!sourceEntry) {
      await fs.rm(targetPath, { recursive: true, force: true });
      continue;
    }

    if (entry.isDirectory() && sourceEntry.isDirectory()) {
      await removeStaleEntries(path.join(sourceRoot, entry.name), targetPath);
      continue;
    }

    if (entry.isDirectory() !== sourceEntry.isDirectory()) {
      await fs.rm(targetPath, { recursive: true, force: true });
    }
  }
};

const syncDist = async () => {
  if (!(await hasPath(tempDistRoot))) {
    throw new Error(`temporary dist is missing: ${tempDistRoot}`);
  }

  await fs.mkdir(distRoot, { recursive: true });

  // Copy hashed assets first, then switch index.html last so nginx never serves
  // an entry page that references files not copied yet.
  await copyTree(tempDistRoot, distRoot, { skipIndex: true });

  const tempIndexPath = path.join(tempDistRoot, 'index.html');
  if (await hasPath(tempIndexPath)) {
    await fs.copyFile(tempIndexPath, path.join(distRoot, 'index.html'));
  }

  await removeStaleEntries(tempDistRoot, distRoot);
  await fs.rm(tempDistRoot, { recursive: true, force: true });
  log('static dist is ready for nginx');
};

const waitBackend = async () => {
  log('waiting for backend health endpoint');
  await run('node', ['./frontend/scripts/wait-backend.mjs'], {
    cwd: repoRoot
  });
};

const startDevServer = async () => {
  log('starting vite dev server');
  await run('npm', ['run', 'dev', '--workspace', 'wunder-frontend'], {
    cwd: repoRoot
  });
};

const main = async () => {
  await ensureDependencies();
  await clearCaches();
  await buildTempDist();
  await syncDist();
  await waitBackend();
  await startDevServer();
};

main().catch((error) => {
  console.error('[frontend][docker] bootstrap failed');
  console.error(error);
  process.exit(1);
});
