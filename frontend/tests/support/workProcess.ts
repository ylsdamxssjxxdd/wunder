import { expect, type Locator } from '@playwright/test';

/**
 * 展开工作过程。
 *
 * 云端版与桌面端同口径：线程里**最近一条带工作过程**的轮次默认展开
 * （见 `src/components/chat/timelineProcessDefault.ts`）。所以按「点一下展开」
 * 写用例会把默认展开的内容又收起来；统一走这里：只在收起时点。
 */
export async function openWorkProcess(head: Locator): Promise<void> {
  await expect(head).toBeVisible();
  if ((await head.getAttribute('aria-expanded')) === 'false') await head.click();
  await expect(head).toHaveAttribute('aria-expanded', 'true');
}
