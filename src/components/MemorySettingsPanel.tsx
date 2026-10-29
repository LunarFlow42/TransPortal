import { useEffect, useState } from 'react';
import { ChevronDown, Cpu, RefreshCw, Sparkles, Check } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button, HoverTipText, Surface } from './ui';
import { fetchMemoryUsage, trimMemoryNow } from '../hooks/useMemoryManager';
import { cn } from '../utils/cn';
import type { MemoryReport, TrimResult } from '../types';

function formatMb(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function MemorySettingsPanel({ bare = false }: { bare?: boolean } = {}) {
  const { t } = useTranslation();
  const [report, setReport] = useState<MemoryReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [trimming, setTrimming] = useState(false);
  const [lastTrim, setLastTrim] = useState<TrimResult | null>(null);
  const [isExpanded, setIsExpanded] = useState(false);

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

  const content = (
    <Surface variant="inner" className="flex flex-col gap-3 p-3">
      {/* 顶部主指标：核心突出健康、真实的 Rust 主程序专有常驻内存 */}
      <div className="flex items-center justify-between">
        <div className="flex flex-col gap-0.5">
          <div className="flex items-center gap-1.5">
            <span className="text-[13px] font-semibold text-[var(--text)]">
              {t('settings.advanced.rustMemory')}
            </span>
            <span className="rounded-full bg-emerald-500/15 px-1.5 py-0.2 text-[9px] font-medium text-emerald-400">
              Native Rust
            </span>
          </div>
          <p className="text-[11px] text-[var(--muted)]">
            {t('settings.advanced.rustMemoryDesc')}
          </p>
        </div>

        <div className="flex items-center gap-2">
          <span className="font-mono text-base font-bold tabular-nums text-emerald-400">
            {report ? formatMb(report.main_working_set_bytes) : '-- MB'}
          </span>
          <button
            onClick={() => void loadUsage()}
            disabled={loading}
            title={t('common.retry')}
            className="grid h-6 w-6 place-items-center rounded-[var(--r-ctrl)] text-[var(--muted)] hover:bg-[var(--surface-2)] hover:text-[var(--text)] transition-colors disabled:opacity-40"
          >
            <RefreshCw className={cn('h-3 w-3', loading && 'animate-spin')} />
          </button>
        </div>
      </div>

      {/* 折叠区切换器 */}
      <div className="border-t border-[var(--stroke)] pt-2">
        <button
          type="button"
          onClick={() => setIsExpanded(!isExpanded)}
          className="flex w-full items-center justify-between py-1 text-left text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        >
          <HoverTipText
            text={isExpanded ? t('settings.advanced.collapseMemory') : t('settings.advanced.expandMemory')}
            tip={t('settings.advanced.memoryWorkingSetTooltip')}
            className="text-[11px]"
          />
          <ChevronDown
            className={cn('h-3.5 w-3.5 transition-transform duration-200', isExpanded && 'rotate-180')}
          />
        </button>

        {isExpanded && (
          <div className="mt-2.5 flex flex-col gap-2.5 animate-in fade-in">
            <div className="grid grid-cols-2 gap-2 text-xs">
              <div className="flex flex-col gap-0.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2">
                <HoverTipText
                  text={t('settings.advanced.totalWorkingSet')}
                  tip={t('settings.advanced.memoryWorkingSetTooltip')}
                  className="text-[10px] text-[var(--faint)]"
                />
                <span className="font-mono font-medium text-[var(--text)]">
                  {report ? formatMb(report.total_working_set_bytes) : '--'}
                </span>
              </div>

              <div className="flex flex-col gap-0.5 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] p-2">
                <span className="text-[10px] text-[var(--faint)]">
                  {t('settings.advanced.totalCommit')}
                </span>
                <span className="font-mono font-medium text-[var(--text)]">
                  {report ? formatMb(report.total_private_bytes) : '--'}
                </span>
              </div>
            </div>

            <p className="text-[10px] text-[var(--faint)] leading-relaxed">
              * 注：进程树总额包含 WebView2 渲染器与显存进程的预分配虚拟页及共享库映射，属于系统正常缓存机制。
            </p>
          </div>
        )}
      </div>

      {lastTrim && (
        <div className="flex items-center gap-2 rounded-[var(--r-ctrl)] border border-emerald-500/20 bg-emerald-500/10 px-2.5 py-1.5 text-xs text-emerald-400">
          <Check className="h-3.5 w-3.5 shrink-0" />
          <span>
            {t('settings.advanced.trimSuccess', {
              size: formatMb(lastTrim.trimmed_bytes),
              count: lastTrim.process_count,
            })}
          </span>
        </div>
      )}

      {/* 释放按钮 */}
      <div className="flex items-center justify-between border-t border-[var(--stroke)] pt-2.5">
        <p className="text-[10px] text-[var(--faint)]">
          失焦或闲置 3 秒时，系统会在后台自动执行双阶段物理内存修剪。
        </p>

        <Button
          size="sm"
          onClick={() => void handleTrim()}
          disabled={trimming}
          className="shrink-0 gap-1.5"
        >
          <Sparkles className="h-3 w-3" />
          <span>{trimming ? t('settings.advanced.trimming') : t('settings.advanced.trimMemory')}</span>
        </Button>
      </div>
    </Surface>
  );

  if (bare) return content;

  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center gap-1.5 px-0.5 text-xs font-semibold text-[var(--text)]">
        <Cpu className="h-3.5 w-3.5 text-[var(--accent-b)]" />
        <span>{t('settings.advanced.memory')}</span>
      </div>
      {content}
    </section>
  );
}
