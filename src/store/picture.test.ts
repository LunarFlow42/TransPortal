import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './useAppStore';
import type { MonitorUpdate, OcrEvent, OcrPicture, PictureTranslation } from '../types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
const picture: OcrPicture = { width: 100, height: 40, data_url: 'data:image/png;base64,fixture',
  blocks: [{ id: 0, text: 'Hello', x: 5, y: 5, width: 80, height: 25 }] };
const translated: PictureTranslation = { target_lang: 'zh-CN', blocks: [{ id: 0, text: '你好', error: null }] };
const event = (id: number): OcrEvent => ({ id, stage: 'ready', mode: 'translate', text: 'Hello', engine: 'onnx',
  language: 'en', elapsed_ms: 10, copied: false, copy_error: null, error: null, picture });
const monitor = (revision: number, patch: Partial<MonitorUpdate> = {}): MonitorUpdate => ({
  id: 1, revision, active: true, selecting: false, paused: false, stage: 'watching', interval_ms: 2000,
  target_lang: 'zh-CN', checks: 1, region: { x: -500, y: 50, width: 100, height: 40 }, error: null,
  frame: { number: 1, picture, translation: translated, engine: 'onnx', elapsed_ms: 20 }, ...patch,
});

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(translated);
  useAppStore.getState().clearLookup();
  useAppStore.setState({ lastOcrEvent: null, autoOcr: null, autoOcrRevision: -1, provider: 'tencent_free', ocrTarget: 'zh-CN' });
});

describe('picture translation sessions', () => {
  it('keeps the screenshot and coordinates, translates only once on ready/replay, and bypasses dictionary routing', async () => {
    useAppStore.getState().applyOcrEvent(event(1));
    useAppStore.getState().applyOcrEvent(event(1));
    await vi.waitFor(() => expect(useAppStore.getState().ocrPictureBusy).toBe(false));
    expect(invoke).toHaveBeenCalledExactlyOnceWith('translate_ocr_picture', {
      id: 1, texts: null, targetLang: 'zh-CN', provider: 'tencent_free', openaiConfig: null,
    });
    expect(useAppStore.getState().ocr?.picture).toEqual(picture);
    expect(useAppStore.getState().ocrPictureTranslation).toEqual(translated);
    expect(useAppStore.getState().lookupStack).toEqual([]);
  });

  it('discards a pending translation after cancellation, including late errors', async () => {
    let reject!: (reason: unknown) => void;
    vi.mocked(invoke).mockImplementation(async (command) => command === 'translate_ocr_picture'
      ? new Promise((_, fail) => { reject = fail; }) : undefined);
    useAppStore.getState().applyOcrEvent(event(1));
    useAppStore.getState().cancelOcr();
    reject('slow old provider error');
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(useAppStore.getState().ocr).toBeNull();
    expect(useAppStore.getState().ocrPictureTranslation).toBeNull();
    expect(useAppStore.getState().ocrPictureError).toBeNull();
  });

  it('uses edited block text and the selected target without changing image coordinates', async () => {
    useAppStore.getState().applyOcrEvent({ ...event(1), mode: 'text' });
    expect(invoke).not.toHaveBeenCalled();
    useAppStore.setState({ ocrTarget: 'en' });
    await useAppStore.getState().translateOcrPicture(['Corrected text']);
    expect(invoke).toHaveBeenCalledWith('translate_ocr_picture', expect.objectContaining({ texts: ['Corrected text'], targetLang: 'en' }));
    expect(useAppStore.getState().ocr?.picture?.blocks[0]).toEqual(picture.blocks[0]);
  });
});

describe('automatic monitor events', () => {
  it('cannot be revived by an older frame after stopping and discarding', () => {
    useAppStore.getState().applyAutoOcr(monitor(1));
    useAppStore.getState().applyAutoOcr(monitor(3, { active: false, stage: 'stopped', frame: null }));
    useAppStore.getState().applyAutoOcr(monitor(2));
    expect(useAppStore.getState().autoOcr).toBeNull();
  });

  it('clears old translations when the watched region becomes blank', () => {
    useAppStore.getState().applyAutoOcr(monitor(1));
    useAppStore.getState().applyAutoOcr(monitor(2, { frame: { number: 2, picture: { ...picture, blocks: [] },
      translation: { blocks: [], target_lang: 'zh-CN' }, engine: 'onnx', elapsed_ms: 20 } }));
    expect(useAppStore.getState().autoOcr?.frame?.translation?.blocks).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('starts a monitor with an explicit interval and provider snapshot', async () => {
    await useAppStore.getState().startAutoOcr(5000);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('begin_auto_ocr', { options: {
      provider: 'tencent_free', openai_config: null, target_lang: 'zh-CN', interval_ms: 5000,
    } });
  });
});

describe('picture original peek/toggle state', () => {
  it('defaults to false and toggles between translated and original view', () => {
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);

    useAppStore.getState().toggleOcrPictureOriginal();
    expect(useAppStore.getState().ocrPictureOriginal).toBe(true);

    useAppStore.getState().toggleOcrPictureOriginal();
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);

    useAppStore.getState().setOcrPictureOriginal(true);
    expect(useAppStore.getState().ocrPictureOriginal).toBe(true);
  });

  it('resets ocrPictureOriginal to false on new OCR events, cancellation, and clearLookup', () => {
    useAppStore.getState().setOcrPictureOriginal(true);
    expect(useAppStore.getState().ocrPictureOriginal).toBe(true);

    useAppStore.getState().applyOcrEvent(event(2));
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);

    useAppStore.getState().setOcrPictureOriginal(true);
    useAppStore.getState().cancelOcr();
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);

    useAppStore.getState().setOcrPictureOriginal(true);
    useAppStore.getState().clearLookup();
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);
  });

  it('respects ocrPictureDefaultView configuration when processing new OCR events', () => {
    expect(useAppStore.getState().ocrPictureDefaultView).toBe('translated');

    // Configure default view to 'original'
    useAppStore.getState().setOcrPictureDefaultView('original');
    expect(useAppStore.getState().ocrPictureDefaultView).toBe('original');

    useAppStore.getState().applyOcrEvent(event(3));
    expect(useAppStore.getState().ocrPictureOriginal).toBe(true);

    useAppStore.getState().clearLookup();
    expect(useAppStore.getState().ocrPictureOriginal).toBe(true);

    // Switch back to 'translated'
    useAppStore.getState().setOcrPictureDefaultView('translated');
    useAppStore.getState().applyOcrEvent(event(4));
    expect(useAppStore.getState().ocrPictureOriginal).toBe(false);
  });
});
