import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { LEARNING_PAGE_SIZE, useLearningStore } from './useLearningStore';
import type { LearningDraft, LearningEntry, LearningPage } from '../types/learning';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));

const draft: LearningDraft = {
  language: 'en', lemma: 'portal', surface: 'portal', phonetic: null,
  pos: 'n.', meaning: '入口', definition_kind: 'dictionary', definition_source: 'local_eccedict',
  context: { text: 'A portal opened.', translation: '一个入口打开了。', source: 'dictionary' },
};
const entry: LearningEntry = { id: '1', usage: draft, created_at: 100, updated_at: 100 };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  useLearningStore.setState({
    entries: [], total: 0, query: '', offset: 0, loading: false,
    error: null, deletingId: null, exporting: null,
  });
});

describe('learning collection IPC and reader cache', () => {
  it('does not issue writes on initialization or list refresh', async () => {
    expect(invoke).not.toHaveBeenCalled();
    vi.mocked(invoke).mockResolvedValue({ entries: [entry], total: 1 });
    await useLearningStore.getState().load();
    expect(invoke).toHaveBeenCalledExactlyOnceWith('learning_list', { query: '', offset: 0, limit: LEARNING_PAGE_SIZE });
    expect(useLearningStore.getState().entries).toEqual([entry]);
  });

  it('enrolls only the explicitly supplied sense and context', async () => {
    vi.mocked(invoke).mockResolvedValue({ entry, created: true });
    await expect(useLearningStore.getState().enroll(draft)).resolves.toEqual({ entry, created: true });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('learning_enroll', { draft });
  });

  it('does not turn a failed enrollment into a saved state', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('disk full'));
    await expect(useLearningStore.getState().enroll(draft)).rejects.toThrow('disk full');
    expect(useLearningStore.getState().entries).toEqual([]);
  });

  it('ignores a stale list response after a newer search', async () => {
    const old = deferred<LearningPage>();
    vi.mocked(invoke).mockReturnValueOnce(old.promise).mockResolvedValueOnce({ entries: [entry], total: 1 });
    const pending = useLearningStore.getState().load('old');
    await useLearningStore.getState().load('portal');
    old.resolve({ entries: [], total: 0 });
    await pending;
    expect(useLearningStore.getState()).toMatchObject({ entries: [entry], query: 'portal', total: 1, loading: false });
  });

  it('preserves the last successful snapshot on read failure and permits retry', async () => {
    useLearningStore.setState({ entries: [entry], total: 1 });
    vi.mocked(invoke).mockRejectedValueOnce(new Error('database unavailable'));
    await useLearningStore.getState().load();
    expect(useLearningStore.getState()).toMatchObject({ entries: [entry], total: 1, loading: false, error: 'Error: database unavailable' });
    vi.mocked(invoke).mockResolvedValueOnce({ entries: [entry], total: 1 });
    await useLearningStore.getState().load();
    expect(useLearningStore.getState().error).toBeNull();
  });

  it('keeps data visible when deletion fails', async () => {
    useLearningStore.setState({ entries: [entry], total: 1 });
    vi.mocked(invoke).mockRejectedValue(new Error('write failed'));
    await expect(useLearningStore.getState().remove(entry.id)).rejects.toThrow('write failed');
    expect(useLearningStore.getState()).toMatchObject({ entries: [entry], total: 1, deletingId: null });
  });

  it('rejects a second delete while the first write is pending', async () => {
    const write = deferred<boolean>();
    vi.mocked(invoke).mockReturnValueOnce(write.promise).mockResolvedValueOnce({ entries: [], total: 0 });
    const pending = useLearningStore.getState().remove('1');
    await expect(useLearningStore.getState().remove('1')).rejects.toThrow('already in progress');
    expect(invoke).toHaveBeenCalledTimes(1);
    write.resolve(true);
    await pending;
    expect(useLearningStore.getState().deletingId).toBeNull();
  });

  it('does not resurrect a committed deletion when the subsequent refresh fails', async () => {
    useLearningStore.setState({ entries: [entry], total: 1 });
    vi.mocked(invoke).mockResolvedValueOnce(true).mockRejectedValueOnce(new Error('read failed'));
    await expect(useLearningStore.getState().remove('1')).resolves.toBe(true);
    expect(useLearningStore.getState()).toMatchObject({ entries: [], total: 0, error: 'Error: read failed' });
  });

  it('does not decrement the count twice if a native change notification refreshed first', async () => {
    useLearningStore.setState({ entries: [entry], total: 2 });
    vi.mocked(invoke).mockImplementationOnce(async () => {
      useLearningStore.setState({ entries: [], total: 1 });
      return true;
    }).mockRejectedValueOnce(new Error('read failed'));
    await useLearningStore.getState().remove('1');
    expect(useLearningStore.getState().total).toBe(1);
  });

  it('returns to a valid page when the final item of the last page is deleted', async () => {
    useLearningStore.setState({ offset: 20, total: 21, entries: [entry] });
    vi.mocked(invoke)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce({ entries: [], total: 20 })
      .mockResolvedValueOnce({ entries: [entry], total: 20 });
    await useLearningStore.getState().remove('1');
    expect(useLearningStore.getState().offset).toBe(0);
    expect(invoke).toHaveBeenLastCalledWith('learning_list', { query: '', offset: 0, limit: 20 });
  });

  it('exports the whole backend snapshot, not only the current filtered page', async () => {
    useLearningStore.setState({ query: 'portal', entries: [entry], total: 1 });
    vi.mocked(invoke).mockResolvedValue({ entry_count: 42 });
    await expect(useLearningStore.getState().exportTo('/chosen/usages.json', 'json')).resolves.toEqual({ entry_count: 42 });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('learning_export', { path: '/chosen/usages.json', format: 'json' });
    expect(useLearningStore.getState()).toMatchObject({ entries: [entry], exporting: null });
  });

  it('retains export failure feedback and resets the busy flag', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('file already exists'));
    await expect(useLearningStore.getState().exportTo('/chosen/usages.tsv', 'tsv')).rejects.toThrow('file already exists');
    expect(useLearningStore.getState()).toMatchObject({ exporting: null, error: 'Error: file already exists' });
  });
});
