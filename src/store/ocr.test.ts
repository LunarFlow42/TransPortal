import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './useAppStore';
import type { OcrConfig, OcrEvent, StreamChunk } from '../types';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: class { onmessage = (_chunk: StreamChunk) => {}; },
}));

const mockInvoke = vi.mocked(invoke);
const ocrEvent = (patch: Partial<OcrEvent> = {}): OcrEvent => ({
  id: 1, mode: 'text', stage: 'ready', text: 'First line.\nSecond line.',
  engine: 'windows', language: 'en-US', elapsed_ms: 75,
  copied: true, copy_error: null, error: null, ...patch,
});

beforeEach(() => {
  useAppStore.getState().clearLookup();
  useAppStore.setState({
    lastOcrEvent: null, capture: null, provider: 'google_free',
    hotkey: 'Alt+Q', ocrEngine: 'windows', ocrLanguage: 'auto', settingsError: null,
  });
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(async (command, args) => command === 'set_ocr_config'
    ? (args as { config: OcrConfig }).config : undefined);
});

describe('OCR workflows', () => {
  it('text-only OCR preserves line breaks and never calls translation or a dictionary', () => {
    const { applyOcrEvent } = useAppStore.getState();
    applyOcrEvent(ocrEvent({ stage: 'capturing', text: '' }));
    applyOcrEvent(ocrEvent({ stage: 'recognizing', text: '' }));
    applyOcrEvent(ocrEvent());
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(useAppStore.getState().ocr?.text).toBe('First line.\nSecond line.');
    expect(useAppStore.getState().lookupStack).toHaveLength(0);
  });

  it('OCR translation uses the configured provider even for a single word', () => {
    useAppStore.setState({ provider: 'tencent_free' });
    useAppStore.getState().applyOcrEvent(ocrEvent({ mode: 'translate', text: 'hello' }));
    expect(mockInvoke).toHaveBeenCalledExactlyOnceWith('translate_stream', expect.objectContaining({
      query: { text: 'hello', target_lang: 'zh-CN' }, provider: 'tencent_free',
    }));
    expect(useAppStore.getState().ocr).toBeNull();
    expect(useAppStore.getState().lookupStack[0]).toMatchObject({ kind: 'sentence', text: 'hello' });
  });

  it('duplicate replay, older sessions and late progress cannot retranslate or replace a result', () => {
    const { applyOcrEvent } = useAppStore.getState();
    const ready = ocrEvent({ id: 2, mode: 'translate' });
    applyOcrEvent(ready);
    const originalNode = useAppStore.getState().lookupStack[0];
    applyOcrEvent(ready);
    applyOcrEvent(ocrEvent({ id: 1, mode: 'translate', text: 'old' }));
    applyOcrEvent(ocrEvent({ id: 2, stage: 'recognizing' }));
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().lookupStack[0].id).toBe(originalNode.id);
    expect(useAppStore.getState().ocr).toBeNull();
  });

  it('cancelled OCR cannot surface late text or start a translation', () => {
    const store = useAppStore.getState();
    store.applyOcrEvent(ocrEvent({ stage: 'recognizing', mode: 'translate' }));
    store.cancelOcr();
    store.applyOcrEvent(ocrEvent({ mode: 'translate' }));
    expect(mockInvoke).toHaveBeenCalledExactlyOnceWith('cancel_ocr');
    expect(useAppStore.getState().ocr).toBeNull();
    expect(useAppStore.getState().lookupStack).toHaveLength(0);
  });

  it('a pending word lookup cannot launch a fallback translation after starting pure OCR', async () => {
    let resolveDictionary!: (value: null) => void;
    mockInvoke.mockImplementation(async (command) => {
      if (command === 'detect_intent') return { intent: 'word' };
      if (command === 'lookup_dict') return new Promise<null>((resolve) => { resolveDictionary = resolve; });
      return undefined;
    });
    const pending = useAppStore.getState().processText('previous');
    await vi.waitFor(() => expect(resolveDictionary).toBeTypeOf('function'));
    useAppStore.getState().applyOcrEvent(ocrEvent({ stage: 'capturing', text: '' }));
    useAppStore.getState().applyOcrEvent(ocrEvent());
    resolveDictionary(null);
    await pending;
    expect(mockInvoke.mock.calls.map(([command]) => command)).toEqual(['detect_intent', 'lookup_dict']);
    expect(useAppStore.getState().ocr?.stage).toBe('ready');
    expect(useAppStore.getState().lookupStack).toHaveLength(0);
  });

  it('late translation stream errors cannot contaminate a new OCR result', () => {
    useAppStore.getState().translateOcrText('Previous screenshot.');
    const args = mockInvoke.mock.calls[0][1] as { onChunk: { onmessage: (chunk: StreamChunk) => void } };
    useAppStore.getState().applyOcrEvent(ocrEvent({ stage: 'capturing', text: '' }));
    useAppStore.getState().applyOcrEvent(ocrEvent());
    args.onChunk.onmessage({ text: '', is_done: true, error: 'old network error' });
    expect(useAppStore.getState().errorMessage).toBeNull();
    expect(useAppStore.getState().ocr?.stage).toBe('ready');
  });

  it('translates the edited OCR text only on an explicit user action', () => {
    useAppStore.getState().applyOcrEvent(ocrEvent());
    expect(mockInvoke).not.toHaveBeenCalled();
    useAppStore.getState().translateOcrText('Corrected OCR text.');
    expect(mockInvoke).toHaveBeenCalledExactlyOnceWith('translate_stream', expect.objectContaining({
      query: { text: 'Corrected OCR text.', target_lang: 'zh-CN' },
    }));
  });
});

describe('native-backed settings', () => {
  it('does not persist an invalid or occupied selection shortcut', async () => {
    mockInvoke.mockRejectedValueOnce('Alt+S is reserved');
    expect(await useAppStore.getState().setHotkey('Alt+S')).toBe(false);
    expect(useAppStore.getState().hotkey).toBe('Alt+Q');
    expect(useAppStore.getState().settingsError).toContain('reserved');
  });

  it('applies an installed OCR language and keeps it if a later change fails', async () => {
    expect(await useAppStore.getState().setOcrLanguage('zh-Hans-CN')).toBe(true);
    expect(mockInvoke).toHaveBeenLastCalledWith('set_ocr_config', { config: { engine: 'windows', language: 'zh-Hans-CN' } });
    mockInvoke.mockRejectedValueOnce('language not installed');
    expect(await useAppStore.getState().setOcrLanguage('ja')).toBe(false);
    expect(useAppStore.getState().ocrLanguage).toBe('zh-Hans-CN');
    expect(await useAppStore.getState().setOcrLanguage('auto')).toBe(true);
    expect(mockInvoke).toHaveBeenLastCalledWith('set_ocr_config', { config: { engine: 'windows', language: null } });
  });

  it('old settings without OCR fields keep the automatic language default', () => {
    useAppStore.getState().hydrate({ hotkey: 'Ctrl+Shift+D', theme: 'dark' });
    expect(useAppStore.getState().ocrLanguage).toBe('auto');
    expect(useAppStore.getState().ocrEngine).toBe('windows');
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it('switches engine and clears incompatible language in one confirmed state update', async () => {
    useAppStore.setState({ ocrLanguage: 'ja' });
    const changes: Array<[string, string]> = [];
    const unsubscribe = useAppStore.subscribe((state) => changes.push([state.ocrEngine, state.ocrLanguage]));
    expect(await useAppStore.getState().setOcrEngine('onnx')).toBe(true);
    unsubscribe();
    expect(mockInvoke).toHaveBeenLastCalledWith('set_ocr_config', { config: { engine: 'onnx', language: null } });
    expect(changes).toEqual([['onnx', 'auto']]);
  });

  it('retains Windows OCR and its language when ONNX is missing or cannot load', async () => {
    useAppStore.setState({ ocrLanguage: 'en-US' });
    mockInvoke.mockRejectedValueOnce('ONNX Runtime cannot load');
    expect(await useAppStore.getState().setOcrEngine('onnx')).toBe(false);
    expect(useAppStore.getState()).toMatchObject({ ocrEngine: 'windows', ocrLanguage: 'en-US' });
  });

  it('restores the selected ONNX engine when hydrating settings in the reader window', () => {
    useAppStore.getState().hydrate({ ocrEngine: 'onnx', ocrLanguage: 'auto' });
    expect(useAppStore.getState().ocrEngine).toBe('onnx');
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});

describe('ONNX image input', () => {
  it('sends local images to Rust and displays text without an automatic translation', async () => {
    await useAppStore.getState().startImageOcr('C:/fixtures/mixed.png');
    expect(mockInvoke).toHaveBeenCalledExactlyOnceWith('ocr_image', { path: 'C:/fixtures/mixed.png', mode: 'text' });
    useAppStore.getState().applyOcrEvent(ocrEvent({ engine: 'onnx', image_path: 'C:/fixtures/mixed.png' }));
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().ocr?.engine).toBe('onnx');
    expect(useAppStore.getState().ocr?.image_path).toBe('C:/fixtures/mixed.png');
  });
});
