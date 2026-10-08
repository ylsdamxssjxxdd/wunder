import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const frontendRoot = resolve(scriptDir, '..');
const candidates = [
  resolve(frontendRoot, 'node_modules/vue-tsc/bin/vue-tsc.js'),
  resolve(frontendRoot, '../node_modules/vue-tsc/bin/vue-tsc.js')
];

const vueTscBin = candidates.find((candidate) => existsSync(candidate));

if (!vueTscBin) {
  console.error('vue-tsc is not installed in the frontend workspace or repository root.');
  process.exit(1);
}

const result = spawnSync(process.execPath, [vueTscBin, ...process.argv.slice(2)], {
  cwd: frontendRoot,
  stdio: 'inherit'
});

process.exit(result.status ?? 1);
