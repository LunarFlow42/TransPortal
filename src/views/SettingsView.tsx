import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
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
  AlertTriangle,
  Search,
  MousePointerClick,
  AppWindow,
  Sliders,
  Cpu,
  ShieldCheck,
  Camera,
  RotateCcw,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useBootstrap } from '../hooks/useBootstrap';
import { useShortcutStatus } from '../hooks/useShortcutStatus';
import { saveSettings } from '../store/persistence';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { closeSettingsWindow } from '../utils/windows';
import { DictionaryPanel } from '../components/DictionaryPanel';
import { OcrSettingsPanel } from '../components/OcrSettingsPanel';
import { TtsSettingsPanel } from '../components/TtsSettingsPanel';
import { IntegrityPanel } from '../components/IntegrityPanel';
import { MemorySettingsPanel } from '../components/MemorySettingsPanel';
import { CaptureDiagnosticsPanel } from '../components/CaptureDiagnosticsPanel';
import { ResizeHandles } from '../components/ResizeHandles';
import { OpenaiModelSelect } from '../components/OpenaiModelSelect';
import { Button, HoverTipText, IconButton, Input, SavedBadge, Section, Select, Surface, Switch } from '../components/ui';
import { setAppLanguage } from '../i18n';
import { cn } from '../utils/cn';
import type { ProviderType, ShortcutActionId, ThemeMode } from '../types';

const THEME_OPTIONS = [
  { value: 'system', label: 'Follow system 跟随系统' },
  { value: 'light', label: 'Light 明亮浅色' },
  { value: 'dark', label: 'Dark 暗夜深色' },
];

const PROVIDER_OPTIONS = [
  { value: 'bing_free', label: '微软必应' },
  { value: 'google_free', label: 'Google Free 谷歌翻译' },
  { value: 'youdao_free', label: 'Youdao Free 有道翻译' },
  { value: 'tencent_free', label: 'Tencent Free 腾讯翻译君' },
  { value: 'openai', label: 'OpenAI 兼容模型' },
];

const TARGET_LANGUAGE_OPTIONS = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'zh-TW', label: '繁體中文' },
  { value: 'fr', label: 'Français' },
  { value: 'de', label: 'Deutsch' },
  { value: 'es', label: 'Español' },
  { value: 'ru', label: 'Русский' },
];

const INK_KEY_OPTIONS = [
  { value: 'None', label: '已禁用' },
  { value: 'Control', label: 'Ctrl 键' },
  { value: 'Alt', label: 'Alt 键' },
  { value: 'Shift', label: 'Shift 键' },
  { value: 'CapsLock', label: '大写锁定键' },
];

const SEARCH_ENGINE_OPTIONS = [
  { value: 'auto', label: '智能自动分流匹配' },
  { value: 'bing', label: '微软必应' },
  { value: 'google', label: '谷歌 Google' },
  { value: 'baidu', label: '百度 搜索' },
  { value: 'custom', label: '自定义搜索引擎 URL' },
];

interface ShortcutGroupItem {
  id: ShortcutActionId;
  labelKey: string;
  descKey: string;
}

const SELECTION_SHORTCUTS: ShortcutGroupItem[] = [
  {
    id: 'selection',
    labelKey: 'settings.shortcuts.selectionAction',
    descKey: 'settings.shortcuts.selectionActionDesc',
  },
  {
    id: 'replace_selection',
    labelKey: 'settings.shortcuts.replaceSelectionAction',
    descKey: 'settings.shortcuts.replaceSelectionActionDesc',
  },
];

const CAPTURE_SHORTCUTS: ShortcutGroupItem[] = [
  {
    id: 'ocr_card',
    labelKey: 'settings.shortcuts.ocrCardAction',
    descKey: 'settings.shortcuts.ocrCardActionDesc',
  },
  {
    id: 'ocr_picture',
    labelKey: 'settings.shortcuts.ocrPictureAction',
    descKey: 'settings.shortcuts.ocrPictureActionDesc',
  },
  {
    id: 'ocr_search',
    labelKey: 'settings.shortcuts.ocrSearchAction',
    descKey: 'settings.shortcuts.ocrSearchActionDesc',
  },
  {
    id: 'silent_ocr',
    labelKey: 'settings.shortcuts.silentOcrAction',
    descKey: 'settings.shortcuts.silentOcrActionDesc',
  },
];

const WORKSPACE_SHORTCUTS: ShortcutGroupItem[] = [
  {
    id: 'input_translate',
    labelKey: 'settings.shortcuts.inputTranslateAction',
    descKey: 'settings.shortcuts.inputTranslateActionDesc',
  },
];

const IN_WINDOW_SHORTCUTS = [
  { key: 'Ctrl+A', descKey: 'settings.shortcuts.inWindow.ctrlA' },
  { key: 'Tab', descKey: 'settings.shortcuts.inWindow.tab' },
  { key: 'Ctrl+Shift+T', descKey: 'settings.shortcuts.inWindow.pin' },
  { key: 'Ctrl+R', descKey: 'settings.shortcuts.inWindow.retry' },
  { key: 'Ctrl+P', descKey: 'settings.shortcuts.inWindow.prev' },
  { key: 'Ctrl+N', descKey: 'settings.shortcuts.inWindow.next' },
  { key: 'Esc', descKey: 'settings.shortcuts.inWindow.esc' },
];

export type SettingsTab = 'general' | 'translation' | 'shortcuts' | 'advanced';

const TABS: { id: SettingsTab; labelKey: string; icon: React.ReactNode }[] = [
  { id: 'general', labelKey: 'settings.tabs.general', icon: <Palette className="h-4 w-4" /> },
  { id: 'translation', labelKey: 'settings.tabs.translation', icon: <Languages className="h-4 w-4" /> },
  { id: 'shortcuts', labelKey: 'settings.tabs.shortcuts', icon: <Keyboard className="h-4 w-4" /> },
  { id: 'advanced', labelKey: 'settings.tabs.advanced', icon: <Sliders className="h-4 w-4" /> },
];

/**
 * `#/settings` — the dedicated borderless settings window.
 */
export const SettingsView: React.FC = () => {
  const { i18n, t } = useTranslation();
  const {
    provider,
    setProvider,
    openaiConfig,
    setOpenaiConfig,
    hotkey,
    ocrCardHotkey,
    ocrPictureHotkey,
    ocrTextHotkey,
    ocrSearchHotkey,
    searchEngine,
    setSearchEngine,
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
    ttsEngine,
    ttsVoice,
    ttsSpeed,
    resetToDefaults,
  } = useAppStore();
  const [recordingAction, setRecordingAction] = useState<ShortcutActionId | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [showResetConfirm, setShowResetConfirm] = useState(false);
  const [isResetting, setIsResetting] = useState(false);
  const [savedPulse, setSavedPulse] = useState(false);
  const saveTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const isInitialMount = React.useRef(true);

  const triggerSaveNotice = React.useCallback(() => {
    setSavedPulse(true);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      setSavedPulse(false);
      saveTimerRef.current = null;
    }, 1500);
  }, []);

  useEffect(() => {
    if (isInitialMount.current) {
      isInitialMount.current = false;
      return;
    }
    triggerSaveNotice();
  }, [
    theme,
    overlayOpacity,
    fontSize,
    smoothTransitions,
    showCrosshairs,
    autostart,
    provider,
    targetLanguage,
    autoPronounce,
    inkTranslateKey,
    doubleCopyEnabled,
    clipboardMonitorEnabled,
    searchEngine,
    ttsEngine,
    ttsVoice,
    ttsSpeed,
    triggerSaveNotice,
  ]);

  const { report, error: shortcutError, retrying, retry } = useShortcutStatus();
  const settingsReady = useBootstrap(true);

  // Hide window on close to enable instant 0ms reopening without rebuilding WebView2.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !recordingAction) {
        event.preventDefault();
        void closeSettingsWindow();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [recordingAction]);

  useEffect(() => {
    const isRecording = Boolean(recordingAction);
    void invoke('set_shortcut_recording', { recording: isRecording }).catch((err) => {
      console.error('Failed to set shortcut recording state:', err);
    });
  }, [recordingAction]);

  useEffect(() => {
    const onWindowBlur = () => {
      setRecordingAction(null);
    };
    window.addEventListener('blur', onWindowBlur);
    return () => {
      window.removeEventListener('blur', onWindowBlur);
      void invoke('set_shortcut_recording', { recording: false }).catch(() => {});
    };
  }, []);

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
      case 'ocr_search':
      case 'ocr_text':
        return ocrSearchHotkey || ocrTextHotkey;
      case 'silent_ocr':
        return silentOcrHotkey;
      case 'input_translate':
        return inputTranslateHotkey;
      default:
        return '';
    }
  };

  const onShortcutKeyDown = async (
    action: ShortcutActionId,
    event: React.KeyboardEvent<HTMLInputElement>,
  ) => {
    event.preventDefault();
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;

    if (event.key === 'Escape') {
      setRecordingAction(null);
      return;
    }

    if (event.key === 'Backspace' || event.key === 'Delete') {
      await setShortcut(action, '');
      setRecordingAction(null);
      return;
    }

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

    await setShortcut(action, [...modifiers, mainKey].join('+'));
    setRecordingAction(null);
  };

  const resetKeybindingsToDefault = async () => {
    await setShortcut('selection', 'Alt+Q');
    await setShortcut('replace_selection', 'Alt+Z');
    await setShortcut('ocr_card', 'Alt+Shift+X');
    await setShortcut('ocr_picture', 'Alt+S');
    await setShortcut('ocr_search', 'Alt+Shift+C');
    await setShortcut('silent_ocr', 'Alt+Shift+S');
    await setShortcut('input_translate', 'Alt+Space');
    await setInkTranslateKey('None');
    await setDoubleCopyEnabled(true);
    await setClipboardMonitorEnabled(false);
    triggerSaveNotice();
  };

  const isMatch = (terms: string[]) => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return true;
    return terms.some((t) => t.toLowerCase().includes(q));
  };

  const isSearching = Boolean(searchQuery.trim());

  const matchAppearance = isSearching && isMatch(['appearance', '外观', '界面', 'theme', '主题', 'opacity', '透明度', '动效', '动画', '过渡', '平滑', 'transition', 'animation', 'font', '字号', 'crosshairs', '对齐', 'language', '语言']);
  const matchStartup = isSearching && isMatch(['startup', '开机启动', '自启', 'launch']);
  const matchProvider = isSearching && isMatch(['provider', '翻译', 'engine', 'openai', 'youdao', 'google', 'tencent', 'target', '语言', '有道', '腾讯', 'bing', '微软']);
  const matchSearchEngine = isSearching && isMatch(['search', '搜索', '搜索引擎', 'google', 'bing', 'baidu', '百度', '必应', '谷歌', '网页搜索']);
  const matchOcr = isSearching && isMatch(['ocr', '文字识别', 'windows ocr', 'onnx', 'paddle', '语言', '引擎', '原图', '译图', '图片展示', '覆写']);
  const matchShortcuts = isSearching && isMatch(['global', 'shortcuts', '全局', '快捷键', '热键', '划词', '替换', 'ocr', '截图', '截图搜索', '静默', '输入翻译', '手势', '监听', '落墨', '双击复制', 'ctrl+c+c', '剪贴板', '内置']);
  const matchDict = isSearching && isMatch(['dictionary', '词典', '发音', 'audio', 'pronounce', 'ecdict']);
  const matchTts = isSearching && isMatch(['tts', '语音', '发音', '朗读', 'speech', 'kokoro', '声音', '音色']);
  const matchMemory = isSearching && isMatch(['memory', '系统', '性能', '内存', '释放', 'ram', 'clean', 'working set']);
  const matchIntegrity = isSearching && isMatch(['integrity', '完整性', '健康', '检测', 'checks', 'sha256', 'hash']);
  const matchCapture = isSearching && isMatch(['capture', 'screenshot', '截图', '诊断', 'wgc', 'dxgi', 'gdi', 'debug', '调试', 'pipeline']);
  const matchReset = isSearching && isMatch(['reset', 'restore', 'default', 'defaults', '默认', '重置', '恢复', '出厂']);

  const anyMatch =
    matchAppearance ||
    matchStartup ||
    matchProvider ||
    matchSearchEngine ||
    matchOcr ||
    matchDict ||
    matchTts ||
    matchShortcuts ||
    matchMemory ||
    matchIntegrity ||
    matchCapture ||
    matchReset;

  const renderShortcutRow = (item: ShortcutGroupItem) => {
    const status = report?.shortcuts.find((s) => s.id === item.id);
    const isRecording = recordingAction === item.id;
    const currentVal = getShortcutValue(item.id);

    return (
      <div
        key={item.id}
        className="flex items-center justify-between gap-3 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-3 py-2.5 transition-colors hover:bg-[var(--surface-3)]/40"
      >
        <div className="flex items-center gap-2 min-w-0">
          <HoverTipText
            text={t(item.labelKey)}
            tip={t(item.descKey)}
            className="text-[13px] font-medium text-[var(--text)]"
          />
          {status?.active && (
            <span className="text-[10px] text-emerald-400 font-mono shrink-0">
              {status.system
                ? (i18n.language.startsWith('zh') ? '系统监听' : 'System Hook')
                : (i18n.language.startsWith('zh') ? '已就绪' : 'Ready')}
            </span>
          )}
          {status?.error && (
            <div className="flex items-center gap-1 text-[11px] text-amber-400 shrink-0">
              <AlertCircle className="h-3 w-3 shrink-0" />
              <span>{status.error}</span>
            </div>
          )}
        </div>

        <div className="w-36 shrink-0">
          <Input
            type="text"
            readOnly
            disabled={!settingsReady}
            value={isRecording ? t('settings.shortcuts.recording') : currentVal}
            onFocus={() => setRecordingAction(item.id)}
            onBlur={() => {
              if (recordingAction === item.id) setRecordingAction(null);
            }}
            onKeyDown={(e) => onShortcutKeyDown(item.id, e)}
            placeholder={t('settings.shortcuts.clickToRecord')}
            className={cn(
              'w-full text-center font-mono text-xs cursor-pointer select-none',
              isRecording
                ? 'border-[rgba(99,102,241,0.75)] shadow-[0_0_0_3px_rgba(99,102,241,0.22)]'
                : 'hover:border-[var(--stroke-strong)]',
            )}
          />
        </div>
      </div>
    );
  };

  return (
    <div className="h-screen w-screen bg-transparent overflow-hidden">
      <div className="window-root surface-hairline relative flex h-full w-full flex-col overflow-hidden rounded-[var(--r-win)] text-[var(--text)]">
        <ResizeHandles />
        <header
          onMouseDown={startWindowDrag}
          className="flex shrink-0 cursor-move items-center justify-between border-b border-[var(--stroke)] px-3.5 py-3"
        >
          <div className="flex items-center gap-2 text-[13px] font-semibold">
            <span className="grid h-5 w-5 place-items-center rounded-[5px] bg-gradient-to-br from-[var(--accent-a)] to-[var(--accent-b)] shadow-[0_4px_12px_rgba(34,211,238,0.35)]">
              <Languages className="h-3 w-3 text-white" />
            </span>
            TransPortal {t('settings.title')}
          </div>
          <div className="flex items-center gap-2">
            <SavedBadge saved={savedPulse} />
            <div className="flex items-center gap-0.5">
              <IconButton title={t('common.close')} onClick={() => void getCurrentWindow().minimize()}>
                <Minus className="h-3.5 w-3.5" />
              </IconButton>
              <IconButton
                title={`${t('common.close')} (Esc)`}
                onClick={() => void closeSettingsWindow()}
                className="hover:bg-rose-500/90 hover:text-white"
              >
                <X className="h-3.5 w-3.5" />
              </IconButton>
            </div>
          </div>
        </header>

        {settingsError && (
          <p role="alert" className="shrink-0 border-b border-rose-500/30 bg-rose-500/10 px-4 py-2 text-xs text-[var(--text)]">
            {settingsError}
          </p>
        )}

        {/* 主体两栏布局：左侧竖向导航侧边栏 + 右侧内容区 */}
        <div className="flex flex-1 min-h-0 overflow-hidden">
          {/* 左侧竖向导航栏 */}
          <aside className="flex w-40 shrink-0 flex-col border-r border-[var(--stroke)] bg-[var(--surface-2)]/30 overflow-y-auto p-2.5 gap-1 select-none">
            <div className="px-2.5 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--faint)]">
              {i18n.language.startsWith('zh') ? '配置导航' : 'Categories'}
            </div>
            {TABS.map((tItem) => {
              const isActive = !isSearching && activeTab === tItem.id;
              return (
                <button
                  key={tItem.id}
                  type="button"
                  onClick={() => {
                    setActiveTab(tItem.id);
                    if (isSearching) setSearchQuery('');
                  }}
                  className={cn(
                    'flex items-center gap-2.5 rounded-[var(--r-ctrl)] px-2.5 py-2 text-xs font-medium transition-all text-left cursor-pointer',
                    isActive
                      ? 'bg-[var(--surface-3)] text-[var(--text)] shadow-sm font-semibold'
                      : 'text-[var(--muted)] hover:text-[var(--text)] hover:bg-[var(--surface-3)]/60',
                  )}
                >
                  <span className={cn('shrink-0', isActive ? 'text-[var(--accent-b)]' : 'text-[var(--muted)]')}>
                    {tItem.icon}
                  </span>
                  <span className="truncate">{t(tItem.labelKey)}</span>
                </button>
              );
            })}
          </aside>

          {/* 右侧设置内容区 */}
          <div className="flex flex-1 min-w-0 flex-col overflow-hidden">
            {/* 顶栏：全局搜索 */}
            <div className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--stroke)] px-4 py-2.5 bg-[var(--surface-2)]/20">
              <div className="relative flex-1 flex items-center">
                <Search className="pointer-events-none absolute left-3 top-2.5 h-3.5 w-3.5 text-[var(--muted)]" />
                <input
                  type="text"
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Escape' && searchQuery) {
                      e.stopPropagation();
                      setSearchQuery('');
                    }
                  }}
                  placeholder={t('common.search')}
                  className="w-full rounded-[var(--r-ctrl)] border border-[var(--stroke)] bg-[var(--surface-2)] pl-8 pr-8 py-1.5 text-xs text-[var(--text)] placeholder-[var(--faint)] outline-none transition-colors focus:border-[var(--accent-b)] focus:bg-[var(--surface-3)]"
                />
                {isSearching && (
                  <button
                    type="button"
                    onClick={() => setSearchQuery('')}
                    title={t('common.clear')}
                    className="absolute right-2.5 flex h-4 w-4 items-center justify-center rounded-full text-[var(--muted)] hover:bg-[var(--hover)] hover:text-[var(--text)] transition-colors cursor-pointer"
                  >
                    <span className="text-xs leading-none">×</span>
                  </button>
                )}
              </div>
            </div>

            {/* 滚动内容卡片区 */}
            <div className="flex flex-1 flex-col gap-4 overflow-y-auto overflow-x-hidden p-4">

              {/* ====== TAB 1: 常规 (General) ====== */}
              {(isSearching ? matchAppearance : activeTab === 'general') && (
                <Section label={t('settings.general.appearance')} icon={<Palette className="h-3.5 w-3.5" />}>
                  <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
                    {/* 界面语言切换器 */}
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.general.language')}
                      </label>
                      <Select
                        ariaLabel="Language"
                        value={i18n.language.startsWith('zh') ? 'zh-CN' : 'en-US'}
                        onChange={(value) => {
                          setAppLanguage(value as 'zh-CN' | 'en-US');
                          triggerSaveNotice();
                        }}
                        options={[
                          { value: 'zh-CN', label: '简体中文' },
                          { value: 'en-US', label: 'English' },
                        ]}
                      />
                    </div>

                    {/* 主题模式 */}
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.general.theme')}
                      </label>
                      <Select
                        ariaLabel="Theme"
                        value={theme}
                        onChange={(value) => setTheme(value as ThemeMode)}
                        options={THEME_OPTIONS}
                      />
                    </div>

                    {/* 窗口透明度 */}
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.general.overlayOpacity')}
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

                    {/* 字号大小 */}
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.general.fontSize')}
                      </label>
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
                      {t('settings.general.fontSizeDesc')}
                    </p>

                    {/* 平滑动画动效 */}
                    <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                      <HoverTipText
                        text={t('settings.general.smoothTransitions')}
                        tip={t('settings.general.smoothTransitionsDesc')}
                        className="text-[13px] font-medium text-[var(--text)]"
                      />
                      <Switch
                        checked={smoothTransitions}
                        onChange={() => setSmoothTransitions(!smoothTransitions)}
                        label={t('settings.general.smoothTransitions')}
                      />
                    </div>

                    {/* 截图辅助十字准星 */}
                    <div className="flex items-center justify-between gap-3 border-t border-[var(--stroke)] pt-2.5">
                      <HoverTipText
                        text={t('settings.general.crosshairs')}
                        tip={t('settings.general.crosshairsDesc')}
                        className="text-[13px] font-medium text-[var(--text)]"
                      />
                      <Switch
                        checked={showCrosshairs}
                        onChange={() => setShowCrosshairs(!showCrosshairs)}
                        label={t('settings.general.crosshairs')}
                      />
                    </div>
                  </Surface>
                </Section>
              )}

              {/* 系统与运行行为 */}
              {(isSearching ? matchStartup : activeTab === 'general') && (
                <Section label={t('settings.general.behavior')} icon={<Power className="h-3.5 w-3.5" />}>
                  <Surface variant="inner" className="flex items-center justify-between gap-3 px-3.5 py-2.5">
                    <HoverTipText
                      text={t('settings.general.autostart')}
                      tip={t('settings.general.autostartDesc')}
                      className="text-[13px] font-medium text-[var(--text)]"
                    />
                    <Switch
                      checked={autostart}
                      onChange={() => void toggleAutostart()}
                      label={t('settings.general.autostart')}
                    />
                  </Surface>
                </Section>
              )}

              {/* ====== TAB 2: 翻译与词典 (Translation & Dictionaries) ====== */}
              {(isSearching ? matchProvider : activeTab === 'translation') && (
                <Section label={t('settings.translation.providers')} icon={<Languages className="h-3.5 w-3.5" />}>
                  <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.translation.providerSelect')}
                      </label>
                      <Select
                        ariaLabel="Translation engine"
                        value={provider}
                        onChange={(value) => setProvider(value as ProviderType)}
                        options={PROVIDER_OPTIONS}
                      />
                    </div>

                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.translation.targetLanguage')}
                      </label>
                      <Select
                        ariaLabel="Target language"
                        value={targetLanguage}
                        onChange={(value) => setTargetLanguage(value)}
                        options={TARGET_LANGUAGE_OPTIONS}
                      />
                    </div>
                    <p className="text-[10px] text-[var(--faint)]">
                      输入语言与目标语言相同时将自动互译对调。
                    </p>

                    {provider === 'openai' ? (
                      <>
                        <div className="flex items-center gap-3">
                          <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                            {t('settings.translation.baseUrl')}
                          </label>
                          <Input
                            type="text"
                            value={openaiConfig.base_url || ''}
                            onChange={(e) => setOpenaiConfig({ base_url: e.target.value })}
                            placeholder="https://api.openai.com/v1"
                          />
                        </div>
                        <div className="flex items-center gap-3">
                          <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                            {t('settings.translation.apiKey')}
                          </label>
                          <Input
                            type="password"
                            value={openaiConfig.api_key}
                            onChange={(e) => setOpenaiConfig({ api_key: e.target.value })}
                            placeholder="sk-..."
                          />
                        </div>
                        <OpenaiModelSelect
                          value={openaiConfig.model || ''}
                          onChange={(model) => setOpenaiConfig({ model })}
                          config={openaiConfig}
                        />
                      </>
                    ) : (
                      <p className="text-[10px] text-[var(--faint)]">
                        免密钥在线通道，开箱即用；Bing / Google / 有道 / 腾讯 均支持智能语言识别。
                      </p>
                    )}
                  </Surface>
                </Section>
              )}

              {/* 网页搜索引擎 */}
              {(isSearching ? matchSearchEngine : activeTab === 'translation') && (
                <Section label={t('settings.general.searchEngine')} icon={<Search className="h-3.5 w-3.5" />}>
                  <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
                    <div className="flex items-center gap-3">
                      <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">
                        {t('settings.general.searchEngineSelect')}
                      </label>
                      <Select
                        ariaLabel="Web search engine"
                        value={['auto', 'bing', 'google', 'baidu'].includes(searchEngine) ? searchEngine : 'custom'}
                        onChange={(value) => {
                          if (value === 'custom') {
                            const fallbackCustom = 'https://duckduckgo.com/?q=%s';
                            setSearchEngine(fallbackCustom);
                            void saveSettings({ searchEngine: fallbackCustom });
                          } else {
                            setSearchEngine(value);
                            void saveSettings({ searchEngine: value });
                          }
                        }}
                        options={SEARCH_ENGINE_OPTIONS}
                      />
                    </div>
                    <p className="text-[10px] text-[var(--faint)]">
                      用于截图文字搜索。默认“自动”将根据系统语言智能选择。
                    </p>
                    {!['auto', 'bing', 'google', 'baidu'].includes(searchEngine) && (
                      <div className="flex items-center gap-3 pt-1">
                        <label className="w-28 shrink-0 text-[13px] text-[var(--muted)]">URL 模板</label>
                        <Input
                          type="text"
                          value={searchEngine}
                          onChange={(e) => {
                            setSearchEngine(e.target.value);
                            void saveSettings({ searchEngine: e.target.value });
                          }}
                          placeholder="https://duckduckgo.com/?q=%s"
                        />
                      </div>
                    )}
                  </Surface>
                </Section>
              )}

              {/* 本地与在线词典 */}
              {(isSearching ? matchDict : activeTab === 'translation') && (
                <Section label={t('settings.translation.dictionaries')} icon={<BookOpen className="h-3.5 w-3.5" />} className="shrink-0">
                  <Surface variant="inner" className="flex flex-col gap-3 p-3">
                    <div className="flex items-center justify-between gap-3 border-b border-[var(--stroke)] pb-2.5">
                      <div className="flex flex-col gap-0.5">
                        <div className="flex items-center gap-1.5 text-[13px] font-medium">
                          <Volume2 className="h-3.5 w-3.5 text-[var(--accent-b)]" />
                          <span>{t('settings.general.autoPronounce')}</span>
                        </div>
                        <p className="text-[10px] text-[var(--faint)]">
                          {t('settings.general.autoPronounceDesc')}
                        </p>
                      </div>
                      <Switch
                        checked={autoPronounce}
                        onChange={() => setAutoPronounce(!autoPronounce)}
                        label={t('settings.general.autoPronounce')}
                      />
                    </div>
                    <DictionaryPanel />
                  </Surface>
                </Section>
              )}

              {/* OCR 引擎与语言包 */}
              {(isSearching ? matchOcr : activeTab === 'translation') && (
                <OcrSettingsPanel disabled={!settingsReady} />
              )}

              {/* TTS 语音合成 */}
              {(isSearching ? matchTts : activeTab === 'translation') && (
                <Section label={t('settings.general.tts')} icon={<Volume2 className="h-3.5 w-3.5" />} className="shrink-0">
                  <Surface variant="inner" className="flex flex-col gap-3 p-3">
                    <TtsSettingsPanel disabled={!settingsReady} />
                  </Surface>
                </Section>
              )}

              {/* ====== TAB 3: 快捷键与手势 (Shortcuts & Gestures) ====== */}
              {(isSearching ? matchShortcuts : activeTab === 'shortcuts') && (
                <>
                  {/* 快捷键顶部操作条 */}
                  <div className="flex items-center justify-between gap-3 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)]/40 px-3.5 py-2">
                    <span className="text-[11px] text-[var(--muted)] flex items-center gap-1.5">
                      {t('settings.shortcuts.recordHint')}
                    </span>
                    <div className="flex items-center gap-2 shrink-0">
                      <Button
                        size="sm"
                        variant="subtle"
                        disabled={retrying || !report?.ready}
                        onClick={() => void retry()}
                        className="h-6 text-[10px]"
                      >
                        <RefreshCw className={cn('h-3 w-3', retrying && 'animate-spin')} />
                        {t('settings.shortcuts.recheckShortcuts')}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => void resetKeybindingsToDefault()}
                        className="h-6 text-[10px] text-[var(--accent-b)] hover:text-white"
                      >
                        <RotateCcw className="h-3 w-3" />
                        {t('settings.shortcuts.resetKeybindings')}
                      </Button>
                    </div>
                  </div>

                  {shortcutError && (
                    <p className="text-xs text-rose-400 border border-rose-500/30 bg-rose-500/10 rounded-[var(--r-inner)] p-2">
                      {shortcutError}
                    </p>
                  )}

                  {/* 卡片 1: 划词与取词翻译 */}
                  <Section
                    label={t('settings.shortcuts.selectionCardTitle')}
                    icon={<MousePointerClick className="h-3.5 w-3.5" />}
                  >
                    <Surface variant="inner" className="flex flex-col gap-3 p-3">
                      <div className="flex flex-col gap-2">
                        {SELECTION_SHORTCUTS.map(renderShortcutRow)}
                      </div>

                      {/* 辅助触发手势分层 */}
                      <div className="border-t border-[var(--stroke)] pt-2.5 flex flex-col gap-2.5">
                        <div className="flex items-center gap-1.5 text-[11px] font-semibold text-[var(--muted)] uppercase tracking-wider">
                          {t('settings.shortcuts.gesturesTitle')}
                        </div>

                        {/* 剪贴板监听翻译 */}
                        <div className="flex items-center justify-between gap-3 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-3 py-2.5 transition-colors hover:bg-[var(--surface-3)]/40">
                          <HoverTipText
                            text={t('settings.shortcuts.clipboardMonitor')}
                            tip={t('settings.shortcuts.clipboardMonitorDesc')}
                            className="text-[13px] font-medium text-[var(--text)]"
                          />
                          <Switch
                            checked={clipboardMonitorEnabled}
                            onChange={(checked) => void setClipboardMonitorEnabled(checked)}
                            label={t('settings.shortcuts.clipboardMonitor')}
                          />
                        </div>

                        {/* 落墨翻译按键 */}
                        <div className="flex items-center justify-between gap-3 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-3 py-2.5 transition-colors hover:bg-[var(--surface-3)]/40">
                          <HoverTipText
                            text={t('settings.shortcuts.inkTranslate')}
                            tip={t('settings.shortcuts.inkTranslateDesc')}
                            className="text-[13px] font-medium text-[var(--text)]"
                          />
                          <div className="w-36 shrink-0">
                            <Select
                              ariaLabel="Ink key"
                              value={inkTranslateKey}
                              onChange={(value) => void setInkTranslateKey(value)}
                              options={INK_KEY_OPTIONS}
                            />
                          </div>
                        </div>

                        {/* 双击复制 Ctrl+C+C */}
                        <div className="flex items-center justify-between gap-3 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-3 py-2.5 transition-colors hover:bg-[var(--surface-3)]/40">
                          <HoverTipText
                            text={t('settings.shortcuts.doubleCopy')}
                            tip={t('settings.shortcuts.doubleCopyDesc')}
                            className="text-[13px] font-medium text-[var(--text)]"
                          />
                          <Switch
                            checked={doubleCopyEnabled}
                            onChange={(checked) => void setDoubleCopyEnabled(checked)}
                            label={t('settings.shortcuts.doubleCopy')}
                          />
                        </div>
                      </div>
                    </Surface>
                  </Section>

                  {/* 卡片 2: 截屏识别与对照 (OCR) */}
                  <Section
                    label={t('settings.shortcuts.captureCardTitle')}
                    icon={<Camera className="h-3.5 w-3.5" />}
                  >
                    <Surface variant="inner" className="flex flex-col gap-2 p-3">
                      {CAPTURE_SHORTCUTS.map(renderShortcutRow)}
                    </Surface>
                  </Section>

                  {/* 卡片 3: 呼出与窗口内置键位 */}
                  <Section
                    label={t('settings.shortcuts.workspaceCardTitle')}
                    icon={<AppWindow className="h-3.5 w-3.5" />}
                  >
                    <Surface variant="inner" className="flex flex-col gap-3 p-3">
                      {WORKSPACE_SHORTCUTS.map(renderShortcutRow)}

                      {/* 窗口内置键盘操作一览 */}
                      <div className="border-t border-[var(--stroke)] pt-2.5 flex flex-col gap-2">
                        <div className="flex items-center gap-1.5 text-[11px] font-semibold text-[var(--muted)] uppercase tracking-wider">
                          {t('settings.shortcuts.inWindowHeader')}
                        </div>
                        <div className="grid grid-cols-2 gap-2">
                          {IN_WINDOW_SHORTCUTS.map((sc) => (
                            <div
                              key={sc.key}
                              className="flex items-center justify-between gap-2 min-w-0 rounded-[var(--r-inner)] border border-[var(--stroke)] bg-[var(--surface-2)] px-2.5 py-1.5 text-xs"
                            >
                              <span className="min-w-0 flex-1 truncate text-[var(--muted)] text-[11px]" title={t(sc.descKey)}>
                                {t(sc.descKey)}
                              </span>
                              <kbd className="shrink-0 rounded-[4px] border border-[var(--stroke)] bg-[var(--surface-3)] px-1.5 py-0.5 font-mono text-[11px] text-[var(--text)]">
                                {sc.key}
                              </kbd>
                            </div>
                          ))}
                        </div>
                      </div>
                    </Surface>
                  </Section>
                </>
              )}

              {/* ====== TAB 4: 高级与诊断 (Advanced & Diagnostics) ====== */}
              {(isSearching ? matchIntegrity : activeTab === 'advanced') && (
                <Section
                  label={t('settings.advanced.integrity')}
                  icon={<ShieldCheck className="h-3.5 w-3.5" />}
                  className="shrink-0"
                >
                  <IntegrityPanel bare />
                </Section>
              )}

              {(isSearching ? matchCapture : activeTab === 'advanced') && (
                <Section
                  label={t('settings.advanced.capture')}
                  icon={<Camera className="h-3.5 w-3.5" />}
                  className="shrink-0"
                >
                  <CaptureDiagnosticsPanel bare />
                </Section>
              )}

              {(isSearching ? matchMemory : activeTab === 'advanced') && (
                <Section
                  label={t('settings.advanced.memory')}
                  icon={<Cpu className="h-3.5 w-3.5" />}
                  className="shrink-0"
                >
                  <MemorySettingsPanel bare />
                </Section>
              )}

              {/* 恢复出厂设置 */}
              {(isSearching ? matchReset : activeTab === 'advanced') && (
                <Section
                  label={t('settings.advanced.resetDefaultsTitle')}
                  icon={<AlertTriangle className="h-3.5 w-3.5 text-amber-500" />}
                  className="shrink-0"
                >
                  <Surface variant="inner" className="flex flex-col gap-3 p-3.5 border border-rose-500/20 bg-rose-500/5">
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-1">
                        <span className="text-[13px] font-semibold text-[var(--text)]">
                          {t('settings.advanced.resetDefaultsTitle')}
                        </span>
                        <p className="text-[11px] text-[var(--muted)] leading-relaxed">
                          {t('settings.advanced.resetDefaultsDesc')}
                        </p>
                      </div>
                      {!showResetConfirm && (
                        <Button
                          variant="danger"
                          size="sm"
                          onClick={() => setShowResetConfirm(true)}
                          className="shrink-0"
                        >
                          {t('settings.advanced.resetButton')}
                        </Button>
                      )}
                    </div>

                    {showResetConfirm && (
                      <div className="rounded-[var(--r-ctrl)] border border-rose-500/40 bg-[var(--surface-1)] p-3 text-xs flex flex-col gap-2.5">
                        <div className="flex items-center gap-2 text-rose-400 font-semibold">
                          <AlertTriangle className="h-4 w-4 shrink-0" />
                          <span>{t('settings.advanced.resetConfirmTitle')}</span>
                        </div>
                        <ul className="text-[11px] text-[var(--muted)] list-disc pl-4 space-y-1">
                          <li>{t('settings.advanced.resetConfirmItem1')}</li>
                          <li>{t('settings.advanced.resetConfirmItem2')}</li>
                          <li>{t('settings.advanced.resetConfirmItem3')}</li>
                          <li className="text-emerald-400 font-medium">
                            {t('settings.advanced.resetConfirmItem4')}
                          </li>
                        </ul>
                        <div className="flex items-center justify-end gap-2 pt-1 border-t border-[var(--stroke)]">
                          <Button
                            variant="ghost"
                            size="sm"
                            disabled={isResetting}
                            onClick={() => setShowResetConfirm(false)}
                          >
                            {t('common.cancel')}
                          </Button>
                          <Button
                            variant="danger"
                            size="sm"
                            disabled={isResetting}
                            onClick={async () => {
                              setIsResetting(true);
                              try {
                                await resetToDefaults();
                                setShowResetConfirm(false);
                                triggerSaveNotice();
                              } finally {
                                setIsResetting(false);
                              }
                            }}
                          >
                            {isResetting ? t('settings.advanced.resetting') : t('settings.advanced.resetConfirmButton')}
                          </Button>
                        </div>
                      </div>
                    )}
                  </Surface>
                </Section>
              )}

              {/* 搜索无结果提示 */}
              {isSearching && !anyMatch && (
                <div className="flex flex-col items-center justify-center p-8 text-center text-xs text-[var(--muted)]">
                  <AlertCircle className="h-8 w-8 text-[var(--muted)] mb-2 opacity-50" />
                  <p>{i18n.language.startsWith('zh') ? `未找到与 “${searchQuery}” 匹配的设置项` : `No settings match "${searchQuery}"`}</p>
                  <p className="text-[11px] text-[var(--faint)] mt-1">
                    {i18n.language.startsWith('zh') ? '请尝试搜索其他关键词（如：主题、快捷键、OCR、语言）' : 'Try searching for other terms like theme, shortcut, OCR, language'}
                  </p>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
