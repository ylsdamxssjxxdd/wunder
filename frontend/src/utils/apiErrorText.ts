const TRACE_ID_RE = /\b(?:trace[_-]?id|err_[a-z0-9]+)\b[:=\s-]*[a-z0-9_-]*/gi;

export const normalizeApiErrorText = (value: string): string => {
  const text = String(value || '')
    .replace(TRACE_ID_RE, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) return '';
  const lowered = text.toLowerCase();
  if (lowered === '[object object]' || lowered === 'object object') {
    return '';
  }
  return text;
};

const containsChinese = (value: string): boolean => /[\u4e00-\u9fff]/.test(value);

const includesAny = (value: string, patterns: string[]): boolean =>
  patterns.some((pattern) => value.includes(pattern));

export const localizeApiErrorText = (
  message: string,
  status: number | null,
  fallback: string,
  defaultFallback = '请求失败'
): string => {
  const normalizedMessage = normalizeApiErrorText(message);
  if (!normalizedMessage) {
    return normalizeApiErrorText(fallback) || defaultFallback;
  }
  if (containsChinese(normalizedMessage)) {
    return normalizedMessage;
  }
  const lowered = normalizedMessage.toLowerCase();
  if (
    includesAny(lowered, [
      'error parsing multipart/form-data request',
      'invalid boundary',
      'multipart',
      'form-data'
    ])
  ) {
    return '上传请求格式错误，请刷新页面后重试。';
  }
  if (
    includesAny(lowered, [
      'payload too large',
      'request body too large',
      'body too large',
      'file too large',
      'entity too large',
      'content too large'
    ])
  ) {
    return '上传内容过大，请压缩后重试。';
  }
  if (
    includesAny(lowered, [
      'network error',
      'failed to fetch',
      'load failed',
      'network request failed',
      'econnrefused',
      'socket hang up',
      'network'
    ])
  ) {
    return '网络连接失败，请检查服务是否可用后重试。';
  }
  if (
    includesAny(lowered, ['timeout', 'timed out', 'econnaborted', 'deadline has elapsed'])
  ) {
    return '请求超时，请稍后重试。';
  }
  if (
    includesAny(lowered, [
      'unauthorized',
      'forbidden',
      'auth required',
      'authentication failed',
      'invalid credentials',
      'permission denied',
      'access denied'
    ])
  ) {
    return status === 401 ? '登录状态已失效，请重新登录。' : '没有权限执行此操作。';
  }
  if (
    includesAny(lowered, [
      'not found',
      'file not found',
      'skill not found',
      'resource not found',
      '404'
    ])
  ) {
    return '目标内容不存在或已被删除。';
  }
  if (
    includesAny(lowered, [
      'bad request',
      'invalid request',
      'invalid parameter',
      'invalid payload',
      'validation failed',
      'missing parameter',
      'required'
    ])
  ) {
    return normalizeApiErrorText(fallback) || '请求参数不正确，请检查后重试。';
  }
  if (status !== null && status >= 500) {
    return '服务暂时异常，请稍后重试。';
  }
  return normalizeApiErrorText(fallback) || '操作失败，请稍后重试。';
};
