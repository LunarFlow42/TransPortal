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
import { useAppStore } from '../store/useAppStore';
import { useTtsStatus } from '../hooks/useTts';
import { cn } from '../utils/cn';

export const TtsSettingsPanel: React.FC<{ disabled?: boolean }> = ({ disabled }) => {
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
        <div className="flex flex-col gap-0.5">
          <div className="flex items-center gap-1.5 text-[13px] font-medium">
            <Sparkles className="h-3.5 w-3.5 text-amber-400" />
            <span>合成引擎</span>
          </div>
          <p className="text-[10px] text-[var(--faint)]">
            选择查词与翻译朗读时使用的语音合成后端。
          </p>
        </div>
        <div className="flex items-center gap-1 rounded-[8px] bg-[var(--surface-muted)] p-1 text-[11px]">
          <button
            type="button"
            onClick={() => setTtsEngine('kokoro_onnx')}
            className={cn(
              'rounded-[6px] px-2.5 py-1 font-medium transition-colors',
              ttsEngine === 'kokoro_onnx'
                ? 'bg-[var(--accent-b)] text-white shadow-sm'
                : 'text-[var(--faint)] hover:text-[var(--text)]',
            )}
          >
            Kokoro ONNX 本地神经网络
          </button>
          <button
            type="button"
            onClick={() => setTtsEngine('web_speech')}
            className={cn(
              'rounded-[6px] px-2.5 py-1 font-medium transition-colors',
              ttsEngine === 'web_speech'
                ? 'bg-[var(--accent-b)] text-white shadow-sm'
                : 'text-[var(--faint)] hover:text-[var(--text)]',
            )}
          >
            Web Speech 系统自带
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
                  <span className="font-medium text-[var(--text)]">正在下载并校验模型资源...</span>
                </>
              ) : isInstalled ? (
                <>
                  <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                  <div>
                    <span className="font-medium text-[var(--text)]">本地模型已就绪</span>
                    <span className="ml-1.5 text-[10px] text-[var(--faint)]">
                      INT8 量化 · ~{formatMb(status?.installed_bytes ?? 0)} MB
                    </span>
                  </div>
                </>
              ) : (
                <>
                  <AlertCircle className="h-4 w-4 text-amber-500" />
                  <div>
                    <span className="font-medium text-[var(--text)]">未安装本地模型</span>
                    <span className="ml-1.5 text-[10px] text-[var(--faint)]">
                      需下载约 {formatMb(status?.download_bytes ?? 94972316)} MB
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
                    : '准备中...'}
                </span>
              ) : isInstalled ? (
                <button
                  type="button"
                  onClick={() => void deleteTts()}
                  className="flex items-center gap-1 rounded-[6px] border border-rose-500/30 px-2 py-1 text-[11px] text-rose-400 transition-colors hover:bg-rose-500/10 hover:text-rose-300"
                  title="卸载模型文件释放空间"
                >
                  <Trash2 className="h-3 w-3" />
                  <span>卸载</span>
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => void install()}
                  disabled={loading}
                  className="flex items-center gap-1 rounded-[6px] bg-[var(--accent-b)] px-3 py-1 text-[11px] font-medium text-white shadow-sm transition-opacity hover:opacity-90"
                >
                  <Download className="h-3 w-3" />
                  <span>一键下载安装</span>
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
                <span>{progress.file || '正在下载权重与音色...'}</span>
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
              <label className="text-[11px] font-medium text-[var(--faint)]">发音人音色</label>
              <select
                value={ttsVoice}
                onChange={(e) => setTtsVoice(e.target.value)}
                className="rounded-[6px] border border-[var(--stroke)] bg-[var(--surface-muted)] px-2.5 py-1.5 text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent-b)]"
              >
                {status?.voices && status.voices.length > 0 ? (
                  status.voices.map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.label}
                    </option>
                  ))
                ) : (
                  <>
                    <option value="af_heart">Heart · 美音女声 · 推荐</option>
                    <option value="af_bella">Bella · 美音女声</option>
                    <option value="am_adam">Adam · 美音男声</option>
                    <option value="bf_emma">Emma · 英音女声</option>
                    <option value="bm_george">George · 英音男声</option>
                  </>
                )}
              </select>
            </div>

            {/* 语速 */}
            <div className="flex flex-col gap-1">
              <div className="flex items-center justify-between text-[11px]">
                <span className="font-medium text-[var(--faint)]">语速</span>
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

          {/* 试听按钮 */}
          <div className="flex items-center justify-between border-t border-[var(--stroke)] pt-2.5">
            <span className="text-[11px] text-[var(--faint)]">
              非英语或中文字符朗读将自动平滑降级至系统发音。
            </span>
            <button
              type="button"
              onClick={handleTest}
              disabled={isPlayingPreview}
              className="flex items-center gap-1.5 rounded-[6px] border border-[var(--stroke)] bg-[var(--surface-muted)] px-3 py-1 text-[11px] font-medium text-[var(--text)] transition-colors hover:bg-[var(--surface-hover)]"
            >
              {isPlayingPreview ? (
                <Loader2 className="h-3 w-3 animate-spin text-[var(--accent-b)]" />
              ) : (
                <Play className="h-3 w-3 text-[var(--accent-b)]" />
              )}
              <span>试听发音</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
};
