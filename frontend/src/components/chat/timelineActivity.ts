import type { TimelineReasoningEntry, TimelineToolEntry } from './toolTimelineModel';

export type ProcessActivity = { kind: string; item_id?: string; tool?: string; created_seq?: number; model_round?: number };
type Translate = (key: string, params?: Record<string, unknown>) => string;

/**
 * 思考内容的一行预览：供收起状态下的条目行与批次条右侧摘要使用。
 * 只取有界尾部并压平空白，行内再用 CSS 省略，避免整段思考进入 DOM。
 * A bounded tail keeps the collapsed row from copying a long thought.
 */
export const reasoningPreview = (text: string): string => {
  let start = Math.max(0, text.length - 640);
  const unit = text.charCodeAt(start);
  if (unit >= 0xdc00 && unit <= 0xdfff) start++;
  return `${start > 0 ? '…' : ''}${text.slice(start).replace(/\s+/g, ' ').trim()}`;
};

export const processActivityLabel = (
  activity: ProcessActivity | null | undefined,
  status: string,
  t: Translate
): string => {
  if (['failed', 'error', 'system_error', 'rejected'].includes(status)) return t('messenger.thread.state.error');
  if (['completed', 'done', 'final'].includes(status)) return t('chat.timeline.processCompleted');
  if (['cancelled', 'stopped', 'interrupted'].includes(status)) return t('chat.timeline.processStopped');
  if (['waiting_input', 'waiting_user_input'].includes(status)) return t('messenger.thread.state.inquiry');
  if (status === 'waiting_approval') return t('messenger.thread.state.approval');
  if (['queued', 'pending'].includes(status)) return t('messenger.thread.state.pending');
  if (activity?.kind === 'tool' && activity.tool) return t('chat.timeline.executingTool', { tool: activity.tool });
  if (activity?.kind === 'thinking') return t('chat.timeline.thinkingRunning');
  if (activity?.kind === 'responding') return t('chat.timeline.generatingReply');
  return t(status === 'running' ? 'chat.timeline.processing' : 'chat.timeline.processCompleted');
};

export const entryActivity = (entry: TimelineReasoningEntry | TimelineToolEntry): ProcessActivity | null => {
  if (entry.kind === 'reasoning') return entry.streaming ? { kind: 'thinking' } : null;
  return entry.status === 'loading' ? { kind: 'tool', tool: entry.toolLabel } : null;
};
