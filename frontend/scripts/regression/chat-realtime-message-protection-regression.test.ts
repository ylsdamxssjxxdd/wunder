import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const frontendRoot = resolve(process.cwd());
const removedModulePath = resolve(frontendRoot, 'src/stores/chatRealtimeMessageProtection.ts');

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8');

test('legacy realtime protected-message patch module is removed', () => {
  assert.equal(existsSync(removedModulePath), false);
});

test('session hydration no longer reinserts protected realtime messages into legacy transcript', () => {
  [
    'src/stores/chatRuntimeState.ts',
    'src/stores/chatSessionOpenLoadActions.ts',
    'src/stores/chatCacheActions.ts'
  ].forEach((relativePath) => {
    const source = readSource(relativePath);
    assert.equal(source.includes("from './chatRealtimeMessageProtection'"), false, relativePath);
    assert.equal(source.includes('mergeProtectedRealtimeMessages'), false, relativePath);
    assert.equal(source.includes('upsertProtectedRealtimeMessage'), false, relativePath);
    assert.equal(source.includes('mergeSessionProtectedRealtimeMessages'), false, relativePath);
  });
});
