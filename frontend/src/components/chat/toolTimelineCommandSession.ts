import type { RawToolRun } from './toolWorkflowRunModel';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import { extractToolResultDataObject, parseToolWorkflowResultObject } from './toolWorkflowResultPayload';

/** Prefer the explicit session identity; a tool call may have several commands. */
export const resolveTimelineCommandSessionRef = (run: RawToolRun): string => {
  const args = extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject);
  if (args?.command_session_id) return String(args.command_session_id);
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

/** Bound rendering while keeping both the command's first lines and live tail. */
export const commandSessionOutputPreview = (text: string, limit = 4000): string => {
  if (text.length <= limit) return text;
  const head = Math.floor(limit / 3);
  return `${text.slice(0, head)}\n… (${text.length - limit} chars omitted)\n${text.slice(-(limit - head))}`;
};
