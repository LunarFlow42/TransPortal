import React from 'react';
import { AlertTriangle, RefreshCw, Settings } from 'lucide-react';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { Button, Surface, Switch } from './ui';
import { useAppStore } from '../store/useAppStore';
import { openSettingsWindow } from '../utils/windows';
import { startWindowDrag } from '../utils/drag';
import type { ShortcutStatus } from '../types';

const Failure: React.FC<{ shortcut: ShortcutStatus }> = ({ shortcut }) => (
  <div className="rounded-[var(--r-inner)] border border-amber-500/30 bg-amber-500/10 p-2.5 text-xs leading-relaxed">
    <p className="font-medium">{shortcut.label} · <kbd>{shortcut.requested}</kbd></p>
    <p className="mt-1 text-[var(--muted)]">
      {shortcut.system ? '快捷键监听未能启用，请重新检测。' : '快捷键未能注册，可能正被其他软件占用。'}
      {shortcut.active && ` 当前仍可使用 ${shortcut.active}。`}
    </p>
    <details className="mt-1 text-[10px] text-[var(--faint)]"><summary>查看原因</summary>{shortcut.error}</details>
  </div>
);

export const StartupShortcutNotice: React.FC<{
  issues: ShortcutStatus[];
  retrying: boolean;
  error: string | null;
  onRetry: () => void;
  onDismiss: () => void;
}> = ({ issues, retrying, error, onRetry, onDismiss }) => (
  <Surface className="flex h-full w-full flex-col justify-between gap-3 p-4 text-[var(--text)]" role="alert">
    <div onMouseDown={startWindowDrag} className="flex cursor-move items-center gap-2 text-sm font-semibold">
      <AlertTriangle className="h-4 w-4 text-amber-500" />快捷键需要处理
    </div>
    <div className="flex flex-1 min-h-0 flex-col gap-2 overflow-y-auto no-scrollbar">
      {issues.map((shortcut) => <Failure key={shortcut.id} shortcut={shortcut} />)}
    </div>
    <p className="text-[11px] text-[var(--muted)]">
      关闭占用该键位的软件后可重新检测。
      {issues.some((shortcut) => shortcut.system) ? '也可通过“打开图片”识别文字。' : '截图翻译也可通过浮层按钮使用。'}
    </p>
    {error && <p className="text-xs text-[var(--muted)]">{error}</p>}
    <div className="flex flex-wrap justify-end gap-1.5">
      <Button size="sm" variant="subtle" onClick={onDismiss}>稍后处理</Button>
      <Button size="sm" onClick={() => void openSettingsWindow()}><Settings className="h-3 w-3" />设置</Button>
      <Button size="sm" variant="primary" disabled={retrying} onClick={onRetry}>
        <RefreshCw className={`h-3 w-3 ${retrying ? 'animate-spin' : ''}`} />重新检测
      </Button>
    </div>
  </Surface>
);

export const ShortcutSettingsStatus: React.FC = () => {
  const { report, error, retrying, retry } = useShortcutStatus();
  const doubleCopyEnabled = useAppStore((state) => state.doubleCopyEnabled);
  return (
    <div className="mt-1 flex flex-col gap-2 border-t border-[var(--stroke)] pt-2.5" aria-label="快捷键检查状态">
      {report?.shortcuts.map((shortcut) => (
        shortcut.error ? <Failure key={shortcut.id} shortcut={shortcut} /> : (
          <div key={shortcut.id} className="flex items-center justify-between gap-2 text-[11px]">
            <span>{shortcut.label} · <kbd>{shortcut.active ?? shortcut.requested}</kbd></span>
            <span className="text-[var(--muted)]">{shortcut.active ? (shortcut.system ? '监听已启用' : '已启用') : shortcut.id === 'double_copy' ? '已关闭' : '检查中…'}</span>
          </div>
        )
      ))}
      {report?.shortcuts.some((shortcut) => shortcut.id === 'double_copy') && <div className="rounded-md bg-[var(--surface-2)] p-2.5">
        <label className="flex items-center justify-between gap-2 text-xs">Ctrl+C+C 翻译
          <Switch label="启用 Ctrl+C+C 翻译" checked={doubleCopyEnabled} disabled={!report.ready}
            onChange={(enabled) => void useAppStore.getState().setDoubleCopyEnabled(enabled)} />
        </label>
        <p className="mt-1 text-[11px] text-[var(--muted)]">选中文字，按住 Ctrl，在 0.5 秒内按两次 C。普通复制保持可用。</p>
      </div>}
      {error && <p role="status" className="text-[11px] text-[var(--muted)]">{error}</p>}
      <Button size="sm" disabled={retrying || !report?.ready} onClick={() => void retry()}>
        <RefreshCw className={`h-3 w-3 ${retrying ? 'animate-spin' : ''}`} />重新检测快捷键
      </Button>
    </div>
  );
};
