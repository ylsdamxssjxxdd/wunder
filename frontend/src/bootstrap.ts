// Keep the entry free of application imports: even static imports execute before
// an awaited frame, pulling Vue, styles and route setup into the paint path.
const startApplication = async () => {
  await import('./main');
};

void startApplication().catch((error: unknown) => {
  console.error('[startup] Application loading failed', error);
});
