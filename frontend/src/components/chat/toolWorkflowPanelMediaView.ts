import { asToolWorkflowResultObject as asObject } from './toolWorkflowResultPayload';
import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

type ObjectValue = Record<string, unknown>;
type Translate = (key: string, params?: Record<string, unknown>) => string;

const text = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      const trimmed = value.trim();
      return trimmed.length > 800 ? `${trimmed.slice(0, 800)}…` : trimmed;
    }
  }
  return '';
};

const statusLabel = (value: unknown, t: Translate): string => {
  switch (String(value || '').toLowerCase()) {
    case 'completed': case 'complete': case 'done': case 'ok': return t('chat.toolWorkflow.resultDone');
    case 'in_progress': case 'running': return t('chat.toolWorkflow.statusRunning');
    case 'pending': case 'awaiting_input': return t('chat.toolWorkflow.statusWaiting');
    default: return text(value);
  }
};

/** Panels and visual media carry structured data rather than stdout. Keep the
 * readable projection bounded and leave attachments/binary payloads untouched. */
export const buildPanelMediaResultView = (
  toolName: string,
  data: ObjectValue | null,
  args: ObjectValue | null,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const name = toolName.trim().toLowerCase();
  if (['question_panel', 'ask_panel', 'question panel', '问询面板'].includes(name)) {
    const source = Array.isArray(data?.questions) || Array.isArray(data?.options) ? data : args;
    if (!source) return null;
    const questions = Array.isArray(source.questions) ? source.questions : [source];
    const groups: ToolWorkflowStructuredView['groups'] = [];
    for (const [index, raw] of questions.slice(0, 4).entries()) {
      const question = asObject(raw);
      if (!question) continue;
      const options = Array.isArray(question.options) ? question.options : [];
      groups.push({
        key: `question-${index}`,
        title: text(question.question) || t('chat.inquiry.defaultQuestion'),
        rows: [{
          key: `question-mode-${index}`,
          title: t(question.multiple === true ? 'chat.toolWorkflow.detail.multipleChoice' : 'chat.toolWorkflow.detail.singleChoice')
        }, ...options.slice(0, 4).flatMap((rawOption, optionIndex) => {
          const option = asObject(rawOption);
          const label = text(option?.label, rawOption);
          return label ? [{
            key: `option-${index}-${optionIndex}`,
            title: label,
            meta: option?.recommended === true ? t('chat.inquiry.recommended') : '',
            body: text(option?.description)
          }] : [];
        })]
      });
    }
    return groups.length ? { variant: 'question', metrics: [], groups } : null;
  }

  if (['update_plan', 'plan_update', '计划面板'].includes(name)) {
    const source = Array.isArray(data?.plan) ? data : args;
    if (!source || !Array.isArray(source.plan)) return null;
    const rows = source.plan.slice(0, 12).flatMap((raw, index) => {
      const step = asObject(raw);
      const title = text(step?.step);
      return title ? [{ key: `step-${index}`, title, meta: statusLabel(step?.status || 'pending', t) }] : [];
    });
    return rows.length ? {
      variant: 'plan', metrics: [],
      groups: [{ key: 'plan', title: text(source.explanation), rows }]
    } : null;
  }

  if (!['read_image', 'view_image', '读图工具'].includes(name)) return null;
  const media = asObject(data?.result) || data;
  const path = text(data?.path, args?.path, media?.name);
  if (!path && !media) return null;
  const kind = text(data?.media_kind, media?.kind);
  const kindKey = ['image', 'video', 'gif'].includes(kind) ? `chat.toolWorkflow.detail.media.${kind}` : '';
  const lines: string[] = [];
  if (kind) lines.push(`${t('chat.toolWorkflow.detail.mediaType')}: ${kindKey ? t(kindKey) : kind}`);
  for (const [key, value] of [
    ['bytes', data?.size_bytes], ['frames', media?.frame_count], ['mediaDuration', media?.duration_ms]
  ] as const) {
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      lines.push(`${t(`chat.toolWorkflow.detail.${key}`)}: ${value}`);
    }
  }
  if (Array.isArray(media?.warnings)) {
    lines.push(...media.warnings.slice(0, 4).map(value => text(value)).filter(Boolean));
  }
  return {
    variant: 'media', metrics: [],
    groups: [{ key: 'media', rows: [{ key: 'media-source', title: path || t('chat.toolWorkflow.toolLabel.readImage'), body: lines.join('\n') }] }]
  };
};
