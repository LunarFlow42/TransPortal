import React, { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open } from '@tauri-apps/plugin-dialog';
import { ImagePlus, Languages, ScanText, Settings, Monitor, Sparkles } from 'lucide-react';
import { LookupDeck } from '../components/LookupDeck';
import { OcrPanel } from '../components/OcrPanel';
import { OcrCardView } from '../components/OcrCardView';
import { AutoOcrPanel } from '../components/AutoOcrPanel';
import { StartupShortcutNotice } from '../components/ShortcutStatusPanel';
import { Button, IconButton, Surface } from '../components/ui';
import { useBootstrap } from '../hooks/useBootstrap';
import { useHideOnBlur } from '../hooks/useHideOnBlur';
import { useMemoryManager } from '../hooks/useMemoryManager';
import { useOcrEvents, useOcrStatus } from '../hooks/useOcr';
import { useAutoOcrEvents } from '../hooks/useAutoOcr';
import { useWindowAnimation } from '../hooks/useWindowAnimation';
import { invoke } from '@tauri-apps/api/core';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { openSettingsWindow, resizeAndClampWindow } from '../utils/windows';
import { suppressAutoHide } from '../utils/autoHide';
import { stopPronunciation } from '../utils/audio';
import { cn } from '../utils/cn';
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

  const [customSize, setCustomSize] = useState<{ width: number; height: number } | null>(() => {
    try {
      const saved = localStorage.getItem('transportal_custom_shell_size');
      if (saved) {
        const parsed = JSON.parse(saved);
        if (parsed && typeof parsed.width === 'number' && typeof parsed.height === 'number') {
          return parsed;
        }
      }
    } catch {}
    return null;
  });
  const isManualResizingRef = React.useRef(false);
  const scaleFactorRef = React.useRef(1);
  const contentRef = React.useRef<HTMLDivElement>(null);
  const [measuredContentHeight, setMeasuredContentHeight] = useState<number | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const [manualInput, setManualInput] = useState('');
  const tabPressTimeRef = React.useRef<number | null>(null);
  const tabInitialStateRef = React.useRef<boolean>(false);

  useHideOnBlur();
  const { stage, dismiss } = useWindowAnimation();
  const smoothTransitions = useAppStore((state) => state.smoothTransitions);
  useMemoryManager();
  useBootstrap(false);
  useOcrEvents();
  useAutoOcrEvents();

  useEffect(() => {
    const unlistenFocus = listen('transportal://focus-input', () => {
      useAppStore.getState().clearLookup();
      setManualInput('');
      setTimeout(() => inputRef.current?.focus(), 50);
    });
    const unlistenClip = listen<string>('transportal://clipboard-text', (event) => {
      if (event.payload?.trim()) {
        void useAppStore.getState().processText(event.payload.trim());
      }
    });
    return () => {
      void unlistenFocus.then((dispose) => dispose());
      void unlistenClip.then((dispose) => dispose());
    };
  }, []);

  const handleResizeMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    // Starting the drag blurs the webview before Rust can report the sizing
    // loop, so hold auto-hide from the press itself. The promise resolves
    // once the OS loop starts, not when it ends; Rust reports the real
    // start/end via `transportal://shell-size-move` / `shell-resizing`.
    suppressAutoHide();
    void getCurrentWindow()
      .startResizeDragging('SouthEast')
      .catch((err) => console.error('startResizeDragging failed:', err));
  };

  const handleResizeDoubleClick = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setCustomSize(null);
    try {
      localStorage.removeItem('transportal_custom_shell_size');
    } catch {}
  };

  useEffect(() => {
    if (!contentRef.current || !hasContent) {
      setMeasuredContentHeight(null);
      return;
    }
    const updateHeight = () => {
      if (contentRef.current) {
        const h = contentRef.current.scrollHeight || contentRef.current.offsetHeight;
        if (h > 0) setMeasuredContentHeight(Math.round(h));
      }
    };
    updateHeight();
    const observer = new ResizeObserver(() => updateHeight());
    observer.observe(contentRef.current);
    return () => observer.disconnect();
  }, [hasContent, topDensity, topNode?.id, topNode?.status, topNode?.translation]);

  useEffect(() => {
    const window = getCurrentWindow();
    const toLogical = (size: { width: number; height: number }, factor: number) => {
      const logical = { width: Math.round(size.width / factor), height: Math.round(size.height / factor) };
      return logical.width >= 320 && logical.height >= 120 ? logical : null;
    };

    // Covers the corner grip and native edge resizing alike. While the user
    // drags, auto-fit must not fight the OS; the final size becomes sticky.
    // (Auto-hide is held for the whole interaction by useHideOnBlur.)
    const unlistenResizing = listen<boolean>('transportal://shell-resizing', async ({ payload: resizing }) => {
      if (resizing) {
        isManualResizingRef.current = true;
        scaleFactorRef.current = await window.scaleFactor().catch(() => 1);
        return;
      }
      try {
        const [size, factor] = await Promise.all([window.innerSize(), window.scaleFactor()]);
        const next = toLogical(size, factor);
        if (next) {
          setCustomSize(next);
          try {
            localStorage.setItem('transportal_custom_shell_size', JSON.stringify(next));
          } catch {}
        }
      } catch {
      } finally {
        isManualResizingRef.current = false;
      }
    });

    const unlistenResized = window.onResized(({ payload: size }) => {
      if (!isManualResizingRef.current) return;
      const next = toLogical(size, scaleFactorRef.current || 1);
      if (next) setCustomSize(next);
    });

    return () => {
      void unlistenResizing.then((dispose) => dispose());
      void unlistenResized.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<CaptureOutcome>('transportal://selection', (event) => {
      applyCapture(event.payload);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [applyCapture]);

  useEffect(() => {
    if (isManualResizingRef.current) return;

    if (showShortcutNotice) {
      const noticeHeight = Math.min(480, Math.max(260, 180 + shortcutIssues.length * 75));
      void resizeAndClampWindow(460, noticeHeight);
      return;
    }
    if (hasContent) {
      // Has lookup content:
      if (customSize) {
        void resizeAndClampWindow(customSize.width, customSize.height);
        return;
      }

      // Auto-fit height based on measured content:
      const targetWidth = 460;
      const contentH = measuredContentHeight ?? (topDensity === 'dense' ? 520 : 250);
      const targetHeight = Math.min(650, Math.max(180, Math.ceil(contentH) + 2));
      void resizeAndClampWindow(targetWidth, targetHeight);
      return;
    }

    if (ocr?.stage === 'ready' && ocr.mode === 'card') {
      const cardWidth = customSize ? Math.max(680, customSize.width) : 740;
      const cardHeight = customSize ? Math.max(480, customSize.height) : 520;
      void resizeAndClampWindow(cardWidth, cardHeight);
      return;
    }

    if (autoOcr || (ocrImageView && ocr?.picture)) {
      void resizeAndClampWindow(760, 680);
      return;
    }
    if (ocr?.stage === 'ready') {
      void resizeAndClampWindow(460, 400);
      return;
    }
    if (ocr) {
      void resizeAndClampWindow(WINDOW_SIZES.compact.width, WINDOW_SIZES.compact.height);
      return;
    }

    const idleWidth = customSize ? Math.max(460, customSize.width) : 460;
    void resizeAndClampWindow(idleWidth, 310);
  }, [
    customSize,
    measuredContentHeight,
    topDensity,
    ocr?.stage,
    ocr?.mode,
    ocrImageView,
    Boolean(autoOcr),
    hasContent,
    showShortcutNotice,
    shortcutIssues.length,
  ]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const store = useAppStore.getState();
      const isEditing =
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement ||
        event.target instanceof HTMLSelectElement;

      if (!isEditing && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
        event.preventDefault();
        void openSettingsWindow();
        return;
      }

      if (!isEditing && !event.ctrlKey && !event.altKey && !event.metaKey && event.key === 'Tab' && store.ocr?.picture) {
        event.preventDefault();
        if (event.repeat) return;
        tabPressTimeRef.current = Date.now();
        tabInitialStateRef.current = store.ocrPictureOriginal;
        store.setOcrPictureOriginal(!store.ocrPictureOriginal);
        return;
      }

      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 't') {
        event.preventDefault();
        store.togglePinned();
        return;
      }

      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 's') {
        event.preventDefault();
        void store.syncToAnki();
        return;
      }

      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === 'r') {
        event.preventDefault();
        const topNode = store.lookupStack.length > 0 ? store.lookupStack[store.lookupStack.length - 1] : null;
        if (topNode) void store.processText(topNode.text);
        return;
      }

      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === 'p') {
        event.preventDefault();
        store.navigateHistory('prev');
        return;
      }

      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === 'n') {
        event.preventDefault();
        store.navigateHistory('next');
        return;
      }

      if (event.key === 'Escape') {
        event.preventDefault();
        if (showShortcutNotice) {
          setDismissedShortcuts(issueKey);
          return;
        }
        if (store.autoOcr) {
          if (store.autoOcr.active) {
            void invoke('stop_auto_ocr', { discard: false });
          } else {
            void invoke('stop_auto_ocr', { discard: true });
            void dismiss({ force: true });
          }
          return;
        }
        if (store.lookupStack.length > 0) {
          if (store.lookupStack.length > 1) {
            store.popLookup();
          } else {
            store.clearLookup();
            stopPronunciation();
            if (!store.ocr) {
              void dismiss({ force: true });
            }
          }
          return;
        }
        if (store.ocr) {
          store.cancelOcr();
          void dismiss({ force: true });
          return;
        }
        stopPronunciation();
        void dismiss({ force: true });
        return;
      }

      if (event.altKey && event.key === 'ArrowLeft') {
        if (store.lookupStack.length > 1) {
          store.popLookup();
        } else if (store.lookupStack.length === 1 && store.ocr) {
          store.clearLookup();
        }
      }
    };

    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === 'Tab' && tabPressTimeRef.current !== null) {
        event.preventDefault();
        const duration = Date.now() - tabPressTimeRef.current;
        const initial = tabInitialStateRef.current;
        tabPressTimeRef.current = null;
        if (duration >= 250) {
          useAppStore.getState().setOcrPictureOriginal(initial);
        }
      }
    };

    const onBlur = () => {
      if (tabPressTimeRef.current !== null) {
        const initial = tabInitialStateRef.current;
        tabPressTimeRef.current = null;
        useAppStore.getState().setOcrPictureOriginal(initial);
      }
    };

    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keyup', onKeyUp);
    window.addEventListener('blur', onBlur);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('blur', onBlur);
    };
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

  const isResizing = isManualResizingRef.current;
  const showTransition = smoothTransitions && !isResizing;

  const animationStyle: React.CSSProperties = showTransition
    ? stage === 'entering' || stage === 'entered'
      ? {
          opacity: 1,
          transform: 'scale(1) translateY(0px)',
          transition:
            'opacity 120ms cubic-bezier(0.16, 1, 0.3, 1), transform 120ms cubic-bezier(0.16, 1, 0.3, 1)',
        }
      : {
          opacity: 0,
          transform: 'scale(0.97) translateY(4px)',
          transition:
            'opacity 100ms cubic-bezier(0.4, 0, 1, 1), transform 100ms cubic-bezier(0.4, 0, 1, 1)',
          pointerEvents: 'none',
        }
    : {
        opacity: 1,
        transform: 'none',
        transition: 'none',
      };

  return (
    <div
      style={animationStyle}
      className={cn(
        'relative flex h-full w-full select-none flex-col items-center bg-transparent overflow-hidden',
        hasContent ? 'justify-start' : 'justify-center',
      )}
    >
      {showShortcutNotice ? (
        <StartupShortcutNotice issues={shortcutIssues} retrying={shortcuts.retrying} error={shortcuts.error}
          onRetry={() => void shortcuts.retry()} onDismiss={() => setDismissedShortcuts(issueKey)} />
      ) : autoOcr ? (
        <AutoOcrPanel event={autoOcr} />
      ) : hasContent ? (
        <div ref={contentRef} className={cn('w-full', customSize && 'h-full flex flex-col')}>
          <LookupDeck fillHeight={Boolean(customSize)} />
        </div>
      ) : ocr ? (
        ocr.stage === 'ready' && ocr.mode === 'card' ? (
          <OcrCardView key={ocr.id} event={ocr} />
        ) : (
          <OcrPanel key={ocr.id} event={ocr} />
        )
      ) : (
        <Surface className="w-full px-5 py-5 text-center">
          <div
            onMouseDown={startWindowDrag}
            className="flex cursor-move items-center justify-end pb-1"
          >
            <IconButton title="Settings" onClick={() => void openSettingsWindow()}>
              <Settings className="h-3.5 w-3.5" />
            </IconButton>
          </div>

          <div className="my-2 w-full">
            <input
              ref={inputRef}
              type="text"
              value={manualInput}
              onChange={(e) => setManualInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && manualInput.trim()) {
                  e.preventDefault();
                  void useAppStore.getState().processText(manualInput.trim());
                  setManualInput('');
                }
              }}
              placeholder="输入单词或句子，按 Enter 翻译..."
              className="w-full rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] px-3 py-1.5 text-xs text-[var(--text)] placeholder-[var(--faint)] outline-none transition-colors focus:border-[var(--accent-b)] focus:bg-[var(--surface-3)]"
            />
          </div>

          <p className="text-xs text-[var(--muted)]">
            划选后按{' '}
            {hotkeyParts.map((part, index) => (
              <React.Fragment key={part}>
                {index > 0 && ' + '}
                <kbd className="rounded-[4px] border border-[var(--stroke)] bg-[var(--surface-2)] px-1 py-0.5 font-mono text-[11px] text-[var(--text)]">
                  {part}
                </kbd>
              </React.Fragment>
            ))}{' '}
            取词翻译
          </p>

          {ocrStatus && (
            <div className="mt-4 flex flex-wrap justify-center gap-2">
              {ocrStatus.supported && <>
                <Button size="sm" variant="primary" title={withKey('框选屏幕文字，图文双栏对照并支持点词查词', 'ocr_card')}
                  disabled={!ocrStatus.engine.available}
                  onClick={() => void startOcr('card')}>
                  <Languages className="h-3.5 w-3.5" />对照翻译
                </Button>
                <Button size="sm" title={withKey('框选屏幕文字，原位置覆盖译文', 'ocr_picture')}
                  disabled={!ocrStatus.engine.available}
                  onClick={() => void startOcr('picture')}>
                  <Sparkles className="h-3.5 w-3.5" />原图翻译
                </Button>
                <Button size="sm" title={withKey('框选屏幕文字，识别后复制', 'ocr_text')}
                  disabled={!ocrStatus.engine.available}
                  onClick={() => void startOcr('text')}>
                  <ScanText className="h-3.5 w-3.5" />提取文字
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
                <select aria-label="自动翻译刷新间隔" value={autoInterval} onChange={(e) => setAutoInterval(Number(e.target.value))} className="cursor-pointer rounded border border-[var(--stroke)] bg-[var(--surface-2)] p-1 text-[11px] text-[var(--text)] focus-ring">
                  <option value={1000}>1 秒</option><option value={2000}>2 秒</option><option value={5000}>5 秒</option><option value={10000}>10 秒</option>
                </select>
                <select aria-label="自动翻译目标语言" value={ocrTarget} onChange={(e) => useAppStore.setState({ ocrTarget: e.target.value })} className="cursor-pointer rounded border border-[var(--stroke)] bg-[var(--surface-2)] p-1 text-[11px] text-[var(--text)] focus-ring">
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

      {/* 边缘直角拉伸手柄 (Resize Grip) */}
      {hasContent && (
        <div
          onMouseDown={handleResizeMouseDown}
          onDoubleClick={handleResizeDoubleClick}
          title="拖拽边缘直角调整长宽，双击恢复自动大小"
          className="group absolute bottom-0.5 right-0.5 z-50 flex h-4 w-4 cursor-se-resize items-end justify-end p-0.5 select-none"
        >
          <svg
            width="8"
            height="8"
            viewBox="0 0 8 8"
            className="text-[var(--faint)] opacity-40 transition-opacity group-hover:opacity-100 group-hover:text-[var(--accent-b)]"
          >
            <line x1="7" y1="1" x2="1" y2="7" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <line x1="7" y1="4" x2="4" y2="7" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            <line x1="7" y1="7" x2="7" y2="7" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </div>
      )}

      {errorMessage && (
        <div className="mt-2 rounded-[var(--r-ctrl)] border border-rose-500/30 bg-rose-500/20 px-2.5 py-1 text-[11px] text-rose-300">
          {errorMessage}
        </div>
      )}
    </div>
  );
};
