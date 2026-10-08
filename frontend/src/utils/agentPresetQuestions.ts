export const normalizeAgentPresetQuestionDrafts = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  return value.map((item) => String(item ?? ''));
};

export const normalizeAgentPresetQuestions = (value: unknown): string[] => {
  const seen = new Set<string>();
  const output: string[] = [];
  normalizeAgentPresetQuestionDrafts(value).forEach((item) => {
    const question = item.trim();
    if (!question || seen.has(question)) return;
    seen.add(question);
    output.push(question);
  });
  return output;
};
