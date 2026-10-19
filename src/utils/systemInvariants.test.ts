import { describe, expect, it } from 'vitest';
import tauriConfig from '../../src-tauri/tauri.conf.json';

describe('System Invariants & Anti-Regression Guard', () => {
  const windows = tauriConfig.app?.windows ?? [];

  it('must declare all three core windows in tauri.conf.json to avoid runtime routing or native desync', () => {
    const labels = windows.map((w) => w.label);
    expect(labels).toContain('shell');
    expect(labels).toContain('settings');
    expect(labels).toContain('ocr-region');
  });

  it('must assign dedicated hash routes to each core window matching React App routing', () => {
    const shell = windows.find((w) => w.label === 'shell');
    const settings = windows.find((w) => w.label === 'settings');
    const ocrRegion = windows.find((w) => w.label === 'ocr-region');

    expect(shell?.url).toBe('index.html#/shell');
    expect(settings?.url).toBe('index.html#/settings');
    expect(ocrRegion?.url).toBe('index.html#/ocr-region');
  });

  it('must enforce transparent window properties and noRedirectionBitmap for DWM composition', () => {
    for (const label of ['shell', 'settings', 'ocr-region']) {
      const win = windows.find((w) => w.label === label);
      expect(win, `Window ${label} should exist`).toBeDefined();
      expect(win?.transparent, `Window ${label} must be transparent`).toBe(true);
      expect(win?.noRedirectionBitmap, `Window ${label} must have noRedirectionBitmap=true`).toBe(true);
    }
  });

  it('settings window must start hidden to be summoned on demand', () => {
    const settings = windows.find((w) => w.label === 'settings');
    expect(settings?.visible).toBe(false);
  });
});
