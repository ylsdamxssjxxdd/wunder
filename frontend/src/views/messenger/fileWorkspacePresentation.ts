type FileWorkspaceScope = 'user' | 'agent';

type Translate = (key: string, params?: Record<string, unknown>) => string;

type ResolveFileWorkspaceEmptyTextOptions = {
  fileScope: FileWorkspaceScope;
  t: Translate;
};

type ResolveFileContainerLifecycleTextOptions = {
  t: Translate;
};

export function resolveFileWorkspaceEmptyText({
  fileScope,
  t
}: ResolveFileWorkspaceEmptyTextOptions): string {
  if (fileScope === 'user') {
    return t('messenger.files.userEmpty');
  }
  return t('workspace.emptyPermanent');
}

export function resolveFileContainerLifecycleText({
  t
}: ResolveFileContainerLifecycleTextOptions): string {
  return t('messenger.files.lifecyclePermanentValue');
}
