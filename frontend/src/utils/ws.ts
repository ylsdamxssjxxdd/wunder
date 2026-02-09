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

const normalizeRequestId = (value) => {
  const cleaned = String(value || '').trim();
  return cleaned || '';
};

const buildEventText = (data) => (typeof data === 'string' ? data : JSON.stringify(data ?? {}));

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
        if (options.closeOnFinal && (eventType === 'final' || eventType === 'error')) {
          try {
            socket.close(1000, eventType === 'final' ? 'final' : 'error_event');
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

export const createWsMultiplexer = (createSocket, options = {}) => {
  const idleTimeoutMs = Number.isFinite(options.idleTimeoutMs) ? options.idleTimeoutMs : 30000;
  const connectTimeoutMs = Number.isFinite(options.connectTimeoutMs) ? options.connectTimeoutMs : 10000;
  const pending = new Map();
  let socket = null;
  let opened = false;
  let connectPromise = null;
  let connectResolve = null;
  let connectReject = null;
  let connectTimer = null;
  let idleTimer = null;

  const clearConnectTimer = () => {
    if (connectTimer) {
      clearTimeout(connectTimer);
      connectTimer = null;
    }
  };

  const clearIdleTimer = () => {
    if (idleTimer) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  };

  const scheduleIdleClose = () => {
    if (idleTimeoutMs <= 0 || pending.size > 0) {
      return;
    }
    clearIdleTimer();
    idleTimer = setTimeout(() => {
      if (pending.size > 0) return;
      try {
        socket?.close(1000, 'idle');
      } catch (error) {
        // ignore
      }
    }, idleTimeoutMs);
  };

  const cleanupSocket = () => {
    clearConnectTimer();
    clearIdleTimer();
    socket = null;
    opened = false;
    connectPromise = null;
    connectResolve = null;
    connectReject = null;
  };

  const cleanupRequest = (entry) => {
    if (entry?.signal && entry?.abortHandler) {
      entry.signal.removeEventListener('abort', entry.abortHandler);
    }
  };

  const resolveRequest = (requestId) => {
    const entry = pending.get(requestId);
    if (!entry) return;
    pending.delete(requestId);
    cleanupRequest(entry);
    entry.resolve();
    scheduleIdleClose();
  };

  const rejectRequest = (requestId, error) => {
    const entry = pending.get(requestId);
    if (!entry) return;
    pending.delete(requestId);
    cleanupRequest(entry);
    entry.reject(error);
    scheduleIdleClose();
  };

  const failAll = (error) => {
    if (pending.size === 0) {
      scheduleIdleClose();
      return;
    }
    [...pending.keys()].forEach((requestId) => rejectRequest(requestId, error));
  };

  const sendMessage = (message) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error('WebSocket not ready');
    }
    socket.send(JSON.stringify(message));
  };

  const sendCancel = (requestId, sessionId) => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    const payload = { type: 'cancel', request_id: requestId };
    if (sessionId) {
      payload.session_id = sessionId;
    }
    try {
      sendMessage(payload);
    } catch (error) {
      // ignore
    }
  };

  const handleMessage = (event) => {
    let payload = null;
    try {
      payload = JSON.parse(event.data);
    } catch (error) {
      return;
    }
    const type = String(payload?.type || '').toLowerCase();
    if (type === 'event') {
      const requestId = normalizeRequestId(payload?.request_id || payload?.requestId);
      if (!requestId) return;
      const entry = pending.get(requestId);
      if (!entry) return;
      const eventPayload = payload?.payload || {};
      const eventType = eventPayload?.event || 'message';
      const eventId = eventPayload?.id || '';
      const dataText = buildEventText(eventPayload?.data);
      entry.onEvent(eventType, dataText, eventId);
      if (entry.closeOnFinal && (eventType === 'final' || eventType === 'error')) {
        resolveRequest(requestId);
      }
      return;
    }
    if (type === 'error') {
      const message = payload?.payload?.message || t('chat.error.requestFailed');
      const err = normalizeError(message, opened ? 'stream' : 'connect');
      err.code = payload?.payload?.code;
      const requestId = normalizeRequestId(payload?.request_id || payload?.requestId);
      if (requestId && pending.has(requestId)) {
        rejectRequest(requestId, err);
      } else {
        failAll(err);
      }
    }
  };

  const bindSocket = () => {
    if (!socket) return;
    socket.onmessage = handleMessage;
    socket.onopen = () => {
      opened = true;
      clearConnectTimer();
      if (connectResolve) {
        connectResolve();
      }
      connectResolve = null;
      connectReject = null;
      scheduleIdleClose();
    };
    socket.onerror = () => {
      if (!opened) {
        const err = normalizeError(t('chat.error.requestFailed'), 'connect');
        if (connectReject) {
          connectReject(err);
        }
        cleanupSocket();
        return;
      }
      failAll(normalizeError(t('chat.error.requestFailed'), 'stream'));
    };
    socket.onclose = () => {
      const phase = opened ? 'stream' : 'connect';
      const err = normalizeError(t('chat.error.requestFailed'), phase);
      if (!opened && connectReject) {
        connectReject(err);
      }
      failAll(err);
      cleanupSocket();
    };
  };

  const ensureConnected = () => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      return Promise.resolve();
    }
    if (socket && socket.readyState === WebSocket.CONNECTING && connectPromise) {
      return connectPromise;
    }
    socket = createSocket();
    opened = false;
    connectPromise = new Promise((resolve, reject) => {
      connectResolve = resolve;
      connectReject = reject;
    });
    bindSocket();
    clearConnectTimer();
    connectTimer = setTimeout(() => {
      if (opened) return;
      const err = normalizeError(t('chat.error.requestFailed'), 'connect');
      try {
        socket?.close();
      } catch (error) {
        // ignore
      }
      if (connectReject) {
        connectReject(err);
      }
      cleanupSocket();
    }, connectTimeoutMs);
    return connectPromise;
  };

  const request = (payload) =>
    new Promise((resolve, reject) => {
      const requestId = normalizeRequestId(payload?.requestId);
      if (!requestId) {
        reject(new Error('request_id required'));
        return;
      }
      const entry = {
        onEvent: typeof payload?.onEvent === 'function' ? payload.onEvent : () => {},
        resolve,
        reject,
        closeOnFinal: payload?.closeOnFinal !== false,
        signal: payload?.signal,
        abortHandler: null
      };
      pending.set(requestId, entry);
      clearIdleTimer();
      const handleAbort = () => {
        if (!pending.has(requestId)) return;
        if (payload?.cancelOnAbort !== false) {
          sendCancel(requestId, payload?.sessionId);
        }
        const err = buildAbortError();
        err.phase = 'aborted';
        rejectRequest(requestId, err);
      };
      if (entry.signal) {
        if (entry.signal.aborted) {
          handleAbort();
          return;
        }
        entry.abortHandler = handleAbort;
        entry.signal.addEventListener('abort', handleAbort, { once: true });
      }
      ensureConnected()
        .then(() => {
          try {
            sendMessage(payload.message);
          } catch (error) {
            rejectRequest(requestId, normalizeError(error?.message, opened ? 'stream' : 'connect'));
          }
        })
        .catch((error) => {
          rejectRequest(requestId, error);
        });
    });

  const close = (code, reason) => {
    if (!socket) return;
    try {
      socket.close(code, reason);
    } catch (error) {
      // ignore
    }
  };

  const isOpen = () => Boolean(socket && socket.readyState === WebSocket.OPEN);

  return {
    request,
    close,
    sendCancel,
    isOpen
  };
};
