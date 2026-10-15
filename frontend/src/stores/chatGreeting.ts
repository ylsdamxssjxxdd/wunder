// 合成问候语：数据层唯一出口。threadView 槽位 → 会话消息数组的所有重建路径
// （打开、提交后、翻页、watcher 恢复、实时 thread_view_change）都必须经过
// threadViewStoreMessages，否则问候语会在任一次投影重建时凭空消失。
// 独立成叶模块：realtime 层（chatThreadRuntime）也要复用，不能挂在
// chatDemoPanels 这类重依赖枢纽上。
import { buildMessage, resolveGreetingContent, resolveTimestampIso } from './chatStats';
import { threadViewSlots } from '@/realtime/chat/threadViewStore';

export interface GreetingMessageOptions {
  greeting?: unknown;
  createdAt?: unknown;
  sessionCreatedAt?: unknown;
}

export const buildGreetingMessage = (createdAt, greeting) => ({
  ...buildMessage('assistant', resolveGreetingContent(greeting), createdAt),
  workflowItems: [],
  workflowStreaming: false,
  isGreeting: true
});

export const resolveGreetingTimestamp = (messages, createdAt) => {
  const direct = resolveTimestampIso(createdAt);
  if (direct) return direct;
  const safeMessages = Array.isArray(messages) ? messages : [];
  const candidate = safeMessages.find((message) => message?.created_at)?.created_at;
  return resolveTimestampIso(candidate);
};

export const ensureGreetingMessage = (messages, options: GreetingMessageOptions = {}) => {
  const safeMessages = Array.isArray(messages) ? messages : [];
  const greetingText = resolveGreetingContent(options?.greeting);
  // 无论历史会话与否，都补一条问候语，保证提示词预览入口稳定可见
  const greetingIndex = safeMessages.findIndex((message) => message?.isGreeting);
  if (greetingIndex >= 0) {
    if (safeMessages[greetingIndex]?.content !== greetingText) {
      safeMessages[greetingIndex].content = greetingText;
    }
    const createdAt = options?.createdAt ?? options?.sessionCreatedAt;
    if (createdAt) {
      const greetingAt = resolveGreetingTimestamp(safeMessages, createdAt);
      if (greetingAt) {
        const currentAt = resolveTimestampIso(safeMessages[greetingIndex]?.created_at);
        if (currentAt !== greetingAt) {
          safeMessages[greetingIndex].created_at = greetingAt;
        }
      }
    }
    return safeMessages;
  }
  const greetingAt = resolveGreetingTimestamp(safeMessages, options?.createdAt ?? options?.sessionCreatedAt);
  return [buildGreetingMessage(greetingAt, greetingText), ...safeMessages];
};

export const threadViewStoreMessages = (store: unknown, key: string, sessionCreatedAt?: unknown) =>
  ensureGreetingMessage(
    (threadViewSlots(key) ?? []).flatMap((s: any) => [s.user, s.assistant]),
    { createdAt: sessionCreatedAt, greeting: (store as any)?.greetingOverride }
  );
