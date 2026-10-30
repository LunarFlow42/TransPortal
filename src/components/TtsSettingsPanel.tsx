import React, { useState } from 'react';
import {
  Download,
  Trash2,
  Play,
  CheckCircle2,
  AlertCircle,
  Loader2,
  Sparkles,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useAppStore } from '../store/useAppStore';
import { useTtsStatus } from '../hooks/useTts';
import { HoverTipText } from './ui';
import { cn } from '../utils/cn';

export const TtsSettingsPanel: React.FC<{ disabled?: boolean }> = ({ disabled }) => {
  const { t } = useTranslation();
  const { ttsEngine, setTtsEngine, ttsVoice, setTtsVoice, ttsSpeed, setTtsSpeed } = useAppStore();
  const { status, error, loading, progress, install, deleteTts, previewVoice } = useTtsStatus();
  const [isPlayingPreview, setIsPlayingPreview] = useState(false);

  const isInstalled = Boolean(status?.installed);
  const isInstalling = Boolean(status?.installing || progress !== null);

  const handleTest = () => {
    setIsPlayingPreview(true);
    previewVoice(ttsVoice, ttsSpeed);
    setTimeout(() => setIsPlayingPreview(false), 2500);
  };

  const formatMb = (bytes: number) => (bytes / (1024 * 1024)).toFixed(1);

  return (
    <div className={cn('flex flex-col gap-3', disabled && 'opacity-60 pointer-events-none')}>
      {/* 引擎切换 */}
      <div className="flex items-center justify-between gap-3 border-b border-[var(--stroke)] pb-2.5">
        <div className="flex items-center gap-1.5 text-[13px] font-medium">
          <Sparkles className="h-3.5 w-3.5 text-amber-400" />
          <HoverTipText
            text={t('settings.tts.engineTitle')}
            tip={t('settings.tts.engineDesc')}
            className="text-[13px] font-medium text-[var(--text)]"
          />
        </div>
        <div className="flex items-center gap-1 rounded-[8px] bg-[var(--surface-muted)] p-1 text-[11px]">
          <button
            type="button"
            onClick={() => setTtsEngine('kokoro_onnx')}
            className={cn(
              'rounded-[6px] px-2.5 py-1 font-medium transition-colors cursor-pointer',
              ttsEngine === 'kokoro_onnx'
                ? 'bg-[var(--accent-b)] text-white shadow-sm'
                : 'text-[var(--faint)] hover:text-[var(--text)]',
            )}
          >
            {t('settings.tts.engineKokoro')}
          </button>
          <button
            type="button"
            onClick={() => setTtsEngine('web_speech')}
            className={cn(
              'rounded-[6px] px-2.5 py-1 font-medium transition-colors cursor-pointer',
              ttsEngine === 'web_speech'
                ? 'bg-[var(--accent-b)] text-white shadow-sm'
                : 'text-[var(--faint)] hover:text-[var(--text)]',
            )}
          >
            {t('settings.tts.engineWebSpeech')}
          </button>
        </div>
      </div>

      {/* Kokoro ONNX 模型状态与控制 */}
      {ttsEngine === 'kokoro_onnx' && (
        <div className="flex flex-col gap-3">
          {/* 状态指示栏 */}
          <div className="flex items-center justify-between rounded-[8px] border border-[var(--stroke)] bg-[var(--surface-muted)]/50 p-2.5">
            <div className="flex items-center gap-2 text-[12px]">
              {isInstalling ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin text-[var(--accent-b)]" />
                  <span className="font-medium text-[var(--text)]">{t('settings.tts.downloading')}</span>
                </>
              ) : isInstalled ? (
                <>
                  <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                  <div>
                    <span className="font-medium text-[var(--text)]">{t('settings.tts.ready')}</span>
                    <span className="ml-1.5 text-[10px] text-[var(--faint)]">
                      {t('settings.tts.int8Quantized', { mb: formatMb(status?.installed_bytes ?? 0) })}
                    </span>
                  </div>
                </>
              ) : (
                <>
                  <AlertCircle className="h-4 w-4 text-amber-500" />
                  <div>
                    <span className="font-medium text-[var(--text)]">{t('settings.tts.notInstalled')}</span>
                    <span className="ml-1.5 text-[10px] text-[var(--faint)]">
                      {t('settings.tts.needDownload', { mb: formatMb(status?.download_bytes ?? 94972316) })}
                    </span>
                  </div>
                </>
              )}
            </div>

            {/* 操作按钮 */}
            <div>
              {isInstalling ? (
                <span className="text-[11px] font-mono text-[var(--accent-b)]">
                  {progress
                    ? `${((progress.downloaded / Math.max(progress.total, 1)) * 100).toFixed(0)}%`
                    : t('settings.tts.preparing')}
                </span>
              ) : isInstalled ? (
                <button
                  type="button"
                  onClick={() => void deleteTts()}
                  className="flex items-center gap-1 rounded-[6px] border border-rose-500/30 px-2 py-1 text-[11px] text-rose-400 transition-colors hover:bg-rose-500/10 hover:text-rose-300 cursor-pointer"
                  title={t('settings.tts.uninstallTitle')}
                >
                  <Trash2 className="h-3 w-3" />
                  <span>{t('settings.tts.uninstall')}</span>
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => void install()}
                  disabled={loading}
                  className="flex items-center gap-1 rounded-[6px] bg-[var(--accent-b)] px-3 py-1 text-[11px] font-medium text-white shadow-sm transition-opacity hover:opacity-90 cursor-pointer"
                >
                  <Download className="h-3 w-3" />
                  <span>{t('settings.tts.downloadInstall')}</span>
                </button>
              )}
            </div>
          </div>

          {/* 进度条 */}
          {isInstalling && progress && (
            <div className="flex flex-col gap-1">
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--surface-muted)]">
                <div
                  className="h-full bg-[var(--accent-b)] transition-all duration-200"
                  style={{
                    width: `${Math.min(
                      100,
                      (progress.downloaded / Math.max(progress.total, 1)) * 100,
                    )}%`,
                  }}
                />
              </div>
              <div className="flex justify-between text-[10px] font-mono text-[var(--faint)]">
                <span>{progress.file || t('settings.tts.downloadingWeights')}</span>
                <span>
                  {formatMb(progress.downloaded)} / {formatMb(progress.total)} MB
                </span>
              </div>
            </div>
          )}

          {error && (
            <p className="rounded-[6px] bg-rose-500/10 p-2 text-[11px] text-rose-400">
              {error}
            </p>
          )}

          {/* 发音人与语速调节 */}
          <div className="grid grid-cols-2 gap-3 pt-1">
            {/* 发音人 */}
            <div className="flex flex-col gap-1">
              <HoverTipText
                text={t('settings.tts.voiceLabel')}
                tip={t('settings.tts.voiceHint')}
                className="text-[11px] font-medium text-[var(--faint)]"
              />
              <select
                value={ttsVoice}
                onChange={(e) => setTtsVoice(e.target.value)}
                className="rounded-[6px] border border-[var(--stroke)] bg-[var(--surface-muted)] px-2.5 py-1.5 text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent-b)] cursor-pointer"
              >
                {status?.voices && status.voices.length > 0 ? (
                  status.voices.map((v) => (
                    <option key={v.id} value={v.id}>
                      {t(`settings.tts.voices.${v.id}`, { defaultValue: v.label })}
                    </option>
                  ))
                ) : (
                  <>
                    <option value="af_heart">{t('settings.tts.voices.af_heart')}</option>
                    <option value="af_bella">{t('settings.tts.voices.af_bella')}</option>
                    <option value="am_adam">{t('settings.tts.voices.am_adam')}</option>
                    <option value="bf_emma">{t('settings.tts.voices.bf_emma')}</option>
                    <option value="bm_george">{t('settings.tts.voices.bm_george')}</option>
                  </>
                )}
              </select>
            </div>

            {/* 语速 */}
            <div className="flex flex-col gap-1">
              <div className="flex items-center justify-between text-[11px]">
                <span className="font-medium text-[var(--faint)]">{t('settings.tts.speedLabel')}</span>
                <span className="font-mono text-[var(--text)]">{ttsSpeed.toFixed(1)}x</span>
              </div>
              <input
                type="range"
                min="0.6"
                max="1.6"
                step="0.1"
                value={ttsSpeed}
                onChange={(e) => setTtsSpeed(parseFloat(e.target.value))}
                className="h-1.5 w-full cursor-pointer appearance-none rounded-lg bg-[var(--stroke)] accent-[var(--accent-b)]"
              />
            </div>
          </div>

          {/* 试听按钮与轻量悬浮提示 */}
          <div className="flex items-center justify-between border-t border-[var(--stroke)] pt-2.5">
            <HoverTipText
              text={t('settings.tts.fallbackHint')}
              tip={t('settings.tts.engineDesc')}
              className="text-[11px] text-[var(--faint)] select-none font-normal"
            />
            <button
              type="button"
              onClick={handleTest}
              disabled={isPlayingPreview}
              className="flex items-center gap-1.5 rounded-[6px] border border-[var(--stroke)] bg-[var(--surface-muted)] px-3 py-1 text-[11px] font-medium text-[var(--text)] transition-colors hover:bg-[var(--surface-hover)] cursor-pointer"
            >
              {isPlayingPreview ? (
                <Loader2 className="h-3 w-3 animate-spin text-[var(--accent-b)]" />
              ) : (
                <Play className="h-3 w-3 text-[var(--accent-b)]" />
              )}
              <span>{t('settings.tts.previewVoice')}</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
