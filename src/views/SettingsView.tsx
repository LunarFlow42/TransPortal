import React, { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  disable as autostartDisable,
  enable as autostartEnable,
  isEnabled as autostartIsEnabled,
} from '@tauri-apps/plugin-autostart';
import {
  BookOpen,
  Keyboard,
  Languages,
  Minus,
  Palette,
  Power,
  Volume2,
  X,
  RefreshCw,
  AlertCircle,
  Search,
  MousePointerClick,
  AppWindow,
} from 'lucide-react';
import { useBootstrap } from '../hooks/useBootstrap';
import { useHideOnBlur } from '../hooks/useHideOnBlur';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { saveSettings } from '../store/persistence';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { DictionaryPanel } from '../components/DictionaryPanel';
import { OcrSettingsPanel } from '../components/OcrSettingsPanel';
import { IntegrityPanel } from '../components/IntegrityPanel';
import { MemorySettingsPanel } from '../components/MemorySettingsPanel';
import { Button, IconButton, Input, Section, Select, Surface, Switch } from '../components/ui';
import { cn } from '../utils/cn';
import type { ProviderType, ShortcutActionId, ThemeMode } from '../types';

const THEME_OPTIONS = [
  { value: 'system', label: 'Follow system' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

const PROVIDER_OPTIONS = [
  { value: 'google_free', label: 'Google Free (zero config)' },
  { value: 'youdao_free', label: 'Youdao Free 有道翻译 (zero config)' },
  { value: 'tencent_free', label: 'Tencent Free 腾讯翻译君 (zero config)' },
  { value: 'openai', label: 'OpenAI Compatible (BYOK SSE)' },
];

const TARGET_LANGUAGE_OPTIONS = [
  { value: 'zh-CN', label: '简体中文 (Simplified Chinese)' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語 (Japanese)' },
  { value: 'ko', label: '한국어 (Korean)' },
  { value: 'zh-TW', label: '繁體中文 (Traditional Chinese)' },
  { value: 'fr', label: 'Français (French)' },
  { value: 'de', label: 'Deutsch (German)' },
  { value: 'es', label: 'Español (Spanish)' },
  { value: 'ru', label: 'Русский (Russian)' },
];

const INK_KEY_OPTIONS = [
  { value: 'None', label: '已禁用 (None)' },
  { value: 'Control', label: 'Ctrl 键' },
  { value: 'Alt', label: 'Alt 键' },
  { value: 'Shift', label: 'Shift 键' },
  { value: 'CapsLock', label: 'Caps Lock 大写锁定' },
];

const SHORTCUT_ITEMS: {
  id: ShortcutActionId;
  label: string;
  description: string;
}[] = [
  {
    id: 'selection',
    label: '划词查询',
    description: '划选屏幕文字后即时查词与翻译',
  },
  {
    id: 'replace_selection',
    label: '替换翻译',
    description: '划选屏幕文字后原地静默极速覆盖译文',
  },
  {
    id: 'ocr_card',
    label: '截图对照翻译',
    description: '框选屏幕长文，图文双栏对照并支持点词查词',
  },
  {
    id: 'ocr_picture',
    label: '原图直接翻译',
    description: '框选屏幕图像，在原图文字位置覆盖译文',
  },
  {
    id: 'ocr_text',
    label: '提取文字',
    description: '框选屏幕文字，识别后自动复制到剪贴板',
  },
  {
    id: 'silent_ocr',
    label: '静默截图 OCR',
    description: '截图识别文字并直接存入剪贴板，后台零视觉干扰',
  },
  {
    id: 'input_translate',
    label: '输入翻译',
    description: '快速呼出输入框，手动输入或粘贴文字即时翻译',
  },
];

const IN_WINDOW_SHORTCUTS = [
  { key: 'Ctrl+A', desc: '打开设置窗口 (非输入焦点时)' },
  { key: 'Tab', desc: '原图与翻译后图片切换 (短按切换 / 长按透视)' },
  { key: 'Ctrl+Shift+T', desc: '固定 / 取消置顶当前浮层' },
  { key: 'Ctrl+R', desc: '重新识别 / 重新翻译当前文本' },
  { key: 'Ctrl+P', desc: '历史记录：查看上一条' },
  { key: 'Ctrl+N', desc: '历史记录：查看下一条' },
  { key: 'Esc', desc: '退出识别 / 清除 / 隐藏窗口' },
];

/**
 * `#/settings` — the dedicated borderless settings window.
 *
 * Rendered in its own always-available window with a self-drawn titlebar. The
 * window is transparent; the rounded glass panel is drawn in CSS so corners
 * stay correct on every platform.
 */
export const SettingsView: React.FC = () => {
  const {
    provider,
    setProvider,
    openaiConfig,
    setOpenaiConfig,
    hotkey,
    ocrCardHotkey,
    ocrPictureHotkey,
    ocrTextHotkey,
    replaceSelectionHotkey,
    silentOcrHotkey,
    inputTranslateHotkey,
    inkTranslateKey,
    setInkTranslateKey,
    clipboardMonitorEnabled,
    setClipboardMonitorEnabled,
    setShortcut,
    doubleCopyEnabled,
    setDoubleCopyEnabled,
    fontSize,
    setFontSize,
    showCrosshairs,
    setShowCrosshairs,
    autoPronounce,
    setAutoPronounce,
    targetLanguage,
    setTargetLanguage,
    settingsError,
    theme,
    setTheme,
    overlayOpacity,
    setOverlayOpacity,
    smoothTransitions,
    setSmoothTransitions,
  } = useAppStore();
  const [recordingAction, setRecordingAction] = useState<ShortcutActionId | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');

  const { report, error: shortcutError, retrying, retry } = useShortcutStatus();

  useHideOnBlur(!recordingAction);
  const settingsReady = useBootstrap(true);

  // Hide window on close to enable instant 0ms reopening without rebuilding WebView2.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !recordingAction) {
        event.preventDefault();
        void getCurrentWindow().hide();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [recordingAction]);

  useEffect(() => {
    let disposed = false;
    void autostartIsEnabled()
      .then((enabled) => {
        if (!disposed) setAutostart(enabled);
      })
      .catch((error) => console.error('autostart check failed:', error));
    return () => {
      disposed = true;
    };
  }, []);

  const toggleAutostart = async () => {
    try {
      if (autostart) {
        await autostartDisable();
      } else {
        await autostartEnable();
      }
      const next = !autostart;
      setAutostart(next);
      await saveSettings({ autostart: next });
    } catch (error) {
      console.error('autostart toggle failed:', error);
    }
  };

  const getShortcutValue = (id: ShortcutActionId) => {
    switch (id) {
      case 'selection':
        return hotkey;
      case 'replace_selection':
        return replaceSelectionHotkey;
      case 'ocr_card':
        return ocrCardHotkey;
      case 'ocr_picture':
        return ocrPictureHotkey;
      case 'ocr_text':
        return ocrTextHotkey;
      case 'silent_ocr':
        return silentOcrHotkey;
      case 'input_translate':
        return inputTranslateHotkey;
      default:
        return '';
    }
  };

  const onShortcutKeyDown = (action: ShortcutActionId, event: React.KeyboardEvent<HTMLInputElement>) => {
    event.preventDefault();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;

    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push('Ctrl');
    if (event.altKey) modifiers.push('Alt');
    if (event.shiftKey) modifiers.push('Shift');
    if (event.metaKey) modifiers.push('Super');

    const { code } = event;
    const mainKey = code.startsWith('Key')
      ? code.slice(3)
      : code.startsWith('Digit')
        ? code.slice(5)
        : code;

    const needsModifier = !mainKey.startsWith('F');
    if ((needsModifier && modifiers.length === 0) || !mainKey) return;

    void setShortcut(action, [...modifiers, mainKey].join('+'));
    setRecordingAction(null);
  };

  const isMatch = (terms: string[]) => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return true;
    return terms.some((t) => t.toLowerCase().includes(q));
  };

  return (
    <div className="h-screen w-screen bg-transparent p-2">
      <div className="window-root surface-hairline relative flex h-full w-full flex-col overflow-hidden rounded-[var(--r-win)] text-[var(--text)]">
        <header
          onMouseDown={startWindowDrag}
          className="flex shrink-0 cursor-move items-center justify-between border-b border-[var(--stroke)] px-3.5 py-3"
        >
          <div className="flex items-center gap-2 text-[13px] font-semibold">
            <span className="grid h-5 w-5 place-items-center rounded-[5px] bg-gradient-to-br from-[var(--accent-a)] to-[var(--accent-b)] shadow-[0_4px_12px_rgba(34,211,238,0.35)]">
              <Languages className="h-3 w-3 text-white" />
            </span>
            TransPortal Settings
          </div>
          <div className="flex items-center gap-0.5">
            <IconButton title="Minimize" onClick={() => void getCurrentWindow().minimize()}>
              <Minus className="h-3.5 w-3.5" />
            </IconButton>
            <IconButton
              title="Close"
              onClick={() => void getCurrentWindow().hide()}
              className="hover:bg-rose-500/90 hover:text-white"
            >
              <X className="h-3.5 w-3.5" />
            </IconButton>
          </div>
        </header>

        {settingsError && (
          <p role="alert" className="shrink-0 border-b border-rose-500/30 bg-rose-500/10 px-4 py-2 text-xs text-[var(--text)]">
            {settingsError}
          </p>
        )}

        <div className="flex flex-1 flex-col gap-4 overflow-y-auto p-4">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-2.5 h-3.5 w-3.5 text-[var(--muted)]" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="搜索设置项、快捷键或功能..."
              className="w-full rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-1)] pl-8 pr-3 py-1.5 text-xs text-[var(--text)] placeholder-[var(--faint)] outline-none transition-colors focus:border-[var(--accent-b)] focus:bg-[var(--surface-2)]"
            />
          </div>

          {isMatch(['appearance', '外观', '界面', 'theme', '主题', 'opacity', '透明度', '动效', '动画', '过渡', '平滑', 'transition', 'animation', 'font', '字号', 'crosshairs', '对齐']) && (
            <Section label="Appearance 外观与界面" icon={<Palette className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
                <div className="flex items-center gap-3">
                  <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">主题模式</label>
                  <Select
                    ariaLabel="Theme"
                    value={theme}
                    onChange={(value) => setTheme(value as ThemeMode)}
                    options={THEME_OPTIONS}
                  />
                </div>
                <div className="flex items-center gap-3">
                  <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">
                    窗口透明度
                  </label>
                  <input
                    type="range"
                    min={25}
                    max={100}
                    step={1}
                    value={Math.round(overlayOpacity * 100)}
                    onChange={(e) => setOverlayOpacity(Number(e.target.value) / 100)}
                    className="flex-1 cursor-pointer accent-[var(--accent-a)]"
                  />
                  <span className="w-9 shrink-0 text-right text-[11px] tabular-nums text-[var(--muted)]">
                    {Math.round(overlayOpacity * 100)}%
                  </span>
                </div>
                <div className="flex items-center gap-3">
                  <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">字号大小</label>
                  <input
                    type="range"
                    min={12}
                    max={20}
                    step={1}
                    value={fontSize}
                    onChange={(e) => setFontSize(Number(e.target.value))}
                    className="flex-1 cursor-pointer accent-[var(--accent-a)]"
                  />
                  <span className="w-9 shrink-0 text-right text-[11px] tabular-nums text-[var(--muted)]">
                    {fontSize}px
                  </span>
                </div>
                <p className="text-[10px] text-[var(--faint)]">
                  仅影响查词浮层（设置窗口固定不透明）。
                </p>

                <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                  <div className="flex flex-col gap-0.5">
                    <span className="text-[13px] font-medium">平滑进出过渡动效</span>
                    <p className="text-[10px] text-[var(--faint)]">
                      浮层唤醒时 120ms 阻尼微弹簧展开，收起时 100ms 优雅淡出，消除首帧闪烁。
                    </p>
                  </div>
                  <Switch
                    checked={smoothTransitions}
                    onChange={() => setSmoothTransitions(!smoothTransitions)}
                    label="平滑进出过渡动效"
                  />
                </div>

                <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                  <div className="flex flex-col gap-0.5">
                    <span className="text-[13px] font-medium">截图十字对齐辅助线</span>
                    <p className="text-[10px] text-[var(--faint)]">
                      在截图取词与框选时显示 1px 细十字对齐辅助线，辅助像素级对齐。
                    </p>
                  </div>
                  <Switch
                    checked={showCrosshairs}
                    onChange={() => setShowCrosshairs(!showCrosshairs)}
                    label="截图十字对齐辅助线"
                  />
                </div>
              </Surface>
            </Section>
          )}

          {isMatch(['provider', '翻译', 'engine', 'openai', 'youdao', 'google', 'tencent', 'target', '语言', '有道', '腾讯']) && (
            <Section label="Translation Provider 翻译引擎" icon={<Languages className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
                <div className="flex items-center gap-3">
                  <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">引擎服务</label>
                  <Select
                    ariaLabel="Translation engine"
                    value={provider}
                    onChange={(value) => setProvider(value as ProviderType)}
                    options={PROVIDER_OPTIONS}
                  />
                </div>

                <div className="flex items-center gap-3">
                  <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">默认目标语言</label>
                  <Select
                    ariaLabel="Target language"
                    value={targetLanguage}
                    onChange={(value) => setTargetLanguage(value)}
                    options={TARGET_LANGUAGE_OPTIONS}
                  />
                </div>
                <p className="text-[10px] text-[var(--faint)]">
                  输入与目标语言相同时将自动对调互译（如默认中文，输入中文时自动译为英文）。
                </p>

                {provider === 'openai' ? (
                  <>
                    <div className="flex items-center gap-3">
                      <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">
                        Base URL
                      </label>
                      <Input
                        type="text"
                        value={openaiConfig.base_url || ''}
                        onChange={(e) => setOpenaiConfig({ base_url: e.target.value })}
                        placeholder="https://api.openai.com/v1"
                      />
                    </div>
                    <div className="flex items-center gap-3">
                      <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">
                        API Key
                      </label>
                      <Input
                        type="password"
                        value={openaiConfig.api_key}
                        onChange={(e) => setOpenaiConfig({ api_key: e.target.value })}
                        placeholder="sk-..."
                      />
                    </div>
                    <div className="flex items-center gap-3">
                      <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">Model</label>
                      <Input
                        type="text"
                        value={openaiConfig.model || ''}
                        onChange={(e) => setOpenaiConfig({ model: e.target.value })}
                        placeholder="gpt-4o-mini"
                      />
                    </div>
                  </>
                ) : (
                  <p className="text-[10px] text-[var(--faint)]">
                    免密钥接口，开箱即用；有道 / 腾讯均支持自动识别源语言。
                  </p>
                )}
              </Surface>
            </Section>
          )}

          {isMatch(['global', 'shortcuts', '全局', '快捷键', '热键', '划词', '替换', 'ocr', '截图', '提取文字', '静默', '输入翻译']) && (
            <Section label="Global Shortcuts 全局快捷键" icon={<Keyboard className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-3 p-3">
                <div className="flex flex-col gap-2.5">
                  {SHORTCUT_ITEMS.filter((item) => !searchQuery.trim() || isMatch([item.label, item.description, item.id])).map((item) => {
                    const status = report?.shortcuts.find((s) => s.id === item.id);
                    const isRecording = recordingAction === item.id;
                    const currentVal = getShortcutValue(item.id);

                    return (
                      <div
                        key={item.id}
                        className="flex flex-col gap-1 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-1)]/40 p-2.5"
                      >
                        <div className="flex items-center justify-between gap-3">
                          <div className="flex flex-col gap-0.5">
                            <div className="flex items-center gap-2">
                              <span className="text-[13px] font-medium text-[var(--text)]">
                                {item.label}
                              </span>
                              {status?.active && (
                                <span className="text-[10px] text-emerald-400 font-mono">
                                  {status.system ? '系统监听' : '已就绪'}
                                </span>
                              )}
                            </div>
                            <span className="text-[11px] text-[var(--muted)]">
                              {item.description}
                            </span>
                          </div>

                          <div className="w-36 shrink-0">
                            <Input
                              type="text"
                              readOnly
                              disabled={!settingsReady}
                              value={isRecording ? '按下组合键…' : currentVal}
                              onFocus={() => setRecordingAction(item.id)}
                              onBlur={() => {
                                if (recordingAction === item.id) setRecordingAction(null);
                              }}
                              onKeyDown={(e) => onShortcutKeyDown(item.id, e)}
                              placeholder="点击录制"
                              className={cn(
                                'text-center font-mono text-xs cursor-pointer select-none',
                                isRecording
                                  ? 'border-[rgba(99,102,241,0.75)] shadow-[0_0_0_3px_rgba(99,102,241,0.22)]'
                                  : 'hover:border-[var(--stroke-hover)]',
                              )}
                            />
                          </div>
                        </div>

                        {status?.error && (
                          <div className="mt-1 flex items-center gap-1.5 text-[11px] text-amber-400">
                            <AlertCircle className="h-3.5 w-3.5 shrink-0" />
                            <span>{status.error}</span>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>

                {shortcutError && (
                  <p className="text-xs text-rose-400 border-t border-[var(--stroke)] pt-1.5">
                    {shortcutError}
                  </p>
                )}

                <div className="flex items-center justify-between gap-2 border-t border-[var(--stroke)] pt-2 text-[10px] text-[var(--faint)]">
                  <span>
                    点击输入框后直接按下组合键（需包含 Ctrl / Alt / Shift / Super，或 F1–F24）。
                  </span>
                  <Button
                    size="sm"
                    variant="subtle"
                    disabled={retrying || !report?.ready}
                    onClick={() => void retry()}
                    className="h-6 text-[10px] shrink-0"
                  >
                    <RefreshCw className={cn('h-3 w-3', retrying && 'animate-spin')} />
                    重新检测快捷键
                  </Button>
                </div>
              </Surface>
            </Section>
          )}

          {isMatch(['gestures', 'monitoring', '手势', '监听', '落墨', '划选', '复制', 'ctrl+c+c', '剪贴板', 'clipboard', 'ink']) && (
            <Section label="Gestures & Monitoring 手势与监听" icon={<MousePointerClick className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-3 p-3">
                {/* 剪贴板监听翻译 */}
                <div className="flex items-center justify-between gap-3">
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium text-[var(--text)]">
                        剪贴板监听翻译
                      </span>
                      <span className="text-[10px] text-[var(--muted)] font-mono">自动弹窗</span>
                    </div>
                    <p className="text-[11px] text-[var(--muted)]">
                      复制外部文本时自动呼出翻译浮层（智能过滤应用自身复制与空文本）。
                    </p>
                  </div>
                  <Switch
                    checked={clipboardMonitorEnabled}
                    onChange={(checked) => void setClipboardMonitorEnabled(checked)}
                    label="启用剪贴板监听翻译"
                  />
                </div>

                {/* 落墨翻译按键 */}
                <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium text-[var(--text)]">
                        落墨取词翻译
                      </span>
                      <span className="text-[10px] text-[var(--muted)] font-mono">长按划选</span>
                    </div>
                    <p className="text-[11px] text-[var(--muted)]">
                      按住单键划选鼠标，松开按键瞬间快速触发取词与翻译。
                    </p>
                  </div>
                  <div className="w-36 shrink-0">
                    <Select
                      ariaLabel="落墨翻译单键"
                      value={inkTranslateKey}
                      onChange={(value) => void setInkTranslateKey(value)}
                      options={INK_KEY_OPTIONS}
                    />
                  </div>
                </div>

                {/* 快捷翻译双击复制开关 */}
                <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-2">
                      <span className="text-[13px] font-medium text-[var(--text)]">
                        快捷翻译 (Ctrl+C+C)
                      </span>
                      <span className="text-[10px] text-[var(--muted)] font-mono">双击复制</span>
                    </div>
                    <p className="text-[11px] text-[var(--muted)]">
                      选中文字，按住 Ctrl，并在 0.5 秒内连按两次 C 触发翻译。
                    </p>
                  </div>
                  <Switch
                    checked={doubleCopyEnabled}
                    onChange={(checked) => void setDoubleCopyEnabled(checked)}
                    label="启用 Ctrl+C+C 快捷翻译"
                  />
                </div>
              </Surface>
            </Section>
          )}

          {isMatch(['in-window', 'window', '软件快捷键', '内置', '窗口快捷键', 'tab', 'ctrl+a', 'ctrl+r', '置顶']) && (
            <Section label="In-Window Shortcuts 软件快捷键" icon={<AppWindow className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-2 p-3">
                <p className="text-[11px] text-[var(--muted)] mb-1">
                  焦点处于查词/翻译浮层窗口内时的便捷键盘操作：
                </p>
                <div className="grid grid-cols-2 gap-2">
                  {IN_WINDOW_SHORTCUTS.map((sc) => (
                    <div
                      key={sc.key}
                      className="flex items-center justify-between rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-1)]/40 px-2.5 py-1.5 text-xs"
                    >
                      <span className="text-[var(--muted)] text-[11px]">{sc.desc}</span>
                      <kbd className="rounded-[4px] border border-[var(--stroke)] bg-[var(--surface-2)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--text)]">
                        {sc.key}
                      </kbd>
                    </div>
                  ))}
                </div>
              </Surface>
            </Section>
          )}

          {isMatch(['ocr', '文字识别', 'windows ocr', 'onnx', 'paddle', '语言', '引擎', '原图', '译图', '图片展示', '覆写']) && (
            <OcrSettingsPanel disabled={!settingsReady} />
          )}

          {isMatch(['startup', '开机启动', '自启', 'launch']) && (
            <Section label="Startup 开机启动" icon={<Power className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex items-center justify-between gap-3 p-3">
                <span className="text-[13px]">开机自动运行 TransPortal</span>
                <Switch
                  checked={autostart}
                  onChange={() => void toggleAutostart()}
                  label="Autostart"
                />
              </Surface>
            </Section>
          )}

          {isMatch(['dictionary', '词典', '发音', 'audio', 'pronounce', 'ecdict']) && (
            <Section label="Dictionary 本地与在线词典" icon={<BookOpen className="h-3.5 w-3.5" />}>
              <Surface variant="inner" className="flex flex-col gap-3 p-3">
                <div className="flex items-center justify-between gap-3 border-b border-[var(--stroke)] pb-2.5">
                  <div className="flex flex-col gap-0.5">
                    <div className="flex items-center gap-1.5 text-[13px] font-medium">
                      <Volume2 className="h-3.5 w-3.5 text-[var(--accent-b)]" />
                      <span>点击单词自动发音</span>
                    </div>
                    <p className="text-[10px] text-[var(--faint)]">
                      在释义、例句或查词卡片中点击单词时，自动播放发音。
                    </p>
                  </div>
                  <Switch
                    checked={autoPronounce}
                    onChange={() => setAutoPronounce(!autoPronounce)}
                    label="点击单词自动发音"
                  />
                </div>
                <DictionaryPanel />
              </Surface>
            </Section>
          )}

          {isMatch(['memory', '系统', '性能', '内存', '释放', 'ram', 'clean', 'working set']) && (
            <MemorySettingsPanel />
          )}

          {isMatch(['integrity', '健康', '检测', 'checks', 'diagnostic']) && (
            <IntegrityPanel />
          )}

          <p className="mt-auto text-center text-[10px] text-[var(--faint)]">
            Settings are saved automatically.
          </p>
        </div>
      </div>
    </div>
  );
};
