import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LearningPanel } from './LearningPanel';
import { useLearningStore } from '../../store/useLearningStore';
import type { LearningEntry } from '../../types/learning';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }));
// Server rendering reads Zustand's startup snapshot by default. Use the real
// store's live reader cache here; IPC and cache races are tested separately.
vi.mock('../../store/useLearningStore', async (original) => {
  const actual = await original<typeof import('../../store/useLearningStore')>();
  return {
    ...actual,
    useLearningStore: Object.assign(() => actual.useLearningStore.getState(), actual.useLearningStore),
  };
});
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'zh-CN' } }),
}));

const entry: LearningEntry = {
  id: '1', created_at: 100, updated_at: 100,
  usage: {
    language: 'en', lemma: 'portal', surface: 'portal', phonetic: '/ˈpɔːtl/',
    pos: 'n.', meaning: '入口', definition_kind: 'dictionary', definition_source: 'youdao',
    context: { text: 'A portal opened.', translation: '一个入口打开了。', source: 'generated' },
  },
};

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  useLearningStore.setState({
    entries: [], total: 0, query: '', offset: 0, loading: false,
    error: null, deletingId: null, exporting: null,
  });
});

describe('saved-vocabulary manager', () => {
  it('renders an empty collection and export entry points without writing data', () => {
    const html = renderToStaticMarkup(React.createElement(LearningPanel));
    expect(html).toContain('learning.empty');
    expect(html.match(/learning.exportAll/g)).toHaveLength(2);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('shows the persisted sense, example provenance, and explicit deletion entry point', () => {
    useLearningStore.setState({ entries: [entry], total: 1 });
    const html = renderToStaticMarkup(React.createElement(LearningPanel));
    expect(html).toContain('portal');
    expect(html).toContain('入口');
    expect(html).toContain('A portal opened.');
    expect(html).toContain('learning.context.generated');
    expect(html).toContain('learning.definition.dictionary');
    expect(html).toContain('learning.deleteUsage');
    expect(html).not.toContain('learning.deleteConfirm');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('renders stored text safely instead of interpreting example markup', () => {
    const unsafe: LearningEntry = {
      ...entry,
      usage: { ...entry.usage, context: { text: '<script>alert(1)</script>', translation: null, source: 'dictionary' } },
    };
    useLearningStore.setState({ entries: [unsafe], total: 1 });
    const html = renderToStaticMarkup(React.createElement(LearningPanel));
    expect(html).toContain('&lt;script&gt;');
    expect(html).not.toContain('<script>');
  });

  it('retains the previous collection alongside a non-destructive failure message', () => {
    useLearningStore.setState({ entries: [entry], total: 1, error: 'database unavailable' });
    const html = renderToStaticMarkup(React.createElement(LearningPanel));
    expect(html).toContain('role="alert"');
    expect(html).toContain('database unavailable');
    expect(html).toContain('A portal opened.');
  });
});
