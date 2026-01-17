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
  return {
    eventType,
    eventId,
    dataText: dataLines.join('\n')
  };
};

export const consumeSseStream = async (response, onEvent) => {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error('SSE 响应不可读取');
  }
  const decoder = new TextDecoder('utf-8');
  let buffer = '';

  // 参照 Wunder 调试面板：按 \n\n 拆分 SSE 事件块
  const flushBlocks = () => {
    const parts = buffer.split('\n\n');
    buffer = parts.pop() || '';
    parts.forEach((part) => {
      if (!part.trim()) return;
      const { eventType, dataText, eventId } = parseSseBlock(part);
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
    const { eventType, dataText, eventId } = parseSseBlock(buffer);
    onEvent(eventType, dataText, eventId);
  }
};
