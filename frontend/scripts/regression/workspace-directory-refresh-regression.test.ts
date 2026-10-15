import './mocks/browserStorage';
import test from 'node:test';
import assert from 'node:assert/strict';
import { useWorkspaceFileTree } from '../../src/views/messenger/workspace/workspaceFileTree';
import { createWorkspaceDirectoryRefresh } from '../../src/views/messenger/workspace/workspaceDirectoryRefresh';
import type { WorkspaceDirectoryPage } from '../../src/views/messenger/workspace/workspaceFileApi';
import type { WorkspaceEntry } from '../../src/views/messenger/workspace/workspaceFileModel';

const entry = (path: string, kind: 'dir' | 'file' = 'file'): WorkspaceEntry => ({
  path, name: path.split('/').at(-1)!, kind, size: 0, updatedTime: ''
});
const page = (path: string, entries: WorkspaceEntry[], version = 1): WorkspaceDirectoryPage => ({
  path, entries, total: entries.length, offset: 0, limit: 100, workspaceId: 'fixture-workspace', treeVersion: version
});
const deferred = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
};

test('forced lifecycle refresh bypasses the unchanged version and updates visible expanded directories', async () => {
  const calls: { path: string; refreshTree?: boolean }[] = [];
  let scan = 0;
  const tree = useWorkspaceFileTree({ loadPage: async (path, options) => {
    calls.push({ path, refreshTree: options.refreshTree });
    if (options.refreshTree) scan++;
    return page(path, path === '' ? [entry('reports', 'dir')]
      : path === 'reports' ? [entry('reports/nested', 'dir'), entry('reports/result.txt')]
      : [entry('reports/nested/result.txt')], 7);
  } });
  tree.patchDirectoryEntries('', [entry('reports', 'dir')]);
  tree.patchDirectoryEntries('reports', [entry('reports/nested', 'dir')]);
  tree.patchDirectoryEntries('reports/nested', []);
  tree.expandedPaths.add('reports'); tree.expandedPaths.add('reports/nested');
  tree.patchDirectoryEntries('hidden', []); tree.expandedPaths.add('hidden');
  tree.workspaceId.value = 'fixture-workspace'; tree.treeVersion.value = 7;
  let stats = 0;
  const refresh = createWorkspaceDirectoryRefresh(tree, { refreshStats: async () => { stats++; } });
  await refresh.handleEvent({ workspaceId: 'other-workspace', forceTree: true });
  await refresh.handleEvent({ treeVersion: 7 });
  assert.equal(calls.length, 0);
  await refresh.handleEvent({ treeVersion: 7, forceTree: true });
  assert.deepEqual(calls.map(call => call.path), ['', 'reports', 'reports/nested']);
  assert.equal(scan, 1);
  assert.equal(stats, 1);
  assert.ok(tree.rows.value.some(row => row.path === 'reports/nested/result.txt'));
  assert.equal(tree.isExpanded('reports'), true);
  refresh.dispose();
});

test('events arriving during a refresh are merged into one trailing forced scan', async () => {
  const started = deferred<void>(); const release = deferred<void>();
  let scans = 0;
  const tree = useWorkspaceFileTree({ loadPage: async (path, options) => {
    if (options.refreshTree) scans++;
    if (scans === 1) { started.resolve(); await release.promise; }
    return page(path, [entry(scans === 1 ? 'before.txt' : 'after.txt')], scans);
  } });
  const busy: boolean[] = [];
  const refresh = createWorkspaceDirectoryRefresh(tree, {
    refreshStats: async () => {}, onBusy: value => busy.push(value)
  });
  const first = refresh.request();
  await started.promise;
  for (let index = 0; index < 30; index++) void refresh.request();
  release.resolve();
  await first;
  assert.equal(scans, 2);
  assert.equal(tree.rows.value[0].path, 'after.txt');
  assert.deepEqual(busy, [true, false]);
  refresh.dispose();
});

test('targeted path events merge without dropping directories and rebuild only one snapshot', async () => {
  const calls: { path: string; refreshTree?: boolean }[] = [];
  const tree = useWorkspaceFileTree({ loadPage: async (path, options) => {
    calls.push({ path, refreshTree: options.refreshTree });
    return page(path, [entry(`${path}/result.txt`)]);
  } });
  tree.patchDirectoryEntries('reports', []); tree.patchDirectoryEntries('images', []);
  const refresh = createWorkspaceDirectoryRefresh(tree, { refreshStats: async () => {} });
  await Promise.all([
    refresh.handleEvent({ paths: ['workspace/reports/result.txt'] }),
    refresh.handleEvent({ paths: ['images/result.txt'] })
  ]);
  assert.deepEqual(calls.map(call => call.path), ['reports', 'images']);
  assert.equal(calls.filter(call => call.refreshTree).length, 1);
  refresh.dispose();
});

test('an invalidation at the refresh completion boundary starts a new scan', async () => {
  let scans = 0;
  let trailing: Promise<void> | undefined;
  const tree = useWorkspaceFileTree({ loadPage: async path => {
    scans++;
    return page(path, [entry(`result-${scans}.txt`)], scans);
  } });
  const refresh = createWorkspaceDirectoryRefresh(tree, { refreshStats: async () => {
    if (scans === 1) queueMicrotask(() => queueMicrotask(() => { trailing = refresh.request(); }));
  } });
  await refresh.request();
  await trailing;
  assert.equal(scans, 2);
  assert.equal(tree.rows.value[0].path, 'result-2.txt');
  refresh.dispose();
});

test('old directory reads cannot overwrite a forced replacement or a reset tree', async () => {
  const stale = deferred<WorkspaceDirectoryPage>();
  let reads = 0;
  const tree = useWorkspaceFileTree({ loadPage: async () => {
    reads++;
    return reads === 1 ? stale.promise : page('', [entry('fresh.txt')], 8);
  } });
  const first = tree.loadDirectory('');
  await tree.refreshDirectory('', { refreshTree: true });
  stale.resolve(page('', [entry('stale.txt')], 2));
  await first;
  assert.equal(tree.rows.value[0].path, 'fresh.txt');
  assert.equal(tree.treeVersion.value, 8);

  const delayed = deferred<WorkspaceDirectoryPage>();
  const resetTree = useWorkspaceFileTree({ loadPage: () => delayed.promise });
  const loading = resetTree.loadDirectory('');
  resetTree.reset();
  delayed.resolve(page('', [entry('old-target.txt')], 3));
  await loading;
  assert.equal(resetTree.workspaceId.value, '');
  assert.deepEqual(resetTree.rows.value, []);
});

test('disposing a refresh stops follow-up directory reads', async () => {
  const started = deferred<void>(); const release = deferred<void>();
  const paths: string[] = [];
  const tree = useWorkspaceFileTree({ loadPage: async path => {
    paths.push(path); started.resolve(); await release.promise;
    return page(path, [entry('reports', 'dir')]);
  } });
  tree.expandedPaths.add('reports');
  const refresh = createWorkspaceDirectoryRefresh(tree, { refreshStats: async () => assert.fail('disposed stats') });
  const first = refresh.request();
  await started.promise;
  void refresh.request();
  refresh.dispose(); release.resolve(); await first;
  assert.deepEqual(paths, ['']);
});
