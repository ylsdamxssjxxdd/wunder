import type { WorkspaceFileTree } from './workspaceFileTree';
import { extractWorkspaceRefreshPaths } from '@/utils/workspaceRefresh';
import { normalizeWorkspaceRelativePath, workspaceParentPath } from './workspaceFileModel';

/** Coalesce invalidations and retain a trailing refresh when writes race a read. */
export const createWorkspaceDirectoryRefresh = (
  tree: WorkspaceFileTree,
  options: { refreshStats: () => Promise<void>; onBusy?: (busy: boolean) => void }
) => {
  const pendingPaths = new Set<string>();
  let full = false;
  let forceTree = false;
  let generation = 0;
  let disposed = false;
  let task: Promise<void> | null = null;

  const request = (refresh: { paths?: string[]; forceTree?: boolean } = {}): Promise<void> => {
    if (disposed) return Promise.resolve();
    forceTree ||= refresh.forceTree !== false;
    if (!refresh.paths?.length) full = true;
    if (!full) {
      for (const path of refresh.paths ?? []) {
        pendingPaths.add(path);
        if (pendingPaths.size > 32) { full = true; break; }
      }
    }
    if (full) pendingPaths.clear();
    if (task) return task;
    const current = generation;
    options.onBusy?.(true);
    const running = (async () => {
      // Batch notifications from the same frame before starting the scan.
      await Promise.resolve();
      try {
        while (current === generation && !disposed && (full || pendingPaths.size)) {
          const refreshAll = full;
          const rebuild = forceTree;
          const paths = refreshAll ? [''] : Array.from(pendingPaths);
          full = false;
          forceTree = false;
          pendingPaths.clear();
          // Exactly one snapshot rebuild per batch. Child reads must use the
          // resulting snapshot, rather than racing the root filesystem scan.
          await tree.refreshDirectory(paths[0], { refreshTree: rebuild });
          if (current !== generation || disposed) break;
          const remaining = refreshAll
            ? tree.rows.value.filter(row => row.kind === 'dir' && row.expanded).map(row => row.path)
            : paths.slice(1);
          for (let index = 0; index < remaining.length; index += 3) {
            if (current !== generation || disposed) break;
            await Promise.all(remaining.slice(index, index + 3).map(path => tree.refreshDirectory(path)));
          }
          if (current === generation && !disposed) await options.refreshStats();
        }
      } finally {
        // Release the task in the same job as the final pending check. A
        // separate promise.finally would leave a gap that can lose an event.
        if (task === running) {
          task = null;
          options.onBusy?.(false);
        }
      }
    })();
    task = running;
    return running;
  };

  const reset = () => {
    generation++;
    full = false;
    forceTree = false;
    pendingPaths.clear();
    task = null;
    options.onBusy?.(false);
  };

  const handleEvent = (detail: Record<string, unknown>): Promise<void> => {
    const workspaceId = String(detail.workspaceId ?? detail.workspace_id ?? '').trim();
    if (workspaceId && tree.workspaceId.value && workspaceId !== tree.workspaceId.value) return Promise.resolve();
    const version = Number(detail.treeVersion ?? detail.tree_version ?? 0);
    if (detail.forceTree !== true && Number.isFinite(version) && version > 0 &&
        tree.treeVersion.value !== null && version <= tree.treeVersion.value) {
      return Promise.resolve();
    }
    const paths = extractWorkspaceRefreshPaths(detail);
    if (detail.forceTree === true || !paths.length) return request();
    const targets = new Set<string>();
    for (const rawPath of paths) {
      let path = normalizeWorkspaceRelativePath(rawPath);
      // Public paths include the physical subfolder represented by our root.
      if (path === 'workspace') path = '';
      else if (path.startsWith('workspace/')) path = path.slice('workspace/'.length);
      const parent = workspaceParentPath(path);
      targets.add(tree.directories.has(path) ? path : tree.directories.has(parent) ? parent : '');
      if (targets.size > 32) return request();
    }
    if (targets.has('')) return request();
    return request({ paths: Array.from(targets) });
  };

  return { request, handleEvent, reset, dispose: () => { disposed = true; reset(); } };
};
