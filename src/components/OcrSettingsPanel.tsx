import React, { useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { Download, LoaderCircle, RefreshCw, ScanText } from 'lucide-react';
import { Button, IconButton, Section, Select, Surface } from './ui';
import { useOcrStatus } from '../hooks/useOcr';
import { useAppStore } from '../store/useAppStore';
import type { OcrEngineId, OcrInstallProgress } from '../types';

const sizeLabel = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MiB`;

export const OcrSettingsPanel: React.FC<{ disabled?: boolean }> = ({ disabled = false }) => {
  const { status, error, loading, refresh } = useOcrStatus();
  const language = useAppStore((state) => state.ocrLanguage);
  const engineId = useAppStore((state) => state.ocrEngine);
  const lastOcrBenchmarkMs = useAppStore((state) => state.lastOcrBenchmarkMs);
  const setLanguage = useAppStore((state) => state.setOcrLanguage);
  const setEngine = useAppStore((state) => state.setOcrEngine);
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
  const options = isOnnx ? [{ value: 'auto', label: '简体中文 + English（混合识别）' }] : [
    { value: 'auto', label: '跟随系统语言' },
    ...(active?.languages.map((item) => ({ value: item.tag, label: `${item.label} (${item.tag})` })) ?? []),
  ];
  const messages = [error, installError, active?.error]
    .filter(Boolean);

  const changeEngine = async (engine: OcrEngineId) => {
    setChanging(true);
    try { await setEngine(engine); } finally { setChanging(false); }
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

  const percent = progress ? Math.min(100, Math.round(progress.downloaded / Math.max(1, progress.total) * 100)) : 0;

  return (
    <Section label="Screenshot OCR" icon={<ScanText className="h-3.5 w-3.5" />}>
      <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
        <div className="flex items-center justify-between gap-2">
          <span className="text-[13px]">本地文字识别</span>
          <IconButton title="检查 OCR 资源与语言包" disabled={loading || busy} onClick={() => void refresh()}>
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
          </IconButton>
        </div>
        <div className="flex items-center gap-3">
          <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">识别引擎</label>
          <Select ariaLabel="OCR 识别引擎" value={engineId}
            onChange={(value) => void changeEngine(value as OcrEngineId)}
            options={status?.engines.map((engine) => ({ value: engine.id, label: `${engine.label}${engine.available ? '' : '（未就绪）'}` })) ?? []}
            disabled={!status || loading || busy} />
        </div>
        <div className="flex items-center gap-3">
          <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">识别语言</label>
          <Select ariaLabel="OCR 识别语言" value={isOnnx ? 'auto' : language}
            onChange={(value) => void setLanguage(value)}
            options={options} disabled={isOnnx || !active?.available || loading || busy} />
        </div>
        {changing && <p role="status" className="flex items-center gap-1.5 text-[11px] text-[var(--muted)]"><LoaderCircle className="h-3 w-3 animate-spin" />正在加载识别引擎…</p>}
        {status && !status.onnx.installed && (
          <div className="flex flex-col gap-2 rounded-[var(--r-inner)] border border-[var(--stroke)] p-2.5">
            <p className="text-[11px] leading-relaxed text-[var(--muted)]">
              ONNX 无需系统语言包。首次下载约 {sizeLabel(status.onnx.download_bytes)}，安装后约 {sizeLabel(status.onnx.installed_bytes)}，之后离线使用。
            </p>
            <Button size="sm" variant="primary" disabled={busy} onClick={() => void install()}>
              {installing ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <Download className="h-3 w-3" />}
              {installing ? '正在安装…' : '安装并启用 ONNX'}
            </Button>
          </div>
        )}
        {installing && (
          <div role="status" className="flex flex-col gap-1.5 text-[11px] text-[var(--muted)]">
            <div className="h-1.5 overflow-hidden rounded-full bg-[var(--surface-3)]" role="progressbar" aria-label="ONNX 资源安装进度" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
              <div className="h-full bg-gradient-to-r from-[var(--accent-a)] to-[var(--accent-b)]" style={{ width: `${percent}%` }} />
            </div>
            {progress?.stage === 'downloading'
              ? `下载资源 ${sizeLabel(progress.downloaded)} / ${sizeLabel(progress.total)}`
              : '正在校验资源并加载模型…'}
          </div>
        )}
        {status?.onnx.installed && <p className="text-[10px] text-[var(--faint)]">ONNX FP32 资源已就绪 · {sizeLabel(status.onnx.installed_bytes)}</p>}

        <div className="flex flex-col gap-2 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2.5">
          <div className="flex items-center justify-between">
            <span className="text-[12px] font-medium text-[var(--text)]">识别性能调优</span>
            {lastOcrBenchmarkMs != null && (
              <span className="font-mono text-[11px] text-[var(--accent-b)]">
                上次识别: {lastOcrBenchmarkMs} ms
              </span>
            )}
          </div>
          <div className="flex items-center justify-between gap-2">
            <span className="text-[11px] text-[var(--muted)]">
              {benchmarkResult != null
                ? `实测耗时: ${benchmarkResult} ms`
                : '测试当前引擎的标准识别耗时'}
            </span>
            <Button size="sm" variant="subtle" disabled={busy || benchmarking} onClick={() => void runBenchmark()}>
              {benchmarking ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <RefreshCw className="h-3 w-3" />}
              {benchmarking ? '测速中…' : '测试测速'}
            </Button>
          </div>
        </div>

        <div className="flex flex-col gap-1.5 text-[11px] text-[var(--muted)]">
          <div className="flex justify-between gap-3"><span>提取文字并复制</span><kbd>Alt + Shift + S</kbd></div>
          <div className="flex justify-between gap-3"><span>截图后翻译</span><kbd>Alt + S</kbd></div>
          <div className="flex justify-between gap-3"><span>默认取词翻译</span><kbd>Alt + Q</kbd></div>
        </div>
        <p className="text-[10px] leading-relaxed text-[var(--faint)]">
          {isOnnx
            ? '中英混合文本在本机识别，首次启用会加载模型。切回系统 OCR 后释放模型会话。'
            : 'Windows OCR 使用已安装的系统语言包；中文截图可明确选择中文识别器。'}
          {status?.supported ? ' 快捷键可在上方设置中自定义。' : ' 此平台可通过浮层的“打开图片”进行识别。'}
        </p>
        {messages.map((message, index) => (
          <p key={index} role="status" className="text-[11px] leading-relaxed text-[var(--muted)]">{message}</p>
        ))}
      </Surface>
    </Section>
  );
};
