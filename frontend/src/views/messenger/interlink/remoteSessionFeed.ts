// AI生成
/**
 * 远程会话订阅通道（方案 §7.4）。
 *
 * `WS /wunder/interlink/remote_ws?target=device:<id>&thread=<local_thread_id>`，
 * 子协议 `wunder-interlink-remote`，服务端是隧道事件的转发器：首包快照，随后增量。
 *
 * 边界：
 * - 重连次数有硬上限（不做无限退避，避免后台僵尸连接）；耗尽后交给用户手动重试；
 * - 只处理 JSON 文本帧，其它帧丢弃并计入 droppedFrames，不静默改变视图；
 * - `stop()` 之后所有回调被切断，组件卸载后不会再往响应式状态里写数据。
 */

import { openInterlinkRemoteSocket, parseInterlinkRemoteFrame } from '@/api/interlink';
import type { InterlinkRemoteFrame } from '@/api/interlink';

export type RemoteFeedStatus = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'closed' | 'error';

export type RemoteSessionFeed = {
  start: () => void;
  stop: () => void;
  status: () => RemoteFeedStatus;
};

const MAX_RECONNECTS = 2;
const RECONNECT_BASE_DELAY_MS = 2_000;

export const createRemoteSessionFeed = (options: {
  target: string;
  threadId: string;
  onFrame: (frame: InterlinkRemoteFrame) => void;
  onStatus: (status: RemoteFeedStatus, detail?: string) => void;
  maxReconnects?: number;
}): RemoteSessionFeed => {
  const maxReconnects = Math.max(0, Number(options.maxReconnects) || MAX_RECONNECTS);
  let socket: WebSocket | null = null;
  let status: RemoteFeedStatus = 'idle';
  let stopped = false;
  let attempts = 0;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  const cleanupSocket = (): void => {
    if (!socket) return;
    const current = socket;
    socket = null;
    current.onopen = null;
    current.onmessage = null;
    current.onerror = null;
    current.onclose = null;
    if (current.readyState === WebSocket.OPEN || current.readyState === WebSocket.CONNECTING) {
      try {
        current.close();
      } catch {
        // 关闭失败不影响解绑
      }
    }
  };

  const scheduleReconnect = (detail: string): void => {
    if (stopped) return;
    if (attempts >= maxReconnects) {
      status = 'error';
      options.onStatus('error', detail);
      return;
    }
    attempts += 1;
    status = 'reconnecting';
    options.onStatus('reconnecting', detail);
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, RECONNECT_BASE_DELAY_MS * attempts);
  };

  const connect = (): void => {
    if (stopped) return;
    if (!options.target || !options.threadId) {
      status = 'error';
      options.onStatus('error', 'MISSING_TARGET');
      return;
    }
    cleanupSocket();
    status = attempts ? 'reconnecting' : 'connecting';
    options.onStatus(status);
    try {
      socket = openInterlinkRemoteSocket(options.target, options.threadId);
    } catch {
      status = 'error';
      options.onStatus('error', 'SOCKET_FAILED');
      return;
    }
    const current = socket;
    current.onopen = () => {
      if (stopped || current !== socket) return;
      attempts = 0;
      status = 'open';
      options.onStatus('open');
    };
    current.onmessage = (event: MessageEvent) => {
      if (stopped || current !== socket) return;
      const frame = parseInterlinkRemoteFrame(event.data);
      if (!frame) {
        options.onStatus(status, 'UNRECOGNIZED_FRAME');
        return;
      }
      options.onFrame(frame);
    };
    current.onerror = () => {
      if (stopped || current !== socket) return;
      scheduleReconnect('SOCKET_ERROR');
    };
    current.onclose = (event: CloseEvent) => {
      if (stopped || current !== socket) return;
      // 服务端主动 close（§7.4 的 close 帧之后断开）视为正常收尾，不再重连。
      if (event.code === 1000) {
        status = 'closed';
        options.onStatus('closed');
        return;
      }
      scheduleReconnect('SOCKET_CLOSED');
    };
  };

  const stop = (): void => {
    stopped = true;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    cleanupSocket();
    status = 'closed';
  };

  return {
    start: connect,
    stop,
    status: () => status
  };
};
