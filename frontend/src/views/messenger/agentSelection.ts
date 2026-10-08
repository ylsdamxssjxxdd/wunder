const normalizeAgentSelectionId = (value: unknown): string => String(value || '').trim();

const dedupeAgentSelectionIds = (ids: readonly unknown[]): string[] => {
  const output: string[] = [];
  const seen = new Set<string>();
  ids.forEach((item) => {
    const normalized = normalizeAgentSelectionId(item);
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    output.push(normalized);
  });
  return output;
};

type ResolveAgentSelectionAfterRemovalOptions = {
  removedId: unknown;
  previousIds: readonly unknown[];
  currentIds: readonly unknown[];
  fallbackId?: unknown;
};

export const resolveAgentSelectionAfterRemoval = ({
  removedId,
  previousIds,
  currentIds,
  fallbackId = ''
}: ResolveAgentSelectionAfterRemovalOptions): string => {
  const normalizedRemovedId = normalizeAgentSelectionId(removedId);
  const normalizedCurrentIds = dedupeAgentSelectionIds(currentIds);
  const normalizedFallbackId = normalizeAgentSelectionId(fallbackId);
  if (!normalizedCurrentIds.length) {
    return normalizedFallbackId;
  }
  if (!normalizedRemovedId) {
    return normalizedCurrentIds[0] || normalizedFallbackId;
  }
  const normalizedPreviousIds = dedupeAgentSelectionIds(previousIds);
  const removedIndex = normalizedPreviousIds.indexOf(normalizedRemovedId);
  if (removedIndex < 0) {
    return normalizedCurrentIds[0] || normalizedFallbackId;
  }
  const targetIndex = Math.min(removedIndex, normalizedCurrentIds.length - 1);
  return normalizedCurrentIds[targetIndex] || normalizedFallbackId;
};
