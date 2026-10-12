import React, { useEffect, useState, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
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
    let md = `### 📸 截图管线诊断报告 (Capture Diagnostics)\n\n`;
    md += `- **时间戳**: \`${diag.timestamp}\`\n`;
    md += `- **最终采用后端**: \`${diag.finalBackend}\`\n`;
    md += `- **捕获总耗时**: \`${diag.totalDurationMs} ms\`\n`;
    md += `- **请求选区**: \`(${diag.targetRegion.x}, ${diag.targetRegion.y}) ${diag.targetRegion.width}×${diag.targetRegion.height}\`\n`;
    if (diag.monitorRect) {
      md += `- **目标显示器**: \`(${diag.monitorRect.x}, ${diag.monitorRect.y}) ${diag.monitorRect.width}×${diag.monitorRect.height}\`\n`;
    }
    if (diag.savedImagePath) {
      md += `- **调试图片落盘**: \`${diag.savedImagePath}\`\n`;
    }

    md += `\n#### 🎯 前台窗口探针 (Foreground Window)\n`;
    if (diag.foregroundWindow) {
      const fg = diag.foregroundWindow;
      md += `- **句柄 (HWND)**: \`0x${fg.hwnd.toString(16).toUpperCase().padStart(8, '0')}\`\n`;
      md += `- **进程名 (EXE)**: \`${fg.processName}\` (PID: \`${fg.processId}\`)\n`;
      md += `- **窗口标题**: \`${fg.title}\`\n`;
      md += `- **窗口类名**: \`${fg.className}\`\n`;
      md += `- **窗口边界**: \`(${fg.rect.x}, ${fg.rect.y}) ${fg.rect.width}×${fg.rect.height}\`\n`;
      md += `- **状态**: Visible: \`${fg.isVisible}\`, Iconic (最小化): \`${fg.isIconic}\`, 覆盖选区: \`${fg.containsTarget}\`\n`;
    } else {
      md += `- *未探测到有效前台窗口（或属于 TransPortal 自身）*\n`;
    }

    md += `\n#### ⚙️ 分层管线执行追踪 (Pipeline Tiers)\n`;
    for (const tier of diag.tiers) {
      const status = tier.success ? '✅ 成功' : tier.attempted ? '❌ 失败/跳过' : '⚪ 未尝试';
      md += `\n##### \`${tier.tier}\` (${status})\n`;
      md += `- 耗时: \`${tier.durationMs} ms\`\n`;
      if (tier.frame0LatencyMs !== undefined && tier.frame0LatencyMs !== null) {
        md += `- Frame 0 抵达延迟: \`${tier.frame0LatencyMs} ms\`\n`;
      }
      if (tier.frame1LatencyMs !== undefined && tier.frame1LatencyMs !== null) {
        md += `- Frame 1 探测耗时: \`${tier.frame1LatencyMs} ms\`\n`;
      }
      if (tier.framesDrained > 0) {
        md += `- 排空缓冲帧数: \`${tier.framesDrained} 帧\`\n`;
      }
      if (tier.stagingCopyMs !== undefined && tier.stagingCopyMs !== null) {
        md += `- Staging 纹理拷贝与映射: \`${tier.stagingCopyMs} ms\`\n`;
      }
      if (tier.attempted) {
        md += `- 是否纯黑画面: \`${tier.isAllBlack}\`\n`;
      }
      if (tier.error) {
        md += `- 错误信息: \`${tier.error}\`\n`;
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
        setError(`复制失败: ${err}`);
      }
    }
  };

  const handleOpenImage = async () => {
    try {
      const path = await invoke<string>('open_capture_debug_image');
      setFeedback(`已在系统查看器中打开: ${path}`);
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
      setFeedback('测试截屏成功，诊断报告已更新！');
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
            最新截图通道：
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
            <span className="text-[11px] text-[var(--muted)]">暂无截图记录</span>
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
              <span>前台活动窗口</span>
            </div>
            {diagnostics.foregroundWindow ? (
              <div className="flex flex-col gap-0.5 text-[var(--muted)]">
                <div className="flex justify-between">
                  <span>进程:</span>
                  <span className="font-mono text-[var(--text)] font-semibold">
                    {diagnostics.foregroundWindow.processName} (PID: {diagnostics.foregroundWindow.processId})
                  </span>
                </div>
                <div className="flex justify-between truncate" title={diagnostics.foregroundWindow.title}>
                  <span>标题:</span>
                  <span className="truncate max-w-[150px] text-[var(--text)] font-mono">
                    {diagnostics.foregroundWindow.title || '(无标题)'}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>尺寸:</span>
                  <span className="font-mono text-[var(--text)]">
                    {diagnostics.foregroundWindow.rect.width}×{diagnostics.foregroundWindow.rect.height}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span>覆盖状态:</span>
                  <span
                    className={
                      diagnostics.foregroundWindow.containsTarget
                        ? 'text-emerald-400 font-medium'
                        : 'text-amber-400'
                    }
                  >
                    {diagnostics.foregroundWindow.containsTarget
                      ? '已完整覆盖选区'
                      : '未完全覆盖选区'}
                  </span>
                </div>
              </div>
            ) : (
              <p className="text-[var(--faint)] italic">无有效外部前台窗口</p>
            )}
          </div>

          {/* 管线分层状态卡片 */}
          <div className="rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2 flex flex-col gap-1">
            <div className="flex items-center gap-1.5 font-medium text-[var(--text)]">
              <Layers className="h-3.5 w-3.5 text-sky-400" />
              <span>分层管道探测</span>
            </div>
            <div className="flex flex-col gap-1 text-[var(--muted)]">
              {diagnostics.tiers.map((t) => (
                <div key={t.tier} className="flex items-center justify-between text-[10px]">
                  <span className="font-mono">{t.tier}:</span>
                  <div className="flex items-center gap-1.5">
                    {t.frame0LatencyMs !== undefined && t.frame0LatencyMs !== null && (
                      <span className="font-mono text-[var(--faint)]">
                        F0: {t.frame0LatencyMs}ms
                      </span>
                    )}
                    {t.frame1LatencyMs !== undefined && t.frame1LatencyMs !== null && (
                      <span className="font-mono text-emerald-400">
                        F1: {t.frame1LatencyMs}ms
                      </span>
                    )}
                    <span
                      className={`font-semibold ${
                        t.success
                          ? 'text-emerald-400'
                          : t.attempted
                          ? 'text-rose-400'
                          : 'text-[var(--faint)]'
                      }`}
                    >
                      {t.success ? '成功' : t.attempted ? '失败' : '跳过'}
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
          {countdown !== null ? `切回游戏 (${countdown}s)…` : '3 秒倒计时测试截屏'}
        </Button>

        <Button
          size="sm"
          disabled={loading || countdown !== null}
          onClick={() => void executeTestCapture()}
          className="h-7 text-[11px]"
        >
          <Camera className="h-3 w-3" />
          立即截屏
        </Button>

        <Button
          size="sm"
          disabled={!diagnostics}
          onClick={() => void handleCopyMarkdown()}
          className="h-7 text-[11px]"
        >
          {copied ? <Check className="h-3 w-3 text-emerald-400" /> : <Copy className="h-3 w-3" />}
          {copied ? '已复制 Markdown' : '一键复制诊断日志'}
        </Button>

        {diagnostics?.savedImagePath && (
          <Button
            size="sm"
            onClick={() => void handleOpenImage()}
            className="h-7 text-[11px]"
          >
            <ExternalLink className="h-3 w-3" />
            查看截屏图片
          </Button>
        )}

        <Button
          size="sm"
          disabled={loading}
          onClick={() => void fetchDiagnostics()}
          className="h-7 text-[11px] ml-auto"
        >
          <RefreshCw className={`h-3 w-3 ${loading ? 'animate-spin' : ''}`} />
          刷新
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
            <span>{showRaw ? '收起 Markdown 报告' : '展开 Markdown 报告'}</span>
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
          截图管线调试与诊断
        </h3>
      </div>
      {content}
    </div>
  );
};
