import test from 'node:test';
import assert from 'node:assert/strict';

type MessageHandler = ((event: MessageEvent<string>) => void) | null;
type GlobalWithBrowserMocks = typeof globalThis & {
  window?: unknown;
  WebSocket?: unknown;
};

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState = FakeWebSocket.CONNECTING;
  sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: MessageHandler = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.();
  }

  send(message: string) {
    this.sent.push(message);
  }

  close() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.();
  }

  emit(payload: unknown) {
    this.onmessage?.({ data: JSON.stringify(payload) } as MessageEvent<string>);
  }
}

const installWebSocketMock = () => {
  const globalRef = globalThis as GlobalWithBrowserMocks;
  const previousWebSocket = globalRef.WebSocket;
  const previousWindow = globalRef.window;
  globalRef.WebSocket = FakeWebSocket;
  globalRef.window = {
    __WUNDER_DESKTOP_RUNTIME__: null,
    location: { origin: 'http://localhost' }
  };
  return () => {
    globalRef.WebSocket = previousWebSocket;
    globalRef.window = previousWindow;
  };
};

test('ws multiplexer resolves queued ack but keeps request-scoped events by default', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-1',
      resolveOnQueued: true,
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-1' },
      onEvent: (eventType, data) => {
        events.push(`${eventType}:${data}`);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-1',
      payload: { event: 'queued', data: { queued: true } }
    });

    await requestPromise;

    socket.emit({
      type: 'event',
      request_id: 'request-1',
      payload: { event: 'delta', data: { text: 'after queued' } }
    });
    socket.emit({
      type: 'event',
      request_id: 'request-1',
      payload: { event: 'final', data: { done: true } }
    });

    await sleep(0);

    assert.deepEqual(events, [
      'queued:{"queued":true}',
      'delta:{"text":"after queued"}',
      'final:{"done":true}'
    ]);
    assert.equal(socket.sent.length, 1);
  } finally {
    restore();
  }
});

test('ws multiplexer can keep pending entry alive after queued ack resolve', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-2',
      resolveOnQueued: true,
      keepPendingAfterQueuedAck: true,
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-2' },
      onEvent: (eventType, data) => {
        events.push(`${eventType}:${data}`);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-2',
      payload: { event: 'queued', data: { queued: true } }
    });

    await requestPromise;
    socket.emit({
      type: 'event',
      request_id: 'request-2',
      payload: { event: 'delta', data: { text: 'after queued' } }
    });
    socket.emit({
      type: 'event',
      request_id: 'request-2',
      payload: { event: 'final', data: { done: true } }
    });

    await sleep(0);

    assert.deepEqual(events, [
      'queued:{"queued":true}',
      'delta:{"text":"after queued"}',
      'final:{"done":true}'
    ]);
    assert.equal(socket.sent.length, 1);
  } finally {
    restore();
  }
});

test('ws multiplexer removes kept queued request on explicit cancel', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-3',
      resolveOnQueued: true,
      keepPendingAfterQueuedAck: true,
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-3' },
      onEvent: (eventType) => {
        events.push(eventType);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-3',
      payload: { event: 'queued', data: { queued: true } }
    });

    await requestPromise;
    client.sendCancel('request-3', 'session-3');

    socket.emit({
      type: 'event',
      request_id: 'request-3',
      payload: { event: 'delta', data: { text: 'ignored' } }
    });

    await sleep(0);

    assert.deepEqual(events, ['queued']);
    assert.deepEqual(JSON.parse(socket.sent[1]), {
      type: 'cancel',
      request_id: 'request-3',
      session_id: 'session-3'
    });
  } finally {
    restore();
  }
});

test('ws multiplexer sends session stop cancel without a request id', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const notifyPromise = client.notify({
      type: 'cancel',
      session_id: 'session-stop',
      payload: {
        session_id: 'session-stop',
        cancel_source: 'user_stop'
      }
    });

    socket.open();
    await notifyPromise;

    assert.deepEqual(JSON.parse(socket.sent[0]), {
      type: 'cancel',
      session_id: 'session-stop',
      payload: {
        session_id: 'session-stop',
        cancel_source: 'user_stop'
      }
    });
  } finally {
    restore();
  }
});

test('ws multiplexer releases kept queued request after grace timeout', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-queued-timeout',
      resolveOnQueued: true,
      keepPendingAfterQueuedAck: true,
      queuedAckGraceMs: 5,
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-queued-timeout' },
      onEvent: (eventType) => {
        events.push(eventType);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-queued-timeout',
      payload: { event: 'queued', data: { queued: true } }
    });

    await requestPromise;
    await sleep(20);
    socket.emit({
      type: 'event',
      request_id: 'request-queued-timeout',
      payload: { event: 'delta', data: { text: 'late ignored' } }
    });

    await sleep(0);

    assert.deepEqual(events, ['queued']);
  } finally {
    restore();
  }
});

test('ws multiplexer does not send backend cancel when local abort opts out', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const controller = new AbortController();
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-abort-local',
      sessionId: 'session-abort-local',
      signal: controller.signal,
      cancelOnAbort: false,
      message: { type: 'run', request_id: 'request-abort-local' },
      onEvent: () => {}
    });

    socket.open();
    await sleep(0);
    controller.abort();

    await assert.rejects(requestPromise, { name: 'AbortError' });
    assert.deepEqual(
      socket.sent.map((item) => JSON.parse(item)),
      [{ type: 'run', request_id: 'request-abort-local' }]
    );
  } finally {
    restore();
  }
});

test('ws multiplexer sends client_abort cancel source on default local abort', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const controller = new AbortController();
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-abort-cancel',
      sessionId: 'session-abort-cancel',
      signal: controller.signal,
      message: { type: 'run', request_id: 'request-abort-cancel' },
      onEvent: () => {}
    });

    socket.open();
    await sleep(0);
    controller.abort();

    await assert.rejects(requestPromise, { name: 'AbortError' });
    assert.deepEqual(
      socket.sent.map((item) => JSON.parse(item)),
      [
        { type: 'run', request_id: 'request-abort-cancel' },
        {
          type: 'cancel',
          request_id: 'request-abort-cancel',
          session_id: 'session-abort-cancel',
          payload: { cancel_source: 'client_abort' }
        }
      ]
    );
  } finally {
    restore();
  }
});

test('ws multiplexer resolves request on terminal thread_status runtime event', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-terminal-thread-status',
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-terminal-thread-status' },
      onEvent: (eventType, data) => {
        events.push(`${eventType}:${data}`);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-terminal-thread-status',
      payload: {
        event: 'thread_status',
        data: { thread_status: 'idle' }
      }
    });

    await requestPromise;
    await sleep(0);

    assert.deepEqual(events, ['thread_status:{"thread_status":"idle"}']);
  } finally {
    restore();
  }
});

test('ws multiplexer resolves request on thread_closed event', async () => {
  const restore = installWebSocketMock();
  try {
    const { createWsMultiplexer } = await import('../../src/utils/ws');
    const socket = new FakeWebSocket();
    const events: string[] = [];
    const client = createWsMultiplexer(() => socket as unknown as WebSocket, {
      idleTimeoutMs: 0,
      pingIntervalMs: 0
    });

    const requestPromise = client.request({
      requestId: 'request-thread-closed',
      closeOnFinal: true,
      message: { type: 'run', request_id: 'request-thread-closed' },
      onEvent: (eventType, data) => {
        events.push(`${eventType}:${data}`);
      }
    });

    socket.open();
    socket.emit({
      type: 'event',
      request_id: 'request-thread-closed',
      payload: {
        event: 'thread_closed',
        data: { status: 'runtime_unloaded' }
      }
    });

    await requestPromise;
    await sleep(0);

    assert.deepEqual(events, ['thread_closed:{"status":"runtime_unloaded"}']);
  } finally {
    restore();
  }
});
