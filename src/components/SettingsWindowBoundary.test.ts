import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { SettingsWindowBoundary, SettingsWindowStatus } from './SettingsWindowBoundary';

vi.mock('../utils/windows', () => ({ closeSettingsWindow: vi.fn() }));
vi.mock('../utils/drag', () => ({ startWindowDrag: vi.fn() }));

describe('Settings window loading and render recovery', () => {
  it('renders an opaque, closable loading surface instead of a transparent fallback', () => {
    const html = renderToStaticMarkup(React.createElement(SettingsWindowStatus));
    expect(html).toContain('window-root');
    expect(html).toContain('role="status"');
    expect(html).toContain('正在加载设置');
    expect(html).toContain('aria-label="关闭设置"');
  });

  it('keeps the settings view untouched when there is no error', () => {
    const child = React.createElement('div', null, 'Settings content');
    const boundary = new SettingsWindowBoundary({ children: child });
    expect(boundary.render()).toBe(child);
  });

  it('replaces a failed settings render with an opaque recovery surface', () => {
    const boundary = new SettingsWindowBoundary({ children: null });
    boundary.state = SettingsWindowBoundary.getDerivedStateFromError();
    const html = renderToStaticMarkup(boundary.render());
    expect(html).toContain('window-root');
    expect(html).toContain('role="alert"');
    expect(html).toContain('重新加载');
    expect(html).toContain('aria-label="关闭设置"');
  });
});
