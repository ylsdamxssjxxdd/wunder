export type ComposerContextDisplayState = {
  scope: string;
  assistant: string;
  observed: boolean;
  used: number | null;
  total: number | null;
};

export const resolveComposerContextDisplay = (
  previous: ComposerContextDisplayState | undefined,
  next: ComposerContextDisplayState
): ComposerContextDisplayState => {
  if (!previous || previous.scope !== next.scope) return next;
  const retainObservation = previous.observed && !next.observed;
  return {
    ...next,
    observed: next.observed || retainObservation,
    // Missing fields are not resets. Explicit zero and decreasing snapshots win.
    used: retainObservation ? previous.used : next.used ?? previous.used,
    total: next.total ?? previous.total
  };
};
