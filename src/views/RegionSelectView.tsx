import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { RegionPurpose, RegionSelection, ScreenRect } from '../types';

const HINTS: Record<RegionPurpose, string> = {
  text: '拖动框选要提取的文字 · 松开鼠标开始识别 · Esc 取消',
  translate: '拖动框选要翻译的文字 · 松开鼠标开始识别 · Esc 取消',
  monitor: '拖动框选持续翻译区域 · 松开鼠标开始 · Esc 取消',
};

const BUSY: Record<RegionPurpose, string> = {
  text: '正在识别…', translate: '正在识别…', monitor: '正在开始自动翻译…',
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
  const start = useRef<{ x: number; y: number } | null>(null);
  const surface = useRef<HTMLDivElement>(null);
  const current = useRef<RegionSelection | null>(null);
  current.current = selection;

  useEffect(() => {
    let disposed = false;
    let sequence = 0;
    const refresh = async () => {
      const request = ++sequence;
      const value = await invoke<RegionSelection | null>('region_selection');
      if (disposed || request !== sequence) return;
      start.current = null; setRect(null); setBusy(false); setError(null); setSelection(value);
    };
    const unlisten = listen('transportal://region-selection', () => { void refresh(); });
    void unlisten.then(refresh).catch((error) => { if (!disposed) setError(String(error)); });
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      void invoke('cancel_region', { id: current.current?.id ?? null });
    };
    window.addEventListener('keydown', escape);
    return () => { disposed = true; window.removeEventListener('keydown', escape); void unlisten.then((dispose) => dispose()); };
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
    onPointerDown={(event) => {
      if (!selection || busy || event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      start.current = point(event); setRect(null); setError(null);
    }}
    onPointerMove={(event) => { if (start.current && !busy) setRect(selectionRect(point(event))); }}
    onPointerCancel={() => { start.current = null; setRect(null); }}
    onPointerUp={(event) => {
      if (!start.current || !selection || busy) return;
      const current = selectionRect(point(event)); start.current = null; setRect(current);
      const left = Math.round(current.x * selection.bounds.width);
      const top = Math.round(current.y * selection.bounds.height);
      const right = Math.round((current.x + current.width) * selection.bounds.width);
      const bottom = Math.round((current.y + current.height) * selection.bounds.height);
      if (right - left < 8 || bottom - top < 8) { setError('请拖动选择至少 8 × 8 像素的区域。'); return; }
      setBusy(true);
      void invoke('confirm_region', { id: selection.id, region: {
        x: selection.bounds.x + left, y: selection.bounds.y + top, width: right - left, height: bottom - top,
      } }).catch((error) => { setBusy(false); setError(String(error)); });
    }}>
    {!rect && <div className="pointer-events-none absolute inset-0 bg-black/25" />}
    {rect && <div className="pointer-events-none absolute border-2 border-cyan-300" style={{
      left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%`,
      boxShadow: '0 0 0 99999px rgba(0,0,0,0.25)',
    }} />}
    <div className="pointer-events-none absolute left-1/2 top-6 -translate-x-1/2 rounded-xl bg-black/85 px-5 py-3 text-sm text-white shadow-lg">
      {error || (busy ? BUSY[purpose] : HINTS[purpose])}
    </div>
  </div>;
}
