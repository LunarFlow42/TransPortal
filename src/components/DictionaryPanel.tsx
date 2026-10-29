import React, { useCallback, useEffect, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { BookOpen, ChevronDown, ChevronUp, Download, Loader2, Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, IconButton, Pill, Switch } from './ui';
import { cn } from '../utils/cn';
import { saveSettings } from '../store/persistence';
import { suppressAutoHide } from '../utils/autoHide';
import type { DictProgress, DictSourceInfo, DictStatus } from '../types';

/** Format large entry counts in a locale-friendly way. */
function formatCount(count: number, isZh: boolean): string {
  if (isZh) {
    if (count >= 10000) return `${(count / 10000).toFixed(2)} 万`;
    if (count >= 1000) return `${(count / 1000).toFixed(1)} 千`;
    return count.toLocaleString();
  }
  if (count >= 1000000) return `${(count / 1000000).toFixed(2)}M`;
  if (count >= 1000) return `${(count / 1000).toFixed(1)}k`;
  return count.toLocaleString();
}

/**
 * Dictionary source configuration.
 *
 * Every lookup source is an ordered, toggleable option. Online sources are
 * enabled by default; the local ECCEDICT store is optional and only available
 * once a dictionary has been imported.
 */
export const DictionaryPanel: React.FC = () => {
  const { t, i18n } = useTranslation();
  const isZh = i18n.language.startsWith('zh');
  const [status, setStatus] = useState<DictStatus | null>(null);
  const [sources, setSources] = useState<DictSourceInfo[]>([]);
  const [busy, setBusy] = useState<'import' | 'download' | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [progress, setProgress] = useState<{
    downloaded: number;
    total: number | null;
  } | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextSources] = await Promise.all([
        invoke<DictStatus>('dict_status'),
        invoke<DictSourceInfo[]>('dict_sources'),
      ]);
      setStatus(nextStatus);
      setSources(nextSources);
    } catch (error) {
      setMessage(String(error));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Refetch sources and persist their enabled flags + order. */
  const refreshAndPersist = useCallback(async () => {
    const list = await invoke<DictSourceInfo[]>('dict_sources');
    setSources(list);
    await saveSettings({
      dictSources: list.map((source) => ({ id: source.id, enabled: source.enabled })),
    });
    await refresh();
  }, [refresh]);

  const toggleSource = async (source: DictSourceInfo) => {
    await invoke('set_dict_source_enabled', { id: source.id, enabled: !source.enabled });
    await refreshAndPersist();
  };

  const moveSource = async (source: DictSourceInfo, delta: number) => {
    await invoke('move_dict_source', { id: source.id, delta });
    await refreshAndPersist();
  };

  const importCsv = async () => {
    suppressAutoHide(120000);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      });
      if (typeof selected !== 'string') return;

      setBusy('import');
      setMessage(t('settings.translation.importing'));
      const count = await invoke<number>('import_eccedict', { path: selected });
      setMessage(t('settings.translation.importedCount', { count: formatCount(count, isZh) }));
      await refresh();
    } catch (error) {
      setMessage(String(error));
    } finally {
      suppressAutoHide(1000);
      setBusy(null);
    }
  };

  const downloadFull = async () => {
    setBusy('download');
    setMessage(t('settings.translation.downloadingDict'));
    const channel = new Channel<DictProgress>();
    channel.onmessage = (progress) => {
      if (progress.stage === 'downloading') {
        setProgress({ downloaded: progress.downloaded, total: progress.total ?? null });
        const mb = (progress.downloaded / 1048576).toFixed(1);
        const total = progress.total
          ? ` / ${(progress.total / 1048576).toFixed(1)} MB`
          : ' MB';
        setMessage(`${isZh ? '正在下载…' : 'Downloading…'} ${mb}${total}`);
      } else if (progress.stage === 'importing') {
        setProgress(null);
        setMessage(t('settings.translation.importing'));
      } else {
        setProgress(null);
      }
    };

    try {
      const count = await invoke<number>('download_eccedict', { onProgress: channel });
      setMessage(t('settings.translation.importedCount', { count: formatCount(count, isZh) }));
      await refresh();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setProgress(null);
      setBusy(null);
    }
  };

  const disabled = busy !== null;

  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-[13px] font-semibold">
          <BookOpen className="h-3.5 w-3.5 text-emerald-400" />
          {t('settings.translation.dictSources')}
        </span>
        <span className="text-[10px] text-[var(--muted)]">
          {status?.installed
            ? t('settings.translation.entriesCount', { count: formatCount(status.entry_count, isZh) })
            : t('settings.translation.onlineOnly')}
        </span>
      </div>

      <div className="flex flex-col gap-1.5">
        {sources.map((source, index) => (
          <div
            key={source.id}
            className="flex items-start gap-2.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-2.5 py-2"
          >
            <Switch
              checked={source.enabled}
              disabled={disabled || (!source.available && source.kind === 'online')}
              onChange={() => void toggleSource(source)}
              label={t('settings.translation.enableSource', { name: source.label })}
              className="mt-0.5"
            />
            <div className="min-w-0 flex-1">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-[12px] font-medium">{source.label}</span>
                <Pill tone={source.kind === 'online' ? 'online' : 'local'}>{source.kind}</Pill>
                {!source.available && (
                  <span className="text-[9px] text-[var(--faint)]">{t('settings.translation.notInstalled')}</span>
                )}
                {source.last_ok !== undefined && source.last_ok !== null && (
                  <span
                    className={cn(
                      'text-[9px]',
                      source.last_ok ? 'text-emerald-400' : 'text-rose-400',
                    )}
                  >
                    {source.last_ok ? `ok ${source.last_latency_ms ?? '?'}ms` : 'fail'}
                  </span>
                )}
              </div>
              <p className="mt-0.5 text-[10.5px] leading-tight text-[var(--muted)]">
                {source.description}
              </p>
            </div>
            <div className="flex flex-col text-[var(--muted)]">
              <IconButton
                title={t('settings.translation.moveUp')}
                disabled={disabled || index === 0}
                onClick={() => void moveSource(source, -1)}
                className="h-4 w-4 disabled:opacity-25"
              >
                <ChevronUp className="h-3 w-3" />
              </IconButton>
              <IconButton
                title={t('settings.translation.moveDown')}
                disabled={disabled || index === sources.length - 1}
                onClick={() => void moveSource(source, 1)}
                className="h-4 w-4 disabled:opacity-25"
              >
                <ChevronDown className="h-3 w-3" />
              </IconButton>
            </div>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-2">
        <Button size="sm" disabled={disabled} onClick={() => void importCsv()}>
          {busy === 'import' ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <Upload className="h-3 w-3" />
          )}
          {t('settings.translation.importCsv')}
        </Button>
        <Button size="sm" disabled={disabled} onClick={() => void downloadFull()}>
          {busy === 'download' ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <Download className="h-3 w-3" />
          )}
          {t('settings.translation.downloadFull')}
        </Button>
      </div>

      {progress && (
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--surface-3)]">
          <div
            className="h-full rounded-full bg-gradient-to-r from-[var(--accent-a)] to-[var(--accent-b)] transition-all"
            style={{
              width: progress.total
                ? `${Math.min(100, (progress.downloaded / progress.total) * 100).toFixed(1)}%`
                : '100%',
            }}
          />
        </div>
      )}

      {status?.installed && (
        <p className="truncate text-[10px] text-[var(--faint)]" title={status.local_path}>
          {status.local_path}
        </p>
      )}
      {message && <p className="text-[10px] text-[var(--muted)]">{message}</p>}
    </div>
  );
};
