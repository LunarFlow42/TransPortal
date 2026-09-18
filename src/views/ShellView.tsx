import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow, LogicalSize } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { ImagePlus, Languages, ScanText, Settings, Monitor } from 'lucide-react';
import { LookupDeck } from '../components/LookupDeck';
import { OcrPanel } from '../components/OcrPanel';
import { AutoOcrPanel } from '../components/AutoOcrPanel';
import { StartupShortcutNotice } from '../components/ShortcutStatusPanel';
import { Button, IconButton, Surface } from '../components/ui';
import { useBootstrap } from '../hooks/useBootstrap';
import { useHideOnBlur } from '../hooks/useHideOnBlur';
import { useOcrEvents, useOcrStatus } from '../hooks/useOcr';
import { useAutoOcrEvents } from '../hooks/useAutoOcr';
import { invoke } from '@tauri-apps/api/core';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { clampCurrentWindow, openSettingsWindow } from '../utils/windows';
import { stopPronunciation } from '../utils/audio';
import type { CaptureOutcome, Density } from '../types';

/** Window footprint per top-card density. */
const WINDOW_SIZES: Record<Density, { width: number; height: number }> = {
  compact: { width: 460, height: 270 },
  dense: { width: 460, height: 620 },
};

/**
 * `#/shell` — the ghost bubble overlay.
 *
 * Hosts the drill-down lookup deck and reacts to the global hotkey capture
 * emitted by the Rust core (`transportal://selection`).
 */
export const ShellView: React.FC = () => {
  const applyCapture = useAppStore((state) => state.applyCapture);
  const lookupStack = useAppStore((state) => state.lookupStack);
  const capture = useAppStore((state) => state.capture);
  const hotkey = useAppStore((state) => state.hotkey);
  const errorMessage = useAppStore((state) => state.errorMessage);
  const ocr = useAppStore((state) => state.ocr);
  const ocrImageView = useAppStore((state) => state.ocrImageView);
  const autoOcr = useAppStore((state) => state.autoOcr);
  const ocrTarget = useAppStore((state) => state.ocrTarget);
  const [autoInterval, setAutoInterval] = useState(2000);
  const startOcr = useAppStore((state) => state.startOcr);
  const { status: ocrStatus } = useOcrStatus();
  const shortcuts = useShortcutStatus();
  const [dismissedShortcuts, setDismissedShortcuts] = useState('');
  const shortcutIssues = shortcuts.report?.ready
    ? shortcuts.report.shortcuts.filter((shortcut) => shortcut.error) : [];
  const issueKey = shortcutIssues.map((shortcut) => `${shortcut.id}:${shortcut.requested}:${shortcut.error}`).join('|');
  const showShortcutNotice = Boolean(issueKey && issueKey !== dismissedShortcuts);

  const topNode = lookupStack.length > 0 ? lookupStack[lookupStack.length - 1] : null;
  const topDensity: Density = topNode?.density ?? 'compact';
  const hasContent = lookupStack.length > 0;

  useHideOnBlur();
  useBootstrap(false);
  useOcrEvents();
  useAutoOcrEvents();

  useEffect(() => {
    const unlisten = listen<CaptureOutcome>('transportal://selection', (event) => {
      applyCapture(event.payload);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [applyCapture]);

  useEffect(() => {
    const size = showShortcutNotice ? { width: 460, height: 460 } : autoOcr || (ocrImageView && ocr?.picture)
      ? { width: 760, height: 680 } : ocr?.stage === 'ready'
      ? { width: 460, height: 400 }
      : ocr ? WINDOW_SIZES.compact : !hasContent ? { width: 460, height: 330 } : WINDOW_SIZES[topDensity];
    void getCurrentWindow().setSize(new LogicalSize(size.width, size.height))
      .then(clampCurrentWindow)
      .catch((error) => console.error('overlay resize failed:', error));
  }, [topDensity, ocr?.stage, ocrImageView, Boolean(autoOcr), hasContent, showShortcutNotice]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const store = useAppStore.getState();

      if (event.key === 'Escape') {
        event.preventDefault();
        if (showShortcutNotice) {
          setDismissedShortcuts(issueKey);
          return;
        }
        if (store.autoOcr) {
          void invoke('stop_auto_ocr', { discard: false });
          return;
        }
        if (store.ocr) store.cancelOcr();
        if (store.lookupStack.length > 1) {
          store.popLookup();
        } else {
          store.clearLookup();
          stopPronunciation();
          void getCurrentWindow().hide();
        }
        return;
      }

      if (event.altKey && event.key === 'ArrowLeft') {
        store.popLookup();
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [showShortcutNotice, issueKey]);

  const hotkeyParts = hotkey.split('+');
  const shortcutKey = (id: string) => shortcuts.report?.shortcuts.find((shortcut) => shortcut.id === id)?.active;
  const withKey = (label: string, id: string) => {
    const key = shortcutKey(id);
    return key ? `${key}：${label}` : label;
  };
  const pickImage = async () => {
    try {
      const path = await open({ multiple: false, directory: false,
        filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'bmp', 'webp', 'tif', 'tiff'] }],
      });
      if (typeof path === 'string') await useAppStore.getState().startImageOcr(path);
    } catch (error) {
      useAppStore.setState({ errorMessage: String(error) });
    }
  };

  return (
    <div className="flex min-h-screen w-full select-none flex-col items-center justify-center bg-transparent p-3">
      {showShortcutNotice ? (
        <StartupShortcutNotice issues={shortcutIssues} retrying={shortcuts.retrying} error={shortcuts.error}
          onRetry={() => void shortcuts.retry()} onDismiss={() => setDismissedShortcuts(issueKey)} />
      ) : autoOcr ? (
        <AutoOcrPanel event={autoOcr} />
      ) : ocr ? (
        <OcrPanel key={ocr.id} event={ocr} />
      ) : hasContent ? (
        <LookupDeck />
      ) : (
        <Surface className="w-full max-w-[420px] px-5 py-6 text-center">
          <div
            onMouseDown={startWindowDrag}
            className="flex cursor-move items-center justify-end"
          >
            <IconButton title="Settings" onClick={() => void openSettingsWindow()}>
              <Settings className="h-3.5 w-3.5" />
            </IconButton>
          </div>

          <p className="mt-2 text-sm text-[var(--muted)]">
            Press{' '}
            {hotkeyParts.map((part, index) => (
              <React.Fragment key={part}>
                {index > 0 && ' + '}
                <kbd className="rounded-[6px] border border-[var(--stroke)] bg-[var(--surface-2)] px-1.5 py-0.5 font-mono text-xs text-[var(--text)]">
                  {part}
                </kbd>
              </React.Fragment>
            ))}{' '}
            to capture selection
          </p>

          {ocrStatus && (
            <div className="mt-4 flex flex-wrap justify-center gap-2">
              {ocrStatus.supported && <>
                <Button size="sm" title={withKey('框选屏幕文字，识别后复制', 'ocr_text')}
                  disabled={!ocrStatus.engine.available}
                  onClick={() => void startOcr('text')}>
                  <ScanText className="h-3.5 w-3.5" />提取文字
                </Button>
                <Button size="sm" variant="primary" title={withKey('框选屏幕文字，识别后翻译', 'ocr_translate')}
                  disabled={!ocrStatus.engine.available}
                  onClick={() => void startOcr('translate')}>
                  <Languages className="h-3.5 w-3.5" />截图翻译
                </Button>
              </>}
              <Button size="sm" title="打开本地图片并提取文字"
                disabled={!ocrStatus.engine.available}
                onClick={() => void pickImage()}>
                <ImagePlus className="h-3.5 w-3.5" />打开图片
              </Button>
              {ocrStatus.supported && <div className="flex w-full flex-wrap items-center justify-center gap-2">
                <Button size="sm" title="持续监控选定区域，内容变化后自动更新译图" disabled={!ocrStatus.engine.available}
                  onClick={() => void useAppStore.getState().startAutoOcr(autoInterval)}><Monitor className="h-3.5 w-3.5" />自动区域翻译</Button>
                <select aria-label="自动翻译刷新间隔" value={autoInterval} onChange={(e) => setAutoInterval(Number(e.target.value))} className="rounded bg-[var(--surface-2)] p-1 text-[11px]">
                  <option value={1000}>1 秒</option><option value={2000}>2 秒</option><option value={5000}>5 秒</option><option value={10000}>10 秒</option>
                </select>
                <select aria-label="自动翻译目标语言" value={ocrTarget} onChange={(e) => useAppStore.setState({ ocrTarget: e.target.value })} className="rounded bg-[var(--surface-2)] p-1 text-[11px]">
                  <option value="zh-CN">简体中文</option><option value="en">English</option><option value="ja">日本語</option><option value="ko">한국어</option>
                </select>
              </div>}
            </div>
          )}

          {capture?.error && (
            <p className="mt-1.5 text-[11px] text-[var(--faint)]">{capture.error}</p>
          )}
        </Surface>
      )}

      {errorMessage && (
        <div className="mt-2 rounded-[var(--r-ctrl)] border border-rose-500/30 bg-rose-500/20 px-2.5 py-1 text-[11px] text-rose-300">
          {errorMessage}
        </div>
      )}
    </div>
  );
};
