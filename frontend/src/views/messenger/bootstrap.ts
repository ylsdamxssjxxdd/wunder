import type { MessengerSection } from '@/stores/sessionHub';

export type MessengerBootstrapTask = {
  run: () => Promise<unknown>;
  critical?: boolean;
  sections?: MessengerSection[];
};

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

export const scheduleMessengerBootstrapBackgroundTasks = (
  tasks: MessengerBootstrapTask[]
): void => {
  if (!tasks.length) {
    return;
  }
  const runAll = () => {
    void Promise.allSettled(tasks.map((task) => task.run()));
  };
  if (typeof window === 'undefined') {
    runAll();
    return;
  }
  if (typeof window.requestIdleCallback === 'function') {
    window.requestIdleCallback(runAll, { timeout: 1200 });
    return;
  }
  window.setTimeout(runAll, 16);
};
