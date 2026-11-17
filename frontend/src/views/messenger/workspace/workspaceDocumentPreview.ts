// AI生成
/**
 * 文本文档（含 Markdown / HTML）预览与编辑的公共加载器。
 *
 * 工作目录区与聊天区引用打开文档时必须走同一套能力：
 * - 文本类返回源码内容，由对话框决定源码编辑或渲染预览；
 * - HTML 返回源码内容 + 可直接挂到 iframe 的对象 URL（内联样式、脚本、图片等相对资源）；
 * - 超限统一返回 `too-large`，避免两端各自判断。
 */
import { buildWorkspaceHtmlPreviewDocument } from '@/components/chat/workspaceHtmlPreview';
import { isWorkspaceHtmlDocumentPath } from '@/utils/workspaceResourcePreview';

export const WORKSPACE_DOCUMENT_TEXT_MAX_BYTES = 2 * 1024 * 1024;

export type WorkspaceDocumentTextPayload = {
  content: string;
  size: number;
  truncated: boolean;
};

export type WorkspaceDocumentPreviewLoad = {
  status: 'ready' | 'too-large';
  /** `html` 时 `previewUrl` 可直接给 iframe，`content` 是原始源码用于编辑。 */
  kind: 'text' | 'html';
  content: string;
  previewUrl: string;
  /** HTML 预览时生成的对象 URL，关闭对话框时需要统一回收。 */
  objectUrls: string[];
  truncated: boolean;
  size: number;
};

const emptyLoad = (status: 'too-large'): WorkspaceDocumentPreviewLoad => ({
  status,
  kind: 'text',
  content: '',
  previewUrl: '',
  objectUrls: [],
  truncated: false,
  size: 0
});

export const loadWorkspaceDocumentPreview = async (options: {
  /** 工作区根相对路径，用于解析 HTML 内的相对资源。 */
  path: string;
  /** 下载工作区根相对路径对应的文件。 */
  fetchBlob: (relativePath: string) => Promise<Blob>;
  /** 读取文本内容（内容端点），仅在非 HTML 文件时调用。 */
  readText: () => Promise<WorkspaceDocumentTextPayload>;
  sizeBytes?: number | null;
  maxBytes?: number;
}): Promise<WorkspaceDocumentPreviewLoad> => {
  const maxBytes = Math.max(1024, Number(options.maxBytes) || WORKSPACE_DOCUMENT_TEXT_MAX_BYTES);
  const declaredSize = Number(options.sizeBytes);
  if (Number.isFinite(declaredSize) && declaredSize > maxBytes) {
    return emptyLoad('too-large');
  }

  const path = String(options.path || '').replace(/\\/g, '/').trim();
  if (isWorkspaceHtmlDocumentPath(path)) {
    const source = await options.fetchBlob(path);
    if (Number(source?.size) > maxBytes) {
      return emptyLoad('too-large');
    }
    const rawHtml = await source.text();
    const previewDocument = await buildWorkspaceHtmlPreviewDocument({
      rawHtml,
      entryPath: path,
      fetchResource: (relativePath) => options.fetchBlob(relativePath)
    });
    return {
      status: 'ready',
      kind: 'html',
      content: rawHtml,
      previewUrl: URL.createObjectURL(
        new Blob([previewDocument.html], { type: 'text/html; charset=utf-8' })
      ),
      objectUrls: previewDocument.objectUrls,
      truncated: false,
      size: Number(source?.size) || rawHtml.length
    };
  }

  const payload = await options.readText();
  if (Number(payload?.size) > maxBytes) {
    return emptyLoad('too-large');
  }
  return {
    status: 'ready',
    kind: 'text',
    content: String(payload?.content ?? ''),
    previewUrl: '',
    objectUrls: [],
    truncated: payload?.truncated === true,
    size: Number(payload?.size) || 0
  };
};
