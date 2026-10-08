export type ApplyPatchPreviewAction = 'add' | 'delete' | 'update' | 'move' | 'other';

export const buildApplyPatchEmptyPreviewText = (action: ApplyPatchPreviewAction): string => {
  if (action === 'delete') {
    return '- whole-file delete; inline diff appears after the tool result arrives';
  }
  if (action === 'move') {
    return '> move/rename only; no inline line diff in patch body';
  }
  if (action === 'add') {
    return '+ no inline added lines in patch body';
  }
  return '~ no inline hunk lines in patch body';
};
