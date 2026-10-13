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
import { asToolWorkflowResultObject, extractToolResultDataObject } from './toolWorkflowResultPayload';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import { buildStructuredToolResultView } from './toolWorkflowStructuredView';
import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

type UnknownObject = Record<string, unknown>;

const parseDetailObject = (value: unknown): UnknownObject | null => {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as UnknownObject;
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed.startsWith('{')) return null;
  try {
    return asToolWorkflowResultObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

export const buildTimelineStructuredView = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): ToolWorkflowStructuredView | null => {
  const detailObject = parseDetailObject(run.resultItem?.detail);
  if (!detailObject) return null;
  const resultObject = asToolWorkflowResultObject(detailObject.result) || detailObject;
  const dataObject = extractToolResultDataObject(resultObject);
  return buildStructuredToolResultView(
    run.toolName,
    resultObject,
    dataObject,
    t,
    extractWorkflowCallArgs(run.callItem, parseDetailObject)
  );
};
