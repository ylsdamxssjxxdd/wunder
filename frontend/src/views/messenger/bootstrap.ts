import type { MessengerSection } from '@/stores/sessionHub';

export type MessengerBootstrapTask = {
  run: () => Promise<unknown>;
  critical?: boolean;
  sections?: MessengerSection[];
};

const MESSENGER_BOOTSTRAP_BACKGROUND_BATCH_SIZE = 2;
const MESSENGER_BOOTSTRAP_IDLE_TIMEOUT = 1200;
const MESSENGER_BOOTSTRAP_FALLBACK_DELAY_MS = 16;

const shouldRunAsCritical = (
  currentSection: MessengerSection,
  task: MessengerBootstrapTask
): boolean => {
  if (task.critical) {
    return true;
  }
  return Array.isArray(task.sections) && task.sections.includes(currentSection);
};

export const splitMessengerBootstrapTasks = (
  currentSection: MessengerSection,
  tasks: MessengerBootstrapTask[]
): { critical: MessengerBootstrapTask[]; background: MessengerBootstrapTask[] } => {
  const critical: MessengerBootstrapTask[] = [];
  const background: MessengerBootstrapTask[] = [];
  tasks.forEach((task) => {
    if (shouldRunAsCritical(currentSection, task)) {
      critical.push(task);
      return;
    }
    background.push(task);
  });
  return { critical, background };
};

export const settleMessengerBootstrapTasks = async (
  tasks: MessengerBootstrapTask[]
): Promise<void> => {
  if (!tasks.length) {
    return;
  }
  await Promise.allSettled(tasks.map((task) => task.run()));
};

const scheduleMessengerBootstrapTaskRunner = (runner: () => void): void => {
  if (typeof window === 'undefined') {
    runner();
    return;
  }
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(runner, { timeout: MESSENGER_BOOTSTRAP_IDLE_TIMEOUT });
    return;
  }
  window.setTimeout(runner, MESSENGER_BOOTSTRAP_FALLBACK_DELAY_MS);
};

export const scheduleMessengerBootstrapBackgroundTasks = (
  tasks: MessengerBootstrapTask[]
): void => {
  if (!tasks.length) {
    return;
  }
  const pendingTasks = tasks.slice();
  const runNextBatch = () => {
    const batch = pendingTasks.splice(0, MESSENGER_BOOTSTRAP_BACKGROUND_BATCH_SIZE);
    if (!batch.length) {
      return;
    }
    void settleMessengerBootstrapTasks(batch).finally(() => {
      if (!pendingTasks.length) {
        return;
      }
      scheduleMessengerBootstrapTaskRunner(runNextBatch);
    });
  };
  scheduleMessengerBootstrapTaskRunner(runNextBatch);
};
