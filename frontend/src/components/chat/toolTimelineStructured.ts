// AI生成
/**
 * 时间线条目的结构化结果视图。
 *
 * `MessageToolWorkflow` 的完整投影里有结构化结果卡（定时任务、读文件、目录、
 * 检索、写文件、数据库、知识库），时间线条目此前只回落成正文文本，等于在
 * 轻量展示窗里丢掉了这批渲染能力。这里按同一套解析口径从 `RawToolRun`
 * 组装视图，交回 `MessageToolWorkflowStructuredSection` 渲染。
 */
import type { RawToolRun } from './toolWorkflowRunModel';
import { parseToolWorkflowResultObject, extractToolResultDataObject } from './toolWorkflowResultPayload';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import { buildStructuredToolResultView } from './toolWorkflowStructuredView';
import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

export const buildTimelineStructuredView = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): ToolWorkflowStructuredView | null => {
  const item = run.resultItem || run.outputItem;
  const resultObject = parseToolWorkflowResultObject(item?.toolResultRawDetail ?? item?.tool_result_raw_detail) ||
    parseToolWorkflowResultObject(item?.detail);
  const dataObject = extractToolResultDataObject(resultObject);
  return buildStructuredToolResultView(
    run.toolName,
    resultObject,
    dataObject,
    t,
    extractWorkflowCallArgs(run.callItem, parseToolWorkflowResultObject)
  );
};
