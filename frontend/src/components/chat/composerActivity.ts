/**
 * 输入区上方计划状态条的数据推导。
 *
 * 运行阶段提示（模型响应中 / 调用工具中 …）不在这里：每个智能体气泡自己
 * 已经带运行信息，输入区上方只在有计划时出现，避免两处重复。
 */

export type PlanProgress = {
  total: number;
  done: number;
};

const normalizeText = (value: unknown): string => String(value ?? '').trim().toLowerCase();

export const summarizePlanProgress = (plan: unknown): PlanProgress => {
  const steps = Array.isArray((plan as { steps?: unknown } | null)?.steps)
    ? ((plan as { steps: Array<{ status?: unknown }> }).steps)
    : [];
  let done = 0;
  for (const step of steps) {
    if (normalizeText(step?.status) === 'completed') {
      done += 1;
    }
  }
  return { total: steps.length, done };
};
