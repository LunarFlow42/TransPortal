import { invoke } from '@tauri-apps/api/core';
import { create } from 'zustand';
import type {
  EnrollResult, LearningDraft, LearningEntry, LearningExportFormat,
  LearningExportResult, LearningPage,
} from '../types/learning';

export const LEARNING_CHANGED_EVENT = 'transportal://learning-changed';
export const LEARNING_PAGE_SIZE = 20;

interface LearningState {
  entries: LearningEntry[];
  total: number;
  query: string;
  offset: number;
  loading: boolean;
  error: string | null;
  deletingId: string | null;
  exporting: LearningExportFormat | null;
  load: (query?: string, offset?: number) => Promise<void>;
  enroll: (draft: LearningDraft) => Promise<EnrollResult>;
  remove: (id: string) => Promise<boolean>;
  exportTo: (path: string, format: LearningExportFormat) => Promise<LearningExportResult>;
  clearError: () => void;
}

let pageRequest = 0;

/** Each webview keeps a reader cache; Rust owns all persistent learning writes. */
export const useLearningStore = create<LearningState>((set, get) => ({
  entries: [],
  total: 0,
  query: '',
  offset: 0,
  loading: false,
  error: null,
  deletingId: null,
  exporting: null,

  load: async (query = get().query, offset = get().offset) => {
    const request = ++pageRequest;
    set({ query, offset, loading: true, error: null });
    try {
      const page = await invoke<LearningPage>('learning_list', {
        query, offset, limit: LEARNING_PAGE_SIZE,
      });
      if (request !== pageRequest) return;
      if (offset > 0 && offset >= page.total) {
        const lastOffset = Math.max(0, Math.floor((page.total - 1) / LEARNING_PAGE_SIZE) * LEARNING_PAGE_SIZE);
        await get().load(query, lastOffset);
        return;
      }
      set({ entries: page.entries, total: page.total });
    } catch (error) {
      if (request === pageRequest) set({ error: String(error) });
    } finally {
      if (request === pageRequest) set({ loading: false });
    }
  },

  // Called only by the word card's explicit confirmation, never by load/mount.
  enroll: (draft) => invoke<EnrollResult>('learning_enroll', { draft }),

  remove: async (id) => {
    if (get().deletingId !== null) throw new Error('A deletion is already in progress');
    set({ deletingId: id, error: null });
    try {
      const deleted = await invoke<boolean>('learning_delete', { id });
      if (deleted) {
        // The write succeeded even if the following refresh fails. Do not show
        // a deleted snapshot again from the last successful reader cache.
        set((state) => ({
          entries: state.entries.filter((entry) => entry.id !== id),
          total: state.entries.some((entry) => entry.id === id)
            ? Math.max(0, state.total - 1)
            : state.total,
        }));
      }
      await get().load();
      return deleted;
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ deletingId: null });
    }
  },

  exportTo: async (path, format) => {
    if (get().exporting !== null) throw new Error('An export is already in progress');
    set({ exporting: format, error: null });
    try {
      return await invoke<LearningExportResult>('learning_export', { path, format });
    } catch (error) {
      set({ error: String(error) });
      throw error;
    } finally {
      set({ exporting: null });
    }
  },

  clearError: () => set({ error: null }),
}));
