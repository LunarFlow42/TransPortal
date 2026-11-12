import React, { useCallback, useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { Bookmark, ChevronLeft, ChevronRight, Download, Loader2, RefreshCw, Search, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, IconButton, Input, Pill } from '../ui';
import { LEARNING_CHANGED_EVENT, LEARNING_PAGE_SIZE, useLearningStore } from '../../store/useLearningStore';
import type { LearningExportFormat } from '../../types/learning';

export const LearningPanel: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { entries, total, offset, loading, error, deletingId, exporting, load, remove, exportTo } = useLearningStore();
  const [query, setQuery] = useState(() => useLearningStore.getState().query);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [choosingFile, setChoosingFile] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const busy = deletingId !== null || exporting !== null || choosingFile;

  const refresh = useCallback(() => {
    const state = useLearningStore.getState();
    void state.load(state.query, state.offset);
  }, []);

  useEffect(() => {
    let disposed = false;
    const unlisten = listen(LEARNING_CHANGED_EVENT, () => { if (!disposed) refresh(); });
    // Settings is prewarmed/hidden; refresh again whenever the user returns.
    window.addEventListener('focus', refresh);
    void unlisten.then(() => { if (!disposed) refresh(); }).catch(() => {});
    return () => {
      disposed = true;
      window.removeEventListener('focus', refresh);
      void unlisten.then((dispose) => dispose()).catch(() => {});
    };
  }, [refresh]);

  useEffect(() => {
    const timer = setTimeout(() => { void load(query, 0); }, 200);
    return () => clearTimeout(timer);
  }, [query, load]);

  const exportCollection = async (format: LearningExportFormat) => {
    if (busy) return;
    setChoosingFile(true);
    setMessage(null);
    useLearningStore.getState().clearError();
    try {
      const path = await save({
        defaultPath: `transportal-usages-${Date.now()}.${format}`,
        filters: [{ name: format.toUpperCase(), extensions: [format] }],
      });
      if (!path) return;
      const result = await exportTo(path, format);
      setMessage(t('learning.exported', { count: result.entry_count, format: format.toUpperCase() }));
    } catch (error) {
      // exportTo also retains its error; file-dialog errors need the same feedback.
      useLearningStore.setState({ error: String(error) });
    } finally {
      setChoosingFile(false);
    }
  };

  const deleteEntry = async (id: string) => {
    setMessage(null);
    try {
      await remove(id);
      setDeleteId(null);
      setMessage(t('learning.deleted'));
    } catch {
      // Keep the confirmation and data visible so a failed write can be retried.
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-[var(--muted)] leading-relaxed">{t('learning.privacyHint')}</p>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="search"
          value={query}
          maxLength={256}
          onChange={(event) => { setQuery(event.target.value); setDeleteId(null); }}
          onKeyDown={(event) => {
            if (event.key === 'Escape' && query) {
              event.stopPropagation();
              setQuery('');
            }
          }}
          leftIcon={<Search className="h-3.5 w-3.5" />}
          placeholder={t('learning.search')}
          aria-label={t('learning.search')}
        />
        <IconButton type="button" title={t('learning.refresh')} aria-label={t('learning.refresh')} disabled={loading || busy} onClick={refresh}>
          <RefreshCw className="h-3.5 w-3.5" />
        </IconButton>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-[11px] text-[var(--muted)]">{t('learning.count', { count: total })}</span>
        <div className="flex items-center gap-2">
          {(['json', 'tsv'] as const).map((format) => (
            <Button type="button" key={format} size="sm" disabled={busy || loading} onClick={() => void exportCollection(format)}>
              {exporting === format ? <Loader2 className="h-3 w-3 animate-spin" /> : <Download className="h-3 w-3" />}
              {t('learning.exportAll', { format: format.toUpperCase() })}
            </Button>
          ))}
        </div>
      </div>
      <p className="text-[11px] text-[var(--muted)]">{t('learning.exportHint')}</p>
      {error && <p role="alert" className="text-xs text-[var(--text)] break-words">{t('learning.operationFailed')} {error}</p>}
      {message && <p role="status" className="text-xs text-[var(--text)]">{message}</p>}
      {loading ? (
        <p role="status" className="flex items-center gap-2 text-xs text-[var(--muted)]"><Loader2 className="h-3.5 w-3.5 animate-spin" />{t('common.loading')}</p>
      ) : entries.length === 0 ? (
        !error && <p className="rounded-[var(--r-inner)] border border-[var(--stroke)] p-4 text-xs text-[var(--muted)]">{t(query ? 'learning.noMatches' : 'learning.empty')}</p>
      ) : (
        <div className="flex flex-col gap-2.5">
          {entries.map((entry) => (
            <article key={entry.id} className="rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-3 flex flex-col gap-2">
              <div className="flex items-start justify-between gap-2">
                <div className="flex flex-wrap items-baseline gap-2 min-w-0">
                  <Bookmark className="h-3 w-3 text-[var(--accent-b)]" />
                  <span className="text-sm font-semibold break-words">{entry.usage.lemma}</span>
                  {entry.usage.phonetic && <span className="text-xs font-mono text-[var(--muted)]">{entry.usage.phonetic}</span>}
                  {entry.usage.pos && <Pill tone="pos">{entry.usage.pos}</Pill>}
                </div>
                <IconButton
                  type="button"
                  title={t('learning.delete')}
                  aria-label={t('learning.deleteUsage', { word: entry.usage.lemma })}
                  disabled={busy || loading}
                  onClick={() => setDeleteId(entry.id)}
                ><Trash2 className="h-3.5 w-3.5" /></IconButton>
              </div>
              <p className="text-xs whitespace-pre-wrap break-words">{entry.usage.meaning}</p>
              <span className="text-[10px] text-[var(--muted)]">
                {t(`learning.definition.${entry.usage.definition_kind}`)} · {entry.usage.definition_source}
                {' · '}{new Date(entry.created_at * 1000).toLocaleDateString(i18n.language)}
              </span>
              {entry.usage.context ? (
                <div className="border-t border-[var(--stroke)] pt-2 text-xs leading-relaxed whitespace-pre-wrap break-words">
                  <span className="text-[10px] text-[var(--muted)]">{t(`learning.context.${entry.usage.context.source}`)}</span>
                  <p className="mt-1">{entry.usage.context.text}</p>
                  {entry.usage.context.translation && <p className="mt-1 text-[var(--muted)]">{entry.usage.context.translation}</p>}
                </div>
              ) : <span className="text-[10px] text-[var(--muted)]">{t('learning.withoutExample')}</span>}
              {deleteId === entry.id && (
                <div className="flex flex-wrap items-center gap-2 border-t border-[var(--stroke)] pt-2">
                  <span className="text-xs">{t('learning.deleteConfirm')}</span>
                  <Button type="button" size="sm" variant="danger" disabled={busy} onClick={() => void deleteEntry(entry.id)}>
                    {deletingId === entry.id && <Loader2 className="h-3 w-3 animate-spin" />}{t('learning.delete')}
                  </Button>
                  <Button type="button" size="sm" variant="subtle" disabled={busy} onClick={() => setDeleteId(null)}>{t('common.cancel')}</Button>
                </div>
              )}
            </article>
          ))}
        </div>
      )}
      {total > LEARNING_PAGE_SIZE && (
        <div className="flex items-center justify-center gap-3 text-xs">
          <IconButton
            type="button" title={t('learning.previous')} aria-label={t('learning.previous')}
            disabled={loading || busy || offset === 0}
            onClick={() => { setDeleteId(null); void load(query, offset - LEARNING_PAGE_SIZE); }}
          ><ChevronLeft className="h-4 w-4" /></IconButton>
          <span className="text-[var(--muted)]">{Math.floor(offset / LEARNING_PAGE_SIZE) + 1} / {Math.ceil(total / LEARNING_PAGE_SIZE)}</span>
          <IconButton
            type="button" title={t('learning.next')} aria-label={t('learning.next')}
            disabled={loading || busy || offset + entries.length >= total}
            onClick={() => { setDeleteId(null); void load(query, offset + LEARNING_PAGE_SIZE); }}
          ><ChevronRight className="h-4 w-4" /></IconButton>
        </div>
      )}
    </div>
  );
};
