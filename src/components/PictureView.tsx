import React, { useEffect, useRef, useState } from 'react';
import { Languages, LoaderCircle, RotateCcw, ZoomIn } from 'lucide-react';
import { Button } from './ui';
import { useAppStore } from '../store/useAppStore';
import { useImageZoomPan } from '../hooks/useImageZoomPan';
import { renderPicture } from '../utils/picture';
import { PictureExportActions } from './PictureExportActions';
import type { OcrPicture, PictureTranslation } from '../types';

interface Props {
  picture: OcrPicture;
  translation: PictureTranslation | null;
  busy?: boolean;
  error?: string | null;
  target?: string;
  onTarget?: (target: string) => void;
  onTranslate?: (texts: string[]) => void;
}

export const PictureView: React.FC<Props> = ({ picture, translation, busy, error, target = 'zh-CN', onTarget, onTranslate }) => {
  const canvas = useRef<HTMLCanvasElement>(null);
  const original = useAppStore((state) => state.ocrPictureOriginal);
  const toggleOriginal = useAppStore((state) => state.toggleOcrPictureOriginal);

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

  const [editing, setEditing] = useState(true);
  const [texts, setTexts] = useState(picture.blocks.map((block) => block.text));
  const [renderError, setRenderError] = useState<string | null>(null);
  useEffect(() => { setTexts(picture.blocks.map((block) => block.text)); }, [picture]);
  useEffect(() => {
    let disposed = false;
    // Render off-screen so an older async image decode cannot overwrite a new frame.
    const work = document.createElement('canvas');
    void renderPicture(work, picture, original ? null : translation).then(() => {
      if (disposed || !canvas.current) return;
      canvas.current.width = work.width;
      canvas.current.height = work.height;
      canvas.current.getContext('2d')?.drawImage(work, 0, 0);
      setRenderError(null);
    }).catch((error) => { if (!disposed) setRenderError(String(error)); });
    return () => { disposed = true; };
  }, [picture, translation, original]);

  const failures = translation?.blocks.filter((block) => block.error) ?? [];
  return <div className="mt-3 space-y-2">
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <Button size="sm" variant={original ? 'primary' : 'subtle'} onClick={toggleOriginal} title="快捷键 Tab：短按切换，长按透视原图">
        {original ? '查看译图 (Tab)' : '查看原图 (Tab)'}
      </Button>
      <Button
        size="sm"
        variant="subtle"
        onClick={() => {
          if (isZoomed) {
            resetZoom();
          } else {
            setZoomScale(1.8);
          }
        }}
        title="支持滚轮缩放与鼠标拖拽平移，双击一键复位"
      >
        {isZoomed ? '适应窗口' : '放大查看'}
      </Button>
      {isZoomed && (
        <button
          type="button"
          onClick={resetZoom}
          title="点击复位为适应窗口，双击图片亦可复位"
          className="flex items-center gap-1 rounded bg-[var(--surface-3)] px-1.5 py-0.5 text-[10px] font-mono text-[var(--accent-b)] hover:bg-[var(--accent-b)]/20 transition-colors cursor-pointer"
        >
          <RotateCcw className="h-2.5 w-2.5" />
          <span>{zoomPercent}</span>
        </button>
      )}
      <span className="ml-auto text-[10px] text-[var(--faint)]">{picture.width} × {picture.height} · {picture.blocks.length} 段</span>
    </div>
    <div
      ref={zoomContainerRef}
      onWheel={onZoomWheel}
      onMouseDown={onZoomMouseDown}
      onMouseMove={onZoomMouseMove}
      onMouseUp={onZoomMouseUp}
      onMouseLeave={onZoomMouseLeave}
      onDoubleClick={onZoomDoubleClick}
      className={`relative max-h-[360px] min-h-[160px] overflow-hidden rounded-lg border border-[var(--stroke)] bg-[var(--surface-2)] flex items-center justify-center p-2 shadow-inner select-none ${
        isZoomDragging ? 'cursor-grabbing' : isZoomed ? 'cursor-grab' : 'cursor-zoom-in'
      }`}
      title={isZoomed ? '滚轮缩放 · 拖拽平移 · 双击复位' : '滚轮放大 · 拖拽平移'}
    >
      <div style={zoomTransformStyle} className="max-w-full max-h-full flex items-center justify-center">
        <canvas
          ref={canvas}
          aria-label={original ? '原始截图' : '文字原位替换后的译图'}
          style={{ maxWidth: '100%', maxHeight: '340px', objectFit: 'contain', display: 'block' }}
        />
      </div>
      {isZoomed && (
        <div
          onClick={resetZoom}
          title="点击一键复位，双击图片亦可复位"
          className="absolute bottom-2 left-2 z-10 flex items-center gap-1 rounded bg-[var(--popover-bg)] px-1.5 py-0.5 shadow-md border border-[var(--stroke)] text-[10px] font-mono text-[var(--text)] hover:text-[var(--accent-b)] transition-colors cursor-pointer backdrop-blur-sm"
        >
          <ZoomIn className="h-3 w-3 text-[var(--accent-b)]" />
          <span>{zoomPercent}</span>
          <RotateCcw className="h-2.5 w-2.5 opacity-60 ml-0.5" />
        </div>
      )}
    </div>
    <div className="flex flex-wrap items-center gap-2">
      {onTranslate && <>
        <select aria-label="图片翻译目标语言" value={target} disabled={busy} onChange={(e) => onTarget?.(e.target.value)} className="cursor-pointer focus-ring rounded-md border border-[var(--stroke)] bg-[var(--surface-2)] p-1.5 text-xs text-[var(--text)]">
          <option value="zh-CN">简体中文</option><option value="en">English</option><option value="ja">日本語</option><option value="ko">한국어</option>
        </select>
        <Button size="sm" variant="primary" disabled={busy || !texts.length || texts.some((text) => !text.trim())} onClick={() => onTranslate(texts)}>
          {busy ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <Languages className="h-3 w-3" />}{busy ? '翻译中…' : translation ? '重新翻译' : '原位翻译'}
        </Button>
      </>}
      <PictureExportActions picture={picture} translation={translation} busy={busy} />
      <Button size="sm" variant="subtle" onClick={() => setEditing(!editing)}>{editing ? '收起文字' : onTranslate ? '校对文字' : '查看文字'}</Button>
    </div>
    {failures.length > 0 && <p role="alert" className="text-xs text-rose-400">{failures.length} 段翻译失败，已保留原文：{failures[0].error}</p>}
    {(error || renderError) && <p role="alert" className="text-xs text-rose-400">{error || renderError}</p>}
    {!picture.blocks.length && <p className="text-xs text-[var(--muted)]">区域内未识别到文字。</p>}
    {editing && <div className="max-h-36 space-y-2 overflow-auto select-text">
      {picture.blocks.map((block, index) => <div key={block.id} className="rounded-md border border-[var(--stroke)] p-2 text-xs">
        {onTranslate ? <textarea aria-label={`原文第 ${index + 1} 段`} value={texts[index] ?? block.text} disabled={busy} rows={2}
          onChange={(e) => setTexts((current) => current.map((text, i) => i === index ? e.target.value : text))}
          className="w-full resize-y bg-transparent text-[var(--text)]" /> : <p className="whitespace-pre-wrap text-[var(--muted)]">{block.text}</p>}
        {translation?.blocks.find((item) => item.id === block.id)?.text && <p className="mt-1 whitespace-pre-wrap">{translation.blocks.find((item) => item.id === block.id)?.text}</p>}
      </div>)}
    </div>}
  </div>;
};
