/**
 * Lazy, patch-only directory tree store for the sidebar file area (B2).
 *
 * Design constraints from the plan (§6.6):
 * - a directory is requested the first time it is expanded (lazy), and only
 *   that directory's node is patched afterwards — no whole-tree rebuild;
 * - each directory page is bounded (`WORKSPACE_DIRECTORY_PAGE_SIZE`), so the
 *   row list grows only when the user asks for another page;
 * - no deep `watch` anywhere: state is a `Map` of plain records plus two sets,
 *   mutated field by field;
 * - entry objects are reused across refreshes (`patchWorkspaceEntries`), so row
 *   keys stay stable and Vue only patches what actually changed.
 */

import { computed, reactive, ref } from 'vue';

import { fetchWorkspaceDirectory, isWorkspaceRequestCancelled } from './workspaceFileApi';
import {
  WORKSPACE_DIRECTORY_PAGE_SIZE,
  appendWorkspacePage,
  normalizeWorkspaceRelativePath,
  patchWorkspaceEntries,
  removeWorkspaceEntries,
  workspaceParentPath,
  workspacePathChain,
  type WorkspaceEntry,
  type WorkspaceVisibleRow
} from './workspaceFileModel';

export type WorkspaceDirectoryState = {
  path: string;
  entries: WorkspaceEntry[];
  total: number;
  offset: number;
  limit: number;
  loading: boolean;
  loadingMore: boolean;
  loaded: boolean;
  error: string;
};

export type UseWorkspaceFileTreeOptions = {
  pageSize?: number;
  onError?: (message: string, error?: unknown) => void;
};

const createDirectoryState = (path: string, pageSize: number): WorkspaceDirectoryState => ({
  path,
  entries: [],
  total: 0,
  offset: 0,
  limit: pageSize,
  loading: false,
  loadingMore: false,
  loaded: false,
  error: ''
});

export const useWorkspaceFileTree = (options: UseWorkspaceFileTreeOptions = {}) => {
  const pageSize = Math.max(1, Number(options.pageSize) || WORKSPACE_DIRECTORY_PAGE_SIZE);

  /** path -> directory node. `''` is the workspace root. */
  const directories = reactive(new Map<string, WorkspaceDirectoryState>());
  const expandedPaths = reactive(new Set<string>());
  const inflight = new Map<string, Promise<void>>();

  const loadingPaths = ref<string[]>([]);
  const activePath = ref('');
  const selectedPaths = reactive(new Set<string>());
  const selectionMode = ref(false);
  const highlightedPath = ref('');

  const ensureDirectory = (path: string): WorkspaceDirectoryState => {
    const key = normalizeWorkspaceRelativePath(path);
    if (!directories.has(key)) {
      directories.set(key, createDirectoryState(key, pageSize));
    }
    // 必须从 reactive Map 读回代理：`directories.set` 存的是原始对象，
    // 直接返回它会让后续 `state.entries = …` / `state.loading = false`
    // 写在非响应式对象上——首次加载的目录会永远停在「加载中」，
    // 必须手动点一次「刷新」（第二次走 `get` 拿到代理）才渲染。
    return directories.get(key) as WorkspaceDirectoryState;
  };

  const syncLoadingPaths = () => {
    const next: string[] = [];
    directories.forEach((state) => {
      if (state.loading || state.loadingMore) next.push(state.path);
    });
    loadingPaths.value = next;
  };

  const loadDirectory = (
    path: string,
    loadOptions: { force?: boolean; append?: boolean } = {}
  ): Promise<void> => {
    const key = normalizeWorkspaceRelativePath(path);
    const state = ensureDirectory(key);
    const append = loadOptions.append === true;

    if (append) {
      if (state.loadingMore) return Promise.resolve();
      if (state.loaded && state.entries.length >= state.total) return Promise.resolve();
    } else if (!loadOptions.force && state.loaded) {
      return Promise.resolve();
    } else if (!loadOptions.force) {
      const existing = inflight.get(key);
      if (existing) return existing;
    }

    const offset = append ? state.entries.length : 0;
    if (append) {
      state.loadingMore = true;
    } else {
      state.loading = true;
    }
    state.error = '';
    syncLoadingPaths();

    const task = (async () => {
      try {
        const page = await fetchWorkspaceDirectory(key, { offset, limit: pageSize });
        // Patch this directory node only; other directories are untouched.
        state.entries = append
          ? appendWorkspacePage(state.entries, page.entries)
          : patchWorkspaceEntries(state.entries, page.entries);
        // Trust the longer of the two so a paged directory keeps its "more" hint.
        state.total = Math.max(Number(page.total) || 0, state.entries.length);
        state.offset = page.offset;
        state.limit = page.limit || pageSize;
        state.loaded = true;
      } catch (error) {
        if (!isWorkspaceRequestCancelled(error)) {
          state.error = String((error as { message?: string })?.message || '');
          options.onError?.(state.error, error);
        }
      } finally {
        if (append) {
          state.loadingMore = false;
        } else {
          state.loading = false;
        }
        syncLoadingPaths();
      }
    })();

    if (!append) {
      inflight.set(key, task);
      void task.finally(() => {
        if (inflight.get(key) === task) inflight.delete(key);
      });
    }
    return task;
  };

  const refreshDirectory = (path: string): Promise<void> =>
    loadDirectory(path, { force: true });

  const expandDirectory = async (path: string): Promise<void> => {
    const key = normalizeWorkspaceRelativePath(path);
    expandedPaths.add(key);
    await loadDirectory(key);
  };

  const collapseDirectory = (path: string): void => {
    expandedPaths.delete(normalizeWorkspaceRelativePath(path));
  };

  const toggleDirectory = async (path: string): Promise<void> => {
    const key = normalizeWorkspaceRelativePath(path);
    if (expandedPaths.has(key)) {
      collapseDirectory(key);
      return;
    }
    await expandDirectory(key);
  };

  const isExpanded = (path: string): boolean =>
    expandedPaths.has(normalizeWorkspaceRelativePath(path));

  const getDirectory = (path: string): WorkspaceDirectoryState | undefined =>
    directories.get(normalizeWorkspaceRelativePath(path));

  /**
   * Flatten expanded directories into visible rows. Cost is O(visible rows),
   * which is bounded by `pageSize` per expanded directory.
   */
  const rows = computed<WorkspaceVisibleRow[]>(() => {
    const result: WorkspaceVisibleRow[] = [];
    const walk = (path: string, depth: number) => {
      const state = directories.get(path);
      if (!state) return;
      state.entries.forEach((entry) => {
        const isDir = entry.kind === 'dir';
        const dirState = isDir ? directories.get(entry.path) : undefined;
        result.push({
          key: entry.path,
          path: entry.path,
          name: entry.name,
          kind: entry.kind,
          size: entry.size,
          updatedTime: entry.updatedTime,
          depth,
          expanded: isDir && expandedPaths.has(entry.path),
          loading: Boolean(dirState?.loading),
          expandable: isDir
        });
        if (isDir && expandedPaths.has(entry.path) && dirState) {
          walk(entry.path, depth + 1);
        }
      });
      // Pagination footer for this directory, rendered as a normal row so the
      // virtual window keeps a single uniform row height.
      const remaining = state.total - state.entries.length;
      if (remaining > 0) {
        result.push({
          key: `${path}::more`,
          path,
          name: '',
          kind: 'more',
          size: 0,
          updatedTime: '',
          depth,
          expanded: false,
          loading: state.loadingMore,
          expandable: false,
          remaining
        });
      }
    };
    walk('', 0);
    return result;
  });

  const rootState = computed(() => directories.get(''));

  const hasMore = (path: string): boolean => {
    const state = directories.get(normalizeWorkspaceRelativePath(path));
    if (!state) return false;
    return state.entries.length < state.total;
  };

  const loadMore = (path: string): Promise<void> => loadDirectory(path, { append: true });

  // ------------------------------------------------------------- selection

  const selectedList = computed(() => Array.from(selectedPaths));

  const clearSelection = () => {
    selectedPaths.clear();
  };

  const exitSelectionMode = () => {
    selectionMode.value = false;
    clearSelection();
  };

  const toggleSelection = (path: string) => {
    const key = normalizeWorkspaceRelativePath(path);
    if (!key) return;
    if (selectedPaths.has(key)) {
      selectedPaths.delete(key);
    } else {
      selectedPaths.add(key);
    }
  };

  const isSelected = (path: string): boolean =>
    selectedPaths.has(normalizeWorkspaceRelativePath(path));

  const setSelection = (paths: string[]) => {
    selectedPaths.clear();
    paths.forEach((path) => {
      const key = normalizeWorkspaceRelativePath(path);
      if (key) selectedPaths.add(key);
    });
  };

  // ---------------------------------------------------------------- reveal

  /** Page forward until `targetPath` shows up in `dirPath` or the pages run out. */
  const ensureEntryLoaded = async (dirPath: string, targetPath: string): Promise<boolean> => {
    const key = normalizeWorkspaceRelativePath(dirPath);
    await loadDirectory(key);
    let state = directories.get(key);
    if (!state) return false;
    let guard = 0;
    while (
      !state.entries.some((entry) => entry.path === targetPath) &&
      state.entries.length < state.total &&
      guard < 20
    ) {
      guard += 1;
      await loadMore(key);
      state = directories.get(key);
      if (!state) return false;
    }
    return state.entries.some((entry) => entry.path === targetPath);
  };

  /**
   * Expand the ancestor chain of `path` and highlight the row, paging forward
   * when the target sits in a directory page that is not loaded yet.
   */
  const revealPath = async (
    path: string,
    revealOptions: { silent?: boolean } = {}
  ): Promise<boolean> => {
    const key = normalizeWorkspaceRelativePath(path);
    if (!key) return false;
    const chain = workspacePathChain(key);
    for (let index = 0; index < chain.length - 1; index += 1) {
      const dirPath = chain[index];
      const parent = workspaceParentPath(dirPath);
      if (parent) await ensureEntryLoaded(parent, dirPath);
      await expandDirectory(dirPath);
    }
    const parent = workspaceParentPath(key);
    const visible = parent ? await ensureEntryLoaded(parent, key) : true;
    if (visible && revealOptions.silent !== true) {
      highlightedPath.value = key;
      window.setTimeout(() => {
        if (highlightedPath.value === key) highlightedPath.value = '';
      }, 2400);
    }
    return visible;
  };

  // ------------------------------------------------------------------ patch

  /** Drop entries that were deleted or moved away, without reloading the tree. */
  const removePaths = (paths: Iterable<string>) => {
    const removed = new Set<string>();
    Array.from(paths).forEach((path) => {
      const key = normalizeWorkspaceRelativePath(path);
      if (!key) return;
      removed.add(key);
      expandedPaths.delete(key);
      directories.delete(key);
      selectedPaths.delete(key);
    });
    if (!removed.size) return;
    directories.forEach((state) => {
      const before = state.entries.length;
      const next = removeWorkspaceEntries(state.entries, removed);
      if (next === state.entries) return;
      state.entries = next;
      state.total = Math.max(0, state.total - (before - next.length));
    });
  };

  const patchDirectoryEntries = (path: string, entries: WorkspaceEntry[]) => {
    const key = normalizeWorkspaceRelativePath(path);
    const state = ensureDirectory(key);
    state.entries = patchWorkspaceEntries(state.entries, entries);
    state.total = Math.max(state.total, state.entries.length);
    state.loaded = true;
  };

  const reset = () => {
    inflight.clear();
    directories.clear();
    expandedPaths.clear();
    selectedPaths.clear();
    selectionMode.value = false;
    activePath.value = '';
    highlightedPath.value = '';
    loadingPaths.value = [];
  };

  const setActivePath = (path: string) => {
    activePath.value = normalizeWorkspaceRelativePath(path);
  };

  return {
    pageSize,
    directories,
    rows,
    rootState,
    expandedPaths,
    loadingPaths,
    activePath,
    selectedPaths,
    selectedList,
    selectionMode,
    highlightedPath,
    loadDirectory,
    refreshDirectory,
    loadMore,
    hasMore,
    expandDirectory,
    collapseDirectory,
    toggleDirectory,
    isExpanded,
    getDirectory,
    ensureDirectory,
    clearSelection,
    exitSelectionMode,
    toggleSelection,
    isSelected,
    setSelection,
    revealPath,
    removePaths,
    patchDirectoryEntries,
    setActivePath,
    reset
  };
};

export type WorkspaceFileTree = ReturnType<typeof useWorkspaceFileTree>;
