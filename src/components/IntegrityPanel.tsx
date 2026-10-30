import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Copy, RefreshCw, ShieldCheck } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, HoverTipText, Section, Surface } from './ui';
import type { IntegrityReport } from '../types';

export const IntegrityPanel: React.FC<{ bare?: boolean }> = ({ bare = false }) => {
  const { t } = useTranslation();
  const [report, setReport] = useState<IntegrityReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    let disposed = false;
    void invoke<IntegrityReport>('integrity_status')
      .then((report) => {
        if (!disposed) setReport(report);
      })
      .catch((error) => {
        if (!disposed) setError(String(error));
      });
    return () => {
      disposed = true;
    };
  }, []);

  const recheck = async () => {
    setChecking(true);
    setError(null);
    try {
      setReport(await invoke<IntegrityReport>('verify_integrity'));
    } catch (error) {
      setError(String(error));
    } finally {
      setChecking(false);
    }
  };

  const copyHash = async () => {
    try {
      await invoke('plugin:clipboard-manager|write_text', { text: report?.actual_sha256 ?? '' });
    } catch (error) {
      setError(String(error));
    }
  };

  const getStatusText = () => {
    if (report?.status === 'verified') return t('settings.integrity.verified');
    if (report?.status === 'development') return t('settings.integrity.development');
    if (report?.status === 'unsupported') return t('settings.integrity.unsupported');
    return report?.error ?? t('settings.integrity.checking');
  };

  const content = (
    <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
      <div className="flex items-center justify-between">
        <p role="status" className="text-[13px] font-medium text-[var(--text)]">
          {getStatusText()}
        </p>
        <HoverTipText
          text={t('settings.integrity.title')}
          tip={t('settings.integrity.hint')}
          className="text-[11px] text-[var(--faint)]"
        />
      </div>

      {report?.actual_sha256 && (
        <details className="text-[11px] text-[var(--muted)]">
          <summary className="cursor-pointer select-none hover:text-[var(--text)]">
            {t('settings.integrity.viewHash')}
          </summary>
          <code className="mt-2 block select-text break-all font-mono leading-relaxed bg-[var(--surface-2)] p-2 rounded-[var(--r-inner)] border border-[var(--stroke)]">
            {report.actual_sha256}
          </code>
        </details>
      )}

      <div className="flex gap-2 pt-1">
        <Button
          size="sm"
          disabled={checking || report?.status !== 'verified'}
          onClick={() => void recheck()}
        >
          <RefreshCw className={`h-3 w-3 ${checking ? 'animate-spin' : ''}`} />
          {t('settings.integrity.recheck')}
        </Button>
        {report?.actual_sha256 && (
          <Button size="sm" variant="subtle" onClick={() => void copyHash()}>
            <Copy className="h-3 w-3" />
            {t('settings.integrity.copyHash')}
          </Button>
        )}
      </div>
      {error && <p role="alert" className="text-[11px] text-rose-400">{error}</p>}
    </Surface>
  );

  if (bare) return content;

  return (
    <Section label={t('settings.integrity.title')} icon={<ShieldCheck className="h-3.5 w-3.5" />}>
      {content}
    </Section>
  );
};
