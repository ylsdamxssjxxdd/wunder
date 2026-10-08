import type { ToolWorkflowStructuredView } from './toolWorkflowTypes';

type Row = Record<string, unknown>;
type Translate = (key: string, params?: Record<string, unknown>) => string;
const object = (value: unknown): Row | null =>
  value && typeof value === 'object' && !Array.isArray(value) ? value as Row : null;
const text = (value: unknown) => typeof value === 'string' ? value : '';

export const buildScheduleResultView = (data: Row, t: Translate): ToolWorkflowStructuredView => {
  const label = (key: string, params?: Record<string, unknown>) => t(`chat.toolWorkflow.schedule.${key}`, params);
  const time = (value: unknown) => {
    if (value === null || value === undefined || value === '') return label('none');
    const date = new Date(typeof value === 'number' ? value * 1000 : String(value));
    return Number.isFinite(date.getTime()) ? `${date.toLocaleString()} (${Intl.DateTimeFormat().resolvedOptions().timeZone})` : text(value);
  };
  const action = text(data.action);
  const actions = ['add', 'update', 'list', 'get', 'enable', 'disable', 'remove', 'run', 'status'];
  const jobs = Array.isArray(data.jobs) ? data.jobs : data.job ? [data.job] : [];
  const userJobs = object(data.user_jobs);
  const metrics: ToolWorkflowStructuredView['metrics'] = [
    { key: 'action', label: label('action'), value: actions.includes(action) ? label(action) : action },
    { key: 'count', label: label('count'), value: String(userJobs?.total ?? jobs.length) }
  ];
  const scheduler = object(data.scheduler);
  if (typeof scheduler?.enabled === 'boolean') metrics.push({ key: 'scheduler', label: label('scheduler'),
    value: label(scheduler.enabled ? 'enabled' : 'disabled'), tone: scheduler.enabled ? 'success' : 'warning' });
  const rows: ToolWorkflowStructuredView['groups'][number]['rows'] = jobs.slice(0, 16).map(object).filter((job): job is Row => !!job).map((job, index) => {
    const schedule = object(job.schedule) ?? {};
    const kind = text(schedule.kind);
    const rule = kind === 'at' ? time(schedule.at)
      : kind === 'every' ? label('every', { seconds: Number(schedule.every_ms) / 1000 })
        : [text(schedule.cron), text(schedule.tz)].filter(Boolean).join(' · ');
    const status = job.running === true ? 'running' : job.enabled === false ? 'disabled' : 'waiting';
    const last = text(job.last_status);
    const lastLabel = ['ok', 'error', 'skipped'].includes(last) ? label(last) : last || label('never');
    return { key: String(job.job_id ?? index), title: text(job.name) || label('task'),
      meta: label(status), tone: job.last_error ? 'danger' : job.running ? 'success' : 'default',
      body: [
        `${label('schedule')}: ${rule || label('none')}`,
        `${label('next')}: ${time(job.next_run_at)}`,
        `${label('last')}: ${lastLabel}${job.last_run_at ? ` · ${time(job.last_run_at)}` : ''}`,
        `${label('delivery')}: ${label(job.session_target === 'isolated' ? 'isolated' : 'main')}`,
        job.session_id ? `${label('thread')}: ${text(job.session_id)}` : '',
        job.last_error ? `${label('error')}: ${text(job.last_error)}` : ''
      ].filter(Boolean).join('\n') };
  });
  if (jobs.length > rows.length) rows.push({ key: 'more', title: label('more', { count: jobs.length - rows.length }) });
  if (!rows.length && action !== 'status') rows.push({ key: 'empty', title: label(action === 'remove' ? 'removed' : 'empty'),
    body: text(data.reason) || text(data.last_error) });
  if (action === 'status' && scheduler) rows.push({ key: 'status', title: label('status'),
    body: `${label('running')}: ${Number(userJobs?.running ?? scheduler.running_jobs ?? 0)}\n${label('next')}: ${time(userJobs?.next_run_at ?? scheduler.next_run_at)}` });
  return { variant: 'schedule', metrics, groups: [{ key: 'jobs', rows }] };
};
