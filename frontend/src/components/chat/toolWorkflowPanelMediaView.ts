import { asToolWorkflowResultObject as asObject } from './toolWorkflowResultPayload';
import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

type ObjectValue = Record<string, unknown>;
type Translate = (key: string, params?: Record<string, unknown>) => string;

const text = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      const trimmed = value.trim();
      return trimmed.length > 800 ? `${trimmed.slice(0, 800)}…` : trimmed;
    }
  }
  return '';
};

const statusLabel = (value: unknown, t: Translate): string => {
  switch (String(value || '').toLowerCase()) {
    case 'completed': case 'complete': case 'done': case 'ok': return t('chat.toolWorkflow.resultDone');
    case 'in_progress': case 'running': return t('chat.toolWorkflow.statusRunning');
    case 'pending': case 'awaiting_input': return t('chat.toolWorkflow.statusWaiting');
    default: return text(value);
  }
};

/** Panels and visual media carry structured data rather than stdout. Keep the
 * readable projection bounded and leave attachments/binary payloads untouched. */
export const buildPanelMediaResultView = (
  toolName: string,
  data: ObjectValue | null,
  args: ObjectValue | null,
  t: Translate
): ToolWorkflowStructuredView | null => {
  const name = toolName.trim().toLowerCase();
  if (['question_panel', 'ask_panel', 'question panel', '问询面板'].includes(name)) {
    const source = Array.isArray(data?.questions) || Array.isArray(data?.options) ? data : args;
    if (!source) return null;
    const questions = Array.isArray(source.questions) ? source.questions : [source];
    const groups: ToolWorkflowStructuredView['groups'] = [];
    for (const [index, raw] of questions.slice(0, 4).entries()) {
      const question = asObject(raw);
      if (!question) continue;
      const options = Array.isArray(question.options) ? question.options : [];
      groups.push({
        key: `question-${index}`,
        title: text(question.question) || t('chat.inquiry.defaultQuestion'),
        rows: [{
          key: `question-mode-${index}`,
          title: t(question.multiple === true ? 'chat.toolWorkflow.detail.multipleChoice' : 'chat.toolWorkflow.detail.singleChoice')
        }, ...options.slice(0, 4).flatMap((rawOption, optionIndex) => {
          const option = asObject(rawOption);
          const label = text(option?.label, rawOption);
          return label ? [{
            key: `option-${index}-${optionIndex}`,
            title: label,
            meta: option?.recommended === true ? t('chat.inquiry.recommended') : '',
            body: text(option?.description)
          }] : [];
        })]
      });
    }
    return groups.length ? { variant: 'question', metrics: [], groups } : null;
  }

  if (['update_plan', 'plan_update', '计划面板'].includes(name)) {
    const source = Array.isArray(data?.plan) ? data : args;
    if (!source || !Array.isArray(source.plan)) return null;
    const rows = source.plan.slice(0, 12).flatMap((raw, index) => {
      const step = asObject(raw);
      const title = text(step?.step);
      return title ? [{ key: `step-${index}`, title, meta: statusLabel(step?.status || 'pending', t) }] : [];
    });
    return rows.length ? {
      variant: 'plan', metrics: [],
      groups: [{ key: 'plan', title: text(source.explanation), rows }]
    } : null;
  }

  if (!['read_image', 'view_image', '\u8bfb\u56fe\u5de5\u5177', '\u8bfb\u56fe', '\u67e5\u770b\u56fe\u7247'].includes(name)) return null;
  const result = asObject(data?.result);
  const path = text(data?.path, args?.path, result?.name);
  if (!path && !result) return null;

  const finite = (value: unknown): number | null =>
    typeof value === 'number' && Number.isFinite(value) ? value : null;
  const dims = (source: ObjectValue | null, prefix: string): [number, number] | null => {
    const width = finite(source?.[`${prefix}width`]);
    const height = finite(source?.[`${prefix}height`]);
    return width && height ? [width, height] : null;
  };
  const baseName = (value: unknown): string => {
    const parts = text(value).split(/[\\/]/);
    return parts[parts.length - 1] || '';
  };
  const view = asObject(data?.view) || asObject(result?.view);
  const source = dims(view, 'source_');
  const seen = dims(view, '');
  const cropCorners = text(args?.crop_xyxy);
  const crop = text(args?.crop) || (cropCorners ? `xyxy:${cropCorners}` : '');
  const cropNumbers = args?.crop ? crop.split(',').map(part => Number(part.trim())) : [];
  const reference = dims(asObject(data?.crop), '') || (cropNumbers.length === 4 && cropNumbers[2] > 0 && cropNumbers[3] > 0
    ? [cropNumbers[2], cropNumbers[3]] as [number, number]
    : source);
  const kind = text(data?.media_kind, result?.kind).toLowerCase();
  const kindKey = ['image', 'video', 'gif'].includes(kind) ? `chat.toolWorkflow.detail.media.${kind}` : '';
  const bytes = finite(data?.size_bytes);
  const firstAttachment = asObject(Array.isArray(result?.attachments) ? result.attachments[0] : null);
  const mime = text(firstAttachment?.content_type, data?.mime_type);
  const format = mime.split('/').pop()?.split('+').pop()?.toUpperCase() || '';
  const lines: string[] = [];

  // 一次读图到底看了哪张图的哪一部分、看到什么数值，逐行交代；
  // 引擎的 read_image_display 是同一口径的单一来源。
  const kindLabel = kindKey ? t(kindKey) : kind;
  const sourceLabel = source ? `${source[0]}×${source[1]}` : '';
  lines.push([t('chat.toolWorkflow.detail.mediaSource'), kindLabel, sourceLabel].filter(Boolean).join(' '));

  if (bytes !== null) {
    const size = bytes >= 1024 * 1024
      ? `${(bytes / 1024 / 1024).toFixed(1)} MB`
      : bytes >= 1024 ? `${Math.round(bytes / 1024)} KB` : `${Math.round(bytes)} B`;
    lines.push(`${t('chat.toolWorkflow.detail.mediaSize')} ${size}${format ? ` · ${format}` : ''}`);
  }

  if (reference && seen && (seen[0] !== reference[0] || seen[1] !== reference[1])) {
    const ratio = (seen[0] / reference[0]) * 100;
    const verb = t(ratio < 100
      ? 'chat.toolWorkflow.detail.mediaDownscaled'
      : 'chat.toolWorkflow.detail.mediaUpscaled');
    const percent = Math.round(ratio < 100 ? ratio : 10000 / ratio);
    lines.push(`${t('chat.toolWorkflow.detail.mediaView')} ${seen[0]}×${seen[1]} · ${verb} ${percent}%`);
  }
  if (crop) lines.push(`${t('chat.toolWorkflow.detail.mediaCrop')} ${crop}`);

  const scale = finite(args?.scale);
  if (scale !== null) lines.push(`${t('chat.toolWorkflow.detail.mediaScale')} ${scale}×`);

  const stats = asObject(data?.stats);
  if (stats) {
    const parts: string[] = [];
    const luma = finite(stats.mean_luma);
    if (luma !== null) parts.push(`${t('chat.toolWorkflow.detail.mediaLuma')} ${luma.toFixed(1)}`);
    const rgb = Array.isArray(stats.background_rgb) ? stats.background_rgb : [];
    if (rgb.length === 3) {
      const hex = rgb
        .map(channel => Math.max(0, Math.min(255, finite(channel) ?? 0)).toString(16).padStart(2, '0'))
        .join('');
      parts.push(`${t('chat.toolWorkflow.detail.mediaBackground')} #${hex}`);
    }
    const nonBackground = finite(stats.non_background_ratio);
    if (nonBackground !== null) {
      parts.push(`${t('chat.toolWorkflow.detail.mediaNonBackground')} ${(nonBackground * 100).toFixed(1)}%`);
    }
    const edges = finite(stats.edge_ratio);
    if (edges !== null) parts.push(`${t('chat.toolWorkflow.detail.mediaEdges')} ${(edges * 100).toFixed(1)}%`);
    if (parts.length) lines.push(`${t('chat.toolWorkflow.detail.mediaStats')} ${parts.join(' · ')}`);
  }

  const frames = finite(result?.frame_count);
  const totalFrames = finite(result?.total_frame_count);
  const rate = finite(result?.applied_frame_rate) ?? finite(result?.requested_frame_rate);
  if (frames !== null && frames > 0 && (kind === 'video' || kind === 'gif'
    || (totalFrames !== null && totalFrames > frames))) {
    const parts = totalFrames !== null && totalFrames > frames
      ? [`${frames} / ${totalFrames}`]
      : [String(frames)];
    if (rate !== null) parts.push(`${rate} fps`);
    lines.push(`${t('chat.toolWorkflow.detail.mediaFrames')} ${parts.join(' · ')}`);
  }

  const duration = finite(result?.duration_ms);
  if (duration !== null && duration > 0) {
    const audio = result?.has_audio === true
      ? ` · ${t('chat.toolWorkflow.detail.mediaHasAudio')}`
      : '';
    lines.push(`${t('chat.toolWorkflow.detail.mediaDuration')} ${(duration / 1000).toFixed(1)}s${audio}`);
  }

  const compare = asObject(data?.compare);
  if (compare) {
    const right = text(compare.right_path);
    lines.push(`${t('chat.toolWorkflow.detail.mediaCompare')}${right ? ` · ${right}` : ''}`);
  }

  if (data?.unchanged_since_last_read === true) {
    const fingerprint = text(data?.sha256_8);
    lines.push(`${t('chat.toolWorkflow.detail.mediaUnchanged')}${fingerprint ? ` · ${fingerprint}` : ''}`);
  }

  const sourceName = baseName(path);
  const outputs = (Array.isArray(result?.attachments) ? result.attachments : [])
    .map(raw => {
      const item = asObject(raw);
      return text(item?.name) || baseName(item?.public_path);
    })
    .filter(name => name && name !== sourceName)
    .slice(0, 2);
  if (outputs.length) {
    lines.push(`${t('chat.toolWorkflow.detail.mediaOutput')} ${outputs.join(' · ')}`);
  }

  const warnings = (Array.isArray(result?.warnings) ? result.warnings : [])
    .slice(0, 4)
    .map(value => text(value))
    .filter(Boolean);
  lines.push(...warnings);

  return {
    variant: 'media', metrics: [],
    groups: [{
      key: 'media',
      rows: [{
        key: 'media-source',
        title: path || t('chat.toolWorkflow.toolLabel.readImage'),
        body: lines.join('\n')
      }]
    }]
  };
};
