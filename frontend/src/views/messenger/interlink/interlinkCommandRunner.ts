// AI生成
/**
 * 远程命令的执行流（方案 §4.3 状态机 / §6.4 按需拉取 / §7.3 审批）。
 *
 * 蜂巢侧只走 REST：`POST /interlink/commands` 发起 → 轮询 `GET /interlink/commands/{id}`
 * 直到终态（实时链路是 WS，本期前端不订阅命令帧）→ 大文件再走 blob 端点。
 * 所有循环都有节奏、超时与 AbortSignal 边界，不做无界重试。
 */

import {
  cancelInterlinkCommand,
  fetchInterlinkCommand,
  fetchInterlinkCommandBlob,
  submitInterlinkCommand
} from '@/api/interlink';
import type {
  InterlinkCommandKind,
  InterlinkCommandRecord,
  InterlinkCommandSubmitResult
} from '@/api/interlink';
import { resolveApiError } from '@/utils/apiError';
import {
  REMOTE_COMMAND_APPROVAL_TIMEOUT_MS,
  REMOTE_COMMAND_POLL_INTERVAL_MS,
  REMOTE_COMMAND_TIMEOUT_MS,
  REMOTE_READ_MAX_BYTES,
  isSessionKind
} from './interlinkNodeModel';

const TERMINAL_STATUSES = new Set(['succeeded', 'failed', 'canceled', 'timeout']);

export const isTerminalCommandStatus = (status: unknown): boolean =>
  TERMINAL_STATUSES.has(String(status || '').trim().toLowerCase());

export type InterlinkCommandRunResult = {
  submit: InterlinkCommandSubmitResult | null;
  record: InterlinkCommandRecord | null;
  status: string;
  approvalState: string;
  errorCode: string;
  errorSummary: string;
  result: Record<string, unknown> | null;
  /** 前端侧超时（命令仍在服务端台账里，可稍后再查）。 */
  timedOut: boolean;
};

const sleep = (ms: number, signal?: AbortSignal): Promise<void> =>
  new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException('aborted', 'AbortError'));
      return;
    }
    const timer = setTimeout(() => {
      signal?.removeEventListener('abort', onAbort);
      resolve();
    }, ms);
    const onAbort = (): void => {
      clearTimeout(timer);
      reject(new DOMException('aborted', 'AbortError'));
    };
    signal?.addEventListener('abort', onAbort, { once: true });
  });

const failedOutcome = (
  submit: InterlinkCommandSubmitResult | null,
  record: InterlinkCommandRecord | null,
  message: string,
  timedOut = false
): InterlinkCommandRunResult => ({
  submit,
  record,
  status: record?.status || submit?.status || 'failed',
  approvalState: record?.approval_state || submit?.approval_state || 'none',
  errorCode: record?.error_code || (timedOut ? 'CLIENT_TIMEOUT' : 'COMMAND_FAILED'),
  errorSummary: record?.error_summary || message,
  result: record?.result || null,
  timedOut
});

/**
 * 发起并轮询一条远程命令。`onState` 用于把 `issued/queued/acked/running` 与审批
 * `pending` 这些中间态实时投影到 UI（§7.3 需要显式展示审批中并可取消）。
 */
export const runInterlinkCommand = async (options: {
  to: string;
  kind: InterlinkCommandKind | string;
  args?: Record<string, unknown>;
  signal?: AbortSignal;
  commandId?: string;
  timeoutMs?: number;
  onState?: (state: { submit: InterlinkCommandSubmitResult; record: InterlinkCommandRecord | null }) => void;
}): Promise<InterlinkCommandRunResult> => {
  let submit: InterlinkCommandSubmitResult | null = null;
  try {
    submit = await submitInterlinkCommand({
      to: options.to,
      kind: options.kind,
      args: options.args || {},
      command_id: options.commandId
    });
  } catch (error) {
    return failedOutcome(null, null, resolveApiError(error, '').message || '');
  }

  // 会话类命令默认要人在回路审批（§7.1），审批超时是 120s，前端等待窗口跟着放宽。
  const timeoutMs =
    Number(options.timeoutMs) ||
    (isSessionKind(options.kind)
      ? REMOTE_COMMAND_APPROVAL_TIMEOUT_MS
      : REMOTE_COMMAND_TIMEOUT_MS);
  const deadline = Date.now() + timeoutMs;
  let lastRecord: InterlinkCommandRecord | null = null;

  try {
    while (Date.now() < deadline) {
      const record = await fetchInterlinkCommand(submit.command_id, { signal: options.signal });
      lastRecord = record;
      options.onState?.({ submit, record });
      if (isTerminalCommandStatus(record.status)) {
        return {
          submit,
          record,
          status: record.status,
          approvalState: record.approval_state,
          errorCode: record.error_code,
          errorSummary: record.error_summary,
          result: record.result,
          timedOut: false
        };
      }
      await sleep(REMOTE_COMMAND_POLL_INTERVAL_MS, options.signal);
    }
  } catch (error) {
    if ((error as { name?: string })?.name === 'AbortError') {
      return failedOutcome(submit, lastRecord, '', false);
    }
    return failedOutcome(submit, lastRecord, resolveApiError(error, '').message || '');
  }

  return failedOutcome(submit, lastRecord, '', true);
};

export const cancelInterlinkCommandRun = async (commandId: string): Promise<boolean> => {
  if (!commandId) return false;
  try {
    return await cancelInterlinkCommand(commandId);
  } catch {
    return false;
  }
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

/** 契约里小文件内联在 command_result（base64 ≤1MB），字段名兼容几种常见写法。 */
const pickInlineBase64 = (result: Record<string, unknown> | null): string => {
  const record = asRecord(result);
  const keys = ['content_base64', 'base64', 'data_base64', 'binary_base64'];
  for (const key of keys) {
    const value = record[key];
    if (typeof value === 'string' && value) return value;
  }
  return '';
};

const pickInlineText = (result: Record<string, unknown> | null): string => {
  const record = asRecord(result);
  const value = record.content ?? record.text;
  return typeof value === 'string' ? value : '';
};

const decodeBase64ToBytes = (value: string): Uint8Array | null => {
  try {
    const binary = atob(value.replace(/\s/g, ''));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return bytes;
  } catch {
    return null;
  }
};

export type RemoteFilePayload = {
  /** 文本内容（已解码）；二进制文件为空串。 */
  text: string;
  /** 供预览用的二进制（文本时为 null，避免多复制一份大对象）。 */
  bytes: Uint8Array | null;
  size: number;
  truncated: boolean;
  /** 内联还是 blob 端点拿到的小节，用于文案与排查。 */
  source: 'inline' | 'blob' | 'none';
};

/**
 * §6.4 云读本：`workspace.read` → 轮询 → 内联结果或 `GET /commands/{id}/blob`。
 * 失败/过大时返回 null，由调用方降级为可读提示，绝不留下空白面板。
 */
export const readRemoteFile = async (options: {
  to: string;
  path: string;
  maxBytes?: number;
  signal?: AbortSignal;
}): Promise<{ payload: RemoteFilePayload | null; failure: string; errorCode: string }> => {
  const outcome = await runInterlinkCommand({
    to: options.to,
    kind: 'workspace.read',
    args: {
      path: options.path,
      max_bytes: Math.max(1024, Number(options.maxBytes) || REMOTE_READ_MAX_BYTES)
    },
    signal: options.signal
  });

  if (outcome.status !== 'succeeded') {
    return {
      payload: null,
      failure: outcome.errorSummary || outcome.errorCode || '',
      errorCode: outcome.errorCode || outcome.status
    };
  }

  const result = outcome.result;
  const size = Number(asRecord(result).size) || 0;
  const truncated = asRecord(result).truncated === true;
  const text = pickInlineText(result);
  const base64 = pickInlineBase64(result);

  if (text) {
    return { payload: { text, bytes: null, size, truncated, source: 'inline' }, failure: '', errorCode: '' };
  }
  if (base64) {
    const bytes = decodeBase64ToBytes(base64);
    if (bytes) {
      return {
        payload: { text: decodeBytesToText(bytes), bytes, size, truncated, source: 'inline' },
        failure: '',
        errorCode: ''
      };
    }
  }

  if (!outcome.submit?.command_id) {
    return { payload: null, failure: '', errorCode: 'NO_COMMAND_ID' };
  }

  try {
    const blob = await fetchInterlinkCommandBlob(outcome.submit.command_id, { signal: options.signal });
    const buffer = new Uint8Array(await blob.arrayBuffer());
    return {
      payload: {
        text: decodeBytesToText(buffer),
        bytes: buffer,
        size: blob.size || buffer.length,
        truncated,
        source: 'blob'
      },
      failure: '',
      errorCode: ''
    };
  } catch (error) {
    return {
      payload: null,
      failure: resolveApiError(error, '').message || '',
      errorCode: 'BLOB_FETCH_FAILED'
    };
  }
};

/** 文本用 UTF-8 解码；非文本（含 NUL/高比例非法字节）返回空串，交给二进制预览路径。 */
const decodeBytesToText = (bytes: Uint8Array): string => {
  if (!bytes.length) return '';
  try {
    const decoded = new TextDecoder('utf-8', { fatal: false }).decode(bytes);
    // 明显的二进制内容（NUL 或大量替换符）不进文本渲染。
    const nulIndex = decoded.indexOf('\u0000');
    if (nulIndex >= 0 && nulIndex < Math.min(decoded.length, 512)) return '';
    const replacementCount = decoded.split('\ufffd').length - 1;
    if (replacementCount > Math.max(8, Math.floor(decoded.length / 200))) return '';
    return decoded;
  } catch {
    return '';
  }
};

export const listRemoteDirectory = async (options: {
  to: string;
  path: string;
  offset?: number;
  limit?: number;
  signal?: AbortSignal;
}): Promise<{ entries: Record<string, unknown>[]; total: number; failure: string }> => {
  const outcome = await runInterlinkCommand({
    to: options.to,
    kind: 'workspace.list',
    args: {
      path: options.path,
      offset: Math.max(0, Number(options.offset) || 0),
      limit: Math.max(1, Number(options.limit) || 200)
    },
    signal: options.signal
  });
  if (outcome.status !== 'succeeded') {
    return { entries: [], total: 0, failure: outcome.errorSummary || outcome.errorCode || '' };
  }
  const result = asRecord(outcome.result);
  const entries = Array.isArray(result.entries) ? (result.entries as unknown[]) : [];
  return {
    entries: entries.map((item) => asRecord(item)),
    total: Number(result.total) || entries.length,
    failure: ''
  };
};
