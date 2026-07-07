export type ToolWorkflowResultObject = Record<string, unknown>;

const DISPLAY_PAYLOAD_KEYS = new Set([
  'answer',
  'body',
  'changed_files',
  'chunks',
  'columns',
  'columns_jsonl',
  'content',
  'content_preview',
  'contentPreview',
  'detail',
  'documents',
  'error_detail_head',
  'files',
  'hits',
  'hits_jsonl',
  'items',
  'items_jsonl',
  'matches',
  'matches_jsonl',
  'model_observation',
  'modelObservation',
  'observation',
  'observation_text',
  'observationText',
  'output',
  'preview',
  'result_preview',
  'rows',
  'rows_jsonl',
  'stderr',
  'stdout',
  'structured_content',
  'structuredContent',
  'text'
]);

const ENVELOPE_MARKER_KEYS = new Set([
  'action',
  'code',
  'error',
  'message',
  'meta',
  'name',
  'ok',
  'operation',
  'status',
  'success',
  'tool',
  'tool_name',
  'toolName',
  'type'
]);

export const asToolWorkflowResultObject = (value: unknown): ToolWorkflowResultObject | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as ToolWorkflowResultObject)
    : null;

const hasDisplayPayload = (value: ToolWorkflowResultObject): boolean =>
  Object.keys(value).some((key) => DISPLAY_PAYLOAD_KEYS.has(key));

const hasEnvelopeMarker = (value: ToolWorkflowResultObject): boolean =>
  Object.keys(value).some((key) => ENVELOPE_MARKER_KEYS.has(key));

const shouldUnwrapNestedData = (
  current: ToolWorkflowResultObject,
  nested: ToolWorkflowResultObject
): boolean => {
  if (!hasDisplayPayload(current)) return true;
  return hasEnvelopeMarker(current) && hasDisplayPayload(nested);
};

export const normalizeToolResultDataObject = (
  value: unknown
): ToolWorkflowResultObject | null => {
  let current = asToolWorkflowResultObject(value);
  for (let depth = 0; current && depth < 5; depth += 1) {
    const nestedData = asToolWorkflowResultObject(current.data);
    if (nestedData && shouldUnwrapNestedData(current, nestedData)) {
      current = nestedData;
      continue;
    }

    const nestedResult = asToolWorkflowResultObject(current.result);
    if (nestedResult && !hasDisplayPayload(current)) {
      current = nestedResult;
      continue;
    }

    return current;
  }
  return current;
};

export const extractToolResultDataObject = (
  resultObject: ToolWorkflowResultObject | null
): ToolWorkflowResultObject | null => {
  if (!resultObject) return null;
  return normalizeToolResultDataObject(
    asToolWorkflowResultObject(resultObject.data) || resultObject
  );
};
