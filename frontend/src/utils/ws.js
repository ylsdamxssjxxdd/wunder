import { t } from '@/i18n';

const buildAbortError = () => {
  try {
    return new DOMException('Aborted', 'AbortError');
  } catch (error) {
    const err = new Error('Aborted');
    err.name = 'AbortError';
    return err;
  }
};

const normalizeError = (message, phase) => {
  const err = new Error(message || t('chat.error.requestFailed'));
  if (phase) {
    err.phase = phase;
  }
  return err;
};

export const consumeWsStream = (socket, onEvent, options = {}) =>
  new Promise((resolve, reject) => {
    let opened = false;
    let settled = false;
    const timeoutMs = Number.isFinite(options.timeoutMs) ? options.timeoutMs : 10000;
    let timeout = null;

    const cleanup = () => {
      if (timeout) {
        clearTimeout(timeout);
        timeout = null;
      }
      if (options.signal) {
        options.signal.removeEventListener('abort', onAbort);
      }
    };

    const settleReject = (error) => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };

    const settleResolve = () => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve();
    };

    const onAbort = () => {
      try {
        socket.close(1000, 'aborted');
      } catch (error) {
        // ignore
      }
      const err = buildAbortError();
      err.phase = 'aborted';
      settleReject(err);
    };

    if (options.signal) {
      if (options.signal.aborted) {
        onAbort();
        return;
      }
      options.signal.addEventListener('abort', onAbort, { once: true });
    }

    timeout = setTimeout(() => {
      if (opened || settled) return;
      try {
        socket.close();
      } catch (error) {
        // ignore
      }
      settleReject(normalizeError(t('chat.error.requestFailed'), 'connect'));
    }, timeoutMs);

    socket.onopen = () => {
      opened = true;
      if (timeout) {
        clearTimeout(timeout);
        timeout = null;
      }
      if (options.onOpen) {
        options.onOpen();
      }
    };

    socket.onmessage = (event) => {
      let payload = null;
      try {
        payload = JSON.parse(event.data);
      } catch (error) {
        return;
      }
      const type = String(payload?.type || '').toLowerCase();
      if (type === 'event') {
        const eventPayload = payload?.payload || {};
        const eventType = eventPayload?.event || 'message';
        const eventId = eventPayload?.id || '';
        const data = eventPayload?.data;
        const dataText = typeof data === 'string' ? data : JSON.stringify(data ?? {});
        onEvent(eventType, dataText, eventId);
        if (options.closeOnFinal && eventType === 'final') {
          try {
            socket.close(1000, 'final');
          } catch (error) {
            // ignore
          }
        }
        return;
      }
      if (type === 'error') {
        const message = payload?.payload?.message || t('chat.error.requestFailed');
        const err = normalizeError(message, opened ? 'stream' : 'connect');
        err.code = payload?.payload?.code;
        try {
          socket.close(1000, 'error');
        } catch (error) {
          // ignore
        }
        settleReject(err);
        return;
      }
    };

    socket.onerror = () => {
      const phase = opened ? 'stream' : 'connect';
      try {
        socket.close();
      } catch (error) {
        // ignore
      }
      settleReject(normalizeError(t('chat.error.requestFailed'), phase));
    };

    socket.onclose = () => {
      settleResolve();
    };
  });
