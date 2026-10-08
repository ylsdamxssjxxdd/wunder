import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const frontendRoot = resolve(process.cwd());
const removedModulePath = resolve(frontendRoot, 'src/stores/chatAssistantMatch.ts');

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8');

test('legacy assistant content-anchor match module is removed', () => {
  assert.equal(existsSync(removedModulePath), false);
});

test('snapshot and foreground hydration no longer use assistant content-anchor matching', () => {
  [
    'src/stores/chatSnapshot.ts',
    'src/stores/chatRuntimeState.ts',
    'src/stores/chatSessionOpenLoadActions.ts'
  ].forEach((relativePath) => {
    const source = readSource(relativePath);
    assert.equal(source.includes("from './chatAssistantMatch'"), false, relativePath);
    assert.equal(source.includes('findAnchoredAssistantContentMatchIndex'), false, relativePath);
    assert.equal(source.includes('buildAssistantMatchEntries'), false, relativePath);
    assert.equal(source.includes('buildAssistantMatchEntryMap'), false, relativePath);
  });
});
