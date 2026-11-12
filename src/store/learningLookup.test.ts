import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from './useAppStore';
import type { DictEntry, LookupNode } from '../types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock('./persistence', () => ({ saveSettings: vi.fn() }));

const wordData: DictEntry = {
  word: 'portal', definitions: [{ pos: 'n.', meanings: ['入口'] }], tags: [], source: 'local_eccedict',
  examples: [{ en: 'A portal opened.', zh: '入口打开了。' }],
};
const node: LookupNode = {
  id: 1, kind: 'word', text: 'portal', density: 'dense', intent: 'word',
  wordData, translation: '', status: 'ready',
};

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockImplementation(async (command) => {
    if (command === 'detect_intent') return { intent: 'word' };
    if (command === 'lookup_dict') return wordData;
    if (command === 'generate_examples') return [{ en: 'The portal leads home.', zh: '入口通往家。' }];
    if (command === 'translate_stream') return;
    throw new Error(`Unexpected IPC: ${command}`);
  });
  useAppStore.setState({
    provider: 'bing_free', openaiConfig: { api_key: '' },
    lookupStack: [], history: [], historyIndex: -1,
  });
});

describe('normal lookup remains separate from learning collection', () => {
  it('lookup and example drill-down never invoke enrollment or additional capture', async () => {
    await useAppStore.getState().processText('portal');
    await useAppStore.getState().pushLookup('portal', { text: 'A portal opened.', word: 'portal' });
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
      'detect_intent', 'lookup_dict', 'lookup_dict',
    ]);
    expect(useAppStore.getState().lookupStack[1]).toMatchObject({
      originText: 'A portal opened.', originWord: 'portal', wordData,
    });
  });

  it('marks only newly generated examples and retains dictionary example data', async () => {
    useAppStore.setState({ provider: 'openai', openaiConfig: { api_key: 'test-only' }, lookupStack: [node] });
    await useAppStore.getState().fetchWordExamples('portal', 1);
    expect(useAppStore.getState().lookupStack[0].wordData?.examples).toEqual([
      wordData.examples[0],
      { en: 'The portal leads home.', zh: '入口通往家。', source: 'generated' },
    ]);
    expect(invoke).toHaveBeenCalledOnce();
    expect(vi.mocked(invoke).mock.calls[0][0]).toBe('generate_examples');
  });

  it('keeps translation provenance when the selected provider subsequently changes', async () => {
    useAppStore.setState({ provider: 'openai', lookupStack: [{ ...node, wordData: null }] });
    await useAppStore.getState().startStreamTranslation('portal', 1);
    useAppStore.setState({ provider: 'google_free' });
    expect(useAppStore.getState().lookupStack[0].translationProvider).toBe('openai');
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual(['translate_stream']);
  });
});
