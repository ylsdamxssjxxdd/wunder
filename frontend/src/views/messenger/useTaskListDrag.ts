import { onBeforeUnmount, onMounted, ref, type Ref } from 'vue';

// Resolve the drop slot from scroll coordinates, not recycled virtual row indices.
export function useTaskListDrag(options: {
  viewport: Ref<HTMLElement | null>;
  items: Readonly<Ref<{ id: string }[]>>;
  rowHeight: number;
  syncViewport: () => void;
  moveItem: (key: string, target: string, position: 'before' | 'after') => void;
}) {
  const dragState = ref({ key: '', targetKey: '', position: 'before' as 'before' | 'after' });
  let frame = 0;
  let pointerY: number | null = null;
  const stopScroll = () => {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    pointerY = null;
  };
  const resetDrag = () => {
    stopScroll();
    dragState.value = { key: '', targetKey: '', position: 'before' };
  };
  const updateTarget = (clientY: number) => {
    const element = options.viewport.value;
    const items = options.items.value;
    if (!element || !items.length) return;
    const rect = element.getBoundingClientRect();
    const offset = element.scrollTop + Math.max(0, Math.min(element.clientHeight, clientY - rect.top));
    const index = Math.max(0, Math.min(items.length - 1, Math.floor(offset / options.rowHeight)));
    const target = items[index];
    dragState.value.targetKey = target.id === dragState.value.key ? '' : target.id;
    dragState.value.position = offset - index * options.rowHeight < options.rowHeight / 2 ? 'before' : 'after';
  };
  const scrollStep = () => {
    frame = 0;
    const element = options.viewport.value;
    if (!element || pointerY === null || !dragState.value.key) return;
    const rect = element.getBoundingClientRect();
    const edge = Math.min(72, rect.height / 4);
    const distance = pointerY < rect.top + edge
      ? pointerY - rect.top - edge
      : pointerY > rect.bottom - edge ? pointerY - rect.bottom + edge : 0;
    if (!distance) return;
    const previous = element.scrollTop;
    element.scrollTop += Math.sign(distance) * Math.max(4, Math.min(22, Math.abs(distance) / 3));
    options.syncViewport();
    updateTarget(pointerY);
    // Stop at the boundary instead of spinning an idle animation loop.
    if (element.scrollTop !== previous) frame = requestAnimationFrame(scrollStep);
  };
  const handleDragStart = (event: DragEvent, key: string) => {
    resetDrag();
    dragState.value.key = key;
    if (event.dataTransfer) {
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData('text/plain', key);
    }
  };
  const handleDragOver = (event: DragEvent) => {
    if (!dragState.value.key) return;
    event.preventDefault();
    stopScroll();
    pointerY = event.clientY;
    updateTarget(pointerY);
    frame = requestAnimationFrame(scrollStep);
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
  };
  const handleDragLeave = (event: DragEvent) => {
    const element = options.viewport.value;
    if (element && event.relatedTarget instanceof Node && element.contains(event.relatedTarget)) return;
    stopScroll();
    dragState.value.targetKey = '';
  };
  const handleDrop = (event: DragEvent) => {
    if (!dragState.value.key) return;
    event.preventDefault();
    updateTarget(event.clientY);
    const { key, targetKey, position } = dragState.value;
    if (targetKey) options.moveItem(key, targetKey, position);
    resetDrag();
  };
  // The source row can leave the virtual window, so cleanup cannot depend on it.
  onMounted(() => {
    window.addEventListener('dragend', resetDrag);
    window.addEventListener('drop', resetDrag);
    window.addEventListener('blur', resetDrag);
  });
  onBeforeUnmount(() => {
    resetDrag();
    window.removeEventListener('dragend', resetDrag);
    window.removeEventListener('drop', resetDrag);
    window.removeEventListener('blur', resetDrag);
  });
  return { dragState, resetDrag, handleDragStart, handleDragOver, handleDragLeave, handleDrop };
}
