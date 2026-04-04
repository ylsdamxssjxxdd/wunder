import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildStructuredToolResultNote,
  buildStructuredToolResultView
} from '../../src/components/chat/toolWorkflowStructuredView';

const messages: Record<string, string> = {
  'chat.toolWorkflow.detail.hits': 'Hits',
  'chat.toolWorkflow.detail.scannedFiles': 'Scanned files'
};

const t = (key: string): string => messages[key] || key;

test('search structured view keeps local-only guidance when there are zero hits', () => {
  const data = {
    returned_match_count: 0,
    scanned_files: 3,
    scope_note: 'Searches local workspace files only.',
    summary: {
      next_hint: 'Use list_files first.'
    }
  };
  const view = buildStructuredToolResultView('search_content', null, data, t);
  assert.ok(view);
  assert.equal(view?.variant, 'search');
  assert.deepEqual(
    view?.metrics.map((item) => [item.key, item.value]),
    [
      ['hits', '0'],
      ['scanned', '3']
    ]
  );
  const rowTitles = view?.groups.flatMap((group) => group.rows.map((row) => row.title)) || [];
  assert.ok(rowTitles.includes('Searches local workspace files only.'));
  assert.ok(rowTitles.includes('Use list_files first.'));
  assert.equal(
    buildStructuredToolResultNote('search_content', null, data, t),
    'Scanned files 3'
  );
});
