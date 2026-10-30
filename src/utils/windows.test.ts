import { describe, expect, it, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { openSettingsWindow, resizeAndClampWindow, setWindowTheme } from './windows';
import { cancelDismiss } from '../hooks/useWindowAnimation';

vi.mock('../hooks/useWindowAnimation', () => ({ cancelDismiss: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({
    setSize: vi.fn(),
    setPosition: vi.fn(),
    outerPosition: vi.fn().mockResolvedValue({ x: 100, y: 100 }),
    outerSize: vi.fn().mockResolvedValue({ width: 400, height: 300 }),
  })),
  currentMonitor: vi.fn().mockResolvedValue({
    workArea: {
      position: { x: 0, y: 0 },
      size: { width: 1920, height: 1080 },
    },
  }),
  PhysicalPosition: class {
    constructor(public x: number, public y: number) {}
  },
}));

describe('windows utilities', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('calls native resize_and_clamp_window via IPC', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await resizeAndClampWindow(500, 400);
    expect(invoke).toHaveBeenCalledWith('resize_and_clamp_window', {
      width: 500,
      height: 400,
    });
  });

  it('calls native set_window_theme via IPC', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await setWindowTheme(true);
    expect(invoke).toHaveBeenCalledWith('set_window_theme', {
      dark: true,
    });
  });

  it('cancels an already-started fade-out before requesting settings', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await openSettingsWindow();
    expect(cancelDismiss).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith('open_settings_window');
    expect(vi.mocked(cancelDismiss).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(invoke).mock.invocationCallOrder[0],
    );
  });
});
