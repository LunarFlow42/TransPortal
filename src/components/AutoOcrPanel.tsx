import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { LoaderCircle, Pause, Play, ScanText, Square, X } from 'lucide-react';
import { Button, IconButton, Surface } from './ui';
import { PictureView } from './PictureView';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import type { MonitorUpdate } from '../types';

const STAGES: Record<MonitorUpdate['stage'], string> = {
  selecting: '框选屏幕区域', watching: '监控中 · 内容变化后更新', recognizing: '正在识别新内容…',
  translating: '正在翻译新内容…', paused: '已暂停', error: '更新失败，将重试', stopped: '已停止',
};

export function AutoOcrPanel({ event }: { event: MonitorUpdate }) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const command = async (name: string, args: Record<string, unknown>) => {
    setBusy(true); setError(null);
    try { await invoke(name, args); }
    catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  const close = async () => {
    await command('stop_auto_ocr', { discard: true });
    void getCurrentWindow().hide();
  };
  return <Surface className="w-full p-4 text-[var(--text)]">
    <div onMouseDown={startWindowDrag} className="flex cursor-move items-center justify-between border-b border-[var(--stroke)] pb-2">
      <span className="flex items-center gap-2 text-xs"><span className={`h-2 w-2 rounded-full ${event.active && !event.paused ? 'animate-pulse bg-emerald-400' : 'bg-[var(--faint)]'}`} />区域自动翻译</span>
      <IconButton title="关闭并停止自动翻译" onClick={() => void close()}><X className="h-3.5 w-3.5" /></IconButton>
    </div>
    <div className="mt-3 flex flex-wrap items-center gap-2">
      {event.active && !event.selecting && <>
        <Button size="sm" disabled={busy} onClick={() => void command('pause_auto_ocr', { paused: !event.paused })}>
          {event.paused ? <Play className="h-3 w-3" /> : <Pause className="h-3 w-3" />}{event.paused ? '继续' : '暂停'}
        </Button>
        <Button size="sm" disabled={busy} onClick={() => void command('stop_auto_ocr', { discard: false })}><Square className="h-3 w-3" />停止</Button>
      </>}
      <Button size="sm" disabled={busy || event.selecting} onClick={() => void useAppStore.getState().startAutoOcr(event.interval_ms)}><ScanText className="h-3 w-3" />重新选区</Button>
      <span role="status" className="text-[11px] text-[var(--muted)]">{STAGES[event.stage] ?? '准备中'} · {event.interval_ms / 1000}s · {event.target_lang}</span>
    </div>
    {event.frame ? <PictureView picture={event.frame.picture} translation={event.frame.translation}
      busy={event.stage === 'recognizing' || event.stage === 'translating'} error={event.error || error} />
      : <div className="flex min-h-40 items-center justify-center gap-2 text-sm text-[var(--muted)]">
        {event.active && !event.error && <LoaderCircle className="h-4 w-4 animate-spin" />}
        {event.error || error || STAGES[event.stage]}
      </div>}
    <p className="mt-2 text-[10px] text-[var(--faint)]">{event.region && `${event.region.width} × ${event.region.height} · `}
      {event.frame && `第 ${event.frame.number} 帧 · OCR ${event.frame.elapsed_ms} ms · `}Esc 停止 · 截图留在本机，仅文字交给所选翻译服务
    </p>
  </Surface>;
}
