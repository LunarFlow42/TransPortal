import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { getVersion } from '@tauri-apps/api/app';
import { useTranslation } from 'react-i18next';
import {
  ExternalLink,
  CheckCircle2,
  AlertCircle,
  RefreshCw,
  Sparkles,
  Heart,
  Download,
  Info,
  Layers,
} from 'lucide-react';
import appIcon from '../assets/app-icon.png';
import { Surface, Button } from './ui';
import type { UpdateCheckResult } from '../types';

const GithubIcon: React.FC<{ className?: string }> = ({ className }) => (
  <svg
    viewBox="0 0 24 24"
    width="16"
    height="16"
    stroke="currentColor"
    strokeWidth="2"
    fill="none"
    strokeLinecap="round"
    strokeLinejoin="round"
    className={className}
  >
    <path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4" />
    <path d="M9 18c-4.51 2-5-2-7-2" />
  </svg>
);

const REPO_URL = 'https://github.com/LunarFlow42/TransPortal';
const ISSUES_URL = 'https://github.com/LunarFlow42/TransPortal/issues';
const RELEASES_URL = 'https://github.com/LunarFlow42/TransPortal/releases';

export const AboutPanel: React.FC = () => {
  const { t } = useTranslation();
  const [version, setVersion] = useState('');
  const [status, setStatus] = useState<'idle' | 'checking' | 'latest' | 'available' | 'error'>('idle');
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [clickCount, setClickCount] = useState(0);
  const [devNotice, setDevNotice] = useState<string | null>(null);

  useEffect(() => {
    void getVersion().then(setVersion).catch(() => {});
  }, []);

  const handleVersionClick = () => {
    const next = clickCount + 1;
    if (next >= 5) {
      setClickCount(0);
      const current = localStorage.getItem('transportal_dev_mode') === 'true';
      const newState = !current;
      localStorage.setItem('transportal_dev_mode', newState.toString());
      window.dispatchEvent(new CustomEvent('transportal:dev-mode-changed', { detail: newState }));
      setDevNotice(newState ? t('settings.about.devModeEnabled') : t('settings.about.devModeDisabled'));
      setTimeout(() => setDevNotice(null), 3000);
    } else {
      setClickCount(next);
      if (next >= 2) {
        setDevNotice(t('settings.about.devModeSteps', { count: 5 - next }));
        setTimeout(() => setDevNotice(null), 1500);
      }
    }
  };

  const handleCheckUpdate = async (silent = false) => {
    setStatus('checking');
    setErrorMessage(null);
    try {
      const res = await invoke<UpdateCheckResult>('check_update');
      setUpdateInfo(res);
      if (res.has_update) {
        setStatus('available');
      } else {
        setStatus('latest');
      }
    } catch (err) {
      if (!silent) {
        setStatus('error');
        setErrorMessage(String(err));
      } else {
        setStatus('idle');
      }
    }
  };

  // 挂载时自动后台静默检查一次
  useEffect(() => {
    void handleCheckUpdate(true);
  }, []);

  const handleOpenLink = (url: string) => {
    void openUrl(url);
  };

  return (
    <div className="flex flex-col gap-5 p-1">
      {/* 软件主视标与信息 */}
      <Surface className="flex flex-col items-center justify-center p-6 text-center bg-gradient-to-b from-[var(--surface-muted)]/40 to-transparent">
        <div className="relative mb-3">
          <img
            src={appIcon}
            alt="TransPortal Logo"
            className="h-16 w-16 rounded-2xl object-cover shadow-[0_8px_24px_rgba(34,211,238,0.35)] ring-1 ring-white/10 select-none"
          />
          <span className="absolute -bottom-1 -right-1 flex h-5 w-5 items-center justify-center rounded-full bg-[var(--surface-2)] shadow-sm">
            <Sparkles className="h-3 w-3 text-amber-400" />
          </span>
        </div>

        <h2 className="text-lg font-bold tracking-tight text-[var(--text)]">TransPortal</h2>
        <p className="mt-1 max-w-sm text-xs text-[var(--muted)]">
          {t('settings.about.slogan', {
            defaultValue: '轻量桌面沉浸式划词翻译与离线多引擎工作台',
          })}
        </p>

        <div className="mt-3 flex items-center gap-2">
          <button
            type="button"
            onClick={handleVersionClick}
            title={import.meta.env.DEV ? 'Development Build' : 'TransPortal Version'}
            className="rounded-full border border-[var(--stroke)] bg-[var(--surface-muted)] px-3 py-1 font-mono text-xs font-semibold text-[var(--text)] shadow-xs hover:border-[var(--accent-b)] cursor-pointer transition-colors"
          >
            v{version || '...'}
          </button>
          {status === 'latest' && (
            <span className="flex items-center gap-1 text-[11px] font-medium text-emerald-400">
              <CheckCircle2 className="h-3.5 w-3.5" />
              {t('settings.about.isLatest', { defaultValue: '已是最新版' })}
            </span>
          )}
          {status === 'available' && (
            <span className="flex items-center gap-1 rounded-full bg-amber-500/10 px-2 py-0.5 text-[11px] font-medium text-amber-400 border border-amber-500/20">
              <Sparkles className="h-3 w-3" />
              {t('settings.about.newVersionBadge', {
                version: updateInfo?.latest_version,
                defaultValue: `新版本 v${updateInfo?.latest_version}`,
              })}
            </span>
          )}
        </div>
        {devNotice && (
          <p className="mt-2 text-[11px] font-medium text-[var(--accent-b)] bg-[var(--surface-2)] border border-[var(--stroke)] rounded px-2.5 py-1">
            {devNotice}
          </p>
        )}
      </Surface>

      {/* 版本与自动更新卡片 */}
      <Surface className="flex flex-col gap-3.5 p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <RefreshCw className={`h-4 w-4 text-[var(--accent-b)] ${status === 'checking' ? 'animate-spin' : ''}`} />
            <span className="text-xs font-semibold text-[var(--text)]">
              {t('settings.about.updateCheckTitle', { defaultValue: '软件更新检查' })}
            </span>
          </div>

          <Button
            variant="ghost"
            size="sm"
            disabled={status === 'checking'}
            onClick={() => void handleCheckUpdate(false)}
            className="text-[11px]"
          >
            {status === 'checking' ? (
              <span className="flex items-center gap-1.5">
                <RefreshCw className="h-3 w-3 animate-spin" />
                {t('settings.about.checking', { defaultValue: '正在检查...' })}
              </span>
            ) : (
              t('settings.about.checkButton', { defaultValue: '检查更新' })
            )}
          </Button>
        </div>

        {/* 发现新版本提示面板 */}
        {status === 'available' && updateInfo && (
          <div className="flex flex-col gap-2.5 rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3.5">
            <div className="flex items-start justify-between gap-2">
              <div>
                <p className="text-xs font-bold text-emerald-300">
                  {t('settings.about.updateFound', {
                    version: updateInfo.latest_version,
                    defaultValue: `发现新版本 v${updateInfo.latest_version}！`,
                  })}
                </p>
                {updateInfo.release_name && (
                  <p className="mt-0.5 text-[11px] font-medium text-[var(--text)]">
                    {updateInfo.release_name}
                  </p>
                )}
                {updateInfo.published_at && (
                  <p className="text-[10px] text-[var(--faint)]">
                    {t('settings.about.publishedAt', { defaultValue: '发布于' })}:{' '}
                    {new Date(updateInfo.published_at).toLocaleDateString()}
                  </p>
                )}
              </div>

              <div className="flex items-center gap-2">
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => handleOpenLink(updateInfo.download_url || updateInfo.release_url)}
                  className="flex items-center gap-1.5 text-[11px]"
                >
                  <Download className="h-3 w-3" />
                  {t('settings.about.downloadRelease', { defaultValue: '下载更新' })}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => handleOpenLink(updateInfo.release_url)}
                  className="text-[11px]"
                >
                  <ExternalLink className="h-3 w-3" />
                  {t('settings.about.viewNotes', { defaultValue: '更新日志' })}
                </Button>
              </div>
            </div>

            {updateInfo.release_notes && (
              <div className="max-h-32 overflow-y-auto rounded bg-[var(--surface-1)]/60 p-2 text-[11px] text-[var(--muted)] font-mono whitespace-pre-wrap select-text border border-[var(--stroke)]/40">
                {updateInfo.release_notes}
              </div>
            )}
          </div>
        )}

        {/* 已是最新版本 */}
        {status === 'latest' && (
          <div className="flex items-center gap-2 rounded-lg border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-400">
            <CheckCircle2 className="h-4 w-4 shrink-0" />
            <span>
              {t('settings.about.upToDateMessage', {
                version,
                defaultValue: `当前版本 (v${version}) 已是最新版本，无需更新。`,
              })}
            </span>
          </div>
        )}

        {/* 检查出错 */}
        {status === 'error' && errorMessage && (
          <div className="flex items-start gap-2 rounded-lg border border-rose-500/20 bg-rose-500/5 px-3 py-2 text-xs text-rose-300">
            <AlertCircle className="h-4 w-4 shrink-0 text-rose-400 mt-0.5" />
            <div className="flex flex-col gap-0.5">
              <span>{t('settings.about.checkFailed', { defaultValue: '检查更新时遇到错误' })}</span>
              <span className="text-[10px] text-[var(--faint)] font-mono">{errorMessage}</span>
            </div>
          </div>
        )}
      </Surface>

      {/* 开源社区与相关链接 */}
      <Surface className="flex flex-col gap-2 p-4">
        <span className="text-xs font-semibold text-[var(--text)] mb-1">
          {t('settings.about.linksTitle', { defaultValue: '项目与社区' })}
        </span>

        <button
          type="button"
          onClick={() => handleOpenLink(REPO_URL)}
          className="flex items-center justify-between rounded-md p-2 text-left transition-colors hover:bg-[var(--surface-muted)] cursor-pointer group"
        >
          <div className="flex items-center gap-2.5">
            <GithubIcon className="h-4 w-4 text-[var(--text)] group-hover:text-[var(--accent-b)]" />
            <div className="flex flex-col">
              <span className="text-xs font-medium text-[var(--text)]">GitHub Repository</span>
              <span className="text-[10px] text-[var(--faint)]">LunarFlow42/TransPortal</span>
            </div>
          </div>
          <ExternalLink className="h-3.5 w-3.5 text-[var(--faint)] group-hover:text-[var(--text)]" />
        </button>

        <button
          type="button"
          onClick={() => handleOpenLink(ISSUES_URL)}
          className="flex items-center justify-between rounded-md p-2 text-left transition-colors hover:bg-[var(--surface-muted)] cursor-pointer group border-t border-[var(--stroke)]/40 pt-2"
        >
          <div className="flex items-center gap-2.5">
            <Info className="h-4 w-4 text-[var(--text)] group-hover:text-[var(--accent-b)]" />
            <div className="flex flex-col">
              <span className="text-xs font-medium text-[var(--text)]">
                {t('settings.about.feedbackTitle', { defaultValue: '问题反馈与建议 (Issues)' })}
              </span>
              <span className="text-[10px] text-[var(--faint)]">
                {t('settings.about.feedbackDesc', { defaultValue: '提交 Bug 报告或功能改进建议' })}
              </span>
            </div>
          </div>
          <ExternalLink className="h-3.5 w-3.5 text-[var(--faint)] group-hover:text-[var(--text)]" />
        </button>

        <button
          type="button"
          onClick={() => handleOpenLink(RELEASES_URL)}
          className="flex items-center justify-between rounded-md p-2 text-left transition-colors hover:bg-[var(--surface-muted)] cursor-pointer group border-t border-[var(--stroke)]/40 pt-2"
        >
          <div className="flex items-center gap-2.5">
            <Layers className="h-4 w-4 text-[var(--text)] group-hover:text-[var(--accent-b)]" />
            <div className="flex flex-col">
              <span className="text-xs font-medium text-[var(--text)]">
                {t('settings.about.releaseHistoryTitle', { defaultValue: '版本发布记录 (Releases)' })}
              </span>
              <span className="text-[10px] text-[var(--faint)]">
                {t('settings.about.releaseHistoryDesc', { defaultValue: '查看各版本的详细改动日志' })}
              </span>
            </div>
          </div>
          <ExternalLink className="h-3.5 w-3.5 text-[var(--faint)] group-hover:text-[var(--text)]" />
        </button>
      </Surface>

      {/* 底部版权与声明 */}
      <div className="flex flex-col items-center justify-center gap-1 text-center text-[10px] text-[var(--faint)] py-2 select-none">
        <p>TransPortal · Open source under MIT License</p>
        <p className="flex items-center gap-1">
          Crafted with <Heart className="h-3 w-3 text-rose-500 fill-rose-500 inline" /> by LunarFlow42
        </p>
      </div>
    </div>
  );
};
