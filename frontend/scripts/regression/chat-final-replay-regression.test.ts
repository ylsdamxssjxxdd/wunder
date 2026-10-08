import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const frontendRoot = resolve(process.cwd());
const removedFinalMergePath = resolve(frontendRoot, 'src/stores/chatFinalTranscriptMerge.ts');

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8');

test('legacy final transcript merge patch module is removed', () => {
  assert.equal(existsSync(removedFinalMergePath), false);
});

test('session open no longer merges adjacent assistant transcript bubbles by content', () => {
  const source = readSource('src/stores/chatSessionOpenLoadActions.ts');
  assert.equal(source.includes("from './chatFinalTranscriptMerge'"), false);
  assert.equal(source.includes('mergeFinalTranscriptAssistantDuplicates'), false);
});
