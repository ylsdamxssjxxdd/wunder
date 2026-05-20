import type {
  RawToolRun,
  WorkflowItem
} from './toolWorkflowRunModel';

type UnknownObject = Record<string, unknown>;

const EXECUTE_COMMAND_TOOL = 'execute_command';

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const parseDetailObject = (detail: unknown): UnknownObject | null => {
  if (typeof detail !== 'string') return null;
  const trimmed = detail.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  try {
    return asObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim();
    }
  }
  return '';
};

const normalizeDetailText = (detail: unknown): string =>
  String(detail || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();

const stringifyDebugPayload = (payload: unknown): string => {
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return '';
  }
};

const isExecuteCommandTool = (toolName: unknown): boolean => {
  const normalized = String(toolName || '').trim().toLowerCase();
  return normalized === EXECUTE_COMMAND_TOOL || normalized.includes('执行命令');
};

const parseArgumentObject = (candidate: unknown): UnknownObject | null => {
  const directObject = asObject(candidate);
  if (directObject) return directObject;
  if (typeof candidate === 'string') {
    return parseDetailObject(candidate);
  }
  return null;
};

const extractExplicitCallArgs = (item: WorkflowItem | null): UnknownObject | null => {
  if (!item) return null;
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject) return null;
  const nestedFunction = asObject(detailObject.function);
  const candidates: unknown[] = [
    detailObject.args,
    detailObject.arguments,
    nestedFunction?.arguments
  ];
  for (const candidate of candidates) {
    const parsed = parseArgumentObject(candidate);
    if (parsed) return parsed;
  }
  return null;
};

export const extractWorkflowCallArgs = (item: WorkflowItem | null): UnknownObject | null => {
  if (!item) return null;
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject) return null;
  return extractExplicitCallArgs(item) || detailObject;
};

const isRuntimeCommandSessionPayload = (payload: UnknownObject): boolean => {
  if (!pickString(payload.command_session_id, payload.commandSessionId)) return false;
  return [
    'exit_code',
    'exitCode',
    'stdout_tail',
    'stdoutTail',
    'stderr_tail',
    'stderrTail',
    'pty_tail',
    'ptyTail',
    'started_at',
    'startedAt',
    'updated_at',
    'updatedAt',
    'ended_at',
    'endedAt',
    'session_id',
    'sessionId',
    'workspace_id',
    'workspaceId',
    'seq',
    'status'
  ].some((key) => Object.prototype.hasOwnProperty.call(payload, key));
};

const extractBareCommandArgs = (item: WorkflowItem | null): UnknownObject | null => {
  if (!item) return null;
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject || isRuntimeCommandSessionPayload(detailObject)) return null;
  const command = pickString(
    detailObject.content,
    detailObject.command,
    detailObject.cmd,
    detailObject.input,
    detailObject.raw,
    detailObject.script
  );
  if (!command) return null;
  const args: UnknownObject = {};
  for (const key of [
    'content',
    'command',
    'cmd',
    'input',
    'raw',
    'script',
    'workdir',
    'cwd',
    'timeout_s',
    'timeout',
    'dry_run'
  ]) {
    if (Object.prototype.hasOwnProperty.call(detailObject, key)) {
      args[key] = detailObject[key];
    }
  }
  return Object.keys(args).length > 0 ? args : { content: command };
};

const extractCommandFromItem = (item: WorkflowItem | null): string => {
  if (!item) return '';
  const explicitArgs = extractExplicitCallArgs(item);
  if (explicitArgs) {
    return pickString(
      explicitArgs.content,
      explicitArgs.command,
      explicitArgs.cmd,
      explicitArgs.input,
      explicitArgs.raw,
      explicitArgs.script
    );
  }
  const bareArgs = extractBareCommandArgs(item);
  if (bareArgs) {
    return pickString(
      bareArgs.content,
      bareArgs.command,
      bareArgs.cmd,
      bareArgs.input,
      bareArgs.raw,
      bareArgs.script
    );
  }
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject) return '';
  return pickString(detailObject.command, detailObject.content);
};

const buildDebugText = (toolName: string, args: UnknownObject): string => {
  const normalized = stringifyDebugPayload({
    tool: toolName,
    arguments: args
  });
  return normalizeDetailText(normalized);
};

export const buildWorkflowToolCallDebugText = (entry: RawToolRun): string => {
  const toolName = entry.toolFunctionName || entry.toolName;
  if (isExecuteCommandTool(entry.toolName)) {
    const rawDetail = normalizeDetailText(
      entry.callItem?.toolCallRawDetail ?? entry.callItem?.tool_call_raw_detail
    );
    if (rawDetail) {
      return rawDetail;
    }
    const explicitArgs = extractExplicitCallArgs(entry.callItem);
    if (explicitArgs) {
      return buildDebugText(toolName, explicitArgs);
    }
    const bareArgs = extractBareCommandArgs(entry.callItem);
    if (bareArgs) {
      return buildDebugText(toolName, bareArgs);
    }
    const command = pickString(extractCommandFromItem(entry.callItem));
    if (command) {
      return buildDebugText(toolName, { content: command });
    }
    return '';
  }

  const callArgs = extractWorkflowCallArgs(entry.callItem);
  if (callArgs) {
    return buildDebugText(toolName, callArgs);
  }
  return normalizeDetailText(entry.callItem?.detail);
};
