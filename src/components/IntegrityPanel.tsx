import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Copy, RefreshCw, ShieldCheck } from 'lucide-react';
import { Button, Section, Surface } from './ui';
import type { IntegrityReport } from '../types';

export const IntegrityPanel: React.FC<{ bare?: boolean }> = ({ bare = false }) => {
  const [report, setReport] = useState<IntegrityReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    let disposed = false;
    void invoke<IntegrityReport>('integrity_status').then((report) => {
      if (!disposed) setReport(report);
    }).catch((error) => { if (!disposed) setError(String(error)); });
    return () => { disposed = true; };
  }, []);

  const recheck = async () => {
    setChecking(true);
    setError(null);
    try { setReport(await invoke<IntegrityReport>('verify_integrity')); }
    catch (error) { setError(String(error)); }
    finally { setChecking(false); }
  };
  const copyHash = async () => {
    try { await invoke('plugin:clipboard-manager|write_text', { text: report?.actual_sha256 ?? '' }); }
    catch (error) { setError(String(error)); }
  };

  const content = (
    <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
      <p role="status" className="text-[13px]">
        {report?.status === 'verified' ? 'SHA-256 与发布校验值一致'
          : report?.status === 'development' ? '开发构建 · 未执行发布校验'
          : report?.status === 'unsupported' ? '当前平台尚未启用启动自校验'
          : report?.error ?? '正在读取校验结果…'}
      </p>
      {report?.actual_sha256 && (
        <details className="text-[11px] text-[var(--muted)]">
          <summary className="cursor-pointer">查看 SHA-256</summary>
          <code className="mt-2 block select-text break-all leading-relaxed">{report.actual_sha256}</code>
        </details>
      )}
      <p className="text-[10px] leading-relaxed text-[var(--faint)]">
        用于检测程序文件变化。未签名的校验文件不能验证发布者身份，也不代替病毒扫描。
      </p>
      <div className="flex gap-2">
        <Button size="sm" disabled={checking || report?.status !== 'verified'} onClick={() => void recheck()}>
          <RefreshCw className={`h-3 w-3 ${checking ? 'animate-spin' : ''}`} />重新校验
        </Button>
        {report?.actual_sha256 && <Button size="sm" onClick={() => void copyHash()}><Copy className="h-3 w-3" />复制哈希</Button>}
      </div>
      {error && <p role="alert" className="text-[11px] text-[var(--muted)]">{error}</p>}
    </Surface>
  );

  if (bare) return content;

  return (
    <Section label="程序完整性" icon={<ShieldCheck className="h-3.5 w-3.5" />}>
      {content}
    </Section>
  );
};
