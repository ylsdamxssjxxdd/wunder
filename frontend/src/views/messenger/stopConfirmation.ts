import { confirmWithFallback } from '@/utils/confirm';

type TranslateFn = (
  key: string,
  params?: Record<string, unknown>
) => string;

export const confirmStopAgentRun = (t: TranslateFn): Promise<boolean> =>
  confirmWithFallback(
    t('chat.stop.confirmMessage'),
    t('chat.stop.confirmTitle'),
    {
      confirmButtonText: t('chat.stop.confirmAction'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    }
  );
