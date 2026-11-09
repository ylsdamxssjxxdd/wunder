import { ElMessage } from 'element-plus';

import { t } from '@/i18n';

/**
 * 统一“下载”出口：文件一律交给浏览器下载器落盘，绝不内联打开或交给本机程序。
 *
 * - 锚点只带 `download`，不带 `target`，因此不会导航/预览；
 * - object URL 保留一段时间再释放，避免大文件刚起始就被撤销而下载失败；
 * - 默认弹出一次“已下载”提示，批量下载等自行汇总时置 `notify: false`。
 */
const OBJECT_URL_LIFETIME_MS = 60 * 1000;
const TOAST_NAME_MAX_LENGTH = 40;

export type DownloadFeedbackOptions = {
  notify?: boolean;
};

const resolveDownloadName = (filename: string): string => {
  const name = String(filename || '').trim();
  return name || 'download';
};

const shortenName = (name: string): string =>
  name.length <= TOAST_NAME_MAX_LENGTH ? name : `${name.slice(0, TOAST_NAME_MAX_LENGTH - 1)}…`;

const clickDownloadAnchor = (url: string, name: string): void => {
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.rel = 'noopener';
  link.style.display = 'none';
  document.body.appendChild(link);
  link.click();
  link.remove();
};

const notifyDownloaded = (name: string): void => {
  ElMessage.success(t('common.downloaded', { name: shortenName(name) }));
};

/** 另存一个 Blob（请求返回的原始字节）。 */
export const saveBlobAsDownload = (
  blob: Blob,
  filename: string,
  options: DownloadFeedbackOptions = {}
): void => {
  const name = resolveDownloadName(filename);
  const objectUrl = URL.createObjectURL(blob);
  clickDownloadAnchor(objectUrl, name);
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), OBJECT_URL_LIFETIME_MS);
  if (options.notify !== false) notifyDownloaded(name);
};

/** 另存已经存在的 object URL（例如预览缓存）；URL 生命周期由持有方管理。 */
export const saveObjectUrlAsDownload = (
  objectUrl: string,
  filename: string,
  options: DownloadFeedbackOptions = {}
): void => {
  const url = String(objectUrl || '').trim();
  if (!url) return;
  const name = resolveDownloadName(filename);
  clickDownloadAnchor(url, name);
  if (options.notify !== false) notifyDownloaded(name);
};
