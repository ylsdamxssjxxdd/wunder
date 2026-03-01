import { t } from '@/i18n';

type WsError = Error & {
  phase?: string;
  code?: unknown;
};

type StreamEventHandler = (eventType: string, data: string, eventId: string) => void;

type ConsumeWsStreamOptions = {
  timeoutMs?: number;
  signal?: AbortSignal;
  onOpen?: () => void;
  closeOnFinal?: boolean;
};

type MultiplexerOptions = {
  idleTimeoutMs?: number;
  connectTimeoutMs?: number;
};

type WsRequestPayload = {
  requestId?: string;
  onEvent?: StreamEventHandler;
  closeOnFinal?: boolean;
  signal?: AbortSignal;
  cancelOnAbort?: boolean;
  sessionId?: string;
  message: unknown;
};

type PendingEntry = {
  onEvent: StreamEventHandler;
  resolve: () => void;
  reject: (error: unknown) => void;
  closeOnFinal: boolean;
  signal?: AbortSignal;
  abortHandler: (() => void) | null;
};

type WsMessagePayload = Record<string, unknown>;

const buildAbortError = (): WsError => {
  try {
    return new DOMException('Aborted', 'AbortError') as WsError;
  } catch {
    const err = new Error('Aborted') as WsError;
    err.name = 'AbortError';
    return err;
  }
};

const normalizeError = (message: string, phase?: string): WsError => {
  const err = new Error(message || t('chat.error.requestFailed')) as WsError;
  if (phase) {
    err.phase = phase;
  }
  return err;
};

const normalizeRequestId = (value: unknown): string => {
  const cleaned = String(value || '').trim();
  return cleaned || '';
};

const buildEventText = (data: unknown): string =>
  typeof data === 'string' ? data : JSON.stringify(data ?? {});

const asPayloadRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

export const consumeWsStream = (
  socket: WebSocket,
  onEvent: StreamEventHandler,
  options: ConsumeWsStreamOptions = {}
): Promise<void> =>
  new Promise<void>((resolve, reject) => {
    let opened = false;
    let settled = false;
    const timeoutMs = Number.isFinite(options.timeoutMs) ? Number(options.timeoutMs) : 10000;
    let timeout: ReturnType<typeof setTimeout> | null = null;

    const cleanup = (): void => {
      if (timeout) {
        clearTimeout(timeout);
        timeout = null;
      }
      if (options.signal) {
        options.signal.removeEventListener('abort', onAbort);
      }
    };

    const settleReject = (error: unknown): void => {
      if (settled) return;
      settled = true;
      cleanup();
      reject(error);
    };

    const settleResolve = (): void => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve();
    };

    const onAbort = (): void => {
      try {
        socket.close(1000, 'aborted');
      } catch {
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
      } catch {
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

    socket.onmessage = (event: MessageEvent<string>) => {
      let payload: WsMessagePayload | null = null;
      try {
        payload = JSON.parse(event.data);
      } catch {
        return;
      }
      const type = String(payload?.type || '').toLowerCase();
      if (type === 'event') {
        const eventPayload = asPayloadRecord(payload?.payload);
        const eventType = String(eventPayload.event || 'message');
        const eventId = String(eventPayload.id || '');
        const dataText = buildEventText(eventPayload.data);
        onEvent(eventType, dataText, eventId);
        if (options.closeOnFinal && (eventType === 'final' || eventType === 'error')) {
          try {
            socket.close(1000, eventType === 'final' ? 'final' : 'error_event');
          } catch {
            // ignore
          }
        }
        return;
      }
      if (type === 'error') {
        const errorPayload = asPayloadRecord(payload?.payload);
        const message = String(errorPayload.message || t('chat.error.requestFailed'));
        const err = normalizeError(message, opened ? 'stream' : 'connect');
        err.code = errorPayload.code;
        try {
          socket.close(1000, 'error');
        } catch {
          // ignore
        }
        settleReject(err);
      }
    };

    socket.onerror = () => {
      const phase = opened ? 'stream' : 'connect';
      try {
        socket.close();
      } catch {
        // ignore
      }
      settleReject(normalizeError(t('chat.error.requestFailed'), phase));
    };

    socket.onclose = () => {
      settleResolve();
    };
  });

export const createWsMultiplexer = (
  createSocket: () => WebSocket,
  options: MultiplexerOptions = {}
) => {
  const idleTimeoutMs = Number.isFinite(options.idleTimeoutMs) ? Number(options.idleTimeoutMs) : 30000;
  const connectTimeoutMs = Number.isFinite(options.connectTimeoutMs)
    ? Number(options.connectTimeoutMs)
    : 10000;
  const pending = new Map<string, PendingEntry>();
  let socket: WebSocket | null = null;
  let opened = false;
  let connectPromise: Promise<void> | null = null;
  let connectResolve: (() => void) | null = null;
  let connectReject: ((error: unknown) => void) | null = null;
  let connectTimer: ReturnType<typeof setTimeout> | null = null;
  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  const clearConnectTimer = (): void => {
    if (connectTimer) {
      clearTimeout(connectTimer);
      connectTimer = null;
    }
  };

  const clearIdleTimer = (): void => {
    if (idleTimer) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  };

  const scheduleIdleClose = (): void => {
    if (idleTimeoutMs <= 0 || pending.size > 0) {
      return;
    }
    clearIdleTimer();
    idleTimer = setTimeout(() => {
      if (pending.size > 0) return;
      try {
        socket?.close(1000, 'idle');
      } catch {
        // ignore
      }
    }, idleTimeoutMs);
  };

  const cleanupSocket = (): void => {
    clearConnectTimer();
    clearIdleTimer();
    socket = null;
    opened = false;
    connectPromise = null;
    connectResolve = null;
    connectReject = null;
  };

  const cleanupRequest = (entry: PendingEntry | undefined): void => {
    if (entry?.signal && entry.abortHandler) {
      entry.signal.removeEventListener('abort', entry.abortHandler);
    }
  };

  const resolveRequest = (requestId: string): void => {
    const entry = pending.get(requestId);
    if (!entry) return;
    pending.delete(requestId);
    cleanupRequest(entry);
    entry.resolve();
    scheduleIdleClose();
  };

  const rejectRequest = (requestId: string, error: unknown): void => {
    const entry = pending.get(requestId);
    if (!entry) return;
    pending.delete(requestId);
    cleanupRequest(entry);
    entry.reject(error);
    scheduleIdleClose();
  };

  const failAll = (error: unknown): void => {
    if (pending.size === 0) {
      scheduleIdleClose();
      return;
    }
    [...pending.keys()].forEach((requestId) => rejectRequest(requestId, error));
  };

  const sendMessage = (message: unknown): void => {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error('WebSocket not ready');
    }
    socket.send(JSON.stringify(message));
  };

  const sendCancel = (requestId: string, sessionId?: string): void => {
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    const payload: WsMessagePayload = { type: 'cancel', request_id: requestId };
    if (sessionId) {
      payload.session_id = sessionId;
    }
    try {
      sendMessage(payload);
    } catch {
      // ignore
    }
  };

  const handleMessage = (event: MessageEvent<string>): void => {
    let payload: WsMessagePayload | null = null;
    try {
      payload = JSON.parse(event.data);
    } catch {
      return;
    }
    const type = String(payload?.type || '').toLowerCase();
    if (type === 'event') {
      const requestId = normalizeRequestId(payload?.request_id || payload?.requestId);
      if (!requestId) return;
      const entry = pending.get(requestId);
      if (!entry) return;
      const eventPayload = asPayloadRecord(payload?.payload);
      const eventType = String(eventPayload.event || 'message');
      const eventId = String(eventPayload.id || '');
      const dataText = buildEventText(eventPayload.data);
      entry.onEvent(eventType, dataText, eventId);
      if (entry.closeOnFinal && (eventType === 'final' || eventType === 'error')) {
        resolveRequest(requestId);
      }
      return;
    }
    if (type === 'error') {
      const errorPayload = asPayloadRecord(payload?.payload);
      const message = String(errorPayload.message || t('chat.error.requestFailed'));
      const err = normalizeError(message, opened ? 'stream' : 'connect');
      err.code = errorPayload.code;
      const requestId = normalizeRequestId(payload?.request_id || payload?.requestId);
      if (requestId && pending.has(requestId)) {
        rejectRequest(requestId, err);
      } else if (requestId) {
        // Ignore errors for fire-and-forget request ids to avoid interrupting active streams.
        return;
      } else {
        failAll(err);
      }
    }
  };

  const bindSocket = (): void => {
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

  const ensureConnected = (): Promise<void> => {
    if (socket && socket.readyState === WebSocket.OPEN) {
      return Promise.resolve();
    }
    if (socket && socket.readyState === WebSocket.CONNECTING && connectPromise) {
      return connectPromise;
    }
    socket = createSocket();
    opened = false;
    connectPromise = new Promise<void>((resolve, reject) => {
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
      } catch {
        // ignore
      }
      if (connectReject) {
        connectReject(err);
      }
      cleanupSocket();
    }, connectTimeoutMs);
    return connectPromise;
  };

  const request = (payload: WsRequestPayload): Promise<void> =>
    new Promise<void>((resolve, reject) => {
      const requestId = normalizeRequestId(payload?.requestId);
      if (!requestId) {
        reject(new Error('request_id required'));
        return;
      }
      const entry: PendingEntry = {
        onEvent: typeof payload?.onEvent === 'function' ? payload.onEvent : () => {},
        resolve,
        reject,
        closeOnFinal: payload?.closeOnFinal !== false,
        signal: payload?.signal,
        abortHandler: null
      };
      pending.set(requestId, entry);
      clearIdleTimer();
      const handleAbort = (): void => {
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
            const source = error as { message?: string };
            rejectRequest(requestId, normalizeError(source?.message || '', opened ? 'stream' : 'connect'));
          }
        })
        .catch((error) => {
          rejectRequest(requestId, error);
        });
    });

  const notify = (message: unknown): Promise<void> =>
    ensureConnected().then(() => {
      try {
        sendMessage(message);
      } catch (error) {
        const source = error as { message?: string };
        throw normalizeError(source?.message || '', opened ? 'stream' : 'connect');
      }
    });

  const close = (code?: number, reason?: string): void => {
    if (!socket) return;
    try {
      socket.close(code, reason);
    } catch {
      // ignore
    }
  };

  const isOpen = (): boolean => Boolean(socket && socket.readyState === WebSocket.OPEN);

  return {
    request,
    notify,
    close,
    sendCancel,
    isOpen
  };
};
