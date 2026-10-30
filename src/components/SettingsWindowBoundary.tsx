import React from 'react';
import { startWindowDrag } from '../utils/drag';
import { closeSettingsWindow } from '../utils/windows';

/** An opaque, usable surface while settings loads or if its render fails. */
export function SettingsWindowStatus({ failed = false }: { failed?: boolean }) {
  return (
    <div className="window-root flex h-screen w-screen flex-col rounded-[var(--r-win)] text-[var(--text)]">
      <header
        onMouseDown={startWindowDrag}
        className="flex cursor-move items-center justify-between border-b border-[var(--stroke)] px-4 py-3"
      >
        <span className="text-[13px] font-semibold">TransPortal Settings</span>
        <button
          type="button"
          aria-label="关闭设置"
          onClick={() => void closeSettingsWindow()}
          className="focus-ring rounded px-2 py-1 text-[var(--muted)]"
        >
          ×
        </button>
      </header>
      <div
        role={failed ? 'alert' : 'status'}
        className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center"
      >
        <p>{failed ? '设置页面加载失败' : '正在加载设置…'}</p>
        {failed && (
          <>
            <p className="text-sm text-[var(--muted)]">可以重新加载页面，或关闭后重试。</p>
            <button
              type="button"
              onClick={() => window.location.reload()}
              className="focus-ring rounded border border-[var(--stroke)] px-4 py-2"
            >
              重新加载
            </button>
          </>
        )}
      </div>
    </div>
  );
}

export class SettingsWindowBoundary extends React.Component<React.PropsWithChildren, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: Error) {
    console.error('Settings window render failed:', error);
  }

  render() {
    return this.state.failed ? <SettingsWindowStatus failed /> : this.props.children;
  }
}
