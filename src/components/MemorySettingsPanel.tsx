import { useEffect, useState } from 'react';
import { Cpu, RefreshCw, Sparkles, Check } from 'lucide-react';
import { Button, Surface } from './ui';
import { fetchMemoryUsage, trimMemoryNow } from '../hooks/useMemoryManager';
import type { MemoryReport, TrimResult } from '../types';

function formatMb(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function MemorySettingsPanel() {
  const [report, setReport] = useState<MemoryReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [trimming, setTrimming] = useState(false);
  const [lastTrim, setLastTrim] = useState<TrimResult | null>(null);

  const loadUsage = async () => {
    setLoading(true);
    try {
      const data = await fetchMemoryUsage();
      setReport(data);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    void fetchMemoryUsage().then((data) => {
      if (!disposed) {
        setReport(data);
        setLoading(false);
      }
    });
    return () => {
      disposed = true;
    };
  }, []);

  const handleTrim = async () => {
    setTrimming(true);
    const result = await trimMemoryNow();
    if (result) {
      setLastTrim(result);
      await loadUsage();
    }
    setTrimming(false);
  };

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5 px-0.5 text-xs font-semibold text-[var(--text)]">
        <Cpu className="h-3.5 w-3.5 text-[var(--accent-b)]" />
        <span>System & Memory 系统与性能</span>
      </div>

      <Surface variant="inner" className="flex flex-col gap-3 p-3">
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-0.5">
            <span className="text-[13px] font-medium text-[var(--text)]">
              进程树物理内存 (Working Set)
            </span>
            <p className="text-[10px] text-[var(--faint)]">
              包含 TransPortal 主程序及 WebView2 渲染器、GPU 显存等全部子进程。
            </p>
          </div>

          <div className="flex items-center gap-2">
            <span className="font-mono text-sm font-semibold tabular-nums text-[var(--accent-b)]">
              {report ? formatMb(report.total_working_set_bytes) : '-- MB'}
            </span>
            <button
              onClick={() => void loadUsage()}
              disabled={loading}
              title="刷新内存状态"
              className="grid h-6 w-6 place-items-center rounded-[var(--r-ctrl)] text-[var(--muted)] hover:bg-[var(--surface-2)] hover:text-[var(--text)] transition-colors disabled:opacity-40"
            >
              <RefreshCw className={`h-3 w-3 ${loading ? 'animate-spin' : ''}`} />
            </button>
          </div>
        </div>

        <div className="grid grid-cols-2 gap-2 text-xs">
          <div className="flex flex-col gap-0.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-1)]/40 p-2">
            <span className="text-[10px] text-[var(--faint)]">Rust 主程序内存</span>
            <span className="font-mono font-medium text-[var(--text)]">
              {report ? formatMb(report.main_working_set_bytes) : '--'}
            </span>
          </div>

          <div className="flex flex-col gap-0.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-1)]/40 p-2">
            <span className="text-[10px] text-[var(--faint)]">总私有提交 (Commit)</span>
            <span className="font-mono font-medium text-[var(--text)]">
              {report ? formatMb(report.total_private_bytes) : '--'}
            </span>
          </div>
        </div>

        {lastTrim && (
          <div className="flex items-center gap-2 rounded-[var(--r-ctrl)] border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1.5 text-xs text-emerald-400">
            <Check className="h-3.5 w-3.5 shrink-0" />
            <span>
              已成功深度释放 <strong>{formatMb(lastTrim.trimmed_bytes)}</strong> 物理内存，当前占用{' '}
              <strong>{formatMb(lastTrim.after_bytes)}</strong>（覆盖 {lastTrim.process_count} 个进程）。
            </span>
          </div>
        )}

        <div className="flex items-center justify-between border-t border-[var(--stroke)] pt-2.5">
          <p className="text-[10px] text-[var(--faint)]">
            浮层窗口失去焦点或闲置满 3 秒时，系统会在后台自动执行双阶段修剪。
          </p>

          <Button
            size="sm"
            onClick={() => void handleTrim()}
            disabled={trimming}
            className="shrink-0 gap-1.5"
          >
            <Sparkles className="h-3 w-3" />
            <span>{trimming ? '正在释放...' : '立即深度释放'}</span>
          </Button>
        </div>
      </Surface>
    </section>
  );
}
