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

/** 可编辑文本（源码进代码编辑器）的单次读取上限。 */
export const WORKSPACE_DOCUMENT_TEXT_MAX_BYTES = 2 * 1024 * 1024;
/**
 * HTML 的成品是页面：默认渲染进 iframe，不需要把源码塞进代码编辑器，
 * 所以预览上限单独放宽。超过文本上限时只给渲染结果，源码编辑请下载后处理。
 */
export const WORKSPACE_HTML_DOCUMENT_MAX_BYTES = 30 * 1024 * 1024;

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
  /** 是否允许继续编辑源码；大文件只读预览时为 false。 */
  editable: boolean;
  size: number;
};

const emptyLoad = (status: 'too-large'): WorkspaceDocumentPreviewLoad => ({
  status,
  kind: 'text',
  content: '',
  previewUrl: '',
  objectUrls: [],
  truncated: false,
  editable: false,
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
  const path = String(options.path || '').replace(/\\/g, '/').trim();
  const isHtmlDocument = isWorkspaceHtmlDocumentPath(path);
  // HTML 默认渲染预览，不进代码编辑器，上限单独放宽到 30MB。
  const previewMaxBytes = isHtmlDocument
    ? Math.max(maxBytes, WORKSPACE_HTML_DOCUMENT_MAX_BYTES)
    : maxBytes;
  const declaredSize = Number(options.sizeBytes);
  if (Number.isFinite(declaredSize) && declaredSize > previewMaxBytes) {
    return emptyLoad('too-large');
  }

  if (isHtmlDocument) {
    const source = await options.fetchBlob(path);
    const size = Number(source?.size) || 0;
    if (size > previewMaxBytes) {
      return emptyLoad('too-large');
    }
    const rawHtml = await source.text();
    const previewDocument = await buildWorkspaceHtmlPreviewDocument({
      rawHtml,
      entryPath: path,
      fetchResource: (relativePath) => options.fetchBlob(relativePath)
    });
    // 超过源码编辑上限时只交付渲染结果：把几十 MB 源码塞进代码编辑器会直接卡死界面。
    const editable = size <= maxBytes;
    return {
      status: 'ready',
      kind: 'html',
      content: editable ? rawHtml : '',
      previewUrl: URL.createObjectURL(
        new Blob([previewDocument.html], { type: 'text/html; charset=utf-8' })
      ),
      objectUrls: previewDocument.objectUrls,
      truncated: false,
      editable,
      size: size || rawHtml.length
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
    editable: payload?.truncated !== true,
    size: Number(payload?.size) || 0
  };
};
