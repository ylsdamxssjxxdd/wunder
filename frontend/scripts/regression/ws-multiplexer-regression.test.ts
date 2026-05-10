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

test('ws multiplexer resolves and clears request-scoped queued ack by default', async () => {
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

    assert.deepEqual(events, ['queued:{"queued":true}']);
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
