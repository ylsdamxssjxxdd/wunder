import test from 'node:test';
import assert from 'node:assert/strict';

const storageMock = {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined
};
(globalThis as typeof globalThis & { localStorage?: unknown }).localStorage = storageMock;

const {
  prepareMessageMarkdownContent
} = require('../../src/utils/messageMarkdown') as typeof import('../../src/utils/messageMarkdown');
const { renderMarkdown } = require('../../src/utils/markdown') as typeof import('../../src/utils/markdown');
const {
  buildAgentWorkspaceScopeId,
  buildWorkspacePublicPathFromScope,
  resolveMarkdownWorkspacePath
} = require('../../src/utils/messageWorkspacePath') as typeof import('../../src/utils/messageWorkspacePath');
const {
  parseWorkspaceResourceUrl
} = require('../../src/utils/workspaceResources') as typeof import('../../src/utils/workspaceResources');
const {
  buildWorkspaceResourceRequestParams
} = require('../../src/utils/workspaceResourceRequest') as typeof import('../../src/utils/workspaceResourceRequest');
const {
  bindWorkspaceImagePreviewState,
  buildWorkspaceResourceErrorDiagnostics,
  isWorkspaceImageBlobLikelyInvalid,
  markWorkspaceImageCardError,
  normalizeWorkspaceImageBlob,
  normalizeWorkspaceImageResponseBlob,
  resolveWorkspaceResourceErrorDiagnostics
} = require('../../src/utils/workspaceResourceCards') as typeof import('../../src/utils/workspaceResourceCards');
const {
  buildAssistantDisplayContent
} = require('../../src/utils/assistantFailureNotice') as typeof import('../../src/utils/assistantFailureNotice');

test('repairs malformed markdown image closings', () => {
  const content =
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123\\\\\\)';
  const repaired = prepareMessageMarkdownContent(content, null);
  assert.equal(
    repaired,
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123)'
  );
});

test('keeps malformed markdown closings inside fenced code blocks untouched', () => {
  const content = [
    '```md',
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123\\\\\\)',
    '```'
  ].join('\n');
  const repaired = prepareMessageMarkdownContent(content, null);
  assert.equal(repaired, content);
});

test('resolves bare relative paths for container scoped workspaces', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'temp_dir/美以伊冲突完整报告_汇总.md',
    ownerId: 'demo-user',
    containerId: 7
  });
  assert.equal(
    resolved,
    '/workspaces/demo-user__c__7/temp_dir/%E7%BE%8E%E4%BB%A5%E4%BC%8A%E5%86%B2%E7%AA%81%E5%AE%8C%E6%95%B4%E6%8A%A5%E5%91%8A_%E6%B1%87%E6%80%BB.md'
  );
});

test('resolves dot relative paths for normal user world workspaces', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: './temp_dir/report.md',
    ownerId: 'world-user'
  });
  assert.equal(resolved, '/workspaces/world-user/temp_dir/report.md');
});

test('resolves bare relative paths for agent scoped workspaces', () => {
  const scopeId = buildAgentWorkspaceScopeId('demo-user', 'analysis-agent');
  assert.equal(scopeId, 'demo-user__a__analysis-agent');
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'temp_dir/conflict-map.png',
    ownerId: 'demo-user',
    workspaceScopeId: scopeId
  });
  assert.equal(
    resolved,
    buildWorkspacePublicPathFromScope(scopeId, 'temp_dir/conflict-map.png')
  );
});

test('normalizes double encoded workspace resource paths', () => {
  const resource = parseWorkspaceResourceUrl(
    '/workspaces/demo-user__c__1/temp_dir/%25E5%258A%25A8%25E5%2591%2598.docx'
  );
  assert.equal(resource?.relativePath, 'temp_dir/动员.docx');
  assert.equal(
    resource?.publicPath,
    '/workspaces/demo-user__c__1/temp_dir/%E5%8A%A8%E5%91%98.docx'
  );
});

test('maps local absolute paths back to workspace resources in desktop mode', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'C:\\workspace\\demo-user__c__7\\temp_dir\\briefing.md',
    ownerId: 'demo-user',
    containerId: 7,
    desktopLocalMode: true,
    workspaceRoot: 'C:\\workspace'
  });
  assert.equal(resolved, '/workspaces/demo-user__c__7/temp_dir/briefing.md');
});

test('keeps public workspace markdown paths without rewriting owner scope', () => {
  assert.equal(
    resolveMarkdownWorkspacePath({
      rawPath: 'workspaces/admin__c__1/love_heart.png',
      ownerId: 'current-user',
      containerId: 9
    }),
    '/workspaces/admin__c__1/love_heart.png'
  );
  assert.equal(
    resolveMarkdownWorkspacePath({
      rawPath: '/workspaces/admin__c__1/love_heart.png',
      ownerId: 'current-user',
      containerId: 9
    }),
    '/workspaces/admin__c__1/love_heart.png'
  );
});

test('uses public workspace paths directly for resource requests', () => {
  const params = buildWorkspaceResourceRequestParams(
    {
      publicPath: '/workspaces/admin__c__1/heart.png',
      relativePath: 'heart.png',
      requestUserId: 'admin',
      requestAgentId: 'default',
      requestContainerId: 1
    },
    { preview: 'png' }
  );
  assert.deepEqual(params, {
    path: '/workspaces/admin__c__1/heart.png',
    preview: 'png'
  });
});

test('keeps scoped parameters for relative workspace resource requests', () => {
  const params = buildWorkspaceResourceRequestParams({
    relativePath: 'heart.png',
    requestUserId: 'demo-user',
    requestAgentId: 'agent-a',
    requestContainerId: 3
  });
  assert.deepEqual(params, {
    path: 'heart.png',
    user_id: 'demo-user',
    agent_id: 'agent-a',
    container_id: '3'
  });
});

test('retypes octet-stream workspace image blobs by filename extension', () => {
  const source = new Blob(['png-bytes'], { type: 'application/octet-stream' });
  const normalized = normalizeWorkspaceImageBlob(source, 'heart.png', 'application/octet-stream');
  assert.equal(normalized.type, 'image/png');
  assert.equal(normalized.size, source.size);
});

test('rejects non-image workspace image responses before object url hydration', () => {
  const source = new Blob(['<html>not an image</html>'], { type: 'text/html' });
  assert.equal(isWorkspaceImageBlobLikelyInvalid(source, 'heart.png', 'text/html'), true);
  const png = new Blob(['png-bytes'], { type: 'application/octet-stream' });
  assert.equal(isWorkspaceImageBlobLikelyInvalid(png, 'heart.png', 'application/octet-stream'), false);
});

test('extracts diagnostics from workspace image json error blobs', async () => {
  const source = new Blob(
    [JSON.stringify({ error: { code: 'AUTH_REQUIRED', message: 'auth required' } })],
    { type: 'application/json' }
  );
  await assert.rejects(
    () =>
      normalizeWorkspaceImageResponseBlob(source, 'heart.png', 'application/json', {
        status: 401,
        headers: { 'content-type': 'application/json' },
        data: source
      }),
    (error: unknown) => {
      const diagnostics = resolveWorkspaceResourceErrorDiagnostics(error);
      assert.equal(diagnostics?.status, 401);
      assert.equal(diagnostics?.code, 'AUTH_REQUIRED');
      assert.equal(diagnostics?.message, 'auth required');
      assert.equal(diagnostics?.contentType, 'application/json');
      assert.equal(diagnostics?.size, source.size);
      return true;
    }
  );
});

test('workspace image card exposes fetch diagnostics on failure', async () => {
  const source = new Blob(
    [JSON.stringify({ error: { code: 'AUTH_REQUIRED', message: 'auth required' } })],
    { type: 'application/json' }
  );
  const diagnostics = await buildWorkspaceResourceErrorDiagnostics({
    status: 401,
    headers: { 'content-type': 'application/json' },
    data: source
  });
  const card = {
    dataset: {},
    title: '',
    removeAttribute(name: string) {
      if (name === 'title') {
        this.title = '';
      }
    },
    classList: {
      values: new Set<string>(),
      add(value: string) {
        this.values.add(value);
      },
      remove(value: string) {
        this.values.delete(value);
      },
      contains(value: string) {
        return this.values.has(value);
      }
    }
  } as unknown as HTMLElement;
  const status = {
    textContent: '',
    title: '',
    removeAttribute(name: string) {
      if (name === 'title') {
        this.title = '';
      }
    }
  } as unknown as HTMLElement;

  markWorkspaceImageCardError(card, status, null, 'failed', diagnostics);

  assert.equal(card.dataset.workspaceState, 'error');
  assert.equal(card.dataset.workspaceErrorStatus, '401');
  assert.equal(card.dataset.workspaceErrorCode, 'AUTH_REQUIRED');
  assert.equal(card.dataset.workspaceErrorContentType, 'application/json');
  assert.equal(card.dataset.workspaceErrorSize, String(source.size));
  assert.match(card.dataset.workspaceErrorDetail || '', /AUTH_REQUIRED/);
  assert.match(card.title, /HTTP 401/);
  assert.equal(status.textContent, 'failed');
});

test('workspace image card waits for image decode before ready state', () => {
  const originalImage = (globalThis as typeof globalThis & { Image?: unknown }).Image;
  let decoder: { onload: null | (() => void); onerror: null | (() => void); src: string } | null = null;
  (globalThis as typeof globalThis & { Image?: unknown }).Image = class {
    onload: null | (() => void) = null;
    onerror: null | (() => void) = null;
    src = '';

    constructor() {
      decoder = this;
    }
  };
  const card = {
    dataset: { workspaceState: 'loading' },
    classList: {
      values: new Set<string>(),
      add(value: string) {
        this.values.add(value);
      },
      remove(value: string) {
        this.values.delete(value);
      },
      contains(value: string) {
        return this.values.has(value);
      }
    }
  } as unknown as HTMLElement;
  const status = { textContent: 'loading' } as HTMLElement;
  const preview = {
    attributes: new Map<string, string>(),
    complete: false,
    naturalWidth: 0,
    naturalHeight: 0,
    set src(value: string) {
      this.attributes.set('src', value);
    },
    get src() {
      return this.attributes.get('src') || '';
    },
    getAttribute(name: string) {
      return this.attributes.get(name) || null;
    },
    removeAttribute(name: string) {
      this.attributes.delete(name);
    },
    onload: null,
    onerror: null
  } as unknown as HTMLImageElement;

  bindWorkspaceImagePreviewState(card, preview, 'blob:wunder-heart', {
    status,
    failedLabel: 'failed'
  });
  try {
    assert.equal(card.dataset.workspaceState, 'loading');
    assert.equal(decoder?.src, 'blob:wunder-heart');
    decoder?.onload?.();
    assert.equal(card.dataset.workspaceState, 'ready');
    assert.equal((preview as unknown as { src: string }).src, 'blob:wunder-heart');
    assert.equal(status.textContent, '');
    assert.equal((card.classList as unknown as { contains: (value: string) => boolean }).contains('is-ready'), true);
  } finally {
    (globalThis as typeof globalThis & { Image?: unknown }).Image = originalImage;
  }
});

test('workspace image card marks decode failures as errors', () => {
  const originalImage = (globalThis as typeof globalThis & { Image?: unknown }).Image;
  let decoder: { onload: null | (() => void); onerror: null | (() => void); src: string } | null = null;
  (globalThis as typeof globalThis & { Image?: unknown }).Image = class {
    onload: null | (() => void) = null;
    onerror: null | (() => void) = null;
    src = '';

    constructor() {
      decoder = this;
    }
  };
  const card = {
    dataset: { workspaceState: 'loading' },
    classList: {
      values: new Set<string>(),
      add(value: string) {
        this.values.add(value);
      },
      remove(value: string) {
        this.values.delete(value);
      },
      contains(value: string) {
        return this.values.has(value);
      }
    }
  } as unknown as HTMLElement;
  const status = { textContent: '' } as HTMLElement;
  let decodeErrorCount = 0;
  const preview = {
    attributes: new Map<string, string>(),
    complete: false,
    naturalWidth: 0,
    naturalHeight: 0,
    set src(value: string) {
      this.attributes.set('src', value);
    },
    get src() {
      return this.attributes.get('src') || '';
    },
    getAttribute(name: string) {
      return this.attributes.get(name) || null;
    },
    removeAttribute(name: string) {
      this.attributes.delete(name);
    },
    onload: null,
    onerror: null
  } as unknown as HTMLImageElement;

  bindWorkspaceImagePreviewState(card, preview, 'blob:wunder-broken', {
    status,
    failedLabel: 'failed',
    onDecodeError: () => {
      decodeErrorCount += 1;
    }
  });
  try {
    assert.equal(decoder?.src, 'blob:wunder-broken');
    decoder?.onerror?.();
    assert.equal(card.dataset.workspaceState, 'error');
    assert.equal(status.textContent, 'failed');
    assert.equal(decodeErrorCount, 1);
    assert.equal((card.classList as unknown as { contains: (value: string) => boolean }).contains('is-error'), true);
  } finally {
    (globalThis as typeof globalThis & { Image?: unknown }).Image = originalImage;
  }
});

test('renders display math blocks with KaTeX', () => {
  const html = renderMarkdown('$$\nM = 3.44 \\times 10^{-3} \\cdot Z^{1/7}\n$$');
  assert.match(html, /ai-math-block/);
  assert.match(html, /katex-display/);
  assert.doesNotMatch(html, /\\times/);
});

test('renders inline math without treating prices as math', () => {
  const html = renderMarkdown('Inline \\(Z = 10^{dBZ/10}\\), price $100 stays text.');
  assert.match(html, /ai-math-inline/);
  assert.match(html, /\$100 stays text/);
});

test('falls back safely for invalid formulas', () => {
  const html = renderMarkdown('$$\n\\badcommand{x}\n$$');
  assert.match(html, /ai-math-block/);
  assert.match(html, /katex-error|badcommand/);
});

const failureNoticeMessages: Record<string, string> = {
  'chat.message.failedInlineTitle': '本次回复未完成',
  'chat.message.failedInlineReason': '错误原因：{detail}',
  'chat.message.failedInlinePartial': '以下内容是失败前已生成的部分输出，仅供参考。',
  'chat.workflow.aborted': '已中止',
  'chat.workflow.abortedDetail': '本次请求已中止',
  'chat.workflow.requestFailed': '请求失败',
  'chat.workflow.error': '错误',
  'chat.workflow.requestFailedDetail': '请求失败，请稍后重试'
};

const failureNoticeTranslator = (key: string, named?: Record<string, unknown>): string => {
  const template = failureNoticeMessages[key] || key;
  return template.replace(/\{(\w+)\}/g, (_token, name: string) => String(named?.[name] ?? ''));
};

test('omits partial block when assistant content is only the same failure detail', () => {
  const detail =
    '模型调用失败: LLM stream request failed: 429 Too Many Requests {"error":{"message":"quota exceeded"}}';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: detail,
      workflowItems: [{ status: 'failed', detail }]
    },
    failureNoticeTranslator
  );
  assert.equal(rendered, `**⚠️ 本次回复未完成**\n\n错误原因：${detail}`);
});

test('keeps partial block but trims duplicated trailing failure line', () => {
  const detail = '模型调用失败: LLM stream request failed: 429 Too Many Requests';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: `先给你一份摘要。\n${detail}`,
      workflowItems: [{ status: 'failed', detail }]
    },
    failureNoticeTranslator
  );
  assert.equal(
    rendered,
    [
      '**⚠️ 本次回复未完成**',
      '',
      `错误原因：${detail}`,
      '',
      '以下内容是失败前已生成的部分输出，仅供参考。',
      '',
      '---',
      '',
      '先给你一份摘要。'
    ].join('\n')
  );
});
