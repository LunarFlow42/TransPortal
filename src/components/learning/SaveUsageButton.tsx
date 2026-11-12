import React, { useEffect, useRef, useState } from 'react';
import { BookmarkPlus, Check, Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, Select } from '../ui';
import { useLearningStore } from '../../store/useLearningStore';
import { learningContexts, learningDraft, learningSenses } from '../../utils/learning';
import type { LookupNode } from '../../types';

/** Inline, opt-in collection. Rendering a card does not read/write learning data. */
export const SaveUsageButton: React.FC<{
  node: LookupNode;
  isTop: boolean;
  translationSource: string;
}> = ({ node, isTop, translationSource }) => {
  const { t } = useTranslation();
  const enroll = useLearningStore((state) => state.enroll);
  const senses = learningSenses(node);
  const contexts = learningContexts(node);
  const [open, setOpen] = useState(false);
  const [senseId, setSenseId] = useState('');
  const [contextId, setContextId] = useState('');
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const savingRef = useRef(false);
  const mountedRef = useRef(false);
  const topRef = useRef(isTop);
  topRef.current = isTop;

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  if (!isTop || senses.length === 0) return null;
  const selectedSense = senses.find((sense) => sense.id === senseId);
  const selectedContext = contexts.find((option) => option.id === contextId)?.context;

  const begin = () => {
    setSenseId(senses[0].id);
    setContextId(contexts[0].id);
    setError(null);
    setMessage(null);
    setOpen(true);
  };

  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (savingRef.current || !topRef.current) return;
    const draft = learningDraft(node, senseId, contextId, translationSource);
    if (!draft) return;
    savingRef.current = true;
    setSaving(true);
    setError(null);
    try {
      const result = await enroll(draft);
      if (!mountedRef.current) return;
      setOpen(false);
      setMessage(t(result.created ? 'learning.saved' : 'learning.alreadySaved'));
    } catch (error) {
      if (mountedRef.current) setError(String(error));
    } finally {
      savingRef.current = false;
      if (mountedRef.current) setSaving(false);
    }
  };

  return (
    <div className="flex flex-col gap-2 border-t border-[var(--stroke)] pt-2">
      {!open ? (
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" size="sm" variant="subtle" onClick={begin}>
            <BookmarkPlus className="h-3.5 w-3.5" />
            {t('learning.rememberUsage')}
          </Button>
          {message && (
            <span role="status" className="flex items-center gap-1 text-[11px] text-[var(--text)]">
              <Check className="h-3 w-3 text-[var(--accent-b)]" />{message}
            </span>
          )}
        </div>
      ) : (
        <form onSubmit={(event) => void save(event)} className="flex flex-col gap-2.5 text-xs">
          <span className="text-[11px] text-[var(--muted)]">{t('learning.chooseUsageHint')}</span>
          <div className="flex items-center gap-2 min-w-0">
            <span className="shrink-0 text-[var(--muted)]">{t('learning.sense')}</span>
            <div className="flex min-w-0 flex-1 [&>div]:min-w-0 [&_[role=listbox]]:w-full [&_[role=listbox]]:max-w-full">
              <Select
                value={senseId}
                onChange={setSenseId}
                disabled={saving}
                ariaLabel={t('learning.sense')}
                className="min-w-0"
                options={senses.map((sense) => ({
                  value: sense.id,
                  label: `${sense.pos} ${sense.meaning}${sense.source === 'local_eccedict' && node.wordData?.source !== 'local_eccedict' ? ` · ${t('card.sourceLocal')}` : ''}`.trim(),
                }))}
              />
            </div>
          </div>
          {selectedSense && <p className="whitespace-pre-wrap break-words">{selectedSense.meaning}</p>}
          {selectedSense?.kind === 'translation' && (
            <p className="text-[11px] text-[var(--muted)]">{t('learning.translationWarning')}</p>
          )}
          <div className="flex items-center gap-2 min-w-0">
            <span className="shrink-0 text-[var(--muted)]">{t('learning.example')}</span>
            <div className="flex min-w-0 flex-1 [&>div]:min-w-0 [&_[role=listbox]]:w-full [&_[role=listbox]]:max-w-full">
              <Select
                value={contextId}
                onChange={setContextId}
                disabled={saving}
                ariaLabel={t('learning.example')}
                className="min-w-0"
                options={contexts.map((option) => ({
                  value: option.id,
                  label: option.context
                    ? `${t(`learning.context.${option.context.source}`)} · ${option.context.text.slice(0, 48)}`
                    : t('learning.withoutExample'),
                }))}
              />
            </div>
          </div>
          {selectedContext ? (
            <div className="rounded-[var(--r-inner)] bg-[var(--surface-2)] p-2 whitespace-pre-wrap break-words leading-relaxed">
              <p>{selectedContext.text}</p>
              {selectedContext.translation && <p className="mt-1 text-[var(--muted)]">{selectedContext.translation}</p>}
            </div>
          ) : <p className="text-[11px] text-[var(--muted)]">{t('learning.noExampleHint')}</p>}
          <div className="flex items-center gap-2">
            <Button type="submit" size="sm" disabled={saving || !selectedSense}>
              {saving ? <Loader2 className="h-3 w-3 animate-spin" /> : <BookmarkPlus className="h-3 w-3" />}
              {t(saving ? 'learning.saving' : 'learning.confirmSave')}
            </Button>
            <Button type="button" size="sm" variant="subtle" disabled={saving} onClick={() => setOpen(false)}>
              {t('common.cancel')}
            </Button>
          </div>
          {error && <p role="alert" className="text-xs text-[var(--text)] break-words">{t('learning.saveFailed')} {error}</p>}
        </form>
      )}
    </div>
  );
};
