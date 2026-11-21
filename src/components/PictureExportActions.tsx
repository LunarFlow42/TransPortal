import React, { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { Copy, Download, LoaderCircle } from 'lucide-react';
import { Button } from './ui';
import { renderPicture } from '../utils/picture';
import { resumeAutoHide, suppressAutoHide } from '../utils/autoHide';
import type { OcrPicture, PictureTranslation } from '../types';

interface Props {
  picture: OcrPicture | null | undefined;
  translation: PictureTranslation | null;
  busy?: boolean;
}

/** Export the translated source image, independently of preview zoom or original-image mode. */
export const PictureExportActions: React.FC<Props> = ({ picture, translation, busy }) => {
  const pending = useRef(false);
  const [exporting, setExporting] = useState<'copy' | 'save' | null>(null);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);
  const hasTranslation = Boolean(translation?.blocks.some((block) => !block.error && block.text.trim()));
  const disabled = !picture || !hasTranslation || busy || exporting !== null;

  const exportImage = async (toFile: boolean) => {
    if (!picture || !translation || !hasTranslation || busy || pending.current) return;
    pending.current = true;
    setExporting(toFile ? 'save' : 'copy');
    setNotice(null);
    // Native save dialogs may stay open for any length of time.
    suppressAutoHide(Infinity);
    try {
      const path = toFile ? await save({
        defaultPath: 'TransPortal-translation.png',
        filters: [{ name: 'PNG 图片', extensions: ['png'] }],
      }) : null;
      if (toFile && !path) return;
      const result = document.createElement('canvas');
      await renderPicture(result, picture, translation);
      await invoke('export_ocr_picture', { dataUrl: result.toDataURL('image/png'), path });
      setNotice({ text: toFile ? '译图已保存' : '译图已复制', error: false });
    } catch (error) {
      setNotice({ text: `${toFile ? '保存' : '复制'}译图失败：${String(error)}`, error: true });
    } finally {
      resumeAutoHide();
      pending.current = false;
      setExporting(null);
    }
  };

  const title = !hasTranslation ? '请先查看译图并等待翻译完成' : '导出完整分辨率的译图';
  return <>
    <Button size="sm" disabled={disabled} title={title} onClick={() => void exportImage(false)}>
      {exporting === 'copy' ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <Copy className="h-3 w-3" />}复制译图
    </Button>
    <Button size="sm" disabled={disabled} title={title} onClick={() => void exportImage(true)}>
      {exporting === 'save' ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <Download className="h-3 w-3" />}保存译图
    </Button>
    {notice && <p role={notice.error ? 'alert' : 'status'} className={`w-full text-[11px] ${notice.error ? 'text-rose-400' : 'text-[var(--muted)]'}`}>{notice.text}</p>}
  </>;
};
