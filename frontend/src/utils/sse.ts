import { t } from '@/i18n';

const parseSseBlock = (block) => {
  const lines = block.split(/\r?\n/);
  let eventType = 'message';
  let eventId = '';
  const dataLines = [];
  lines.forEach((line) => {
    if (line.startsWith('event:')) {
      eventType = line.slice(6).trim();
    } else if (line.startsWith('id:')) {
      eventId = line.slice(3).trim();
    } else if (line.startsWith('data:')) {
      dataLines.push(line.slice(5).trim());
    }
  });
  if (dataLines.length === 0) {
    return null;
  }
  return {
    eventType,
    eventId,
    dataText: dataLines.join('\n')
  };
};

export const consumeSseStream = async (response, onEvent) => {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error(t('chat.sse.unreadable'));
  }
  const decoder = new TextDecoder('utf-8');
  let buffer = '';

  // 参照 Wunder 调试面板：按 \n\n 拆分 SSE 事件块
  const flushBlocks = () => {
    const parts = buffer.split('\n\n');
    buffer = parts.pop() || '';
    parts.forEach((part) => {
      if (!part.trim()) return;
      const parsed = parseSseBlock(part);
      if (!parsed) return;
      const { eventType, dataText, eventId } = parsed;
      onEvent(eventType, dataText, eventId);
    });
  };

  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    flushBlocks();
  }

  if (buffer.trim()) {
    const parsed = parseSseBlock(buffer);
    if (parsed) {
      const { eventType, dataText, eventId } = parsed;
      onEvent(eventType, dataText, eventId);
    }
  }
};
