import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Check, Copy, Languages, LoaderCircle, Pin, ScanText, Settings, X } from 'lucide-react';
import { Button, IconButton, Surface } from './ui';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { openSettingsWindow } from '../utils/windows';
import type { OcrEvent } from '../types';
import { PictureView } from './PictureView';

export const OcrPanel: React.FC<{ event: OcrEvent }> = ({ event }) => {
  const [text, setText] = useState(event.text);
  const [copied, setCopied] = useState(event.copied);
  const [copyError, setCopyError] = useState(event.copy_error);
  const pinned = useAppStore((state) => state.pinned);
  const togglePinned = useAppStore((state) => state.togglePinned);
  const imageView = useAppStore((state) => state.ocrImageView);
  const translation = useAppStore((state) => state.ocrPictureTranslation);
  const translating = useAppStore((state) => state.ocrPictureBusy);
  const translationError = useAppStore((state) => state.ocrPictureError);
  const target = useAppStore((state) => state.ocrTarget);
  const ready = event.stage === 'ready';
  const failed = event.stage === 'error';

  useEffect(() => {
    setText(event.text);
    setCopied(event.copied);
    setCopyError(event.copy_error);
  }, [event.text, event.copied, event.copy_error]);

  const close = () => {
    useAppStore.getState().cancelOcr();
    useAppStore.getState().clearLookup();
    void getCurrentWindow().hide();
  };

  const copy = async () => {
    try {
      await invoke('plugin:clipboard-manager|write_text', { text });
      setCopied(true);
      setCopyError(null);
    } catch (error) {
      setCopyError(`复制失败：${String(error)}`);
    }
  };

  return (
    <Surface className="w-full p-4 text-[var(--text)]">
      <div
        onMouseDown={startWindowDrag}
        className="flex cursor-move items-center justify-between border-b border-[var(--stroke)] pb-2"
      >
        <div className="flex items-center gap-2 text-xs text-[var(--muted)]">
          <span className={`h-1.5 w-1.5 rounded-full ${failed ? 'bg-rose-400' : ready ? 'bg-emerald-400' : 'animate-pulse bg-[var(--accent-b)]'}`} />
          {event.mode === 'translate' ? '截图翻译' : '提取文字'}
        </div>
        <div className="flex items-center gap-0.5">
          <IconButton title={pinned ? 'Unpin' : 'Pin'} active={pinned} onClick={togglePinned}>
            <Pin className="h-3.5 w-3.5" />
          </IconButton>
          <IconButton title="OCR 设置" onClick={() => void openSettingsWindow()}>
            <Settings className="h-3.5 w-3.5" />
          </IconButton>
          <IconButton title="关闭" onClick={close} className="hover:bg-rose-500/90 hover:text-white">
            <X className="h-3.5 w-3.5" />
          </IconButton>
        </div>
      </div>

      {ready && imageView && event.picture ? (
        <PictureView picture={event.picture} translation={translation} busy={translating} error={translationError}
          target={target} onTarget={(ocrTarget) => useAppStore.setState({ ocrTarget })}
          onTranslate={(texts) => void useAppStore.getState().translateOcrPicture(texts)} />
      ) : ready ? (
        <>
          <textarea
            aria-label="识别文字，可编辑后复制或翻译"
            value={text}
            onChange={(event) => { setText(event.target.value); setCopied(false); }}
            spellCheck={false}
            className="focus-ring mt-3 h-48 w-full resize-none select-text rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-3 text-sm leading-relaxed text-[var(--text)]"
          />
          <div className="mt-2 flex items-center justify-between gap-2">
            <span className="text-[10px] text-[var(--faint)]">
              {Array.from(text).length} 字符
              {event.language && ` · ${event.language}`}
              {event.elapsed_ms != null && ` · ${event.elapsed_ms} ms`}
            </span>
            <div className="flex shrink-0 gap-1.5">
              {event.picture && <Button size="sm" onClick={() => void useAppStore.getState().translateOcrPicture()}>原位翻译</Button>}
              <Button size="sm" onClick={() => void copy()} disabled={!text.trim()}>
                {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                {copied ? '已复制' : '复制'}
              </Button>
              <Button size="sm" variant="primary" disabled={!text.trim()}
                onClick={() => useAppStore.getState().translateOcrText(text)}>
                <Languages className="h-3 w-3" />翻译
              </Button>
            </div>
          </div>
          {copyError && <p role="status" className="mt-2 text-[11px] text-[var(--muted)]">{copyError}</p>}
        </>
      ) : (
        <div className="flex min-h-28 flex-col items-center justify-center gap-3 py-3 text-center" role="status">
          {failed ? (
            <>
              <p className="text-xs leading-relaxed text-[var(--text)]">{event.error}</p>
              <Button size="sm" onClick={() => void (event.image_path
                ? useAppStore.getState().startImageOcr(event.image_path, event.mode)
                : useAppStore.getState().startOcr(event.mode))}>
                <ScanText className="h-3.5 w-3.5" />{event.image_path ? '重新识别图片' : '重新截图'}
              </Button>
            </>
          ) : (
            <>
              <LoaderCircle className="h-5 w-5 animate-spin text-[var(--accent-b)]" />
              <p className="text-xs text-[var(--muted)]">
                {event.stage === 'capturing' ? '在屏幕上框选文字，Esc 取消' : '正在本地识别文字…'}
              </p>
              <Button size="sm" variant="subtle" onClick={close}>取消</Button>
            </>
          )}
        </div>
      )}
    </Surface>
  );
};
