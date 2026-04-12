import test from 'node:test';
import assert from 'node:assert/strict';

import {
  collectWorkspaceRefreshTargets,
  shouldAcceptWorkspaceTreeVersion,
  shouldWorkspacePreviewReload,
  type WorkspaceRefreshEntryLike
} from '../../src/components/chat/workspacePanelRefreshPlanner';

test('workspace refresh targets collapse file updates to the visible parent directory', () => {
  const entries: WorkspaceRefreshEntryLike[] = [
    {
      path: 'docs',
      type: 'dir',
      childrenLoaded: true,
      children: [
        {
          path: 'docs/report.md',
          type: 'file',
          name: 'report.md'
        }
      ]
    }
  ];
  assert.deepEqual(
    collectWorkspaceRefreshTargets({
      currentPath: 'docs',
      changedPaths: ['docs/report.md'],
      entries,
      maxTargets: 6
    }),
    {
      targets: ['docs'],
      forceFullReload: false
    }
  );
});

test('workspace refresh planner falls back to full reload when targets fan out too much', () => {
  const entries: WorkspaceRefreshEntryLike[] = [
    { path: 'a', type: 'dir' },
    { path: 'b', type: 'dir' },
    { path: 'c', type: 'dir' }
  ];
  assert.deepEqual(
    collectWorkspaceRefreshTargets({
      currentPath: '',
      changedPaths: ['a/file.txt', 'b/file.txt', 'c/file.txt'],
      entries,
      maxTargets: 2
    }),
    {
      targets: [],
      forceFullReload: true
    }
  );
});

test('workspace preview reload detection treats empty path hints as affected', () => {
  assert.equal(shouldWorkspacePreviewReload('docs/report.md', []), true);
  assert.equal(
    shouldWorkspacePreviewReload('docs/report.md', ['docs/report.md']),
    true
  );
  assert.equal(
    shouldWorkspacePreviewReload('docs/report.md', ['docs']),
    true
  );
  assert.equal(
    shouldWorkspacePreviewReload('docs/report.md', ['images/logo.png']),
    false
  );
});

test('workspace tree version gating only skips versions that are already applied', () => {
  assert.equal(shouldAcceptWorkspaceTreeVersion(null, 5), true);
  assert.equal(shouldAcceptWorkspaceTreeVersion(5, 5), false);
  assert.equal(shouldAcceptWorkspaceTreeVersion(4, 5), false);
  assert.equal(shouldAcceptWorkspaceTreeVersion(6, 5), true);
});
