export const shouldRenderWorkflowShell = (options: {
  visible?: unknown;
  entryCount?: unknown;
  hasPendingPlaceholder?: unknown;
}): boolean =>
  Boolean(options.visible) &&
  (Number(options.entryCount) > 0 || options.hasPendingPlaceholder === true);
