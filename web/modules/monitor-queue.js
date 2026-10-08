import { getWunderBase } from './api.js';
import { notify } from './notify.js';
import { t } from './i18n.js?v=20260710-01';

export const appendQueuePriorityAction = (container, session, refresh) => {
  if (!['queued', 'waiting'].includes(String(session.status || '').toLowerCase())) return;
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = t('monitor.queue.priority');
  button.title = t('monitor.queue.priorityHint');
  button.addEventListener('click', async event => {
    event.stopPropagation();
    if (button.disabled || !window.confirm(t('monitor.queue.priorityConfirm'))) return;
    button.disabled = true;
    try {
      const response = await fetch(`${getWunderBase()}/admin/monitor/${encodeURIComponent(session.session_id)}/priority`, { method: 'POST' });
      if (!response.ok) throw new Error(t('monitor.queue.priorityFailed'));
      const result = await response.json();
      notify(t(result.pause_requested ? 'monitor.queue.pausing' : 'monitor.queue.prioritized'), 'info');
      await refresh();
    } catch (error) {
      notify(error.message || t('monitor.queue.priorityFailed'), 'error');
    } finally {
      button.disabled = false;
    }
  });
  container.appendChild(button);
};
