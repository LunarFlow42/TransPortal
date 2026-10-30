import React from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, RefreshCw, Settings } from 'lucide-react';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { Button, Surface, Switch } from './ui';
import { useAppStore } from '../store/useAppStore';
import { openSettingsWindow } from '../utils/windows';
import { startWindowDrag } from '../utils/drag';
import type { ShortcutStatus } from '../types';

const Failure: React.FC<{ shortcut: ShortcutStatus }> = ({ shortcut }) => {
  const { t } = useTranslation();
  return (
    <div className="rounded-[var(--r-inner)] border border-amber-500/30 bg-amber-500/10 p-2.5 text-xs leading-relaxed">
      <p className="font-medium">{shortcut.label} · <kbd>{shortcut.requested}</kbd></p>
      <p className="mt-1 text-[var(--muted)]">
        {shortcut.system ? t('settings.shortcuts.systemError') : t('settings.shortcuts.conflictError')}
        {shortcut.active && t('settings.shortcuts.stillActive', { hotkey: shortcut.active })}
      </p>
      <details className="mt-1 text-[10px] text-[var(--faint)]">
        <summary>{t('settings.shortcuts.viewReason')}</summary>
        {shortcut.error}
      </details>
    </div>
  );
};

export const StartupShortcutNotice: React.FC<{
  issues: ShortcutStatus[];
  retrying: boolean;
  error: string | null;
  onRetry: () => void;
  onDismiss: () => void;
}> = ({ issues, retrying, error, onRetry, onDismiss }) => {
  const { t } = useTranslation();
  return (
    <Surface className="flex h-full w-full flex-col justify-between gap-3 p-4 text-[var(--text)]" role="alert">
      <div onMouseDown={startWindowDrag} className="flex cursor-move items-center gap-2 text-sm font-semibold">
        <AlertTriangle className="h-4 w-4 text-amber-500" />
        {t('settings.shortcuts.noticeTitle')}
      </div>
      <div className="flex flex-1 min-h-0 flex-col gap-2 overflow-y-auto no-scrollbar">
        {issues.map((shortcut) => <Failure key={shortcut.id} shortcut={shortcut} />)}
      </div>
      <p className="text-[11px] text-[var(--muted)]">
        {t('settings.shortcuts.closeConflictingApps')}
        {issues.some((shortcut) => shortcut.system)
          ? ` ${t('settings.shortcuts.openImageAlternative')}`
          : ` ${t('settings.shortcuts.overlayButtonAlternative')}`}
      </p>
      {error && <p className="text-xs text-[var(--muted)]">{error}</p>}
      <div className="flex flex-wrap justify-end gap-1.5">
        <Button size="sm" variant="subtle" onClick={onDismiss}>
          {t('settings.shortcuts.dismissLater')}
        </Button>
        <Button size="sm" onClick={() => void openSettingsWindow()}>
          <Settings className="h-3 w-3" />
          {t('settings.title')}
        </Button>
        <Button size="sm" variant="primary" disabled={retrying} onClick={onRetry}>
          <RefreshCw className={`h-3 w-3 ${retrying ? 'animate-spin' : ''}`} />
          {t('settings.shortcuts.recheck')}
        </Button>
      </div>
    </Surface>
  );
};

export const ShortcutSettingsStatus: React.FC = () => {
  const { t } = useTranslation();
  const { report, error, retrying, retry } = useShortcutStatus();
  const doubleCopyEnabled = useAppStore((state) => state.doubleCopyEnabled);
  return (
    <div className="mt-1 flex flex-col gap-2 border-t border-[var(--stroke)] pt-2.5" aria-label="快捷键检查状态">
      {report?.shortcuts.map((shortcut) => (
        shortcut.error ? <Failure key={shortcut.id} shortcut={shortcut} /> : (
          <div key={shortcut.id} className="flex items-center justify-between gap-2 text-[11px]">
            <span>{shortcut.label} · <kbd>{shortcut.active ?? shortcut.requested}</kbd></span>
            <span className="text-[var(--muted)]">
              {shortcut.active
                ? (shortcut.system ? t('settings.shortcuts.hookActive') : t('settings.shortcuts.active'))
                : shortcut.id === 'double_copy'
                ? t('settings.shortcuts.disabled')
                : t('settings.shortcuts.checking')}
            </span>
          </div>
        )
      ))}
      {report?.shortcuts.some((shortcut) => shortcut.id === 'double_copy') && (
        <div className="rounded-md bg-[var(--surface-2)] p-2.5">
          <label className="flex items-center justify-between gap-2 text-xs">
            {t('settings.shortcuts.doubleCopy')}
            <Switch
              label={t('settings.shortcuts.doubleCopy')}
              checked={doubleCopyEnabled}
              disabled={!report.ready}
              onChange={(enabled) => void useAppStore.getState().setDoubleCopyEnabled(enabled)}
            />
          </label>
          <p className="mt-1 text-[11px] text-[var(--muted)]">
            {t('settings.shortcuts.doubleCopyPrompt')}
          </p>
        </div>
      )}
      {error && <p role="status" className="text-[11px] text-[var(--muted)]">{error}</p>}
      <Button size="sm" disabled={retrying || !report?.ready} onClick={() => void retry()}>
        <RefreshCw className={`h-3 w-3 ${retrying ? 'animate-spin' : ''}`} />
        {t('settings.shortcuts.recheckShortcuts')}
      </Button>
    </div>
  );
};
