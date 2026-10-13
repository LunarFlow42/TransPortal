import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { loadSettings } from '../store/persistence';
import type { RegionPurpose, RegionSelection, ScreenRect } from '../types';

const HINTS: Record<RegionPurpose, string> = {
  text: '拖动框选要提取的文字 · 松开鼠标开始识别 · 右键或 Esc 取消',
  silent: '拖动框选静默提取文字 · 松开鼠标识别并复制 · 右键或 Esc 取消',
  translate: '拖动框选要翻译的文字 · 松开鼠标开始识别 · 右键或 Esc 取消',
  picture: '拖动框选原图覆盖翻译区域 · 松开鼠标开始识别 · 右键或 Esc 取消',
  card: '拖动框选图文对照翻译区域 · 松开鼠标开始识别 · 右键或 Esc 取消',
  monitor: '拖动框选持续翻译区域 · 松开鼠标开始 · 右键或 Esc 取消',
};

const BUSY: Record<RegionPurpose, string> = {
  text: '正在识别…',
  silent: '正在后台识别并写入剪贴板…',
  translate: '正在识别…',
  picture: '正在识别并准备原位翻译…',
  card: '正在识别并准备对照翻译…',
  monitor: '正在开始自动翻译…',
};

/**
 * `#/ocr-region` — one selector for snips and auto-translation.
 *
 * An instant, transparent overlay reveals the live desktop directly.
 * The user drags a rectangle over the real screen with outer darkening.
 */
export function RegionSelectView() {
  const [selection, setSelection] = useState<RegionSelection | null>(null);
  const [rect, setRect] = useState<ScreenRect | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showCrosshairs, setShowCrosshairs] = useState(true);
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const start = useRef<{ x: number; y: number } | null>(null);
  const surface = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const current = useRef<RegionSelection | null>(null);
  current.current = selection;

  const reset = useCallback(() => {
    start.current = null;
    setRect(null);
    setBusy(false);
    setError(null);
    setSelection(null);
    const canvas = canvasRef.current;
    if (canvas) {
      const ctx = canvas.getContext('2d');
      ctx?.clearRect(0, 0, canvas.width, canvas.height);
    }
  }, []);

  useEffect(() => {
    if (!selection) return;
    let active = true;
    const canvas = canvasRef.current;
    if (!canvas) {
      void invoke('show_region_window');
      return;
    }

    canvas.width = selection.bounds.width;
    canvas.height = selection.bounds.height;
    const ctx = canvas.getContext('2d', { willReadFrequently: false });
    if (!ctx) {
      void invoke('show_region_window');
      return;
    }

    void invoke<ArrayBuffer>('get_frozen_pixels', { id: selection.id })
      .then((buffer) => {
        if (!active || !buffer) {
          if (active) void invoke('show_region_window');
          return;
        }
        const pixels = new Uint8ClampedArray(buffer);
        if (pixels.length === selection.bounds.width * selection.bounds.height * 4) {
          const imgData = new ImageData(pixels, selection.bounds.width, selection.bounds.height);
          ctx.putImageData(imgData, 0, 0);
        } else {
          console.warn('Frozen pixels size mismatch:', pixels.length, selection.bounds);
        }
        requestAnimationFrame(() => {
          if (!active) return;
          void invoke('show_region_window');
        });
      })
      .catch((e) => {
        console.warn('Failed to load raw frozen pixels:', e);
        if (active) {
          void invoke('show_region_window');
        }
      });

    return () => {
      active = false;
    };
  }, [selection]);

  useEffect(() => {
    let disposed = false;
    let sequence = 0;

    const refresh = async () => {
      const request = ++sequence;
      const value = await invoke<RegionSelection | null>('region_selection');
      if (disposed || request !== sequence) return;
      start.current = null;
      setRect(null);
      setBusy(false);
      setError(null);
      setSelection(value);
    };

    const unlistenSelection = listen<RegionSelection>('transportal://region-selection', (event) => {
      start.current = null;
      setRect(null);
      setBusy(false);
      setError(null);
      if (event.payload && typeof event.payload === 'object') {
        setSelection(event.payload);
      } else {
        void refresh();
      }
    });

    const unlistenReset = listen('transportal://region-reset', () => {
      reset();
    });

    void loadSettings().then((s) => {
      if (!disposed && s.showCrosshairs !== undefined) {
        setShowCrosshairs(s.showCrosshairs);
      }
    });

    void refresh();

    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      reset();
      void invoke('cancel_region', { id: current.current?.id ?? null });
    };
    const onContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };
    window.addEventListener('keydown', escape);
    window.addEventListener('contextmenu', onContextMenu);
    return () => {
      disposed = true;
      window.removeEventListener('keydown', escape);
      window.removeEventListener('contextmenu', onContextMenu);
      void unlistenSelection.then((dispose) => dispose()).catch(() => {});
      void unlistenReset.then((dispose) => dispose()).catch(() => {});
    };
  }, []);

  const point = (event: React.PointerEvent) => {
    const box = surface.current!.getBoundingClientRect();
    return { x: Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)), y: Math.max(0, Math.min(1, (event.clientY - box.top) / box.height)) };
  };
  const selectionRect = (end: { x: number; y: number }) => {
    const from = start.current!;
    return { x: Math.min(from.x, end.x), y: Math.min(from.y, end.y), width: Math.abs(from.x - end.x), height: Math.abs(from.y - end.y) };
  };
  const purpose = selection?.purpose ?? 'text';
  return <div ref={surface} className="fixed inset-0 cursor-crosshair overflow-hidden select-none bg-transparent"
    onContextMenu={(event) => {
      event.preventDefault();
      event.stopPropagation();
      reset();
      void invoke('cancel_region', { id: current.current?.id ?? null });
    }}
    onPointerDown={(event) => {
      if (event.button === 2) {
        event.preventDefault();
        reset();
        void invoke('cancel_region', { id: current.current?.id ?? null });
        return;
      }
      if (!selection || busy || event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      start.current = point(event); setRect(null); setError(null);
    }}
    onPointerMove={(event) => {
      if (showCrosshairs) {
        setCursor({ x: event.clientX, y: event.clientY });
      }
      if (start.current && !busy) setRect(selectionRect(point(event)));
    }}
    onPointerLeave={() => {
      if (showCrosshairs) setCursor(null);
    }}
    onPointerCancel={() => { start.current = null; setRect(null); }}
    onPointerUp={(event) => {
      if (!start.current || !selection || busy) return;
      const currentRect = selectionRect(point(event)); start.current = null; setRect(currentRect);
      const left = Math.round(currentRect.x * selection.bounds.width);
      const top = Math.round(currentRect.y * selection.bounds.height);
      const right = Math.round((currentRect.x + currentRect.width) * selection.bounds.width);
      const bottom = Math.round((currentRect.y + currentRect.height) * selection.bounds.height);
      const spanW = right - left;
      const spanH = bottom - top;

      // 如果用户仅仅是轻微点击（位移小于 4 像素），判定为误触或希望切回桌面程序，立即自动取消退出
      if (spanW < 4 && spanH < 4) {
        reset();
        void invoke('cancel_region', { id: current.current?.id ?? null });
        return;
      }

      if (spanW < 8 || spanH < 8) {
        setError('请拖动选择至少 8 × 8 像素的区域。');
        setRect(null);
        return;
      }
      setBusy(true);
      void invoke('confirm_region', { id: selection.id, region: {
        x: selection.bounds.x + left, y: selection.bounds.y + top, width: spanW, height: spanH,
      } })
      .then(() => {
        reset();
      })
      .catch((error) => {
        setBusy(false);
        setError(String(error));
      });
    }}>
    <canvas
      ref={canvasRef}
      className="pointer-events-none absolute inset-0 h-full w-full select-none"
    />
    {showCrosshairs && cursor && (
      <>
        <div
          className="pointer-events-none absolute left-0 right-0 h-[1px] bg-cyan-400/40"
          style={{ top: `${cursor.y}px` }}
        />
        <div
          className="pointer-events-none absolute top-0 bottom-0 w-[1px] bg-cyan-400/40"
          style={{ left: `${cursor.x}px` }}
        />
      </>
    )}
    {selection && !rect && <div className="pointer-events-none absolute inset-0 bg-black/25" />}
    {selection && rect && <div className="pointer-events-none absolute border-2 border-cyan-300" style={{
      left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%`,
      boxShadow: '0 0 0 99999px rgba(0,0,0,0.25)',
    }} />}
    {selection && (
      <div className="pointer-events-none absolute left-1/2 top-6 -translate-x-1/2 rounded-xl bg-black/85 px-5 py-3 text-sm text-white shadow-lg">
        {error || (busy ? BUSY[purpose] : HINTS[purpose])}
      </div>
    )}
  </div>;
}
