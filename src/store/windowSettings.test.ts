import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAppStore } from './useAppStore';
import type { PersistedSettings } from '../types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock('./persistence', () => ({ saveSettings: vi.fn() }));

beforeEach(() => useAppStore.setState({ overlayOpacity: 0.72 }));

describe('window settings restored from JSON or another window', () => {
  it.each(['0.8', '', {}, null, undefined, NaN, Infinity])(
    'keeps a usable numeric opacity after invalid runtime value %j', (value) => {
      useAppStore.getState().hydrate({ overlayOpacity: value } as Partial<PersistedSettings>);
      expect(useAppStore.getState().overlayOpacity.toFixed(3)).toBe('0.720');
    },
  );

  it.each([[0, 0.25], [-1, 0.25], [1.5, 1], [0.85, 0.85]])(
    'restores opacity %s within the supported range', (value, expected) => {
      useAppStore.getState().hydrate({ overlayOpacity: value });
      expect(useAppStore.getState().overlayOpacity).toBe(expected);
    },
  );

  it('keeps restored opacity when a later setting update has no opacity', () => {
    useAppStore.getState().hydrate({ overlayOpacity: 0.9 });
    useAppStore.getState().hydrate({ theme: 'dark' });
    expect(useAppStore.getState().overlayOpacity).toBe(0.9);
  });
});
