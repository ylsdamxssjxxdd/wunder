import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  collectWorkspaceRefreshTargets,
  hasLoadedWorkspaceDirectoryChildren,
  preserveWorkspaceExpandedChildren,
  shouldAcceptWorkspaceTreeVersion,
  shouldWorkspacePreviewReload,
  type WorkspaceRefreshEntryLike
} from '../../src/components/chat/workspacePanelRefreshPlanner';

const frontendRoot = resolve(process.cwd());

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8').replace(/\r\n/g, '\n');

const ensureBrowserRuntimeStub = (): void => {
  const root = globalThis as typeof globalThis & {
    window?: { location?: { origin?: string } };
    localStorage?: {
      getItem: (key: string) => string | null;
      setItem: (key: string, value: string) => void;
      removeItem: (key: string) => void;
    };
  };
  if (!root.window) {
    root.window = { location: { origin: 'http://localhost' } };
  } else if (!root.window.location) {
    root.window.location = { origin: 'http://localhost' };
  }
  if (!root.window.location.origin) {
    root.window.location.origin = 'http://localhost';
  }
  if (!root.localStorage) {
    const values = new Map<string, string>();
    root.localStorage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value);
      },
      removeItem: (key: string) => {
        values.delete(key);
      }
    };
  }
};

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

test('workspace background refresh stays interaction-preserving while directories expand', () => {
  const source = readSource('src/components/chat/WorkspacePanel.vue');
  const loadStart = source.indexOf('const loadWorkspace = async');
  assert.ok(loadStart >= 0);
  const loadSearchStart = source.indexOf('const loadWorkspaceSearch = async', loadStart);
  assert.ok(loadSearchStart > loadStart);
  const loadSource = source.slice(loadStart, loadSearchStart);

  assert.ok(loadSource.includes('if (!preserveInteraction) {\n    state.loading = true;'));
  assert.ok(loadSource.includes('if (!preserveInteraction) {\n      state.loading = false;'));
  assert.ok(source.includes('const workspaceDirectoryLoadingPaths = new Set<string>();'));
  const toggleStart = source.indexOf('const toggleWorkspaceDirectory = async');
  assert.ok(toggleStart >= 0);
  const toggleEnd = source.indexOf('const refreshWorkspace = async', toggleStart);
  assert.ok(toggleEnd > toggleStart);
  const toggleSource = source.slice(toggleStart, toggleEnd);
  assert.ok(toggleSource.indexOf('if (state.expanded.has(entryPath))') < toggleSource.indexOf('if (workspaceDirectoryLoadingPaths.has(entryPath)) return;'));
  assert.ok(source.includes('workspaceDirectoryLoadingPaths.add(entryPath);'));
  assert.ok(source.includes('workspaceDirectoryLoadingPaths.delete(entryPath);'));
  assert.ok(source.includes('if (state.loading || workspaceDirectoryLoadingPaths.size > 0)'));
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

test('workspace refresh paths normalize public generated resource aliases', async () => {
  ensureBrowserRuntimeStub();
  const {
    extractWorkspaceRefreshPaths,
    isWorkspacePathAffected
  } = await import('../../src/utils/workspaceRefresh');
  const paths = extractWorkspaceRefreshPaths({
    public_path: '/workspaces/user__c__2/images/output.png',
    data: {
      outputPath: 'reports/final.pdf',
      workspace_relative_path: 'images/output.png'
    },
    meta: {
      savedPath: 'audio/result.mp3'
    }
  });

  assert.deepEqual(paths.sort(), [
    'audio/result.mp3',
    'images/output.png',
    'reports/final.pdf'
  ]);
  assert.equal(isWorkspacePathAffected('images/output.png', paths), true);
});
