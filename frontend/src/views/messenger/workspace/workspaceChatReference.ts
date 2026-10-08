/**
 * B2 -> B4 hand-off channel ("引用到聊天").
 *
 * The file area never reaches into the composer directly. It pushes a bounded
 * list of references here; `ChatComposer` drains the list with a single shallow
 * `watch` and turns each item into an `@<relative path>` token plus a chip.
 * B4 can replace the drain site with the real attachment/quote model without
 * touching the file area.
 *
 * A second, equally small channel lets the timeline (B3) ask the file area to
 * reveal a path (`[src/foo.rs]` labels) without importing its component.
 */

import { ref } from 'vue';

import { normalizeWorkspaceRelativePath, workspaceBaseName } from './workspaceFileModel';

export type WorkspaceChatReference = {
  path: string;
  name: string;
  isDir: boolean;
};

const MAX_PENDING_REFERENCES = 8;

const pendingReferences = ref<WorkspaceChatReference[]>([]);

export const pendingWorkspaceChatReferences = pendingReferences;

export const queueWorkspaceChatReference = (reference: {
  path: string;
  name?: string;
  isDir?: boolean;
}): boolean => {
  const path = normalizeWorkspaceRelativePath(reference?.path);
  if (!path) return false;
  const name = String(reference?.name || '').trim() || workspaceBaseName(path);
  const next = pendingReferences.value.filter((item) => item.path !== path);
  next.push({ path, name, isDir: reference?.isDir === true });
  // Bounded: keep the most recent references only.
  pendingReferences.value = next.slice(-MAX_PENDING_REFERENCES);
  return true;
};

export const takeWorkspaceChatReferences = (): WorkspaceChatReference[] => {
  if (!pendingReferences.value.length) return [];
  const items = pendingReferences.value.slice();
  pendingReferences.value = [];
  return items;
};

// ------------------------------------------------------------------ reveal

type WorkspaceRevealRequest = {
  path: string;
  token: number;
};

const revealRequest = ref<WorkspaceRevealRequest | null>(null);
let revealToken = 0;

export const pendingWorkspaceReveal = revealRequest;

/** Ask the file area to expand and highlight a workspace-relative path. */
export const requestWorkspaceReveal = (path: string): void => {
  const normalized = normalizeWorkspaceRelativePath(path);
  if (!normalized) return;
  revealToken += 1;
  revealRequest.value = { path: normalized, token: revealToken };
};

export const clearWorkspaceReveal = (token?: number): void => {
  if (!revealRequest.value) return;
  if (token !== undefined && revealRequest.value.token !== token) return;
  revealRequest.value = null;
};

export const WORKSPACE_REFERENCE_MAX_ITEMS = MAX_PENDING_REFERENCES;
