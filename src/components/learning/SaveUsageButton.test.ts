import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SaveUsageButton } from './SaveUsageButton';
import type { LookupNode } from '../../types';
import zh from '../../i18n/locales/zh-CN.json';
import en from '../../i18n/locales/en-US.json';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const node: LookupNode = {
  id: 1, kind: 'word', text: 'portal', density: 'dense', intent: 'word',
  wordData: {
    word: 'portal', definitions: [{ pos: 'n.', meanings: ['入口'] }],
    tags: [], examples: [{ en: 'A portal opened.', zh: '入口打开了。' }], source: 'local_eccedict',
  },
  translation: '', status: 'ready',
};

beforeEach(() => vi.mocked(invoke).mockReset());

describe('collection is opt-in and does not interfere with lookup rendering', () => {
  it('only renders a small entry point on arrival, with no IPC or open editor', () => {
    const html = renderToStaticMarkup(React.createElement(SaveUsageButton, {
      node, isTop: true, translationSource: 'bing_free',
    }));
    expect(html).toContain('learning.rememberUsage');
    expect(html).not.toContain('<form');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('does not render interactive collection controls in underlying stack frames', () => {
    const html = renderToStaticMarkup(React.createElement(SaveUsageButton, {
      node, isTop: false, translationSource: 'bing_free',
    }));
    expect(html).toBe('');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('does not render collection for sentence translations', () => {
    const html = renderToStaticMarkup(React.createElement(SaveUsageButton, {
      node: { ...node, kind: 'sentence' }, isTop: true, translationSource: 'bing_free',
    }));
    expect(html).toBe('');
  });

  it('has matching Chinese and English keys for collection and source labels', () => {
    const keys = (value: object): string[] => Object.entries(value).flatMap(([key, item]) =>
      typeof item === 'object' ? keys(item).map((child) => `${key}.${child}`) : [key],
    ).sort();
    expect(keys(zh.learning)).toEqual(keys(en.learning));
    expect(zh.settings.tabs.learning).toBe('生词收藏');
    expect(en.settings.tabs.learning).toBe('Saved Vocabulary');
  });
});
