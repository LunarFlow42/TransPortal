import React, { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  disable as autostartDisable,
  enable as autostartEnable,
  isEnabled as autostartIsEnabled,
} from '@tauri-apps/plugin-autostart';
import { Keyboard, Languages, Minus, Palette, Power, X } from 'lucide-react';
import { useBootstrap } from '../hooks/useBootstrap';
import { saveSettings } from '../store/persistence';
import { useAppStore } from '../store/useAppStore';
import { startWindowDrag } from '../utils/drag';
import { DictionaryPanel } from '../components/DictionaryPanel';
import { OcrSettingsPanel } from '../components/OcrSettingsPanel';
import { ShortcutSettingsStatus } from '../components/ShortcutStatusPanel';
import { IntegrityPanel } from '../components/IntegrityPanel';
import { IconButton, Input, Section, Select, Surface, Switch } from '../components/ui';
import type { ProviderType, ThemeMode } from '../types';

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
    setHotkey,
    settingsError,
    theme,
    setTheme,
    overlayOpacity,
    setOverlayOpacity,
  } = useAppStore();
  const [recording, setRecording] = useState(false);
  const [autostart, setAutostart] = useState(false);

  const settingsReady = useBootstrap(true);

  // Hide instead of destroying so the window can be reopened instantly.
  useEffect(() => {
    const window = getCurrentWindow();
    const unlisten = window.onCloseRequested((event) => {
      event.preventDefault();
      void window.hide();
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    void autostartIsEnabled()
      .then(setAutostart)
      .catch((error) => console.error('autostart check failed:', error));
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

  const onHotkeyKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
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

    void setHotkey([...modifiers, mainKey].join('+'));
    setRecording(false);
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
              onClick={() => void getCurrentWindow().close()}
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

        <div className="flex flex-1 flex-col gap-5 overflow-y-auto p-4">
          <Section label="Appearance" icon={<Palette className="h-3.5 w-3.5" />}>
            <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
              <div className="flex items-center gap-3">
                <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">Theme</label>
                <Select
                  ariaLabel="Theme"
                  value={theme}
                  onChange={(value) => setTheme(value as ThemeMode)}
                  options={THEME_OPTIONS}
                />
              </div>
              <div className="flex items-center gap-3">
                <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">
                  Overlay opacity
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
              <p className="text-[10px] text-[var(--faint)]">
                仅影响查词浮层（设置窗口固定不透明）。
              </p>
            </Surface>
          </Section>

          <Section label="Translation Provider">
            <Surface variant="inner" className="flex flex-col gap-2.5 p-3">
              <div className="flex items-center gap-3">
                <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">Engine</label>
                <Select
                  ariaLabel="Translation engine"
                  value={provider}
                  onChange={(value) => setProvider(value as ProviderType)}
                  options={PROVIDER_OPTIONS}
                />
              </div>

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

          <Section label="Global Shortcut" icon={<Keyboard className="h-3.5 w-3.5" />}>
            <Surface variant="inner" className="flex flex-col gap-2 p-3">
              <div className="flex items-center gap-3">
                <label className="w-24 shrink-0 text-[13px] text-[var(--muted)]">Capture</label>
                <Input
                  type="text"
                  readOnly
                  disabled={!settingsReady}
                  value={recording ? 'Press a key combination…' : hotkey}
                  onFocus={() => setRecording(true)}
                  onBlur={() => setRecording(false)}
                  onKeyDown={onHotkeyKeyDown}
                  className={
                    recording
                      ? 'border-[rgba(99,102,241,0.65)] text-center font-mono shadow-[0_0_0_3px_rgba(99,102,241,0.22)]'
                      : 'text-center font-mono'
                  }
                />
              </div>
              <p className="text-[10px] text-[var(--faint)]">
                Click the field and press a combination (e.g. Alt+T). At least one modifier is
                required (F-keys excepted).
              </p>
              <ShortcutSettingsStatus />
            </Surface>
          </Section>

          <OcrSettingsPanel disabled={!settingsReady} />

          <Section label="Startup" icon={<Power className="h-3.5 w-3.5" />}>
            <Surface variant="inner" className="flex items-center justify-between gap-3 p-3">
              <span className="text-[13px]">Launch TransPortal on system startup</span>
              <Switch
                checked={autostart}
                onChange={() => void toggleAutostart()}
                label="Autostart"
              />
            </Surface>
          </Section>

          <Section label="Dictionary">
            <Surface variant="inner" className="p-3">
              <DictionaryPanel />
            </Surface>
          </Section>

          <IntegrityPanel />

          <p className="mt-auto text-center text-[10px] text-[var(--faint)]">
            Settings are saved automatically.
          </p>
        </div>
      </div>
    </div>
  );
};
