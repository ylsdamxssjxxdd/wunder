type FileWorkspaceScope = 'user' | 'agent';

type Translate = (key: string, params?: Record<string, unknown>) => string;

type ResolveFileWorkspaceEmptyTextOptions = {
  fileScope: FileWorkspaceScope;
  desktopLocalMode: boolean;
  t: Translate;
};

type ResolveFileContainerLifecycleTextOptions = {
  fileScope: FileWorkspaceScope;
  desktopLocalMode: boolean;
  entryCount: number;
  latestUpdatedAt: number;
  now: number;
  t: Translate;
};

const AGENT_CONTAINER_TTL_MS = 24 * 60 * 60 * 1000;

function formatRemainingDuration(ms: number, t: Translate): string {
  const safe = Math.max(0, Math.floor(ms / 1000));
  const days = Math.floor(safe / 86400);
  const hours = Math.floor((safe % 86400) / 3600);
  const minutes = Math.floor((safe % 3600) / 60);

  if (days > 0) {
    return t('messenger.files.lifecycleDaysHours', { days, hours });
  }
  if (hours > 0) {
    return t('messenger.files.lifecycleHoursMinutes', { hours, minutes });
  }
  return t('messenger.files.lifecycleMinutes', { minutes: Math.max(1, minutes) });
}

export function resolveFileWorkspaceEmptyText({
  fileScope,
  desktopLocalMode,
  t
}: ResolveFileWorkspaceEmptyTextOptions): string {
  if (fileScope === 'user') {
    return t('messenger.files.userEmpty');
  }
  if (desktopLocalMode) {
    return t('workspace.emptyPermanent');
  }
  return t('workspace.empty');
}

export function resolveFileContainerLifecycleText({
  fileScope,
  desktopLocalMode,
  entryCount,
  latestUpdatedAt,
  now,
  t
}: ResolveFileContainerLifecycleTextOptions): string {
  if (fileScope === 'user' || desktopLocalMode) {
    return t('messenger.files.lifecyclePermanentValue');
  }
  if (!entryCount || latestUpdatedAt <= 0) {
    return t('messenger.files.lifecycleEmptyValue');
  }
  const remaining = latestUpdatedAt + AGENT_CONTAINER_TTL_MS - now;
  if (remaining <= 0) {
    return t('messenger.files.lifecycleExpiredValue');
  }
  return t('messenger.files.lifecycleRemainingValue', {
    remaining: formatRemainingDuration(remaining, t)
  });
}
