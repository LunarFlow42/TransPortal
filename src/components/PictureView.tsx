import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { Copy, Download, Languages, LoaderCircle } from 'lucide-react';
import { Button } from './ui';
import { renderPicture } from '../utils/picture';
import { suppressAutoHide } from '../utils/autoHide';
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
  const [original, setOriginal] = useState(false);
  const [zoom, setZoom] = useState(false);
  const [editing, setEditing] = useState(true);
  const [texts, setTexts] = useState(picture.blocks.map((block) => block.text));
  const [notice, setNotice] = useState<string | null>(null);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
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

  const exportImage = async (toFile: boolean) => {
    setExporting(true); setNotice(null);
    try {
      suppressAutoHide(60_000);
      const path = toFile ? await save({ defaultPath: 'TransPortal-translation.png', filters: [{ name: 'PNG 图片', extensions: ['png'] }] }) : null;
      if (toFile && !path) return;
      const result = document.createElement('canvas');
      await renderPicture(result, picture, translation);
      await invoke('export_ocr_picture', { dataUrl: result.toDataURL('image/png'), path });
      setNotice(toFile ? '译图已保存' : '译图已复制');
    } catch (error) { setNotice(String(error)); }
    finally { setExporting(false); }
  };

  const failures = translation?.blocks.filter((block) => block.error) ?? [];
  return <div className="mt-3 space-y-2">
    <div className="flex flex-wrap items-center gap-2 text-xs">
      <Button size="sm" variant={original ? 'primary' : 'subtle'} onClick={() => setOriginal(!original)}>{original ? '查看译图' : '查看原图'}</Button>
      <Button size="sm" variant="subtle" onClick={() => setZoom(!zoom)}>{zoom ? '适应窗口' : '100% 大小'}</Button>
      <span className="ml-auto text-[10px] text-[var(--faint)]">{picture.width} × {picture.height} · {picture.blocks.length} 段</span>
    </div>
    <div className="max-h-[330px] overflow-auto rounded-lg border border-[var(--stroke)] bg-[var(--surface-2)]">
      <canvas ref={canvas} aria-label={original ? '原始截图' : '文字原位替换后的译图'} style={{ width: zoom ? picture.width : '100%', display: 'block' }} />
    </div>
    <div className="flex flex-wrap items-center gap-2">
      {onTranslate && <>
        <select aria-label="图片翻译目标语言" value={target} disabled={busy} onChange={(e) => onTarget?.(e.target.value)} className="focus-ring rounded-md bg-[var(--surface-2)] p-1.5 text-xs">
          <option value="zh-CN">简体中文</option><option value="en">English</option><option value="ja">日本語</option><option value="ko">한국어</option>
        </select>
        <Button size="sm" variant="primary" disabled={busy || !texts.length || texts.some((text) => !text.trim())} onClick={() => onTranslate(texts)}>
          {busy ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <Languages className="h-3 w-3" />}{busy ? '翻译中…' : translation ? '重新翻译' : '原位翻译'}
        </Button>
      </>}
      <Button size="sm" disabled={!translation || busy || exporting} onClick={() => void exportImage(false)}><Copy className="h-3 w-3" />复制译图</Button>
      <Button size="sm" disabled={!translation || busy || exporting} onClick={() => void exportImage(true)}><Download className="h-3 w-3" />保存译图</Button>
      <Button size="sm" variant="subtle" onClick={() => setEditing(!editing)}>{editing ? '收起文字' : onTranslate ? '校对文字' : '查看文字'}</Button>
    </div>
    {failures.length > 0 && <p role="alert" className="text-xs text-rose-400">{failures.length} 段翻译失败，已保留原文：{failures[0].error}</p>}
    {(error || renderError) && <p role="alert" className="text-xs text-rose-400">{error || renderError}</p>}
    {notice && <p role="status" className="text-xs text-[var(--muted)]">{notice}</p>}
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
