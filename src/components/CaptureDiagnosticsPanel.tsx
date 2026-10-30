import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import {
  Camera,
  Check,
  Clock,
  Copy,
  ExternalLink,
  Layers,
  Monitor,
  RefreshCw,
  Terminal,
} from 'lucide-react';
import { Button, Surface } from './ui';
import type { CaptureDiagnostics } from '../types';

export const CaptureDiagnosticsPanel: React.FC<{ bare?: boolean }> = ({ bare = false }) => {
  const { t, i18n } = useTranslation();
  const [diagnostics, setDiagnostics] = useState<CaptureDiagnostics | null>(null);
  const [loading, setLoading] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [copied, setCopied] = useState(false);
  const [feedback, setFeedback] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState(false);

  const countdownTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchDiagnostics = async () => {
    try {
      const res = await invoke<CaptureDiagnostics | null>('get_capture_diagnostics');
      if (res) setDiagnostics(res);
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    void fetchDiagnostics();
    return () => {
      if (countdownTimerRef.current) clearInterval(countdownTimerRef.current);
    };
  }, []);

  const formatMarkdown = (diag: CaptureDiagnostics): string => {
    const isZh = i18n.language.startsWith('zh');
    let md = isZh
      ? `### 📸 截图管线诊断报告 (Capture Diagnostics)\n\n`
      : `### 📸 Capture Diagnostics Report\n\n`;
    md += `- **${t('settings.capture.time')}**: \`${diag.timestamp}\`\n`;
    md += `- **${t('settings.capture.backend')}**: \`${diag.finalBackend}\`\n`;
    md += `- **${t('settings.capture.duration')}**: \`${diag.totalDurationMs} ms\`\n`;
    md += `- **${t('settings.capture.targetRegion')}**: \`(${diag.targetRegion.x}, ${diag.targetRegion.y}) ${diag.targetRegion.width}×${diag.targetRegion.height}\`\n`;
    if (diag.monitorRect) {
      md += `- **${t('settings.capture.monitor')}**: \`(${diag.monitorRect.x}, ${diag.monitorRect.y}) ${diag.monitorRect.width}×${diag.monitorRect.height}\`\n`;
    }
    if (diag.savedImagePath) {
      md += `- **${t('settings.capture.debugImage')}**: \`${diag.savedImagePath}\`\n`;
    }

    md += `\n#### 🎯 ${t('settings.capture.foregroundWindow')}\n`;
    if (diag.foregroundWindow) {
      const fg = diag.foregroundWindow;
      md += `- **HWND**: \`0x${fg.hwnd.toString(16).toUpperCase().padStart(8, '0')}\`\n`;
      md += `- **${t('settings.capture.process')}**: \`${fg.processName}\` (PID: \`${fg.processId}\`)\n`;
      md += `- **${t('settings.capture.windowTitle')}**: \`${fg.title}\`\n`;
      md += `- **Class**: \`${fg.className}\`\n`;
      md += `- **${t('settings.capture.size')}**: \`(${fg.rect.x}, ${fg.rect.y}) ${fg.rect.width}×${fg.rect.height}\`\n`;
      md += `- **${t('settings.capture.coverage')}**: Visible: \`${fg.isVisible}\`, Iconic: \`${fg.isIconic}\`, Contains: \`${fg.containsTarget}\`\n`;
    } else {
      md += `- *${t('settings.capture.noFg')}*\n`;
    }

    md += `\n#### ⚙️ ${t('settings.capture.pipelineTiers')}\n`;
    for (const tier of diag.tiers) {
      const status = tier.success
        ? `✅ ${t('settings.capture.tierSuccess')}`
        : tier.attempted
        ? `❌ ${t('settings.capture.tierFailed')}`
        : `⚪ ${t('settings.capture.tierNotAttempted')}`;
      md += `\n##### \`${tier.tier}\` (${status})\n`;
      md += `- Duration: \`${tier.durationMs} ms\`\n`;
      if (tier.frame0LatencyMs !== undefined && tier.frame0LatencyMs !== null) {
        md += `- Frame 0 Latency: \`${tier.frame0LatencyMs} ms\`\n`;
      }
      if (tier.frame1LatencyMs !== undefined && tier.frame1LatencyMs !== null) {
        md += `- Frame 1 Latency: \`${tier.frame1LatencyMs} ms\`\n`;
      }
      if (tier.framesDrained > 0) {
        md += `- Drained Frames: \`${tier.framesDrained}\`\n`;
      }
      if (tier.stagingCopyMs !== undefined && tier.stagingCopyMs !== null) {
        md += `- Staging Copy & Map: \`${tier.stagingCopyMs} ms\`\n`;
      }
      if (tier.attempted) {
        md += `- All-black: \`${tier.isAllBlack}\`\n`;
      }
      if (tier.error) {
        md += `- Error: \`${tier.error}\`\n`;
      }
    }
    return md;
  };

  const handleCopyMarkdown = async () => {
    if (!diagnostics) return;
    try {
      const text = formatMarkdown(diagnostics);
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      try {
        await invoke('plugin:clipboard-manager|write_text', {
          text: formatMarkdown(diagnostics),
        });
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
      } catch (err) {
        setError(t('settings.capture.copyFailed', { error: String(err) }));
      }
    }
  };

  const handleOpenImage = async () => {
    try {
      const path = await invoke<string>('open_capture_debug_image');
      setFeedback(t('settings.capture.openedInViewer', { path }));
      setTimeout(() => setFeedback(null), 3000);
    } catch (err) {
      setError(String(err));
    }
  };

  const executeTestCapture = async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await invoke<CaptureDiagnostics>('test_screen_capture', { delaySecs: 0 });
      setDiagnostics(res);
      setFeedback(t('settings.capture.successNotice'));
      setTimeout(() => setFeedback(null), 3000);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  const startCountdownTest = () => {
    if (countdown !== null) return;
    setCountdown(3);
    let count = 3;
    countdownTimerRef.current = setInterval(() => {
      count -= 1;
      if (count > 0) {
        setCountdown(count);
      } else {
        if (countdownTimerRef.current) clearInterval(countdownTimerRef.current);
        setCountdown(null);
        void executeTestCapture();
      }
    }, 1000);
  };

  const content = (
    <Surface variant="inner" className="flex flex-col gap-3 p-3">
      {/* 状态徽标与概要 */}
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--stroke)] pb-2.5">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-medium text-[var(--text)]">
            {t('settings.capture.latestChannel')}
          </span>
          {diagnostics ? (
            <span
              className={`rounded px-1.5 py-0.5 font-mono text-[11px] font-bold ${
                diagnostics.finalBackend === 'WGC_WINDOW'
                  ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30'
                  : diagnostics.finalBackend === 'WGC_MONITOR'
                  ? 'bg-sky-500/15 text-sky-400 border border-sky-500/30'
                  : diagnostics.finalBackend === 'DXGI'
                  ? 'bg-amber-500/15 text-amber-400 border border-amber-500/30'
                  : diagnostics.finalBackend === 'GDI'
                  ? 'bg-orange-500/15 text-orange-400 border border-orange-500/30'
                  : 'bg-rose-500/15 text-rose-400 border border-rose-500/30'
              }`}
            >
              {diagnostics.finalBackend}
            </span>
          ) : (
            <span className="text-[11px] text-[var(--muted)]">{t('settings.capture.noRecord')}</span>
          )}

          {diagnostics && (
            <span className="text-[11px] font-mono text-[var(--muted)]">
              {diagnostics.totalDurationMs} ms
            </span>
          )}
        </div>

        <div className="flex items-center gap-1.5">
          {diagnostics && (
            <span className="text-[10px] text-[var(--faint)] font-mono">
              {diagnostics.timestamp}
            </span>
          )}
        </div>
      </div>

      {/* 前台窗口与显示器概览 */}
      {diagnostics && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-2 text-[11px]">
          {/* 前台窗口卡片 */}
          <div className="rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2 flex flex-col gap-1">
            <div className="flex items-center gap-1.5 font-medium text-[var(--text)]">
              <Monitor className="h-3.5 w-3.5 text-indigo-400" />
              <span>{t('settings.capture.fgWindow')}</span>
            </div>
            {diagnostics.foregroundWindow ? (
              <div className="flex flex-col gap-0.5 text-[var(--muted)]">
                <div className="flex justify-between">
                  <span>{t('settings.capture.process')}</span>
                  <span className="font-mono text-[var(--text)] font-semibold">
                    {diagnostics.foregroundWindow.processName} (PID: {diagnostics.foregroundWindow.processId})
                  </span>
                </div>
                <div className="flex justify-between truncate" title={diagnostics.foregroundWindow.title}>
                  <span>{t('settings.capture.windowTitle')}</span>
                  <span className="truncate max-w-[150px] text-[var(--text)] font-mono">
                    {diagnostics.foregroundWindow.title || t('settings.capture.noTitle')}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>{t('settings.capture.size')}</span>
                  <span className="font-mono text-[var(--text)]">
                    {diagnostics.foregroundWindow.rect.width}×{diagnostics.foregroundWindow.rect.height}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>{t('settings.capture.coverage')}</span>
                  <span
                    className={
                      diagnostics.foregroundWindow.containsTarget
                        ? 'text-emerald-400 font-medium'
                        : 'text-amber-400'
                    }
                  >
                    {diagnostics.foregroundWindow.containsTarget
                      ? t('settings.capture.fullyCovered')
                      : t('settings.capture.notFullyCovered')}
                  </span>
                </div>
              </div>
            ) : (
              <p className="text-[var(--faint)] italic">{t('settings.capture.noValidFg')}</p>
            )}
          </div>

          {/* 管线分层状态卡片 */}
          <div className="rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2 flex flex-col gap-1">
            <div className="flex items-center gap-1.5 font-medium text-[var(--text)]">
              <Layers className="h-3.5 w-3.5 text-sky-400" />
              <span>{t('settings.capture.tierProbing')}</span>
            </div>
            <div className="flex flex-col gap-1 text-[var(--muted)]">
              {diagnostics.tiers.map((tierItem) => (
                <div key={tierItem.tier} className="flex items-center justify-between text-[10px]">
                  <span className="font-mono">{tierItem.tier}:</span>
                  <div className="flex items-center gap-1.5">
                    {tierItem.frame0LatencyMs !== undefined && tierItem.frame0LatencyMs !== null && (
                      <span className="font-mono text-[var(--faint)]">
                        F0: {tierItem.frame0LatencyMs}ms
                      </span>
                    )}
                    {tierItem.frame1LatencyMs !== undefined && tierItem.frame1LatencyMs !== null && (
                      <span className="font-mono text-emerald-400">
                        F1: {tierItem.frame1LatencyMs}ms
                      </span>
                    )}
                    <span
                      className={`font-semibold ${
                        tierItem.success
                          ? 'text-emerald-400'
                          : tierItem.attempted
                          ? 'text-rose-400'
                          : 'text-[var(--faint)]'
                      }`}
                    >
                      {tierItem.success
                        ? t('settings.capture.success')
                        : tierItem.attempted
                        ? t('settings.capture.failed')
                        : t('settings.capture.skipped')}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* 提示信息 */}
      {feedback && (
        <p className="text-[11px] text-emerald-400 bg-emerald-500/10 border border-emerald-500/20 rounded px-2 py-1">
          {feedback}
        </p>
      )}
      {error && (
        <p className="text-[11px] text-rose-400 bg-rose-500/10 border border-rose-500/20 rounded px-2 py-1">
          {error}
        </p>
      )}

      {/* 操作按钮组 */}
      <div className="flex flex-wrap items-center gap-2 pt-1 border-t border-[var(--stroke)]">
        <Button
          size="sm"
          variant="primary"
          disabled={loading || countdown !== null}
          onClick={startCountdownTest}
          className="h-7 text-[11px]"
        >
          <Clock className="h-3 w-3" />
          {countdown !== null
            ? t('settings.capture.countdownGame', { count: countdown })
            : t('settings.capture.countdownBtn')}
        </Button>

        <Button
          size="sm"
          disabled={loading || countdown !== null}
          onClick={() => void executeTestCapture()}
          className="h-7 text-[11px]"
        >
          <Camera className="h-3 w-3" />
          {t('settings.capture.captureNow')}
        </Button>

        <Button
          size="sm"
          disabled={!diagnostics}
          onClick={() => void handleCopyMarkdown()}
          className="h-7 text-[11px]"
        >
          {copied ? <Check className="h-3 w-3 text-emerald-400" /> : <Copy className="h-3 w-3" />}
          {copied ? t('settings.capture.copiedMarkdown') : t('settings.capture.copyMarkdown')}
        </Button>

        {diagnostics?.savedImagePath && (
          <Button
            size="sm"
            onClick={() => void handleOpenImage()}
            className="h-7 text-[11px]"
          >
            <ExternalLink className="h-3 w-3" />
            {t('settings.capture.viewImage')}
          </Button>
        )}

        <Button
          size="sm"
          disabled={loading}
          onClick={() => void fetchDiagnostics()}
          className="h-7 text-[11px] ml-auto"
        >
          <RefreshCw className={`h-3 w-3 ${loading ? 'animate-spin' : ''}`} />
          {t('settings.capture.refresh')}
        </Button>
      </div>

      {/* 原始日志折叠区块 */}
      {diagnostics && (
        <div className="border-t border-[var(--stroke)] pt-2">
          <button
            type="button"
            onClick={() => setShowRaw(!showRaw)}
            className="flex items-center gap-1 text-[11px] text-[var(--muted)] hover:text-[var(--text)] cursor-pointer"
          >
            <Terminal className="h-3 w-3" />
            <span>{showRaw ? t('settings.capture.collapseReport') : t('settings.capture.expandReport')}</span>
          </button>

          {showRaw && (
            <pre className="mt-2 p-2.5 rounded-[var(--r-inner)] bg-[var(--surface-1)] border border-[var(--stroke)] text-[10px] font-mono text-[var(--text)] overflow-x-auto whitespace-pre-wrap select-text leading-relaxed">
              {formatMarkdown(diagnostics)}
            </pre>
          )}
        </div>
      )}
    </Surface>
  );

  if (bare) return content;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Camera className="h-4 w-4 text-[var(--accent-b)]" />
        <h3 className="text-sm font-medium text-[var(--text)]">
          {t('settings.capture.panelTitle')}
        </h3>
      </div>
      {content}
    </div>
  );
};
