import React, { useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { Download, LoaderCircle, RefreshCw, ScanText } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, HoverTipText, IconButton, Section, Select, Surface } from './ui';
import { useOcrStatus } from '../hooks/useOcr';
import { useAppStore } from '../store/useAppStore';
import type { OcrEngineId, OcrInstallProgress } from '../types';

const sizeLabel = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MiB`;

export const OcrSettingsPanel: React.FC<{ disabled?: boolean }> = ({ disabled = false }) => {
  const { t } = useTranslation();
  const { status, error, loading, refresh } = useOcrStatus();
  const language = useAppStore((state) => state.ocrLanguage);
  const engineId = useAppStore((state) => state.ocrEngine);
  const lastOcrBenchmarkMs = useAppStore((state) => state.lastOcrBenchmarkMs);
  const ocrPictureDefaultView = useAppStore((state) => state.ocrPictureDefaultView);
  const setLanguage = useAppStore((state) => state.setOcrLanguage);
  const setEngine = useAppStore((state) => state.setOcrEngine);
  const setOcrPictureDefaultView = useAppStore((state) => state.setOcrPictureDefaultView);
  const benchmarkOcr = useAppStore((state) => state.benchmarkOcr);
  const [changing, setChanging] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [benchmarking, setBenchmarking] = useState(false);
  const [benchmarkResult, setBenchmarkResult] = useState<number | null>(null);
  const [progress, setProgress] = useState<OcrInstallProgress | null>(null);
  const [installError, setInstallError] = useState<string | null>(null);
  const active = status?.engines.find((engine) => engine.id === engineId);
  const isOnnx = engineId === 'onnx';
  const busy = disabled || changing || installing || Boolean(status?.onnx.installing);

  const options = isOnnx
    ? [{ value: 'auto', label: t('settings.ocr.mixedLang') }]
    : [
        { value: 'auto', label: t('settings.ocr.systemLang') },
        ...(active?.languages.map((item) => ({ value: item.tag, label: item.label })) ?? []),
      ];
  const messages = [error, installError, active?.error].filter(Boolean);

  const changeEngine = async (engine: OcrEngineId) => {
    setChanging(true);
    try {
      await setEngine(engine);
    } finally {
      setChanging(false);
    }
  };

  const runBenchmark = async () => {
    setBenchmarking(true);
    try {
      const ms = await benchmarkOcr();
      setBenchmarkResult(ms);
    } catch (e) {
      console.error(e);
    } finally {
      setBenchmarking(false);
    }
  };

  const install = async () => {
    setInstalling(true);
    setInstallError(null);
    setProgress(null);
    const onProgress = new Channel<OcrInstallProgress>();
    onProgress.onmessage = setProgress;
    try {
      await invoke('install_onnx_ocr', { onProgress });
      await setEngine('onnx');
    } catch (error) {
      setInstallError(String(error));
    } finally {
      setInstalling(false);
      await refresh();
    }
  };

  const percent = progress
    ? Math.min(100, Math.round((progress.downloaded / Math.max(1, progress.total)) * 100))
    : 0;

  return (
    <Section label={t('settings.ocr.title')} icon={<ScanText className="h-3.5 w-3.5" />}>
      <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
        <div className="flex items-center justify-between gap-2 border-b border-[var(--stroke)] pb-2">
          <HoverTipText
            text={t('settings.ocr.subtitle')}
            tip={t('settings.ocr.footerHint')}
            className="text-[13px] font-semibold text-[var(--text)]"
          />
          <IconButton
            title={t('settings.ocr.refreshHint')}
            disabled={loading || busy}
            onClick={() => void refresh()}
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
          </IconButton>
        </div>

        {/* 识别引擎 */}
        <div className="flex items-center gap-3">
          <div className="w-28 shrink-0">
            <HoverTipText
              text={t('settings.ocr.engine')}
              tip={t('settings.ocr.engineHint')}
              className="text-[13px] text-[var(--muted)] font-normal"
            />
          </div>
          <Select
            ariaLabel={t('settings.ocr.engine')}
            value={engineId}
            onChange={(value) => void changeEngine(value as OcrEngineId)}
            options={
              status?.engines.map((engine) => ({
                value: engine.id,
                label: `${engine.label}${engine.available ? '' : ` - ${t('settings.ocr.notReady')}`}`,
              })) ?? []
            }
            disabled={!status || loading || busy}
          />
        </div>

        {/* 识别语言 */}
        <div className="flex items-center gap-3">
          <div className="w-28 shrink-0">
            <HoverTipText
              text={t('settings.ocr.language')}
              tip={t('settings.ocr.languageHint')}
              className="text-[13px] text-[var(--muted)] font-normal"
            />
          </div>
          <Select
            ariaLabel={t('settings.ocr.language')}
            value={isOnnx ? 'auto' : language}
            onChange={(value) => void setLanguage(value)}
            options={options}
            disabled={isOnnx || !active?.available || loading || busy}
          />
        </div>

        {/* 默认展示视图 */}
        <div className="flex items-center gap-3">
          <div className="w-28 shrink-0">
            <HoverTipText
              text={t('settings.ocr.pictureDefaultView')}
              tip={t('settings.ocr.pictureDefaultViewHint')}
              className="text-[13px] text-[var(--muted)] font-normal"
            />
          </div>
          <Select
            ariaLabel={t('settings.ocr.pictureDefaultView')}
            value={ocrPictureDefaultView}
            onChange={(value) => setOcrPictureDefaultView(value as 'translated' | 'original')}
            options={[
              { value: 'translated', label: t('settings.ocr.viewTranslated') },
              { value: 'original', label: t('settings.ocr.viewOriginal') },
            ]}
            disabled={busy}
          />
        </div>

        {changing && (
          <p role="status" className="flex items-center gap-1.5 text-[11px] text-[var(--muted)]">
            <LoaderCircle className="h-3 w-3 animate-spin" />
            {t('settings.ocr.loadingEngine')}
          </p>
        )}

        {status && !status.onnx.installed && (
          <div className="flex flex-col gap-2 rounded-[var(--r-inner)] border border-[var(--stroke)] p-2.5">
            <p className="text-[11px] leading-relaxed text-[var(--muted)]">
              {t('settings.ocr.onnxHint')}
            </p>
            <Button size="sm" variant="primary" disabled={busy} onClick={() => void install()}>
              {installing ? (
                <LoaderCircle className="h-3 w-3 animate-spin" />
              ) : (
                <Download className="h-3 w-3" />
              )}
              {installing ? t('settings.ocr.installing') : t('settings.ocr.installOnnx')}
            </Button>
          </div>
        )}

        {installing && (
          <div role="status" className="flex flex-col gap-1.5 text-[11px] text-[var(--muted)]">
            <div
              className="h-1.5 overflow-hidden rounded-full bg-[var(--surface-3)]"
              role="progressbar"
              aria-label={t('settings.ocr.title')}
              aria-valuenow={percent}
              aria-valuemin={0}
              aria-valuemax={100}
            >
              <div
                className="h-full bg-gradient-to-r from-[var(--accent-a)] to-[var(--accent-b)]"
                style={{ width: `${percent}%` }}
              />
            </div>
            {progress?.stage === 'downloading'
              ? t('settings.ocr.downloadingModel', {
                  downloaded: sizeLabel(progress.downloaded),
                  total: sizeLabel(progress.total),
                })
              : t('settings.ocr.verifyingModel')}
          </div>
        )}

        {status?.onnx.installed && (
          <p className="text-[10px] text-[var(--faint)]">
            {t('settings.ocr.onnxReady', { size: sizeLabel(status.onnx.installed_bytes) })}
          </p>
        )}

        {/* 性能测速卡片 */}
        <div className="flex flex-col gap-2 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2.5">
          <div className="flex items-center justify-between">
            <HoverTipText
              text={t('settings.ocr.benchmarkTitle')}
              tip={t('settings.ocr.benchmarkHint')}
              className="text-[12px] font-medium text-[var(--text)]"
            />
            {lastOcrBenchmarkMs != null && (
              <span className="font-mono text-[11px] text-[var(--accent-b)]">
                {t('settings.ocr.lastBenchmark', { ms: lastOcrBenchmarkMs })}
              </span>
            )}
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="text-[11px] text-[var(--muted)]">
              {benchmarkResult != null
                ? t('settings.ocr.benchmarkResult', { ms: benchmarkResult })
                : t('settings.ocr.benchmarkHint')}
            </span>
            <Button
              size="sm"
              variant="subtle"
              disabled={busy || benchmarking}
              onClick={() => void runBenchmark()}
            >
              {benchmarking ? (
                <LoaderCircle className="h-3 w-3 animate-spin" />
              ) : (
                <RefreshCw className="h-3 w-3" />
              )}
              {benchmarking ? t('settings.ocr.benchmarking') : t('settings.ocr.startBenchmark')}
            </Button>
          </div>
        </div>

        {messages.map((message, index) => (
          <p key={index} role="status" className="text-[11px] leading-relaxed text-[var(--muted)]">
            {message}
          </p>
        ))}
      </Surface>
    </Section>
  );
};
