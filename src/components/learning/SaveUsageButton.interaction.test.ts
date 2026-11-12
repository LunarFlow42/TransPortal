import React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SaveUsageButton } from './SaveUsageButton';
import type { LookupNode } from '../../types';
import type { EnrollResult } from '../../types/learning';

const lifecycle = vi.hoisted(() => ({
  cursor: 0,
  values: [] as unknown[],
  cleanups: [] as Array<() => void>,
  enroll: vi.fn(),
}));

// Exercise the actual event handlers without adding a browser/test dependency.
vi.mock('react', async (original) => ({
  ...await original<typeof import('react')>(),
  useState: (initial: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = initial;
    return [lifecycle.values[index], (value: unknown) => { lifecycle.values[index] = value; }];
  },
  useRef: (current: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = { current };
    return lifecycle.values[index];
  },
  useEffect: (effect: () => (() => void)) => {
    const index = lifecycle.cursor++;
    if (index in lifecycle.values) return;
    lifecycle.values[index] = true;
    lifecycle.cleanups.push(effect());
  },
}));
vi.mock('../../store/useLearningStore', () => ({
  useLearningStore: (select: (state: { enroll: typeof lifecycle.enroll }) => unknown) => select({ enroll: lifecycle.enroll }),
}));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const node: LookupNode = {
  id: 1, kind: 'word', text: 'bank', density: 'dense', intent: 'word', status: 'ready', translation: '',
  wordData: {
    word: 'bank', definitions: [{ pos: 'n.', meanings: ['银行', '河岸'] }], tags: [], source: 'youdao',
    examples: [
      { en: 'She went to the bank.', zh: '她去了银行。' },
      { en: 'We sat on the bank.', zh: '我们坐在岸边。', source: 'generated' },
    ],
  },
};

interface ElementProps {
  children?: React.ReactNode;
  onClick?: () => void;
  onSubmit?: (event: React.FormEvent) => void;
  onChange?: (value: string) => void;
  ariaLabel?: string;
  type?: string;
  disabled?: boolean;
}

function elements(value: React.ReactNode): React.ReactElement<ElementProps>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!React.isValidElement<ElementProps>(value)) return [];
  return [value, ...elements(value.props.children)];
}

function render() {
  lifecycle.cursor = 0;
  const tree = SaveUsageButton({ node, isTop: true, translationSource: 'bing_free' });
  if (tree instanceof Promise) throw new Error('Expected a synchronous word-card component');
  return tree;
}

function begin() {
  elements(render()).find((element) => element.props.onClick)!.props.onClick!();
}

function submit() {
  elements(render()).find((element) => element.type === 'form')!.props.onSubmit!({ preventDefault: vi.fn() } as unknown as React.FormEvent);
}

const tick = async () => { await Promise.resolve(); await Promise.resolve(); };

beforeEach(() => {
  lifecycle.cleanups.splice(0).forEach((cleanup) => cleanup());
  lifecycle.values.length = 0;
  lifecycle.cursor = 0;
  lifecycle.enroll.mockReset();
});

describe('explicit inline collection interactions', () => {
  it('opening and cancelling the form never collects a lookup', () => {
    begin();
    expect(elements(render()).some((element) => element.type === 'form')).toBe(true);
    const cancel = elements(render()).find((element) => element.props.children === 'common.cancel')!;
    cancel.props.onClick!();
    expect(elements(render()).some((element) => element.type === 'form')).toBe(false);
    expect(lifecycle.enroll).not.toHaveBeenCalled();
  });

  it('submits the selected sense and existing example only, then confirms the save', async () => {
    lifecycle.enroll.mockResolvedValue({ created: true } as EnrollResult);
    begin();
    const controls = elements(render());
    controls.find((element) => element.props.ariaLabel === 'learning.sense')!.props.onChange!(JSON.stringify(['n.', '河岸']));
    controls.find((element) => element.props.ariaLabel === 'learning.example')!.props.onChange!('example:1');
    submit();
    await tick();
    expect(lifecycle.enroll).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      meaning: '河岸', definition_source: 'youdao',
      context: { text: 'We sat on the bank.', translation: '我们坐在岸边。', source: 'generated' },
    }));
    expect(elements(render()).some((element) => element.type === 'form')).toBe(false);
    expect(JSON.stringify(render())).toContain('learning.saved');
  });

  it('prevents repeated submissions while saving and preserves the form after failure', async () => {
    let reject!: (reason: unknown) => void;
    lifecycle.enroll.mockReturnValue(new Promise((_, fail) => { reject = fail; }));
    begin();
    submit();
    submit();
    expect(lifecycle.enroll).toHaveBeenCalledOnce();
    expect(elements(render()).find((element) => element.props.type === 'submit')?.props.disabled).toBe(true);
    reject(new Error('disk full'));
    await tick();
    expect(elements(render()).some((element) => element.type === 'form')).toBe(true);
    expect(JSON.stringify(render())).toContain('disk full');
    expect(elements(render()).find((element) => element.props.type === 'submit')?.props.disabled).toBe(false);
  });

  it('allows no example and reports an existing usage without creating a second record', async () => {
    lifecycle.enroll.mockResolvedValue({ created: false } as EnrollResult);
    begin();
    elements(render()).find((element) => element.props.ariaLabel === 'learning.example')!.props.onChange!('none');
    submit();
    await tick();
    expect(lifecycle.enroll).toHaveBeenCalledWith(expect.objectContaining({ context: null }));
    expect(JSON.stringify(render())).toContain('learning.alreadySaved');
  });
});
