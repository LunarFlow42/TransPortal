import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { loadSettings } from '../store/persistence';
import type { RegionPurpose, RegionSelection, ScreenRect } from '../types';
import { paintFrozenPreview } from '../utils/frozenPreview';

const HINTS: Record<RegionPurpose, string> = {
  text: '拖动框选要提取的文字 · 松开鼠标开始识别 · 右键或 Esc 取消',
  silent: '拖动框选静默提取文字 · 松开鼠标识别并复制 · 右键或 Esc 取消',
  search: '拖动框选要搜索的内容 · 松开鼠标识别并在浏览器中搜索 · 右键或 Esc 取消',
  translate: '拖动框选要翻译的文字 · 松开鼠标开始识别 · 右键或 Esc 取消',
  picture: '拖动框选原图覆盖翻译区域 · 松开鼠标开始识别 · 右键或 Esc 取消',
  card: '拖动框选图文对照翻译区域 · 松开鼠标开始识别 · 右键或 Esc 取消',
  monitor: '拖动框选持续翻译区域 · 松开鼠标开始 · 右键或 Esc 取消',
};

const BUSY: Record<RegionPurpose, string> = {
  text: '正在识别…',
  silent: '正在后台识别并写入剪贴板…',
  search: '正在识别并打开浏览器搜索…',
  translate: '正在识别…',
  picture: '正在识别并准备原位翻译…',
  card: '正在识别并准备对照翻译…',
  monitor: '正在开始自动翻译…',
};

/**
 * `#/ocr-region` — one selector for snips and auto-translation.
 *
 * The selection and OCR must use the very same frozen frame, even if the
 * game has continued rendering or the previous IPC reply is still in flight.
 */
export function RegionSelectView() {
  const [selection, setSelection] = useState<RegionSelection | null>(null);
  const [paintedId, setPaintedId] = useState<number | null>(null);
  const [rect, setRect] = useState<ScreenRect | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showCrosshairs, setShowCrosshairs] = useState(true);
  const [cursor, setCursor] = useState<{ x: number; y: number } | null>(null);
  const start = useRef<{ x: number; y: number } | null>(null);
  const surface = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const current = useRef<RegionSelection | null>(null);

  const select = useCallback((next: RegionSelection | null) => {
    if (current.current?.id === next?.id) return;
    current.current = next;
    start.current = null;
    setCursor(null);
    setRect(null);
    setBusy(false);
    setError(null);
    setPaintedId(null);
    setSelection(next);
  }, []);

  const cancel = useCallback(() => {
    const id = current.current?.id;
    select(null);
    if (id !== undefined) void invoke('cancel_region', { id });
  }, [select]);

  useEffect(() => {
    if (!selection) return;
    let active = true;
    const isCurrent = () => active && current.current?.id === selection.id;
    void paintFrozenPreview(selection, (id) => invoke('get_frozen_pixels', { id }), isCurrent, (pixels) => {
      const canvas = canvasRef.current;
      const ctx = canvas?.getContext('2d');
      if (!ctx || !canvas) throw new Error('无法显示冻结画面。');
      canvas.width = selection.bounds.width;
      canvas.height = selection.bounds.height;
      ctx.putImageData(new ImageData(pixels, canvas.width, canvas.height), 0, 0);
    }).then((painted) => {
      if (!painted || !isCurrent()) return;
      setPaintedId(selection.id);
      // Animation frames can be suspended while this webview is still hidden.
      return invoke('show_region_window', { id: selection.id });
    }).catch((e) => {
      if (isCurrent()) setError(`${String(e)} · 右键或 Esc 取消后重试`);
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
      const previous = current.current;
      const value = await invoke<RegionSelection | null>('region_selection');
      if (disposed || request !== sequence || current.current !== previous) return;
      select(value);
    };

    const unlistenSelection = listen<RegionSelection>('transportal://region-selection', (event) => {
      if (disposed) return;
      ++sequence; // invalidate any older region_selection reply
      if (event.payload && typeof event.payload === 'object') {
        select(event.payload);
      } else {
        void refresh();
      }
    });

    const unlistenReset = listen('transportal://region-reset', () => {
      if (!disposed) void refresh();
    });

    void loadSettings().then((s) => {
      if (!disposed && s.showCrosshairs !== undefined) {
        setShowCrosshairs(s.showCrosshairs);
      }
    });

    void Promise.all([unlistenSelection, unlistenReset]).then(() => {
      if (!disposed) void refresh();
    });

    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      cancel();
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
  }, [select, cancel]);

  const point = (event: React.PointerEvent) => {
    const box = surface.current!.getBoundingClientRect();
    return { x: Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)), y: Math.max(0, Math.min(1, (event.clientY - box.top) / box.height)) };
  };
  const selectionRect = (end: { x: number; y: number }) => {
    const from = start.current!;
    return { x: Math.min(from.x, end.x), y: Math.min(from.y, end.y), width: Math.abs(from.x - end.x), height: Math.abs(from.y - end.y) };
  };
  const purpose = selection?.purpose ?? 'text';
  const ready = selection !== null && paintedId === selection.id;
  return <div ref={surface} className="fixed inset-0 cursor-crosshair overflow-hidden select-none bg-transparent"
    onContextMenu={(event) => {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }}
    onPointerDown={(event) => {
      if (event.button === 2) {
        event.preventDefault();
        cancel();
        return;
      }
      if (!ready || current.current?.id !== selection.id || busy || event.button !== 0) return;
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
      if (!start.current || !ready || current.current?.id !== selection.id || busy) return;
      const currentRect = selectionRect(point(event)); start.current = null; setRect(currentRect);
      const left = Math.round(currentRect.x * selection.bounds.width);
      const top = Math.round(currentRect.y * selection.bounds.height);
      const right = Math.round((currentRect.x + currentRect.width) * selection.bounds.width);
      const bottom = Math.round((currentRect.y + currentRect.height) * selection.bounds.height);
      const spanW = right - left;
      const spanH = bottom - top;

      // 如果用户仅仅是轻微点击（位移小于 4 像素），判定为误触或希望切回桌面程序，立即自动取消退出
      if (spanW < 4 && spanH < 4) {
        cancel();
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
        if (current.current?.id === selection.id) select(null);
      })
      .catch((error) => {
        if (current.current?.id !== selection.id) return;
        setBusy(false);
        setError(String(error));
      });
    }}>
    <canvas
      key={selection?.id ?? 'empty'}
      ref={canvasRef}
      className="pointer-events-none absolute inset-0 h-full w-full select-none"
    />
    {selection && !ready && <div className="pointer-events-none absolute inset-0 bg-neutral-900" />}
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
        {error ? <span role="alert">{error}</span> : !ready ? '正在加载本次截图…' : busy ? BUSY[purpose] : HINTS[purpose]}
      </div>
    )}
  </div>;
}
