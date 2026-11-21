import React from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { AboutPanel } from './AboutPanel';
import { useAppStore } from '../store/useAppStore';

const lifecycle = vi.hoisted(() => ({
  cursor: 0,
  values: [] as unknown[],
  dependencies: [] as Array<React.DependencyList | undefined>,
  effects: [] as Array<() => void | (() => void)>,
}));

// Render the actual controls and run effects after each render without a DOM.
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
  useCallback: (callback: unknown, dependencies: React.DependencyList) => {
    const index = lifecycle.cursor++;
    const previous = lifecycle.dependencies[index];
    if (!previous || dependencies.some((dependency, i) => dependency !== previous[i])) {
      lifecycle.values[index] = callback;
      lifecycle.dependencies[index] = dependencies;
    }
    return lifecycle.values[index];
  },
  useEffect: (effect: () => void | (() => void), dependencies: React.DependencyList) => {
    const index = lifecycle.cursor++;
    const previous = lifecycle.dependencies[index];
    if (!previous || dependencies.some((dependency, i) => dependency !== previous[i])) {
      lifecycle.dependencies[index] = dependencies;
      lifecycle.effects.push(effect);
    }
  },
}));
vi.mock('../store/useAppStore', async (original) => {
  const actual = await original<typeof import('../store/useAppStore')>();
  const store = actual.useAppStore;
  return { ...actual, useAppStore: Object.assign(
    (selector: (state: ReturnType<typeof store.getState>) => unknown) => selector(store.getState()),
    store,
  ) };
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn(async () => '0.1.3') }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

interface ControlProps {
  children?: React.ReactNode;
  onClick?: () => void;
  onChange?: (enabled: boolean) => void;
  label?: string;
  checked?: boolean;
  disabled?: boolean;
}

function elements(value: React.ReactNode): React.ReactElement<ControlProps>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!React.isValidElement<ControlProps>(value)) return [];
  return [value, ...elements(value.props.children)];
}

function render(settingsReady = true) {
  lifecycle.cursor = 0;
  const tree = AboutPanel({ settingsReady });
  if (tree instanceof Promise) throw new Error('Expected synchronous rendering');
  lifecycle.effects.splice(0).forEach((effect) => effect());
  return elements(tree);
}

const switchControl = (settingsReady = true) => render(settingsReady).find((element) => element.props.label === 'settings.about.autoCheckUpdates')!;
const tick = async () => { await Promise.resolve(); await Promise.resolve(); };

beforeEach(() => {
  vi.clearAllMocks();
  lifecycle.cursor = 0;
  lifecycle.values.length = 0;
  lifecycle.dependencies.length = 0;
  lifecycle.effects.length = 0;
  useAppStore.setState(useAppStore.getInitialState());
  vi.mocked(invoke).mockResolvedValue({ has_update: false, latest_version: '0.1.3' });
});

describe('automatic update checking', () => {
  it('is enabled by default and automatically checks only once per mount', async () => {
    expect(switchControl().props.checked).toBe(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('check_update');
    await tick();
    render();
    expect(invoke).toHaveBeenCalledOnce();
  });

  it('waits for saved preferences before requesting and honors a restored disabled preference', () => {
    expect(switchControl(false).props.disabled).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    useAppStore.getState().hydrate({ autoCheckUpdates: false });
    expect(switchControl(true).props.checked).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('automatically checks once initialization finishes with the preference enabled', () => {
    render(false);
    expect(invoke).not.toHaveBeenCalled();
    render(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('check_update');
  });

  it('allows manual checking while automatic checking is disabled', async () => {
    useAppStore.getState().hydrate({ autoCheckUpdates: false });
    const controls = render();
    expect(invoke).not.toHaveBeenCalled();
    controls.find((element) => element.props.children === 'settings.about.checkButton')!.props.onClick!();
    expect(invoke).toHaveBeenCalledExactlyOnceWith('check_update');
    await tick();
    expect(JSON.stringify(render())).toContain('settings.about.upToDateMessage');
    expect(useAppStore.getState().autoCheckUpdates).toBe(false);
  });

  it('enabling the restored disabled preference triggers a check and disabling it prevents further automatic checks', () => {
    render(false);
    useAppStore.getState().hydrate({ autoCheckUpdates: false });
    const toggle = switchControl(true);
    expect(invoke).not.toHaveBeenCalled();
    toggle.props.onChange!(true);
    expect(useAppStore.getState().autoCheckUpdates).toBe(true);
    render(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('check_update');
    switchControl().props.onChange!(false);
    expect(useAppStore.getState().autoCheckUpdates).toBe(false);
    render();
    expect(invoke).toHaveBeenCalledOnce();
  });

  it('still reports manual check failures when automatic checks are disabled', async () => {
    useAppStore.getState().hydrate({ autoCheckUpdates: false });
    vi.mocked(invoke).mockRejectedValueOnce(new Error('network unavailable'));
    render().find((element) => element.props.children === 'settings.about.checkButton')!.props.onClick!();
    await tick();
    expect(JSON.stringify(render())).toContain('network unavailable');
    expect(invoke).toHaveBeenCalledOnce();
  });
});
