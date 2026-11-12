import React from 'react';
import { useTranslation } from 'react-i18next';
import { Pill } from './ui';
import { InteractiveSentence } from './InteractiveSentence';
import type { DictEntry } from '../types';

function sourceKey(source: string): string {
  return ({ youdao: 'card.sourceYoudao', google: 'card.sourceGoogle', local_eccedict: 'card.sourceLocal' } as Record<string, string>)[source] ?? source;
}

/** Show the origin and lexical importance without pretending corpus ranks are current. */
export const DictionaryMetadata: React.FC<{ entry: DictEntry }> = ({ entry }) => {
  const { t } = useTranslation();
  const supplement = entry.local_supplement;
  return (
    <div className="flex flex-col gap-1.5 text-[10px] text-[var(--muted)]">
      <div className="flex flex-wrap items-center gap-1.5">
        <span>{t(sourceKey(entry.source))}</span>
        {supplement && <span title={supplement.fields.map((field) => t(`card.localFields.${field}`)).join('、')}>
          · {t('card.localSupplementBadge', { word: supplement.word })}
        </span>}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {entry.collins != null && entry.collins > 0 && <Pill>{t('card.collinsStars', { count: entry.collins })}</Pill>}
        {entry.oxford && <Pill>{t('card.oxfordCore')}</Pill>}
        {entry.bnc != null && entry.bnc > 0 && <Pill><span title={t('card.frequencyHint')}>{t('card.bncRank', { rank: entry.bnc.toLocaleString() })}</span></Pill>}
        {entry.frq != null && entry.frq > 0 && <Pill><span title={t('card.frequencyHint')}>{t('card.frqRank', { rank: entry.frq.toLocaleString() })}</span></Pill>}
      </div>
    </div>
  );
};

/** Keep local meanings and examples attributed and collapsed, not mixed into
 * the online provider's meanings (which remain the primary answer). */
export const LocalDictionaryDetails: React.FC<{ entry: DictEntry }> = ({ entry }) => {
  const { t } = useTranslation();
  const local = entry.local_supplement;
  if (!local || (!local.definitions.length && !local.english_definitions.length && !local.examples.length)) return null;
  return (
    <details className="border-t border-[var(--stroke)] pt-2 text-xs">
      <summary className="cursor-pointer text-[var(--muted)] focus-ring rounded-[var(--r-ctrl)]">
        {t('card.localDetails', { word: local.word })}
      </summary>
      <div className="mt-2 flex flex-col gap-2.5">
        {local.phonetic && <p className="text-[var(--muted)] font-mono">{local.phonetic}</p>}
        {local.definitions.map((definition, index) => (
          <div key={`zh:${index}`} className="flex items-start gap-2">
            <Pill tone="pos" className="shrink-0">{definition.pos}</Pill>
            <p className="break-words">{definition.meanings.join('；')}</p>
          </div>
        ))}
        {local.english_definitions.length > 0 && <div className="flex flex-col gap-1.5">
          <span className="text-[10px] text-[var(--muted)]">{t('card.localEnglishDefinitions')}</span>
          {local.english_definitions.map((definition, index) => (
            <div key={`en:${index}`} className="flex items-start gap-2">
              <Pill tone="pos" className="shrink-0">{definition.pos}</Pill>
              <p className="break-words">{definition.meanings.join('；')}</p>
            </div>
          ))}
        </div>}
        {local.examples.map((example, index) => (
          <div key={index} className="rounded-[var(--r-inner)] bg-[var(--surface-2)] p-2">
            <span className="text-[10px] text-[var(--muted)]">{t('card.localExample')}</span>
            <InteractiveSentence sentence={example.en} />
            {example.zh && <p className="mt-1 text-[var(--muted)]">{example.zh}</p>}
          </div>
        ))}
      </div>
    </details>
  );
};
