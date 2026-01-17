import { defineStore } from 'pinia';

const PERFORMANCE_STORAGE_KEY = 'wille-performance-mode';
const PERFORMANCE_MODES = ['low', 'high'];

const normalizePerformanceMode = (value) => {
  if (PERFORMANCE_MODES.includes(value)) {
    return value;
  }
  return 'low';
};

const readPerformanceFromStorage = () =>
  normalizePerformanceMode(localStorage.getItem(PERFORMANCE_STORAGE_KEY));

const applyPerformanceToDocument = (mode) => {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-performance-mode', mode);
};

export const usePerformanceStore = defineStore('performance', {
  state: () => {
    const mode = readPerformanceFromStorage();
    applyPerformanceToDocument(mode);
    return {
      mode
    };
  },
  actions: {
    setMode(mode) {
      const nextMode = normalizePerformanceMode(mode);
      this.mode = nextMode;
      localStorage.setItem(PERFORMANCE_STORAGE_KEY, nextMode);
      applyPerformanceToDocument(nextMode);
    },
    toggleMode() {
      const nextMode = this.mode === 'low' ? 'high' : 'low';
      this.setMode(nextMode);
    }
  }
});
