import { spawnSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { basename, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const tests = process.argv.slice(2).map((value) => String(value || '').trim()).filter(Boolean);
if (!tests.length) {
  process.stderr.write('At least one regression test file is required.\n');
  process.exit(2);
}

const frontendRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const workspaceRoot = resolve(frontendRoot, '..');
const esbuild = resolve(workspaceRoot, 'node_modules', 'esbuild', 'bin', 'esbuild');
const outputDir = resolve(workspaceRoot, 'temp_dir', 'frontend-tests');
const agentAvatarCatalogMock = resolve(frontendRoot, 'scripts', 'regression', 'mocks', 'agentAvatarCatalog.ts');
mkdirSync(outputDir, { recursive: true });

for (const testFile of tests) {
  const source = resolve(frontendRoot, 'scripts', 'regression', testFile);
  const output = resolve(outputDir, `${basename(testFile, '.ts')}.cjs`);
  const build = spawnSync(
    process.execPath,
    [
      esbuild,
      source,
      '--bundle',
      '--platform=node',
      '--format=cjs',
      '--alias:@=./src',
      `--alias:@/utils/agentAvatarCatalog=${agentAvatarCatalogMock}`,
      `--outfile=${output}`
    ],
    { cwd: frontendRoot, stdio: 'inherit' }
  );
  if (build.status !== 0) process.exit(build.status ?? 1);

  const run = spawnSync(process.execPath, [output], { cwd: frontendRoot, stdio: 'inherit' });
  if (run.status !== 0) process.exit(run.status ?? 1);
}
