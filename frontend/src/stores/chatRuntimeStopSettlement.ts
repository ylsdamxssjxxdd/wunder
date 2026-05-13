type AbortableControllerLike = {
  abort?: () => void;
  signal?: {
    aborted?: boolean;
  };
} | null | undefined;

type RuntimeLike = Record<string, any>;

const abortController = (controller: AbortableControllerLike): void => {
  if (!controller || controller.signal?.aborted === true || typeof controller.abort !== 'function') {
    return;
  }
  controller.abort();
};

const clearTimer = (timer: unknown): void => {
  if (timer) {
    clearTimeout(timer as ReturnType<typeof setTimeout>);
  }
};

export const settleStoppedRuntimeLocalState = (
  runtime: RuntimeLike | null | undefined,
  options: { abortReason?: string } = {}
): boolean => {
  if (!runtime) return false;
  const abortReason = String(options.abortReason || 'user_stop').trim() || 'user_stop';
  if (runtime.sendController) {
    runtime.sendAbortReason = runtime.sendAbortReason || abortReason;
    abortController(runtime.sendController);
  }
  if (runtime.resumeController) {
    runtime.resumeAbortReason = runtime.resumeAbortReason || abortReason;
    abortController(runtime.resumeController);
  }
  abortController(runtime.compactController);
  abortController(runtime.watchController);
  clearTimer(runtime.watchdogTimer);
  clearTimer(runtime.watchReconcileTimer);
  clearTimer(runtime.slowClientResumeTimer);

  runtime.sendController = null;
  runtime.resumeController = null;
  runtime.compactController = null;
  runtime.watchController = null;
  runtime.sendRequestId = null;
  runtime.resumeRequestId = null;
  runtime.watchRequestId = null;
  runtime.sendStartedAt = 0;
  runtime.sendLastEventAt = 0;
  runtime.resumeStartedAt = 0;
  runtime.resumeLastEventAt = 0;
  runtime.watchActiveRoundCount = 0;
  runtime.watchLastEventAt = 0;
  runtime.watchReconcileAt = 0;
  runtime.watchdogTimer = null;
  runtime.watchdogBusy = false;
  runtime.watchReconcileTimer = null;
  runtime.slowClientResumeTimer = null;
  runtime.slowClientResumeAfterEventId = 0;
  runtime.streamLifecycle = 'idle';
  runtime.stopRequested = false;
  runtime.sendAbortReason = '';
  runtime.resumeAbortReason = '';
  runtime.activeTurnId = '';
  runtime.pendingApprovalIds = [];
  runtime.pendingApprovalCount = 0;
  runtime.waitingForUserInput = false;
  runtime.loaded = true;
  runtime.threadStatus = 'idle';
  return true;
};
