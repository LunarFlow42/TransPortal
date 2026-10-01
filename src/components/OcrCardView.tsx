import React, { useEffect, useState, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { dismissShell } from '../hooks/useWindowAnimation';
import {
  Check,
  Copy,
  GripVertical,
  Languages,
  LoaderCircle,
  Maximize2,
  Minimize2,
  Pin,
  RefreshCw,
  RotateCcw,
  Settings,
  X,
  Sparkles,
  ZoomIn,
} from 'lucide-react';
import { Button, IconButton, Surface } from './ui';
import { InteractiveSentence } from './InteractiveSentence';
import { useAppStore } from '../store/useAppStore';
import { useImageZoomPan } from '../hooks/useImageZoomPan';
import { saveSettings } from '../store/persistence';
import { startWindowDrag } from '../utils/drag';
import { openSettingsWindow } from '../utils/windows';
import { renderPicture } from '../utils/picture';
import { cn } from '../utils/cn';
import type { OcrEvent, ProviderType, TranslateResult } from '../types';

const TARGET_LANG_OPTIONS = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'fr', label: 'Français' },
  { value: 'de', label: 'Deutsch' },
  { value: 'es', label: 'Español' },
  { value: 'ru', label: 'Русский' },
];

export const OcrCardView: React.FC<{ event: OcrEvent }> = ({ event }) => {
  const storeTargetLang = useAppStore((state) => state.ocrTarget || state.targetLanguage);
  const [targetLang, setTargetLang] = useState<string>(storeTargetLang || 'zh-CN');
  const [translation, setTranslation] = useState<string>('');
  const [translating, setTranslating] = useState<boolean>(false);
  const [translationError, setTranslationError] = useState<string | null>(null);

  const [copiedOriginal, setCopiedOriginal] = useState(false);
  const [copiedTranslation, setCopiedTranslation] = useState(false);

  const {
    isDragging: isZoomDragging,
    isZoomed,
    zoomPercent,
    containerRef: zoomContainerRef,
    resetZoom,
    setScale: setZoomScale,
    onWheel: onZoomWheel,
    onMouseDown: onZoomMouseDown,
    onMouseMove: onZoomMouseMove,
    onMouseUp: onZoomMouseUp,
    onMouseLeave: onZoomMouseLeave,
    onDoubleClick: onZoomDoubleClick,
    transformStyle: zoomTransformStyle,
  } = useImageZoomPan();

  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const copyOriginalTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copyTranslationTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (copyOriginalTimerRef.current) clearTimeout(copyOriginalTimerRef.current);
      if (copyTranslationTimerRef.current) clearTimeout(copyTranslationTimerRef.current);
    };
  }, []);

  const ocrPictureOriginal = useAppStore((state) => state.ocrPictureOriginal);
  const toggleOcrPictureOriginal = useAppStore((state) => state.toggleOcrPictureOriginal);
  const pictureTranslation = useAppStore((state) => state.ocrPictureTranslation);
  const pictureBusy = useAppStore((state) => state.ocrPictureBusy);

  const pinned = useAppStore((state) => state.pinned);
  const togglePinned = useAppStore((state) => state.togglePinned);
  const provider = useAppStore((state) => state.provider);
  const setProvider = useAppStore((state) => state.setProvider);
  const openaiConfig = useAppStore((state) => state.openaiConfig);

  const translateText = useCallback(async (textToTranslate: string, target: string) => {
    if (!textToTranslate.trim()) return;
    setTranslating(true);
    setTranslationError(null);
    try {
      const result = await invoke<TranslateResult>('translate_text', {
        query: { text: textToTranslate, target_lang: target },
        provider,
        openaiConfig: provider === 'openai' ? openaiConfig : null,
      });
      setTranslation(result.translated_text);
    } catch (err) {
      setTranslationError(String(err));
    } finally {
      setTranslating(false);
    }
  }, [provider, openaiConfig]);

  useEffect(() => {
    if (event.text) {
      void translateText(event.text, targetLang);
    }
  }, [event.id, event.text, targetLang, translateText]);

  useEffect(() => {
    if (!ocrPictureOriginal && !pictureTranslation && !pictureBusy && event.picture) {
      void useAppStore.getState().translateOcrPicture();
    }
  }, [ocrPictureOriginal, pictureTranslation, pictureBusy, event.picture]);

  useEffect(() => {
    if (!event.picture) return;
    let disposed = false;
    const work = document.createElement('canvas');
    void renderPicture(work, event.picture, ocrPictureOriginal ? null : pictureTranslation).then(() => {
      if (disposed || !canvasRef.current) return;
      canvasRef.current.width = work.width;
      canvasRef.current.height = work.height;
      const ctx = canvasRef.current.getContext('2d');
      ctx?.drawImage(work, 0, 0);
    }).catch((err) => {
      console.error('Failed to render picture overlay in card view:', err);
    });
    return () => {
      disposed = true;
    };
  }, [event.picture, ocrPictureOriginal, pictureTranslation]);

  const handleTargetLangChange = (lang: string) => {
    setTargetLang(lang);
    useAppStore.setState({ ocrTarget: lang });
    void translateText(event.text, lang);
  };

  const close = () => {
    useAppStore.getState().cancelOcr();
    useAppStore.getState().clearLookup();
    void dismissShell({ force: true, immediate: true });
  };

  const copyOriginal = async () => {
    try {
      await invoke('plugin:clipboard-manager|write_text', { text: event.text });
      setCopiedOriginal(true);
      if (copyOriginalTimerRef.current) clearTimeout(copyOriginalTimerRef.current);
      copyOriginalTimerRef.current = setTimeout(() => {
        setCopiedOriginal(false);
        copyOriginalTimerRef.current = null;
      }, 2000);
    } catch (err) {
      console.error('Failed to copy original text:', err);
    }
  };

  const copyTranslation = async () => {
    if (!translation) return;
    try {
      await invoke('plugin:clipboard-manager|write_text', { text: translation });
      setCopiedTranslation(true);
      if (copyTranslationTimerRef.current) clearTimeout(copyTranslationTimerRef.current);
      copyTranslationTimerRef.current = setTimeout(() => {
        setCopiedTranslation(false);
        copyTranslationTimerRef.current = null;
      }, 2000);
    } catch (err) {
      console.error('Failed to copy translation:', err);
    }
  };

  const switchToPicture = () => {
    const current = useAppStore.getState().ocr;
    if (current) {
      useAppStore.setState({ ocr: { ...current, mode: 'picture' }, ocrImageView: true });
      void useAppStore.getState().translateOcrPicture();
    }
  };

  return (
    <Surface className="w-full text-[var(--text)] flex flex-col select-none overflow-hidden h-full max-h-[92vh] p-0">
      {/* 顶部标题栏 & 控制区域 */}
      <div
        onMouseDown={startWindowDrag}
        className="flex cursor-grab active:cursor-grabbing items-center justify-between border-b border-[var(--stroke)] px-3.5 py-2 bg-[var(--surface-2)]/65 shadow-[inset_0_1px_0_var(--highlight)] backdrop-blur-sm shrink-0 select-none"
      >
        <div className="flex items-center gap-1.5 text-xs font-medium">
          <GripVertical className="h-3.5 w-3.5 text-[var(--muted)] opacity-40 hover:opacity-90 transition-opacity" />
          <span className="h-2 w-2 rounded-full bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.5)]" />
          <span>截图对照翻译</span>
          {event.language && (
            <span className="rounded bg-[var(--surface-3)] px-1.5 py-0.5 text-[10px] font-mono text-[var(--muted)]">
              {event.language}
            </span>
          )}
          {event.elapsed_ms != null && (
            <span className="text-[10px] text-[var(--faint)]">
              {event.elapsed_ms}ms
            </span>
          )}
        </div>

        <div className="flex items-center gap-1.5">
          {/* 目标语言切换下拉 */}
          <div className="flex items-center gap-1 text-xs">
            <Languages className="h-3 w-3 text-[var(--muted)]" />
            <select
              aria-label="目标语言"
              value={targetLang}
              onChange={(e) => handleTargetLangChange(e.target.value)}
              className="cursor-pointer rounded border border-[var(--stroke)] bg-[var(--surface-3)] px-1.5 py-0.5 text-[11px] text-[var(--text)] focus-ring outline-none"
            >
              {TARGET_LANG_OPTIONS.map((opt) => (
                <option key={opt.value} value={opt.value}>
                  {opt.label}
                </option>
              ))}
            </select>
          </div>

          {/* 翻译引擎切换下拉 */}
          <select
            aria-label="翻译引擎"
            value={provider}
            onChange={(e) => {
              const next = e.target.value as ProviderType;
              setProvider(next);
              void saveSettings({ provider: next });
              void translateText(event.text, targetLang);
            }}
            className="cursor-pointer rounded border border-[var(--stroke)] bg-[var(--surface-3)] px-1.5 py-0.5 text-[11px] font-mono text-[var(--muted)] hover:text-[var(--text)] focus-ring outline-none transition-colors"
            title="切换翻译引擎"
          >
            <option value="bing_free">Bing</option>
            <option value="google_free">Google</option>
            <option value="youdao_free">有道</option>
            <option value="tencent_free">腾讯</option>
            <option value="openai">OpenAI</option>
          </select>

          <IconButton
            title="重新翻译"
            onClick={() => void translateText(event.text, targetLang)}
            disabled={translating}
          >
            <RefreshCw className={cn('h-3.5 w-3.5', translating && 'animate-spin')} />
          </IconButton>

          <IconButton title={pinned ? 'Unpin' : 'Pin'} active={pinned} onClick={togglePinned}>
            <Pin className="h-3.5 w-3.5" />
          </IconButton>

          <IconButton title="设置" onClick={() => void openSettingsWindow()}>
            <Settings className="h-3.5 w-3.5" />
          </IconButton>

          <IconButton
            title="关闭 (Esc)"
            onClick={close}
            className="hover:bg-rose-500/90 hover:text-white"
          >
            <X className="h-3.5 w-3.5" />
          </IconButton>
        </div>
      </div>

      {/* 主体左图右文双栏布局 */}
      <div className="flex flex-1 min-h-0 gap-3.5 overflow-hidden p-3.5">
        {/* 左栏：截图原图与覆写译图预览区 */}
        <div className="w-[42%] max-w-[340px] shrink-0 flex flex-col gap-2 min-h-0 border-r border-[var(--stroke)] pr-3.5">
          <div className="flex items-center justify-between text-[11px] text-[var(--muted)] shrink-0">
            <div className="flex items-center gap-1.5">
              <span className="font-medium">{ocrPictureOriginal ? '截图原图' : '译图覆写'}</span>
              <Button
                size="sm"
                variant={ocrPictureOriginal ? 'primary' : 'subtle'}
                className="h-5 px-1.5 text-[10px]"
                onClick={toggleOcrPictureOriginal}
                title="快捷键 Tab：短按切换，长按透视原图"
              >
                {ocrPictureOriginal ? '查看译图 (Tab)' : '查看原图 (Tab)'}
              </Button>
            </div>
            <div className="flex items-center gap-1">
              {event.picture && (
                <span className="font-mono text-[10px] text-[var(--faint)]">
                  {event.picture.width} × {event.picture.height}
                </span>
              )}
              {isZoomed && (
                <button
                  type="button"
                  onClick={resetZoom}
                  title="点击复位为适应窗口 (双击图片也可复位)"
                  className="flex items-center gap-0.5 rounded bg-[var(--surface-3)] px-1 py-0.5 text-[10px] font-mono text-[var(--accent-b)] hover:bg-[var(--accent-b)]/20 transition-colors cursor-pointer"
                >
                  <RotateCcw className="h-2.5 w-2.5" />
                  <span>{zoomPercent}</span>
                </button>
              )}
              <IconButton
                title={isZoomed ? '复位为适应窗口 (双击图片也可复位)' : '放大查看 (支持鼠标滚轮自由缩放/拖拽)'}
                onClick={() => {
                  if (isZoomed) {
                    resetZoom();
                  } else {
                    setZoomScale(1.8);
                  }
                }}
              >
                {isZoomed ? (
                  <Minimize2 className="h-3 w-3 text-[var(--accent-b)]" />
                ) : (
                  <Maximize2 className="h-3 w-3" />
                )}
              </IconButton>
            </div>
          </div>

          {event.picture ? (
            <div
              ref={zoomContainerRef}
              onWheel={onZoomWheel}
              onMouseDown={onZoomMouseDown}
              onMouseMove={onZoomMouseMove}
              onMouseUp={onZoomMouseUp}
              onMouseLeave={onZoomMouseLeave}
              onDoubleClick={onZoomDoubleClick}
              className={cn(
                'relative flex-1 min-h-0 overflow-hidden rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)]/60 flex items-center justify-center p-1.5 shadow-inner select-none',
                isZoomDragging ? 'cursor-grabbing' : isZoomed ? 'cursor-grab' : 'cursor-zoom-in',
              )}
              title={isZoomed ? '滚轮缩放 · 拖拽平移 · 双击复位' : '滚轮放大 · 拖拽平移'}
            >
              <div
                style={zoomTransformStyle}
                className="max-w-full max-h-full flex items-center justify-center"
              >
                <canvas
                  ref={canvasRef}
                  aria-label={ocrPictureOriginal ? '原始截图' : '文字原位替换后的译图'}
                  className="max-w-full max-h-full object-contain rounded select-none pointer-events-auto"
                  style={{ display: 'block' }}
                />
              </div>

              {isZoomed && (
                <div
                  onClick={resetZoom}
                  title="点击一键复位（双击图片也可复位）"
                  className="absolute bottom-2 left-2 z-10 flex items-center gap-1 rounded bg-[var(--surface-1)]/90 px-1.5 py-0.5 shadow-md border border-[var(--stroke)] text-[10px] font-mono text-[var(--text)] hover:text-[var(--accent-b)] transition-colors cursor-pointer backdrop-blur-sm"
                >
                  <ZoomIn className="h-3 w-3 text-[var(--accent-b)]" />
                  <span>{zoomPercent}</span>
                  <RotateCcw className="h-2.5 w-2.5 opacity-60 ml-0.5" />
                </div>
              )}

              {pictureBusy && !ocrPictureOriginal && (
                <div className="absolute inset-0 flex items-center justify-center bg-black/30 backdrop-blur-[1px] rounded-[var(--r-inner)] pointer-events-none">
                  <div className="flex items-center gap-1.5 bg-[var(--surface-1)]/90 px-2.5 py-1 rounded-full shadow text-[11px] text-[var(--text)]">
                    <LoaderCircle className="h-3 w-3 animate-spin text-[var(--accent-b)]" />
                    <span>覆写翻译中…</span>
                  </div>
                </div>
              )}
            </div>
          ) : (
            <div className="flex-1 min-h-0 rounded-[var(--r-inner)] border border-dashed border-[var(--stroke)] flex items-center justify-center text-xs text-[var(--muted)]">
              暂无截图预览
            </div>
          )}

          {/* 切换至原位覆写翻译 */}
          <Button
            size="sm"
            variant="subtle"
            className="w-full text-[11px] shrink-0 mt-0.5"
            onClick={switchToPicture}
            title="在原图片上覆盖显示翻译结果"
          >
            <Sparkles className="h-3 w-3 text-[var(--accent-b)]" />
            切换原图覆写翻译
          </Button>
        </div>

        {/* 右栏：双语对照与长文点词查词区 */}
        <div className="flex-1 min-w-0 flex flex-col gap-3 overflow-y-auto no-scrollbar pr-0.5">
          {/* 原文模块 */}
          <div className="flex flex-col gap-1.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)]/40 p-3">
            <div className="flex items-center justify-between shrink-0">
              <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text)]">
                <span>原文</span>
                <span className="text-[10px] font-normal text-[var(--accent-b)] bg-[var(--accent-b)]/10 px-1.5 py-0.2 rounded">
                  点击单词即刻查词
                </span>
              </div>
              <Button
                size="sm"
                variant="subtle"
                onClick={() => void copyOriginal()}
                className="h-6 px-2 text-[11px]"
              >
                {copiedOriginal ? (
                  <>
                    <Check className="h-3 w-3 text-emerald-400" />
                    已复制
                  </>
                ) : (
                  <>
                    <Copy className="h-3 w-3" />
                    复制原文
                  </>
                )}
              </Button>
            </div>

            {/* 交互式分词句子渲染 */}
            <div className="mt-1 min-h-[4rem] text-sm leading-relaxed text-[var(--text)] select-text">
              <InteractiveSentence
                sentence={event.text}
                className="leading-relaxed"
              />
            </div>
          </div>

          {/* 译文模块 */}
          <div className="flex flex-col gap-1.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)]/40 p-3">
            <div className="flex items-center justify-between shrink-0">
              <div className="flex items-center gap-1.5 text-xs font-semibold text-[var(--text)]">
                <span>译文</span>
                <span className="text-[10px] font-normal text-[var(--muted)]">
                  {provider}
                </span>
              </div>
              <Button
                size="sm"
                variant="subtle"
                disabled={!translation || translating}
                onClick={() => void copyTranslation()}
                className="h-6 px-2 text-[11px]"
              >
                {copiedTranslation ? (
                  <>
                    <Check className="h-3 w-3 text-emerald-400" />
                    已复制
                  </>
                ) : (
                  <>
                    <Copy className="h-3 w-3" />
                    复制译文
                  </>
                )}
              </Button>
            </div>

            {/* 译文内容 */}
            <div className="mt-1 min-h-[4rem] text-sm leading-relaxed select-text">
              {translating ? (
                <div className="flex items-center gap-2 py-4 text-xs text-[var(--muted)]">
                  <LoaderCircle className="h-4 w-4 animate-spin text-[var(--accent-b)]" />
                  <span>正在翻译中…</span>
                </div>
              ) : translationError ? (
                <div className="rounded border border-rose-500/30 bg-rose-500/10 p-2 text-xs text-rose-300">
                  <p>翻译失败：{translationError}</p>
                  <Button
                    size="sm"
                    className="mt-1.5 h-6 text-[10px]"
                    onClick={() => void translateText(event.text, targetLang)}
                  >
                    重试
                  </Button>
                </div>
              ) : (
                <p className="whitespace-pre-wrap text-[var(--text)] leading-relaxed">
                  {translation || '等待翻译结果…'}
                </p>
              )}
            </div>
          </div>
        </div>
      </div>
    </Surface>
  );
};
