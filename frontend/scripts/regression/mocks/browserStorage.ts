// Imported before browser modules so their module-level preferences can load.
Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
  getItem: () => null, setItem: () => {}, removeItem: () => {}
} });
