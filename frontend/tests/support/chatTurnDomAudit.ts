// Runs in the browser, independently of the application projection/reducer.
export function installChatTurnDomAudit() {
  const storageKey = 'fixture-chat-turn-audit';
  const audit = JSON.parse(sessionStorage.getItem(storageKey) || 'null') || {
    duplicate: 0, structure: 0, completionNotices: 0, completionEvents: 0, samples: 0
  };
  const inspect = (container: ParentNode): number => {
    let violations = 0;
    const roots = new Set<string>();
    container.querySelectorAll<HTMLElement>('.messenger-turn').forEach(turn => {
      const root = turn.dataset.rootTurnId;
      if (!root || roots.has(root)) violations++;
      if (root) roots.add(root);
      const slots = Array.from(turn.children) as HTMLElement[];
      if (slots.length !== 2 || slots[0]?.dataset.turnSlot !== 'user' || slots[1]?.dataset.turnSlot !== 'assistant') violations++;
      for (const [index, slot] of slots.entries()) {
        const bubbles = slot.querySelectorAll<HTMLElement>('.messenger-message');
        if (bubbles.length !== 1 || bubbles[0]?.dataset.turnId !== root ||
            bubbles[0]?.classList.contains('mine') !== (index === 0)) violations++;
        if (index === 1 && ['final', 'failed', 'cancelled'].includes(bubbles[0]?.dataset.messageStatus || '') &&
            /Queued|正在排队/.test(bubbles[0]?.querySelector('.messenger-message-stats')?.textContent || '')) violations++;
      }
    });
    container.querySelectorAll('.messenger-message[data-turn-id]').forEach(bubble => {
      if (!bubble.closest('.messenger-turn, .messenger-greeting-region')) violations++;
    });
    return violations;
  };
  (window as any).__chatInspectTurns = inspect;
  (window as any).__chatRenderAudit = audit;
  const persist = () => sessionStorage.setItem(storageKey, JSON.stringify(audit));
  window.addEventListener('wunder:agent-runtime-refresh', event => {
    audit.completionEvents += (event as CustomEvent).detail?.completedTurns?.length || 0;
    persist();
  });
  const completionNodes = new WeakSet<Element>();
  new MutationObserver(() => {
    // MutationObserver batches a synchronous Vue patch, but still catches
    // intermediate renders that a periodic 50 ms poll would miss.
    audit.structure += inspect(document);
    audit.samples++;
    const seen = new Set<string>();
    document.querySelectorAll<HTMLElement>('.messenger-message[data-turn-id]:not(.mine)').forEach(row => {
      if (!row.dataset.turnId) return;
      if (seen.has(row.dataset.turnId)) audit.duplicate++;
      seen.add(row.dataset.turnId);
    });
    document.querySelectorAll('.el-message--success').forEach(node => {
      if (completionNodes.has(node) || !/(has completed the task|已完成任务)/.test(node.textContent || '')) return;
      completionNodes.add(node);
      audit.completionNotices++;
    });
    persist();
  }).observe(document, { childList: true, subtree: true, attributes: true, characterData: true });
}
