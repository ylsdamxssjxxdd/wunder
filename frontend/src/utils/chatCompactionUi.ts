type UnknownObject = Record<string, unknown>;

type TranslateFn = (key: string, params?: Record<string, unknown>) => string;

export type CompactionStageState = 'done' | 'active' | 'pending' | 'warning';

export type CompactionStageView = {
  key: string;
  label: string;
  detail: string;
  state: CompactionStageState;
};

export type CompactionMetricTone = 'default' | 'success' | 'warning';

export type CompactionMetricView = {
  key: string;
  label: string;
  value: string;
  tone: CompactionMetricTone;
};

export type CompactionDetailView = {
  key: string;
  label: string;
  value: string;
};

export type CompactionUsageBarView = {
  beforeRatio: number | null;
  afterRatio: number | null;
  beforeBarRatio: number | null;
  afterBarRatio: number | null;
  beforeLabel: string;
  afterLabel: string;
  tone: 'info' | 'success' | 'warning' | 'danger';
};

export type CompactionFailureView = {
  title: string;
  description: string;
  suggestions: string[];
};

export type CompactionOutputView = {
  key: string;
  title: string;
  body: string;
  tone: 'default' | 'warning';
};

export type CompactionView = {
  headline: string;
  description: string;
  stages: CompactionStageView[];
  metrics: CompactionMetricView[];
  details: CompactionDetailView[];
  outputs: CompactionOutputView[];
  outputEmpty: string;
  usageBar: CompactionUsageBarView | null;
  failure: CompactionFailureView | null;
};

export type CompactionDisplay = {
  summaryTitle: string;
  summaryNote: string;
  summaryNoteTone: 'info' | 'success' | 'warning';
  resultSummary: string;
  resultBody: string;
  copyBody: string;
  view: CompactionView;
};

const numberFormatter = new Intl.NumberFormat('en-US');

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string') {
      const trimmed = value.trim();
      if (trimmed) return trimmed;
    }
  }
  return '';
};

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

const toBool = (...values: unknown[]): boolean | null => {
  for (const value of values) {
    if (typeof value === 'boolean') return value;
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (normalized === 'true') return true;
      if (normalized === 'false') return false;
    }
  }
  return null;
};

const formatTokenCount = (value: number | null): string =>
  value === null ? '' : `${numberFormatter.format(value)} tokens`;

const formatTokenTransition = (before: number | null, after: number | null): string => {
  if (before === null && after === null) return '';
  if (before !== null && after !== null) return `${formatTokenCount(before)} → ${formatTokenCount(after)}`;
  if (before !== null) return formatTokenCount(before);
  return formatTokenCount(after);
};

const appendLine = (lines: string[], label: string, value: string): void => {
  const trimmed = value.trim();
  if (trimmed) {
    lines.push(`${label}: ${trimmed}`);
  }
};

const appendDetail = (
  details: CompactionDetailView[],
  key: string,
  label: string,
  value: string
): void => {
  const trimmed = value.trim();
  if (trimmed) {
    details.push({ key, label, value: trimmed });
  }
};

const pushMetric = (
  metrics: CompactionMetricView[],
  key: string,
  label: string,
  value: string,
  tone: CompactionMetricTone = 'default'
): void => {
  const trimmed = value.trim();
  if (trimmed) {
    metrics.push({ key, label, value: trimmed, tone });
  }
};

const resolveReasonLabel = (reason: string, t: TranslateFn): string => {
  if (reason === 'history') return t('chat.toolWorkflow.compaction.reason.history');
  if (reason === 'overflow') return t('chat.toolWorkflow.compaction.reason.overflow');
  if (reason === 'overflow_recovery') return t('chat.toolWorkflow.compaction.reason.overflowRecovery');
  return reason || t('chat.toolWorkflow.compaction.reason.default');
};

export const resolveCompactionInstanceLabel = (
  workflowRef: unknown,
  t: TranslateFn
): string => {
  const normalized = pickString(workflowRef).toLowerCase();
  if (!normalized.startsWith('compaction:')) return '';
  const parts = normalized.split(':').map((part) => part.trim()).filter(Boolean);
  if (parts.length < 3) return '';
  const workflowTitle = t('chat.toolWorkflow.title');
  const isChineseUi = /智能体循环|鍟嗚兘浣撳惊鐜?/u.test(workflowTitle);
  const resolveLabel = (key: string, fallback: string, params?: Record<string, unknown>) => {
    const translated = t(key, params);
    return translated === key ? fallback : translated;
  };
  if (parts[1] === 'manual') {
    return resolveLabel(
      'chat.toolWorkflow.compaction.instanceManual',
      isChineseUi ? '手动触发压缩' : 'Manual compaction'
    );
  }
  const index = Number.parseInt(parts[parts.length - 1] || '', 10);
  if (!Number.isFinite(index) || index <= 0) return '';
  return resolveLabel(
    'chat.toolWorkflow.compaction.instanceAuto',
    isChineseUi ? `第 ${index} 次压缩` : `Compaction #${index}`,
    { index }
  );
};

const buildRequestBudgetLine = (
  messageBudget: number | null,
  requestOverheadTokens: number | null,
  limit: number | null
): string => {
  const parts: string[] = [];
  if (messageBudget !== null) parts.push(`messages ${formatTokenCount(messageBudget)}`);
  if (requestOverheadTokens !== null) parts.push(`tools ${formatTokenCount(requestOverheadTokens)}`);
  const left = parts.join(' + ');
  if (!left && limit === null) return '';
  if (left && limit !== null) return `${left} ≤ limit ${formatTokenCount(limit)}`;
  if (left) return left;
  return `limit ${formatTokenCount(limit)}`;
};

export const resolveCompactionProgressTitle = (
  stage: unknown,
  summary: unknown,
  t: TranslateFn
): string | null => {
  const normalizedStage = String(stage || '').trim().toLowerCase();
  if (normalizedStage === 'compacting') {
    return t('chat.workflow.compactionRunning');
  }
  if (normalizedStage === 'context_overflow_recovery') {
    return t('chat.workflow.compactionRecovering');
  }
  if (normalizedStage === 'context_guard') {
    return t('chat.workflow.compactionGuard');
  }
  const normalizedSummary = pickString(summary);
  return normalizedSummary || null;
};

const resolveRunningNote = (stage: string, t: TranslateFn): string => {
  if (stage === 'context_overflow_recovery') {
    return t('chat.toolWorkflow.compaction.noteRecoveringLive');
  }
  if (stage === 'context_guard') {
    return t('chat.toolWorkflow.compaction.noteGuardLive');
  }
  return t('chat.toolWorkflow.compaction.noteRunningLive');
};

const resolveRunningSummary = (stage: string, t: TranslateFn): string => {
  if (stage === 'context_overflow_recovery') {
    return t('chat.toolWorkflow.compaction.summaryRecoveringLive');
  }
  if (stage === 'context_guard') {
    return t('chat.toolWorkflow.compaction.summaryGuardLive');
  }
  return t('chat.toolWorkflow.compaction.summaryRunningLive');
};

const resolveStageDetail = (fallback: string, value: string, t: TranslateFn): string =>
  value.trim() || fallback || t('chat.toolWorkflow.compaction.stage.pending');

const resolveUsageRatio = (value: number | null, limit: number | null): number | null => {
  if (value === null || limit === null || limit <= 0) return null;
  const ratio = value / limit;
  if (!Number.isFinite(ratio)) return null;
  return Math.max(0, ratio);
};

const clampUsageBarRatio = (value: number | null): number | null => {
  if (value === null) return null;
  return Math.max(0, Math.min(value, 1));
};

const formatPercent = (value: number | null): string => {
  if (value === null) return '';
  return `${Math.round(value * 100)}%`;
};

const looksLikeContextOverflow = (text: string): boolean => {
  const normalized = text.trim().toLowerCase();
  if (!normalized) return false;
  return [
    'context_window_exceeded',
    'context length exceeded',
    'context window',
    'input exceeds the context window',
    'exceeds the model',
    'prompt is too long',
    '上下文',
    '超限',
    '过长'
  ].some((token) => normalized.includes(token));
};

export const buildCompactionDisplay = (
  detailObject: UnknownObject | null,
  status: string,
  t: TranslateFn
): CompactionDisplay => {
  const normalizedStatus = pickString(detailObject?.status, status).toLowerCase();
  const normalizedStage = pickString(detailObject?.stage).toLowerCase();
  const isRunning = normalizedStatus === 'loading' || normalizedStatus === 'pending';
  const reason = pickString(detailObject?.reason).toLowerCase();
  const summaryFallback = toBool(detailObject?.summary_fallback) === true;
  const usedFallback = summaryFallback || normalizedStatus === 'fallback';
  const guardApplied =
    toBool(detailObject?.context_guard_applied) === true
    || normalizedStatus === 'guard_only'
    || normalizedStage === 'context_guard';
  const summaryRemoved =
    toBool(detailObject?.context_guard_summary_removed, detailObject?.summary_removed) === true;
  const summaryTrimmed =
    toBool(detailObject?.context_guard_summary_trimmed, detailObject?.summary_trimmed) === true;
  const currentTrimmed =
    toBool(detailObject?.context_guard_current_user_trimmed, detailObject?.current_user_trimmed) === true;

  const projectedBefore = toOptionalInt(
    detailObject?.projected_request_tokens,
    detailObject?.total_tokens,
    detailObject?.context_tokens,
    detailObject?.context_guard_tokens_before
  );
  const projectedAfter = toOptionalInt(
    detailObject?.projected_request_tokens_after,
    detailObject?.total_tokens_after,
    detailObject?.context_tokens_after,
    detailObject?.context_guard_tokens_after
  );
  const projectedTransition = formatTokenTransition(projectedBefore, projectedAfter);

  const messageBefore = toOptionalInt(
    detailObject?.context_tokens,
    detailObject?.history_usage,
    detailObject?.context_guard_tokens_before
  );
  const messageAfter = toOptionalInt(
    detailObject?.context_tokens_after,
    detailObject?.context_guard_tokens_after
  );
  const messageTransition = formatTokenTransition(messageBefore, messageAfter);

  const currentTransition = formatTokenTransition(
    toOptionalInt(
      detailObject?.context_guard_current_user_tokens_before,
      detailObject?.current_user_tokens_before
    ),
    toOptionalInt(
      detailObject?.context_guard_current_user_tokens_after,
      detailObject?.current_user_tokens_after
    )
  );
  const currentState = currentTrimmed
    ? t('chat.toolWorkflow.compaction.detail.valueTrimmed')
    : t('chat.toolWorkflow.compaction.detail.valuePreserved');

  const summaryTransition = formatTokenTransition(
    toOptionalInt(detailObject?.context_guard_summary_tokens_before, detailObject?.summary_tokens),
    toOptionalInt(detailObject?.context_guard_summary_tokens_after)
  );
  const summaryState = summaryRemoved
    ? t('chat.toolWorkflow.compaction.detail.valueRemoved')
    : summaryTrimmed
      ? t('chat.toolWorkflow.compaction.detail.valueTrimmed')
      : '';

  const requestBudget = buildRequestBudgetLine(
    toOptionalInt(detailObject?.message_budget),
    toOptionalInt(detailObject?.request_overhead_tokens),
    toOptionalInt(detailObject?.limit)
  );
  const limit = toOptionalInt(detailObject?.limit);
  const persistedBaseline = formatTokenCount(toOptionalInt(detailObject?.persisted_context_tokens));
  const resetMode = pickString(detailObject?.reset_mode, detailObject?.resetMode);
  const errorCode = pickString(detailObject?.error_code, detailObject?.code);
  const errorMessage = pickString(
    detailObject?.error_message,
    detailObject?.message,
    detailObject?.error,
    detailObject?.detail
  );
  const hasOverflowFailure =
    normalizedStatus === 'failed'
    && (errorCode === 'CONTEXT_WINDOW_EXCEEDED' || looksLikeContextOverflow(errorMessage));
  const overflowFailureSummary = hasOverflowFailure
    ? t('chat.toolWorkflow.compaction.summaryFailedOverflow')
    : '';
  const modelOutput = pickString(
    detailObject?.summary_model_output,
    detailObject?.summaryModelOutput,
    detailObject?.compaction_model_output,
    detailObject?.compactionModelOutput
  );
  const injectedSummary = pickString(
    detailObject?.summary_text,
    detailObject?.summaryText,
    detailObject?.summary_context_text,
    detailObject?.summaryContextText,
    detailObject?.compaction_summary_text,
    detailObject?.compactionSummaryText
  );

  const resultLine =
    normalizedStatus === 'failed'
      ? t('chat.toolWorkflow.compaction.detail.resultFailed')
      : normalizedStatus === 'guard_only'
      ? t('chat.toolWorkflow.compaction.detail.resultGuardOnly')
      : normalizedStatus === 'skipped'
        ? t('chat.toolWorkflow.compaction.detail.resultSkipped')
        : usedFallback
          ? t('chat.toolWorkflow.compaction.detail.resultFallback')
          : t('chat.toolWorkflow.compaction.detail.resultDone');

  const note = isRunning
    ? resolveRunningNote(normalizedStage, t)
    : hasOverflowFailure
      ? t('chat.toolWorkflow.compaction.noteFailedOverflow')
    : usedFallback
      ? t('chat.toolWorkflow.compaction.noteFallback')
      : reason === 'overflow_recovery'
        ? t('chat.toolWorkflow.compaction.noteRecovered')
        : normalizedStatus === 'guard_only'
          ? t('chat.toolWorkflow.compaction.noteGuardOnly')
          : normalizedStatus === 'skipped'
            ? t('chat.toolWorkflow.compaction.noteSkipped')
            : t('chat.toolWorkflow.compaction.notePrepared');
  const noteTone: CompactionDisplay['summaryNoteTone'] = isRunning
    ? 'info'
    : hasOverflowFailure
      ? 'warning'
    : usedFallback
      ? 'warning'
      : reason === 'overflow_recovery'
        ? 'success'
        : normalizedStatus === 'guard_only'
          ? 'warning'
          : 'info';

  const progressTitle = resolveCompactionProgressTitle(normalizedStage, detailObject?.summary, t);
  const summaryTitleBase = isRunning && progressTitle
    ? progressTitle
    : reason === 'history'
      ? t('chat.toolWorkflow.compaction.titleHistory')
      : reason === 'overflow_recovery'
        ? t('chat.toolWorkflow.compaction.titleRecovery')
        : reason === 'overflow'
          ? t('chat.toolWorkflow.compaction.titleOverflow')
          : t('chat.toolWorkflow.compaction.title');
  const summaryTitle = projectedTransition && !isRunning
    ? `${summaryTitleBase} ${projectedTransition}`
    : summaryTitleBase;

  let resultSummary = isRunning
    ? resolveRunningSummary(normalizedStage, t)
    : hasOverflowFailure
      ? overflowFailureSummary
    : reason === 'history'
      ? t('chat.toolWorkflow.compaction.summaryHistory')
      : reason === 'overflow_recovery'
        ? t('chat.toolWorkflow.compaction.summaryRecovery')
        : reason === 'overflow'
          ? t('chat.toolWorkflow.compaction.summaryOverflow')
          : t('chat.toolWorkflow.compaction.summaryDefault');
  if (!isRunning && normalizedStatus === 'guard_only') {
    resultSummary = t('chat.toolWorkflow.compaction.summaryGuardOnly');
  } else if (!isRunning && normalizedStatus === 'skipped') {
    resultSummary = t('chat.toolWorkflow.compaction.summarySkipped');
  }
  if (!isRunning && usedFallback) {
    resultSummary = `${resultSummary} ${t('chat.toolWorkflow.compaction.summaryFallbackAppend')}`.trim();
  }

  const lines: string[] = [];
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.reason'), resolveReasonLabel(reason, t));
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.projectedRequest'), projectedTransition);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.messageContext'), messageTransition);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.requestBudget'), requestBudget);
  appendLine(
    lines,
    t('chat.toolWorkflow.compaction.detail.currentQuestion'),
    currentTransition ? `${currentTransition} (${currentState})` : ''
  );
  appendLine(
    lines,
    t('chat.toolWorkflow.compaction.detail.summary'),
    summaryTransition ? `${summaryTransition}${summaryState ? ` (${summaryState})` : ''}` : summaryState
  );
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.persistedBaseline'), persistedBaseline);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.resetMode'), resetMode);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.errorCode'), errorCode);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.errorMessage'), errorMessage);
  appendLine(lines, t('chat.toolWorkflow.compaction.detail.result'), resultLine);

  const outputs: CompactionOutputView[] = [];
  if (modelOutput) {
    outputs.push({
      key: 'model-output',
      title: usedFallback
        ? t('chat.toolWorkflow.compaction.output.fallbackTitle')
        : t('chat.toolWorkflow.compaction.output.modelTitle'),
      body: modelOutput,
      tone: usedFallback ? 'warning' : 'default'
    });
  }
  if (injectedSummary && injectedSummary !== modelOutput) {
    outputs.push({
      key: 'injected-summary',
      title: t('chat.toolWorkflow.compaction.output.injectedTitle'),
      body: injectedSummary,
      tone: 'default'
    });
  }
  const outputEmpty = isRunning
    ? t('chat.toolWorkflow.compaction.output.pending')
    : normalizedStatus === 'guard_only'
      ? t('chat.toolWorkflow.compaction.output.emptyGuardOnly')
      : normalizedStatus === 'skipped'
        ? t('chat.toolWorkflow.compaction.output.emptySkipped')
        : t('chat.toolWorkflow.compaction.output.empty');

  const beforeRatio = resolveUsageRatio(projectedBefore, limit);
  const afterRatio = resolveUsageRatio(projectedAfter, limit);
  const beforeBarRatio = clampUsageBarRatio(beforeRatio);
  const afterBarRatio = clampUsageBarRatio(afterRatio);
  const usageBar: CompactionUsageBarView | null =
    beforeRatio === null && afterRatio === null
      ? null
      : {
          beforeRatio,
          afterRatio,
          beforeBarRatio,
          afterBarRatio,
          beforeLabel: projectedBefore !== null
            ? t('chat.toolWorkflow.compaction.usage.before', {
                tokens: formatTokenCount(projectedBefore),
                percent: formatPercent(beforeRatio)
              })
            : '',
          afterLabel: projectedAfter !== null
            ? t('chat.toolWorkflow.compaction.usage.after', {
                tokens: formatTokenCount(projectedAfter),
                percent: formatPercent(afterRatio)
              })
            : '',
          tone: hasOverflowFailure
            ? 'danger'
            : afterRatio !== null && afterRatio >= 0.9
              ? 'warning'
              : afterRatio !== null && beforeRatio !== null && afterRatio < beforeRatio
                ? 'success'
                : 'info'
        };

  const metrics: CompactionMetricView[] = [];
  pushMetric(
    metrics,
    'projected-request',
    t('chat.toolWorkflow.compaction.metric.projectedRequest'),
    projectedTransition,
    !isRunning && projectedBefore !== null && projectedAfter !== null && projectedAfter < projectedBefore
      ? 'success'
      : 'default'
  );
  pushMetric(
    metrics,
    'message-context',
    t('chat.toolWorkflow.compaction.metric.messageContext'),
    messageTransition,
    !isRunning && messageBefore !== null && messageAfter !== null && messageAfter < messageBefore
      ? 'success'
      : 'default'
  );
  pushMetric(
    metrics,
    'current-question',
    t('chat.toolWorkflow.compaction.metric.currentQuestion'),
    currentTransition ? `${currentTransition} (${currentState})` : currentState,
    currentTrimmed ? 'warning' : 'default'
  );
  pushMetric(
    metrics,
    'summary',
    t('chat.toolWorkflow.compaction.metric.summary'),
    summaryTransition ? `${summaryTransition}${summaryState ? ` (${summaryState})` : ''}` : summaryState,
    summaryRemoved || summaryTrimmed ? 'warning' : 'default'
  );

  const details: CompactionDetailView[] = [];
  appendDetail(details, 'reason', t('chat.toolWorkflow.compaction.detail.reason'), resolveReasonLabel(reason, t));
  appendDetail(details, 'request-budget', t('chat.toolWorkflow.compaction.detail.requestBudget'), requestBudget);
  appendDetail(details, 'persisted-baseline', t('chat.toolWorkflow.compaction.detail.persistedBaseline'), persistedBaseline);
  appendDetail(details, 'reset-mode', t('chat.toolWorkflow.compaction.detail.resetMode'), resetMode);
  appendDetail(details, 'error-code', t('chat.toolWorkflow.compaction.detail.errorCode'), errorCode);
  appendDetail(details, 'error-message', t('chat.toolWorkflow.compaction.detail.errorMessage'), errorMessage);
  appendDetail(details, 'result', t('chat.toolWorkflow.compaction.detail.result'), resultLine);

  const failure: CompactionFailureView | null = hasOverflowFailure
    ? {
        title: t('chat.toolWorkflow.compaction.failure.title'),
        description: t('chat.toolWorkflow.compaction.failure.description'),
        suggestions: [
          t('chat.toolWorkflow.compaction.failure.suggestionNewThread'),
          t('chat.toolWorkflow.compaction.failure.suggestionShortenInput'),
          t('chat.toolWorkflow.compaction.failure.suggestionRetry')
        ]
      }
    : null;

  // Build a lightweight step timeline so the UI can show the compaction lifecycle,
  // even when the backend is still streaming progress events.
  const detectState: CompactionStageState = normalizedStage === 'context_overflow_recovery'
    ? 'active'
    : isRunning
      ? 'done'
      : 'done';
  const compactState: CompactionStageState = normalizedStage === 'compacting'
    ? 'active'
    : normalizedStatus === 'skipped'
      ? 'warning'
      : normalizedStage === 'context_guard' || !isRunning
        ? 'done'
        : 'pending';
  const guardState: CompactionStageState = normalizedStage === 'context_guard'
    ? 'active'
    : guardApplied
      ? normalizedStatus === 'guard_only'
        ? 'warning'
        : 'done'
      : 'pending';
  const resumeState: CompactionStageState = isRunning
    ? 'pending'
    : usedFallback
      ? 'warning'
      : 'done';

  const stages: CompactionStageView[] = [
    {
      key: 'detect',
      label: t('chat.toolWorkflow.compaction.stage.detect'),
      detail: resolveStageDetail(resultSummary, resolveReasonLabel(reason, t), t),
      state: detectState
    },
    {
      key: 'compact',
      label: t('chat.toolWorkflow.compaction.stage.compact'),
      detail: normalizedStatus === 'skipped'
        ? t('chat.toolWorkflow.compaction.stage.notNeeded')
        : resolveStageDetail(resultSummary, projectedTransition || messageTransition, t),
      state: compactState
    },
    {
      key: 'guard',
      label: t('chat.toolWorkflow.compaction.stage.guard'),
      detail: guardApplied
        ? resolveStageDetail(resultSummary, currentTransition ? `${currentTransition} (${currentState})` : summaryState, t)
        : t('chat.toolWorkflow.compaction.stage.notNeeded'),
      state: guardState
    },
    {
      key: 'resume',
      label: t('chat.toolWorkflow.compaction.stage.resume'),
      detail: isRunning
        ? t('chat.toolWorkflow.compaction.stage.pending')
        : resolveStageDetail(resultSummary, resultLine, t),
      state: resumeState
    }
  ];

  const copyBlocks = [...lines];
  outputs.forEach((output) => {
    if (copyBlocks.length > 0) {
      copyBlocks.push('');
    }
    copyBlocks.push(`${output.title}:`);
    copyBlocks.push(output.body);
  });
  const copyBody = copyBlocks.join('\n').trim();

  return {
    summaryTitle,
    summaryNote: note,
    summaryNoteTone: noteTone,
    resultSummary,
    resultBody: lines.join('\n'),
    copyBody,
    view: {
      headline: summaryTitle,
      description: resultSummary,
      stages,
      metrics,
      details,
      outputs,
      outputEmpty,
      usageBar,
      failure
    }
  };
};
