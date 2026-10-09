// AI生成
import { spawnSync } from 'node:child_process';
import { closeSync, mkdirSync, openSync, readSync } from 'node:fs';
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

// esbuild's postinstall step replaces `bin/esbuild` with a hardlink to the
// platform binary on non-Windows platforms, so that path can be an ELF/Mach-O
// executable rather than the JavaScript shim. Running such a binary through
// `node` makes Node try to parse it as JavaScript and fail with
// "SyntaxError: Invalid or unexpected token". Detect the shebang and only wrap
// with `node` when the entry is actually a script (which is what Windows keeps).
function isNodeScript(file) {
  const fd = openSync(file, 'r');
  try {
    const head = Buffer.alloc(2);
    const read = readSync(fd, head, 0, 2, 0);
    return read === 2 && head[0] === 0x23 && head[1] === 0x21; // "#!"
  } finally {
    closeSync(fd);
  }
}

for (const testFile of tests) {
  const source = resolve(frontendRoot, 'scripts', 'regression', testFile);
  const output = resolve(outputDir, `${basename(testFile, '.ts')}.cjs`);
  const esbuildArgs = [
    source,
    '--bundle',
    '--platform=node',
    '--format=cjs',
    '--define:import.meta.env={}',
    '--alias:@=./src',
    `--alias:@/utils/agentAvatarCatalog=${agentAvatarCatalogMock}`,
    `--outfile=${output}`
  ];
  const esbuildIsScript = isNodeScript(esbuild);
  const build = spawnSync(
    esbuildIsScript ? process.execPath : esbuild,
    esbuildIsScript ? [esbuild, ...esbuildArgs] : esbuildArgs,
    { cwd: frontendRoot, stdio: 'inherit' }
  );
  if (build.status !== 0) process.exit(build.status ?? 1);

  const run = spawnSync(process.execPath, [output], { cwd: frontendRoot, stdio: 'inherit', timeout: 60_000 });
  if (run.error) process.stderr.write(`Regression process failed (${run.error.code ?? 'unknown'}): ${testFile}\n`);
  if (run.status !== 0) process.exit(run.status ?? 1);
}