import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';

export type WorkspaceHtmlPreviewResourceFetcher = (relativePath: string) => Promise<Blob>;

export type WorkspaceHtmlPreviewDocument = {
  html: string;
  objectUrls: string[];
};

type WorkspaceHtmlPreviewOptions = {
  rawHtml: string;
  entryPath: string;
  fetchResource: WorkspaceHtmlPreviewResourceFetcher;
};

type ResourceBuildContext = {
  fetchResource: WorkspaceHtmlPreviewResourceFetcher;
  objectUrls: string[];
  resourceCache: Map<string, Promise<string>>;
};

const ABSOLUTE_URI_RE = /^[a-zA-Z][a-zA-Z\d+.-]*:/;
const RESOURCE_QUERY_OR_HASH_RE = /[?#]/;
const CSS_URL_RE = /url\(\s*(['"]?)([^'")]+)\1\s*\)/gi;
const CSS_IMPORT_RE = /@import\s+(?:url\(\s*)?(['"])([^'"]+)\1\s*\)?/gi;
const JS_IMPORT_RE =
  /\b((?:import|export)\s+(?:[^'"]*?\s+from\s*)?|import\s*\(\s*|new\s+URL\s*\(\s*)(['"])([^'"]+)\2/g;

const MIME_BY_EXTENSION: Record<string, string> = {
  css: 'text/css',
  gif: 'image/gif',
  html: 'text/html',
  htm: 'text/html',
  ico: 'image/x-icon',
  jpeg: 'image/jpeg',
  jpg: 'image/jpeg',
  js: 'text/javascript',
  json: 'application/json',
  mjs: 'text/javascript',
  mp4: 'video/mp4',
  ogg: 'audio/ogg',
  otf: 'font/otf',
  png: 'image/png',
  svg: 'image/svg+xml',
  ttf: 'font/ttf',
  txt: 'text/plain',
  wasm: 'application/wasm',
  wav: 'audio/wav',
  webm: 'video/webm',
  webp: 'image/webp',
  woff: 'font/woff',
  woff2: 'font/woff2',
  xml: 'application/xml'
};

const getWorkspaceParentPath = (path: string): string => {
  const normalized = normalizeWorkspacePath(path);
  if (!normalized) return '';
  const parts = normalized.split('/').filter(Boolean);
  parts.pop();
  return parts.join('/');
};

const splitUrlPath = (value: string): { path: string; suffix: string } => {
  const text = String(value || '').trim();
  const match = text.match(RESOURCE_QUERY_OR_HASH_RE);
  if (!match || match.index === undefined) {
    return { path: text, suffix: '' };
  }
  return {
    path: text.slice(0, match.index),
    suffix: text.slice(match.index)
  };
};

const extractHashSuffix = (value: string): string => {
  const hashIndex = String(value || '').indexOf('#');
  return hashIndex >= 0 ? String(value || '').slice(hashIndex) : '';
};

const safeDecodePath = (value: string): string =>
  String(value || '')
    .split('/')
    .map((part) => {
      try {
        return decodeURIComponent(part);
      } catch {
        return part;
      }
    })
    .join('/');

const isExternalOrSpecialUrl = (value: string): boolean => {
  const text = String(value || '').trim();
  if (!text || text.startsWith('#') || text.startsWith('?') || text.startsWith('//')) {
    return true;
  }
  return ABSOLUTE_URI_RE.test(text);
};

const normalizeWorkspacePublicResourcePath = (path: string): string => {
  const normalized = safeDecodePath(String(path || '').replace(/\\/g, '/')).replace(/^\/+/, '');
  if (!normalized.startsWith('workspaces/')) {
    return normalized;
  }
  const parts = normalized.split('/').filter(Boolean);
  if (parts.length < 3) {
    return '';
  }
  return parts.slice(2).join('/');
};

const resolveWorkspaceResourcePath = (rawUrl: string, baseDirectoryPath: string): string => {
  const text = String(rawUrl || '').trim();
  if (!text || isExternalOrSpecialUrl(text)) {
    return '';
  }

  const { path } = splitUrlPath(text.replace(/\\/g, '/'));
  if (!path) return '';
  if (path.startsWith('workspaces/')) {
    return normalizeWorkspacePath(normalizeWorkspacePublicResourcePath(path));
  }
  if (path.startsWith('/')) {
    const normalizedRootPath = path.replace(/\\/g, '/');
    if (
      normalizedRootPath.startsWith('/workspaces/') ||
      normalizedRootPath.startsWith('workspaces/')
    ) {
      return normalizeWorkspacePath(normalizeWorkspacePublicResourcePath(path));
    }
    return normalizeWorkspacePath(safeDecodePath(path).replace(/^\/+/, ''));
  }

  const baseParts = normalizeWorkspacePath(baseDirectoryPath).split('/').filter(Boolean);
  for (const segment of safeDecodePath(path).split('/')) {
    const token = segment.trim();
    if (!token || token === '.') continue;
    if (token === '..') {
      if (!baseParts.length) {
        return '';
      }
      baseParts.pop();
      continue;
    }
    baseParts.push(token);
  }
  return normalizeWorkspacePath(baseParts.join('/'));
};

const extensionFromPath = (path: string): string => {
  const filename = String(path || '').split('/').pop() || '';
  const dotIndex = filename.lastIndexOf('.');
  if (dotIndex < 0 || dotIndex === filename.length - 1) return '';
  return filename.slice(dotIndex + 1).toLowerCase();
};

const inferMimeType = (path: string, fallback = ''): string =>
  MIME_BY_EXTENSION[extensionFromPath(path)] || fallback || 'application/octet-stream';

const normalizeBlobType = (blob: Blob, path: string, fallbackType = ''): Blob => {
  const expectedType = inferMimeType(path, fallbackType);
  if (!expectedType || blob.type === expectedType) {
    return blob;
  }
  if (!blob.type || blob.type === 'application/octet-stream') {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob;
};

const replaceAsync = async (
  source: string,
  pattern: RegExp,
  replacer: (match: RegExpExecArray) => Promise<string>
): Promise<string> => {
  const matches = Array.from(source.matchAll(pattern));
  if (!matches.length) return source;
  const replacements = await Promise.all(matches.map((match) => replacer(match)));
  let output = '';
  let cursor = 0;
  matches.forEach((match, index) => {
    const start = match.index ?? 0;
    output += source.slice(cursor, start);
    output += replacements[index] ?? match[0];
    cursor = start + match[0].length;
  });
  output += source.slice(cursor);
  return output;
};

const buildObjectUrl = (blob: Blob, ctx: ResourceBuildContext): string => {
  const url = URL.createObjectURL(blob);
  ctx.objectUrls.push(url);
  return url;
};

const rewriteCssResources = async (
  cssText: string,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<string> => {
  const withUrls = await replaceAsync(cssText, CSS_URL_RE, async (match) => {
    const rawUrl = String(match[2] || '').trim();
    const resolvedUrl = await createWorkspaceResourceObjectUrl(rawUrl, baseDirectoryPath, ctx);
    if (!resolvedUrl) return match[0];
    return `url("${resolvedUrl}")`;
  });

  return replaceAsync(withUrls, CSS_IMPORT_RE, async (match) => {
    const rawUrl = String(match[2] || '').trim();
    const resolvedUrl = await createWorkspaceResourceObjectUrl(rawUrl, baseDirectoryPath, ctx);
    if (!resolvedUrl) return match[0];
    return `@import "${resolvedUrl}"`;
  });
};

const rewriteJsModuleResources = async (
  jsText: string,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<string> =>
  replaceAsync(jsText, JS_IMPORT_RE, async (match) => {
    const rawUrl = String(match[3] || '').trim();
    const resolvedUrl = await createWorkspaceResourceObjectUrl(rawUrl, baseDirectoryPath, ctx);
    if (!resolvedUrl) return match[0];
    return `${match[1]}${match[2]}${resolvedUrl}${match[2]}`;
  });

const createWorkspaceResourceObjectUrl = async (
  rawUrl: string,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<string> => {
  const relativePath = resolveWorkspaceResourcePath(rawUrl, baseDirectoryPath);
  if (!relativePath) return '';
  const hashSuffix = extractHashSuffix(rawUrl);
  const withHashSuffix = (url: string): string => (url && hashSuffix ? `${url}${hashSuffix}` : url);

  const cacheKey = relativePath;
  const cached = ctx.resourceCache.get(cacheKey);
  if (cached) return cached.then(withHashSuffix);

  const request = (async () => {
    try {
      const blob = await ctx.fetchResource(relativePath);
      const extension = extensionFromPath(relativePath);
      if (extension === 'css') {
        const cssText = await blob.text();
        const rewrittenCss = await rewriteCssResources(
          cssText,
          getWorkspaceParentPath(relativePath),
          ctx
        );
        return buildObjectUrl(new Blob([rewrittenCss], { type: 'text/css' }), ctx);
      }
      if (extension === 'js' || extension === 'mjs') {
        const jsText = await blob.text();
        const rewrittenJs = await rewriteJsModuleResources(
          jsText,
          getWorkspaceParentPath(relativePath),
          ctx
        );
        return buildObjectUrl(new Blob([rewrittenJs], { type: 'text/javascript' }), ctx);
      }
      return buildObjectUrl(normalizeBlobType(blob, relativePath), ctx);
    } catch {
      return '';
    }
  })();
  ctx.resourceCache.set(cacheKey, request);
  return request.then(withHashSuffix);
};

const rewriteSrcset = async (
  value: string,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<string> => {
  const candidates = String(value || '')
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
  if (!candidates.length) return value;
  const rewritten = await Promise.all(
    candidates.map(async (candidate) => {
      const [rawUrl, ...descriptor] = candidate.split(/\s+/);
      const resolvedUrl = await createWorkspaceResourceObjectUrl(rawUrl, baseDirectoryPath, ctx);
      return [resolvedUrl || rawUrl, ...descriptor].join(' ');
    })
  );
  return rewritten.join(', ');
};

const rewriteElementAttribute = async (
  element: Element,
  attributeName: string,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<void> => {
  const rawValue = element.getAttribute(attributeName);
  if (!rawValue) return;
  const resolvedUrl = await createWorkspaceResourceObjectUrl(rawValue, baseDirectoryPath, ctx);
  if (resolvedUrl) {
    element.setAttribute(attributeName, resolvedUrl);
  }
};

const rewriteDocumentResources = async (
  document: Document,
  baseDirectoryPath: string,
  ctx: ResourceBuildContext
): Promise<void> => {
  const rewrites: Promise<void>[] = [];
  document.querySelectorAll('link[href]').forEach((element) => {
    rewrites.push(rewriteElementAttribute(element, 'href', baseDirectoryPath, ctx));
  });
  document
    .querySelectorAll('script[src], img[src], video[src], audio[src], source[src], track[src], embed[src], input[src]')
    .forEach((element) => {
      rewrites.push(rewriteElementAttribute(element, 'src', baseDirectoryPath, ctx));
    });
  document.querySelectorAll('object[data]').forEach((element) => {
    rewrites.push(rewriteElementAttribute(element, 'data', baseDirectoryPath, ctx));
  });
  document.querySelectorAll('[poster]').forEach((element) => {
    rewrites.push(rewriteElementAttribute(element, 'poster', baseDirectoryPath, ctx));
  });
  document.querySelectorAll('img[srcset], source[srcset]').forEach((element) => {
    rewrites.push(
      (async () => {
        const rawValue = element.getAttribute('srcset');
        if (!rawValue) return;
        element.setAttribute('srcset', await rewriteSrcset(rawValue, baseDirectoryPath, ctx));
      })()
    );
  });
  document.querySelectorAll('style').forEach((element) => {
    rewrites.push(
      (async () => {
        element.textContent = await rewriteCssResources(
          element.textContent || '',
          baseDirectoryPath,
          ctx
        );
      })()
    );
  });
  document.querySelectorAll('script:not([src])').forEach((element) => {
    const type = String(element.getAttribute('type') || '').trim().toLowerCase();
    if (type && type !== 'module' && type !== 'text/javascript' && type !== 'application/javascript') {
      return;
    }
    rewrites.push(
      (async () => {
        element.textContent = await rewriteJsModuleResources(
          element.textContent || '',
          baseDirectoryPath,
          ctx
        );
      })()
    );
  });
  document.querySelectorAll('[style]').forEach((element) => {
    rewrites.push(
      (async () => {
        const rawValue = element.getAttribute('style');
        if (!rawValue) return;
        element.setAttribute(
          'style',
          await rewriteCssResources(rawValue, baseDirectoryPath, ctx)
        );
      })()
    );
  });
  await Promise.all(rewrites);
};

const serializePreviewDocument = (document: Document, rawHtml: string): string => {
  const doctype = rawHtml.match(/^\s*<!doctype[^>]*>/i)?.[0] || '<!DOCTYPE html>';
  return `${doctype}\n${document.documentElement.outerHTML}`;
};

export const buildWorkspaceHtmlPreviewDocument = async ({
  rawHtml,
  entryPath,
  fetchResource
}: WorkspaceHtmlPreviewOptions): Promise<WorkspaceHtmlPreviewDocument> => {
  const ctx: ResourceBuildContext = {
    fetchResource,
    objectUrls: [],
    resourceCache: new Map()
  };
  const html = String(rawHtml || '');
  const baseDirectoryPath = getWorkspaceParentPath(entryPath);

  if (typeof DOMParser === 'undefined') {
    return { html, objectUrls: ctx.objectUrls };
  }

  const document = new DOMParser().parseFromString(html, 'text/html');
  if (!document.head.querySelector('meta[charset]')) {
    const meta = document.createElement('meta');
    meta.setAttribute('charset', 'UTF-8');
    document.head.prepend(meta);
  }
  await rewriteDocumentResources(document, baseDirectoryPath, ctx);
  return {
    html: serializePreviewDocument(document, html),
    objectUrls: ctx.objectUrls
  };
};
