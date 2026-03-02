type RegionPickerLabels = {
  title?: string;
  hint?: string;
  cancelText?: string;
};

type Rect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

const clamp = (value: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, value));

const normalizeRect = (startX: number, startY: number, endX: number, endY: number): Rect => {
  const left = Math.min(startX, endX);
  const top = Math.min(startY, endY);
  const width = Math.abs(endX - startX);
  const height = Math.abs(endY - startY);
  return { left, top, width, height };
};

export const pickScreenshotRegionFromDataUrl = async (
  dataUrl: string,
  labels: RegionPickerLabels = {}
): Promise<string | null> => {
  if (
    typeof window === 'undefined' ||
    typeof document === 'undefined' ||
    !String(dataUrl || '').startsWith('data:image/')
  ) {
    return null;
  }

  const image = await new Promise<HTMLImageElement | null>((resolve) => {
    const next = new Image();
    next.onload = () => resolve(next);
    next.onerror = () => resolve(null);
    next.src = dataUrl;
  });

  if (!image || !image.naturalWidth || !image.naturalHeight) {
    return null;
  }

  return new Promise<string | null>((resolve) => {
    let done = false;
    let dragStartX = 0;
    let dragStartY = 0;
    let dragging = false;
    let displayWidth = 0;
    let displayHeight = 0;

    const overlay = document.createElement('div');
    overlay.style.position = 'fixed';
    overlay.style.inset = '0';
    overlay.style.background = 'rgba(2, 6, 23, 0.72)';
    overlay.style.zIndex = '2147483000';
    overlay.style.display = 'flex';
    overlay.style.flexDirection = 'column';
    overlay.style.alignItems = 'center';
    overlay.style.justifyContent = 'center';
    overlay.style.gap = '14px';
    overlay.style.userSelect = 'none';

    const toolbar = document.createElement('div');
    toolbar.style.display = 'flex';
    toolbar.style.alignItems = 'center';
    toolbar.style.gap = '14px';
    toolbar.style.padding = '10px 14px';
    toolbar.style.borderRadius = '10px';
    toolbar.style.border = '1px solid rgba(148, 163, 184, 0.34)';
    toolbar.style.background = 'rgba(15, 23, 42, 0.86)';
    toolbar.style.color = '#e2e8f0';
    toolbar.style.fontSize = '13px';
    toolbar.style.lineHeight = '1.4';

    const titleText = document.createElement('div');
    titleText.textContent = String(labels.title || '').trim() || 'Select screenshot region';
    titleText.style.fontWeight = '600';
    toolbar.appendChild(titleText);

    const hintText = document.createElement('div');
    hintText.textContent =
      String(labels.hint || '').trim() || 'Drag to select area. Press Esc to cancel.';
    hintText.style.color = '#94a3b8';
    toolbar.appendChild(hintText);

    const cancelButton = document.createElement('button');
    cancelButton.type = 'button';
    cancelButton.textContent = String(labels.cancelText || '').trim() || 'Cancel';
    cancelButton.style.border = '1px solid rgba(148, 163, 184, 0.5)';
    cancelButton.style.background = 'rgba(15, 23, 42, 0.9)';
    cancelButton.style.color = '#e2e8f0';
    cancelButton.style.borderRadius = '8px';
    cancelButton.style.padding = '6px 10px';
    cancelButton.style.cursor = 'pointer';
    toolbar.appendChild(cancelButton);

    const stage = document.createElement('div');
    stage.style.position = 'relative';
    stage.style.border = '1px solid rgba(148, 163, 184, 0.42)';
    stage.style.borderRadius = '12px';
    stage.style.overflow = 'hidden';
    stage.style.cursor = 'crosshair';
    stage.style.boxShadow = '0 20px 42px rgba(2, 6, 23, 0.45)';
    stage.style.touchAction = 'none';

    const canvas = document.createElement('canvas');
    canvas.style.display = 'block';
    canvas.style.width = '100%';
    canvas.style.height = '100%';
    stage.appendChild(canvas);

    const selectionBox = document.createElement('div');
    selectionBox.style.position = 'absolute';
    selectionBox.style.pointerEvents = 'none';
    selectionBox.style.border = '1px solid #f97316';
    selectionBox.style.background = 'rgba(249, 115, 22, 0.2)';
    selectionBox.style.display = 'none';
    stage.appendChild(selectionBox);

    overlay.appendChild(toolbar);
    overlay.appendChild(stage);
    document.body.appendChild(overlay);

    const cleanup = (result: string | null) => {
      if (done) return;
      done = true;
      window.removeEventListener('resize', renderImageToCanvas);
      document.removeEventListener('keydown', handleKeydown, true);
      window.removeEventListener('mousemove', handleMouseMove, true);
      window.removeEventListener('mouseup', handleMouseUp, true);
      stage.removeEventListener('mousedown', handleMouseDown);
      cancelButton.removeEventListener('click', handleCancel);
      overlay.removeEventListener('contextmenu', preventContextMenu);
      if (overlay.parentNode) {
        overlay.parentNode.removeChild(overlay);
      }
      resolve(result);
    };

    const preventContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };

    const renderImageToCanvas = () => {
      const viewportWidth = Math.max(1, window.innerWidth);
      const viewportHeight = Math.max(1, window.innerHeight);
      const maxWidth = Math.max(320, viewportWidth - 64);
      const maxHeight = Math.max(200, viewportHeight - 154);
      const scale = Math.min(maxWidth / image.naturalWidth, maxHeight / image.naturalHeight, 1);
      displayWidth = Math.max(1, Math.floor(image.naturalWidth * scale));
      displayHeight = Math.max(1, Math.floor(image.naturalHeight * scale));
      stage.style.width = `${displayWidth}px`;
      stage.style.height = `${displayHeight}px`;
      canvas.width = displayWidth;
      canvas.height = displayHeight;
      const ctx = canvas.getContext('2d');
      if (!ctx) {
        cleanup(null);
        return;
      }
      ctx.clearRect(0, 0, displayWidth, displayHeight);
      ctx.drawImage(image, 0, 0, displayWidth, displayHeight);
      selectionBox.style.display = 'none';
    };

    const getStagePoint = (event: MouseEvent) => {
      const rect = stage.getBoundingClientRect();
      return {
        x: clamp(event.clientX - rect.left, 0, rect.width),
        y: clamp(event.clientY - rect.top, 0, rect.height)
      };
    };

    const updateSelectionBox = (left: number, top: number, width: number, height: number) => {
      selectionBox.style.display = 'block';
      selectionBox.style.left = `${left}px`;
      selectionBox.style.top = `${top}px`;
      selectionBox.style.width = `${width}px`;
      selectionBox.style.height = `${height}px`;
    };

    const cropSelection = (rect: Rect): string | null => {
      if (rect.width < 3 || rect.height < 3) return null;
      const sourceX = Math.floor((rect.left / displayWidth) * image.naturalWidth);
      const sourceY = Math.floor((rect.top / displayHeight) * image.naturalHeight);
      const sourceWidth = Math.max(1, Math.floor((rect.width / displayWidth) * image.naturalWidth));
      const sourceHeight = Math.max(1, Math.floor((rect.height / displayHeight) * image.naturalHeight));
      const clampedSourceWidth = Math.min(sourceWidth, image.naturalWidth - sourceX);
      const clampedSourceHeight = Math.min(sourceHeight, image.naturalHeight - sourceY);
      const output = document.createElement('canvas');
      output.width = clampedSourceWidth;
      output.height = clampedSourceHeight;
      const outputCtx = output.getContext('2d');
      if (!outputCtx) return null;
      outputCtx.drawImage(
        image,
        sourceX,
        sourceY,
        clampedSourceWidth,
        clampedSourceHeight,
        0,
        0,
        clampedSourceWidth,
        clampedSourceHeight
      );
      return output.toDataURL('image/png');
    };

    const handleMouseDown = (event: MouseEvent) => {
      if (event.button !== 0 || done) return;
      event.preventDefault();
      const point = getStagePoint(event);
      dragStartX = point.x;
      dragStartY = point.y;
      dragging = true;
      updateSelectionBox(point.x, point.y, 0, 0);
    };

    const handleMouseMove = (event: MouseEvent) => {
      if (!dragging || done) return;
      event.preventDefault();
      const point = getStagePoint(event);
      const rect = normalizeRect(dragStartX, dragStartY, point.x, point.y);
      updateSelectionBox(rect.left, rect.top, rect.width, rect.height);
    };

    const handleMouseUp = (event: MouseEvent) => {
      if (!dragging || done) return;
      event.preventDefault();
      dragging = false;
      const point = getStagePoint(event);
      const rect = normalizeRect(dragStartX, dragStartY, point.x, point.y);
      const cropped = cropSelection(rect);
      if (!cropped) {
        selectionBox.style.display = 'none';
        return;
      }
      cleanup(cropped);
    };

    const handleCancel = () => {
      cleanup(null);
    };

    const handleKeydown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      cleanup(null);
    };

    renderImageToCanvas();
    window.addEventListener('resize', renderImageToCanvas);
    document.addEventListener('keydown', handleKeydown, true);
    window.addEventListener('mousemove', handleMouseMove, true);
    window.addEventListener('mouseup', handleMouseUp, true);
    stage.addEventListener('mousedown', handleMouseDown);
    cancelButton.addEventListener('click', handleCancel);
    overlay.addEventListener('contextmenu', preventContextMenu);
  });
};
