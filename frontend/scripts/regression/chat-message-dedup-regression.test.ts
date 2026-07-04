import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const frontendRoot = resolve(process.cwd());
const removedModulePath = resolve(frontendRoot, 'src/stores/chatMessageDedup.ts');

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8');

test('legacy assistant dedupe patch module is removed', () => {
  assert.equal(existsSync(removedModulePath), false);
});

test('chat hydration and cache paths no longer use assistant content dedupe', () => {
  [
    'src/stores/chatRuntimeState.ts',
    'src/stores/chatSessionOpenLoadActions.ts',
    'src/stores/chatCacheActions.ts'
  ].forEach((relativePath) => {
    const source = readSource(relativePath);
    assert.equal(source.includes("from './chatMessageDedup'"), false, relativePath);
    assert.equal(source.includes('dedupeAssistantMessages'), false, relativePath);
    assert.equal(source.includes('dedupeAssistantMessagesInPlace'), false, relativePath);
  });
});
