import { useCallback, useRef, useState } from 'react';

export interface ImageZoomPanOptions {
  minScale?: number;
  maxScale?: number;
  initialScale?: number;
}

export interface ImageZoomPanReturn {
  scale: number;
  offset: { x: number; y: number };
  isDragging: boolean;
  isZoomed: boolean;
  zoomPercent: string;
  containerRef: React.RefObject<HTMLDivElement | null>;
  resetZoom: () => void;
  setScale: (scale: number) => void;
  onWheel: (e: React.WheelEvent<HTMLElement>) => void;
  onMouseDown: (e: React.MouseEvent<HTMLElement>) => void;
  onMouseMove: (e: React.MouseEvent<HTMLElement>) => void;
  onMouseUp: (e: React.MouseEvent<HTMLElement>) => void;
  onMouseLeave: (e: React.MouseEvent<HTMLElement>) => void;
  onDoubleClick: (e: React.MouseEvent<HTMLElement>) => void;
  transformStyle: React.CSSProperties;
}

export function useImageZoomPan(options: ImageZoomPanOptions = {}): ImageZoomPanReturn {
  const { minScale = 0.5, maxScale = 5.0, initialScale = 1.0 } = options;

  const [scale, setScaleState] = useState(initialScale);
  const [offset, setOffset] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const dragStartRef = useRef<{
    startX: number;
    startY: number;
    initialOffsetX: number;
    initialOffsetY: number;
  } | null>(null);

  const isZoomed = Math.abs(scale - 1.0) > 0.02 || Math.abs(offset.x) > 2 || Math.abs(offset.y) > 2;
  const zoomPercent = `${Math.round(scale * 100)}%`;

  const resetZoom = useCallback(() => {
    setScaleState(1.0);
    setOffset({ x: 0, y: 0 });
    setIsDragging(false);
    dragStartRef.current = null;
  }, []);

  const setScale = useCallback(
    (newScale: number) => {
      const clamped = Math.min(maxScale, Math.max(minScale, newScale));
      setScaleState(clamped);
      if (Math.abs(clamped - 1.0) < 0.01) {
        setOffset({ x: 0, y: 0 });
      }
    },
    [minScale, maxScale],
  );

  const onWheel = useCallback(
    (e: React.WheelEvent<HTMLElement>) => {
      e.preventDefault();
      e.stopPropagation();

      const container = containerRef.current;
      const rect = container?.getBoundingClientRect();

      // Mouse position relative to center of container
      const mouseX = rect ? e.clientX - rect.left - rect.width / 2 : 0;
      const mouseY = rect ? e.clientY - rect.top - rect.height / 2 : 0;

      // Wheel delta step: smooth exponential zoom
      const zoomFactor = e.deltaY < 0 ? 1.15 : 1 / 1.15;
      const newScale = Math.min(maxScale, Math.max(minScale, scale * zoomFactor));

      if (newScale === scale) return;

      // Zoom toward cursor:
      // invariant: mouse point on image stays under mouse cursor
      const scaleRatio = newScale / scale;
      const newOffsetX = mouseX - (mouseX - offset.x) * scaleRatio;
      const newOffsetY = mouseY - (mouseY - offset.y) * scaleRatio;

      setScaleState(newScale);
      setOffset({
        x: Math.abs(newScale - 1.0) < 0.01 ? 0 : Math.round(newOffsetX),
        y: Math.abs(newScale - 1.0) < 0.01 ? 0 : Math.round(newOffsetY),
      });
    },
    [scale, offset, minScale, maxScale],
  );

  const onMouseDown = useCallback(
    (e: React.MouseEvent<HTMLElement>) => {
      // Left click only
      if (e.button !== 0) return;

      setIsDragging(true);
      dragStartRef.current = {
        startX: e.clientX,
        startY: e.clientY,
        initialOffsetX: offset.x,
        initialOffsetY: offset.y,
      };
    },
    [offset],
  );

  const onMouseMove = useCallback(
    (e: React.MouseEvent<HTMLElement>) => {
      if (!isDragging || !dragStartRef.current) return;

      const dx = e.clientX - dragStartRef.current.startX;
      const dy = e.clientY - dragStartRef.current.startY;

      const container = containerRef.current;
      const containerWidth = container?.clientWidth || 300;
      const containerHeight = container?.clientHeight || 200;

      // Prevent moving completely out of view:
      // Allow panning up to container dimensions multiplied by scale
      const maxAllowedX = Math.max(containerWidth * 0.8, containerWidth * scale * 0.5);
      const maxAllowedY = Math.max(containerHeight * 0.8, containerHeight * scale * 0.5);

      const targetX = dragStartRef.current.initialOffsetX + dx;
      const targetY = dragStartRef.current.initialOffsetY + dy;

      setOffset({
        x: Math.min(maxAllowedX, Math.max(-maxAllowedX, targetX)),
        y: Math.min(maxAllowedY, Math.max(-maxAllowedY, targetY)),
      });
    },
    [isDragging, scale],
  );

  const onMouseUp = useCallback(() => {
    setIsDragging(false);
    dragStartRef.current = null;
  }, []);

  const onMouseLeave = useCallback(() => {
    setIsDragging(false);
    dragStartRef.current = null;
  }, []);

  const onDoubleClick = useCallback(
    (e: React.MouseEvent<HTMLElement>) => {
      e.preventDefault();
      e.stopPropagation();
      resetZoom();
    },
    [resetZoom],
  );

  const transformStyle: React.CSSProperties = {
    transform: `translate3d(${offset.x}px, ${offset.y}px, 0) scale(${scale})`,
    transformOrigin: 'center center',
    transition: isDragging ? 'none' : 'transform 0.08s ease-out',
    userSelect: 'none',
  };

  return {
    scale,
    offset,
    isDragging,
    isZoomed,
    zoomPercent,
    containerRef,
    resetZoom,
    setScale,
    onWheel,
    onMouseDown,
    onMouseMove,
    onMouseUp,
    onMouseLeave,
    onDoubleClick,
    transformStyle,
  };
}
