type UnknownObject = Record<string, unknown>;

const toOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.round(value);
    }
    if (typeof value === 'string') {
      const normalized = Number(value.trim());
      if (Number.isFinite(normalized)) {
        return Math.round(normalized);
      }
    }
  }
  return null;
};

export const resolveCompactionDividerTransitionTokens = (
  detail: UnknownObject | null | undefined
): { before: number; after: number } | null => {
  if (!detail) return null;

  const confirmedBefore = toOptionalInt(
    detail.final_context_tokens_before,
    detail.persisted_context_tokens
  );
  const confirmedAfter = toOptionalInt(detail.final_context_tokens);
  if (
    confirmedBefore !== null &&
    confirmedAfter !== null &&
    confirmedAfter > 0 &&
    confirmedAfter < confirmedBefore
  ) {
    return { before: confirmedBefore, after: confirmedAfter };
  }

  const messageBefore = toOptionalInt(
    detail.context_tokens,
    detail.history_usage,
    detail.context_guard_tokens_before
  );
  const messageAfter = toOptionalInt(
    detail.context_tokens_after,
    detail.context_guard_tokens_after
  );
  if (
    messageBefore !== null &&
    messageAfter !== null &&
    messageAfter > 0 &&
    messageAfter < messageBefore
  ) {
    return { before: messageBefore, after: messageAfter };
  }

  const projectedBefore = toOptionalInt(
    detail.projected_request_tokens,
    detail.total_tokens
  );
  const projectedAfter = toOptionalInt(
    detail.projected_request_tokens_after,
    detail.total_tokens_after
  );
  if (
    projectedBefore !== null &&
    projectedAfter !== null &&
    projectedAfter > 0 &&
    projectedAfter < projectedBefore
  ) {
    return { before: projectedBefore, after: projectedAfter };
  }

  return null;
};
