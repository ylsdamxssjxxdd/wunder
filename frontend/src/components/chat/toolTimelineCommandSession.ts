import type { RawToolRun } from './toolWorkflowRunModel';
import type { CommandSessionRuntimeEntry } from '@/stores/commandSessions';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import { extractToolResultDataObject, parseToolWorkflowResultObject } from './toolWorkflowResultPayload';

export type CommandInteractionKind = 'execute' | 'write' | 'poll';
export type CommandDisplayStatus = 'loading' | 'completed' | 'failed' | 'cancelled';

export const resolveCommandDisplayStatus = (
  fallback: CommandDisplayStatus,
  commandSettled: boolean,
  snapshot: CommandSessionRuntimeEntry | null
): CommandDisplayStatus => {
  if (!snapshot) return fallback;
  if (snapshot.status === 'failed_to_start') return 'failed';
  if (snapshot.status === 'running') return commandSettled ? fallback : 'loading';
  if (snapshot.timedOut || snapshot.error) return 'failed';
  if (snapshot.exitCode === 0) return 'completed';
  if (snapshot.exitCode !== null) return 'failed';
  return commandSettled ? fallback : 'failed';
};

const normalizeToolName = (run: RawToolRun): string =>
  String(run.toolRuntimeName || run.toolName || run.toolDisplayName || '').trim().toLowerCase();

export const isExecuteCommandTimelineTool = (run: RawToolRun): boolean => {
  const name = normalizeToolName(run);
  return name === 'execute_command' || name === 'exec_command' || name.includes('执行命令');
};

export const isCommandSessionTimelineTool = (run: RawToolRun): boolean => {
  const name = normalizeToolName(run);
  return name === 'command_session' || name === 'write_stdin' || name === 'write_command_stdin' ||
    name.includes('命令会话') || name.includes('命令交互');
};

/**
 * 命令会话一个工具两种动作：空 `chars` 是轮询新增输出，非空是写 stdin。时间线上
 * 两种动作都只写一个动词「执行」（`chat.timeline.commandRan`）：动作差别已经由
 * 行尾的命令文本与状态点说明，行首不必再分叉。命令行侧仍按动作细分的词表在引擎
 * （wunder-runtime 的 `tools::tool_verbs`），那是终端里的说明，不要搬到时间线。
 */
export const resolveCommandInteractionKind = (run: RawToolRun): CommandInteractionKind | null => {
  if (isExecuteCommandTimelineTool(run)) return 'execute';
  if (!isCommandSessionTimelineTool(run)) return null;
  const args = extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject);
  const chars = args?.chars ?? args?.input;
  return typeof chars === 'string' && chars.length > 0 ? 'write' : 'poll';
};

/** Prefer the explicit session identity; a tool call may have several commands. */
export const resolveTimelineCommandSessionRef = (run: RawToolRun): string => {
  const args = extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject);
  const sessionRef = args?.command_session_id ?? args?.session_id;
  if (sessionRef !== undefined && sessionRef !== null) return String(sessionRef);
  for (const item of [run.resultItem, run.outputItem, run.callItem]) {
    const direct = item?.commandSessionId ?? item?.command_session_id;
    if (direct) return String(direct);
    const data = extractToolResultDataObject(parseToolWorkflowResultObject(
      item?.toolResultRawDetail ?? item?.tool_result_raw_detail ?? item?.detail
    ));
    if (data?.command_session_id) return String(data.command_session_id);
  }
  return '';
};

export const resolveCommandInteractionInput = (run: RawToolRun): string => {
  const args = extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject);
  const chars = args?.chars ?? args?.input;
  return typeof chars === 'string' ? chars : '';
};

export const resolveExecuteCommandText = (run: RawToolRun): string => {
  if (!isExecuteCommandTimelineTool(run)) return '';
  const args = extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject);
  for (const value of [args?.content, args?.cmd, args?.command]) {
    if (typeof value === 'string' && value.trim()) return value.trim();
  }
  return '';
};

type UnknownObject = Record<string, unknown>;
const asObject = (value: unknown): UnknownObject | null =>
  value && typeof value === 'object' && !Array.isArray(value) ? value as UnknownObject : null;

const outputFromObject = (value: unknown): string => {
  const object = asObject(value);
  if (!object) return '';
  const exitCode = typeof object.returncode === 'number'
    ? object.returncode
    : typeof object.exit_code === 'number' ? object.exit_code : null;
  const hasSeparateError = Boolean(object.error) || (exitCode !== null && exitCode > 0);
  const chunks = [object.pty, object.stdout, ...(hasSeparateError ? [] : [object.stderr])]
    .filter((item): item is string => typeof item === 'string' && item.length > 0);
  if (chunks.length) return chunks.join('\n');
  return typeof object.output === 'string' ? object.output : '';
};

export const resolveCommandRunOutput = (run: RawToolRun): string => {
  for (const item of [run.resultItem, run.outputItem]) {
    const result = parseToolWorkflowResultObject(
      item?.toolResultRawDetail ?? item?.tool_result_raw_detail ?? item?.detail
    );
    const data = extractToolResultDataObject(result);
    if (!data) continue;
    if (Array.isArray(data.results)) {
      const output = data.results.map(outputFromObject).filter(Boolean).join('\n');
      if (output) return output;
    }
    const output = outputFromObject(data);
    if (output) return output;
  }
  return '';
};

/** Bound rendering while keeping both the command's first lines and live tail. */
export const commandSessionOutputPreview = (text: string, limit = 4000): string => {
  if (text.length <= limit) return text;
  const head = Math.floor(limit / 3);
  return `${text.slice(0, head)}\n… (${text.length - limit} chars omitted)\n${text.slice(-(limit - head))}`;
};
