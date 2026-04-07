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

const parseBoolean = (value) => {
  if (value == null || value === '') {
    return false;
  }
  return ['1', 'true', 'yes', 'on'].includes(String(value).trim().toLowerCase());
};

const skipDistBuild = parseBoolean(process.env.FRONTEND_SKIP_DIST_BUILD);

const hasPath = async (targetPath) => {
  try {
    await fs.access(targetPath);
    return true;
  } catch (_error) {
    return false;
  }
};

const findFirstExistingPath = async (candidates) => {
  for (const candidate of candidates) {
    if (await hasPath(candidate)) {
      return candidate;
    }
  }
  return '';
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

const ensureEmptyDirectory = async (targetPath) => {
  const stat = await fs.lstat(targetPath).catch(() => null);
  if (stat && !stat.isDirectory()) {
    await fs.rm(targetPath, { recursive: true, force: true });
  }

  // Preserve the directory root because Docker named-volume mount points can
  // fail with EBUSY when the script tries to remove the mount itself.
  await fs.mkdir(targetPath, { recursive: true });
  const entries = await fs.readdir(targetPath, { withFileTypes: true });
  await Promise.all(
    entries.map((entry) =>
      fs.rm(path.join(targetPath, entry.name), { recursive: true, force: true })
    )
  );
};

const resolveLinuxRollupNativePackageName = () => {
  if (process.platform !== 'linux') return '';
  const isMusl = !process.report?.getReport?.()?.header?.glibcVersionRuntime;
  if (process.arch === 'x64') {
    return isMusl ? '@rollup/rollup-linux-x64-musl' : '@rollup/rollup-linux-x64-gnu';
  }
  if (process.arch === 'arm64') {
    return isMusl ? '@rollup/rollup-linux-arm64-musl' : '@rollup/rollup-linux-arm64-gnu';
  }
  if (process.arch === 'arm') {
    return '@rollup/rollup-linux-arm-gnueabihf';
  }
  return '';
};

const resolveLinuxEsbuildNativePackageName = () => {
  if (process.platform !== 'linux') return '';
  if (process.arch === 'x64') {
    return '@esbuild/linux-x64';
  }
  if (process.arch === 'arm64') {
    return '@esbuild/linux-arm64';
  }
  if (process.arch === 'arm') {
    return '@esbuild/linux-arm';
  }
  return '';
};

const hasLinuxNativeDependency = async (packageName, markerRelativePaths = ['package.json']) => {
  if (!packageName) {
    return true;
  }
  const packageSegments = packageName.split('/');
  const candidates = [];
  for (const root of [repoRoot, frontendRoot]) {
    for (const markerRelativePath of markerRelativePaths) {
      candidates.push(
        path.join(root, 'node_modules', ...packageSegments, ...markerRelativePath.split('/'))
      );
    }
  }
  return Boolean(await findFirstExistingPath(candidates));
};

const hasLinuxRollupNativeDependency = async () =>
  hasLinuxNativeDependency(resolveLinuxRollupNativePackageName(), [
    'package.json',
    `rollup.${process.platform}-${process.arch}${process.arch === 'arm64' ? '-gnu' : ''}.node`
  ]);

const resolveViteEntry = async () =>
  findFirstExistingPath([
    path.join(repoRoot, 'node_modules', 'vite', 'bin', 'vite.js'),
    path.join(frontendRoot, 'node_modules', 'vite', 'bin', 'vite.js')
  ]);

const ensureDependencies = async () => {
  const viteEntry = await resolveViteEntry();
  const viteReady = Boolean(viteEntry);
  const rollupNativeReady = await hasLinuxRollupNativeDependency();
  const esbuildNativeReady = await hasLinuxNativeDependency(
    resolveLinuxEsbuildNativePackageName(),
    ['package.json', 'bin/esbuild']
  );
  if (viteReady && rollupNativeReady && esbuildNativeReady) {
    return viteEntry;
  }

  if (!viteReady) {
    log('vite package is missing, reinstalling workspace dependencies');
  } else {
    log('linux native frontend dependency is missing, reinstalling workspace dependencies');
  }
  try {
    await run(
      'npm',
      [
        'ci',
        '--workspace',
        'wunder-frontend',
        '--include-workspace-root=false',
        '--include=optional'
      ],
      {
        cwd: repoRoot
      }
    );
  } catch (error) {
    throw new Error(
      `workspace dependency reinstall failed. The mounted node_modules likely came from another OS/arch and this container needs network access to refresh them. ${error.message}`
    );
  }
  const resolvedViteEntry = await resolveViteEntry();
  if (!resolvedViteEntry) {
    throw new Error('vite package is unavailable after npm ci; check workspace dependency installation');
  }
  if (!(await hasLinuxRollupNativeDependency())) {
    throw new Error(
      'rollup native dependency is unavailable after npm ci; check npm optional dependency settings'
    );
  }
  if (!(await hasLinuxNativeDependency(resolveLinuxEsbuildNativePackageName()))) {
    throw new Error(
      'esbuild native dependency is unavailable after npm ci; check npm optional dependency settings'
    );
  }
  return resolvedViteEntry;
};

const clearCaches = async () => {
  await Promise.all(viteCacheRoots.map((targetPath) => fs.rm(targetPath, { recursive: true, force: true })));
  await ensureEmptyDirectory(tempDistRoot);
};

const buildTempDist = async (viteEntry) => {
  log('building static assets into temporary dist');
  // Call Vite directly so cross-platform npm ci does not depend on `.bin` shims.
  await run(process.execPath, [viteEntry, 'build', '--outDir', 'dist.__docker_tmp'], {
    cwd: frontendRoot
  });
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
  await ensureEmptyDirectory(tempDistRoot);
  log('static dist is ready for nginx');
};

const waitBackend = async () => {
  log('waiting for backend health endpoint');
  await run('node', ['./frontend/scripts/wait-backend.mjs'], {
    cwd: repoRoot
  });
};

const ensurePrebuiltDist = async () => {
  const distIndexPath = path.join(distRoot, 'index.html');
  if (!(await hasPath(distIndexPath))) {
    throw new Error(
      `FRONTEND_SKIP_DIST_BUILD requires an existing dist/index.html at ${distIndexPath}`
    );
  }
  log('reusing existing frontend/dist without running vite build');
};

const startDevServer = async () => {
  log('starting vite dev server');
  await run(process.execPath, [path.join(frontendRoot, 'scripts', 'dev-server.mjs')], {
    cwd: frontendRoot
  });
};

const keepContainerAlive = async (reason) => {
  log(reason);
  await new Promise(() => {});
};

const main = async () => {
  let viteEntry = '';
  try {
    viteEntry = await ensureDependencies();
  } catch (error) {
    if (!parseBoolean(process.env.FRONTEND_ALLOW_PREBUILT_DIST)) {
      throw error;
    }
    if (!(await hasPath(path.join(distRoot, 'index.html')))) {
      throw error;
    }
    console.warn(
      `[frontend][docker] dependency bootstrap failed, reusing existing dist without vite dev server: ${error.message}`
    );
    await keepContainerAlive('reusing existing frontend/dist for nginx static serving');
    return;
  }
  if (skipDistBuild) {
    await ensurePrebuiltDist();
  } else {
    await clearCaches();
    await buildTempDist(viteEntry);
    await syncDist();
  }
  await waitBackend();
  await startDevServer();
};

main().catch((error) => {
  console.error('[frontend][docker] bootstrap failed');
  console.error(error);
  process.exit(1);
});
