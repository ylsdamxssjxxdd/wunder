/** Small, durable plan presentation shared by the window and log projections. */
export interface ChatPlanStep { step: string; status: 'pending' | 'in_progress' | 'completed' }
export interface ChatPlan {
  item_id?: string;
  revision?: number;
  created_seq?: number;
  explanation: string;
  steps: ChatPlanStep[];
}

const record = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};

const preview = (value: string, limit: number): string => {
  let text = '';
  let bytes = 0;
  for (const character of value) {
    const point = character.codePointAt(0)!;
    bytes += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
    if (bytes > limit) break;
    text += character;
  }
  return text;
};

export const normalizeChatPlan = (value: unknown): ChatPlan | null => {
  const raw = record(value);
  const source = { ...raw, ...record(raw.data), ...record(raw.payload) };
  const input = Array.isArray(source.plan) ? source.plan : Array.isArray(source.steps) ? source.steps : [];
  let active = false;
  const steps: ChatPlanStep[] = [];
  for (const entry of input.slice(0, 32)) {
    const item = record(entry);
    const text = item.step ?? item.title ?? (typeof entry === 'string' ? entry : '');
    if (typeof text !== 'string' || !text.trim()) continue;
    const rawStatus = String(item.status ?? '').trim().toLowerCase().replace(/[-\s]+/g, '_');
    let status: ChatPlanStep['status'] = 'pending';
    if (['completed', 'complete', 'done'].includes(rawStatus)) status = 'completed';
    else if (['in_progress', 'inprogress'].includes(rawStatus) && !active) {
      status = 'in_progress';
      active = true;
    }
    steps.push({ step: preview(text.trim(), 256), status });
  }
  if (!steps.length) return null;
  return { explanation: typeof source.explanation === 'string' ? preview(source.explanation.trim(), 512) : '', steps };
};
