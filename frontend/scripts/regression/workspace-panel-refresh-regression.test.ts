import test from 'node:test';
import assert from 'node:assert/strict';

import {
  collectWorkspaceRefreshTargets,
  hasLoadedWorkspaceDirectoryChildren,
  preserveWorkspaceExpandedChildren,
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

test('workspace background refresh preserves stale children for expanded directories', () => {
  const previousEntries: WorkspaceRefreshEntryLike[] = [
    {
      path: 'docs',
      type: 'dir',
      childrenLoaded: true,
      children: [
        {
          path: 'docs/report.md',
          type: 'file',
          name: 'report.md'
        },
        {
          path: 'docs/nested',
          type: 'dir',
          childrenLoaded: true,
          children: [
            {
              path: 'docs/nested/item.txt',
              type: 'file',
              name: 'item.txt'
            }
          ]
        }
      ]
    }
  ];
  const nextEntries: WorkspaceRefreshEntryLike[] = [
    {
      path: 'docs',
      type: 'dir',
      childrenLoaded: false,
      children: []
    }
  ];

  const merged = preserveWorkspaceExpandedChildren({
    nextEntries,
    previousEntries,
    expandedPaths: ['docs', 'docs/nested']
  });

  assert.equal(merged[0].childrenLoaded, false);
  assert.deepEqual(
    merged[0].children?.map((entry) => entry.path),
    ['docs/report.md', 'docs/nested']
  );
  const nested = merged[0].children?.[1];
  assert.equal(nested?.childrenLoaded, false);
  assert.deepEqual(
    nested?.children?.map((entry) => entry.path),
    ['docs/nested/item.txt']
  );
  assert.notEqual(merged[0].children, previousEntries[0].children);

  const emptyMerged = preserveWorkspaceExpandedChildren({
    nextEntries: [{ path: 'empty', type: 'dir', childrenLoaded: false }],
    previousEntries: [{ path: 'empty', type: 'dir', childrenLoaded: true, children: [] }],
    expandedPaths: ['empty']
  });
  assert.equal(emptyMerged[0].childrenLoaded, false);
  assert.deepEqual(emptyMerged[0].children, []);
});

test('workspace loaded directory check requires a real children array', () => {
  assert.equal(
    hasLoadedWorkspaceDirectoryChildren({
      path: 'dir',
      type: 'dir',
      childrenLoaded: true
    }),
    false
  );
  assert.equal(
    hasLoadedWorkspaceDirectoryChildren({
      path: 'dir',
      type: 'dir',
      childrenLoaded: true,
      children: []
    }),
    true
  );
  assert.equal(
    hasLoadedWorkspaceDirectoryChildren({
      path: 'dir',
      type: 'dir',
      childrenLoaded: false,
      children: [{ path: 'dir/file.txt', type: 'file' }]
    }),
    false
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
