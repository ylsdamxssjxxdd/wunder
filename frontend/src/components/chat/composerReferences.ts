/**
 * Composer reference model (B4, promoted from the B2 minimal wiring).
 *
 * A reference is a workspace-relative path the user pinned into the composer
 * from the file area (`queueWorkspaceChatReference`) or from the composer's own
 * "+ -> 引用文件" picker. Chips are removable and the send path turns them into a
 * trailing plain-text block:
 *
 *   引用文件
 *   @src/app/main.ts
 *   @docs/notes.md
 *
 * The `@<relative path>` token form is the existing convention shared with the
 * file area and the timeline; nothing here invents a backend field — the block
 * travels as ordinary message text inside the user turn.
 */

import { normalizeWorkspaceRelativePath, workspaceBaseName } from '@/views/messenger/workspace/workspaceFileModel';

export type ComposerReference = {
  /** Stable key: `ws:<path>`; used for removal and de-duplication. */
  id: string;
  /** Workspace-relative path (never an absolute host path). */
  path: string;
  name: string;
  isDir: boolean;
  source: 'workspace';
};

export const COMPOSER_REFERENCE_MAX = 8;

export const buildComposerReferenceId = (path: string): string => `ws:${path}`;

export const normalizeComposerReference = (input: {
  path?: unknown;
  name?: unknown;
  isDir?: unknown;
}): ComposerReference | null => {
  const path = normalizeWorkspaceRelativePath(input?.path);
  if (!path) return null;
  const name = String(input?.name || '').trim() || workspaceBaseName(path);
  return {
    id: buildComposerReferenceId(path),
    path,
    name,
    isDir: input?.isDir === true,
    source: 'workspace'
  };
};

export const normalizeComposerReferences = (
  input: readonly { path?: unknown; name?: unknown; isDir?: unknown }[]
): ComposerReference[] => {
  const items: ComposerReference[] = [];
  const seen = new Set<string>();
  (Array.isArray(input) ? input : []).forEach((entry) => {
    if (items.length >= COMPOSER_REFERENCE_MAX) return;
    const reference = normalizeComposerReference(entry);
    if (!reference || seen.has(reference.id)) return;
    seen.add(reference.id);
    items.push(reference);
  });
  return items;
};

/** Merge new references into the current set; keeps the most recent ones. */
export const mergeComposerReferences = (
  current: readonly ComposerReference[],
  incoming: readonly { path?: unknown; name?: unknown; isDir?: unknown }[]
): { items: ComposerReference[]; added: number } => {
  const items = Array.isArray(current) ? current.slice() : [];
  const known = new Set(items.map((item) => item.id));
  let added = 0;
  (Array.isArray(incoming) ? incoming : []).forEach((entry) => {
    const reference = normalizeComposerReference(entry);
    if (!reference || known.has(reference.id)) return;
    known.add(reference.id);
    items.push(reference);
    added += 1;
  });
  return { items: items.slice(-COMPOSER_REFERENCE_MAX), added };
};

export const removeComposerReference = (
  current: readonly ComposerReference[],
  id: string
): ComposerReference[] => (Array.isArray(current) ? current : []).filter((item) => item.id !== id);

/**
 * Build the trailing reference block appended to the outgoing message text.
 * `title` comes from i18n so the block stays localized.
 */
export const buildComposerReferenceBlock = (
  references: readonly ComposerReference[],
  title: string
): string => {
  if (!Array.isArray(references) || !references.length) return '';
  const header = String(title || '').trim();
  const lines = references.map((item) => `@${item.path}`);
  return [header, ...lines].filter(Boolean).join('\n');
};

export const buildComposerSendContent = (
  text: unknown,
  references: readonly ComposerReference[],
  title: string
): string => {
  const body = String(text || '').trim();
  const block = buildComposerReferenceBlock(references, title);
  if (!block) return body;
  return body ? `${body}\n\n${block}` : block;
};
