import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ArrowRightLeft, LoaderCircle } from 'lucide-react';
import { Button } from './ui';
import { useAppStore } from '../store/useAppStore';
import { suppressAutoHide } from '../utils/autoHide';
import type { LookupNode, TranslateResult } from '../types';

/** Only a root capture can replace the matching original selection. */
export function ReplaceSelectionButton({ node }: { node: LookupNode }) {
  const capture = useAppStore((state) => state.capture);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!capture?.replace_token || capture.text.trim() !== node.text.trim()) return null;
  const token = capture.replace_token;
  const replace = async () => {
    setBusy(true); setError(null); suppressAutoHide(35_000);
    try {
      const state = useAppStore.getState();
      const text = node.translation.trim() || (await invoke<TranslateResult>('translate_text', {
        query: { text: node.text, target_lang: state.ocrTarget }, provider: state.provider,
        openaiConfig: state.provider === 'openai' ? state.openaiConfig : null,
      })).translated_text;
      if (useAppStore.getState().capture?.replace_token !== token || useAppStore.getState().lookupStack[0]?.id !== node.id) {
        throw new Error('取词已变化，已取消替换。');
      }
      await invoke('replace_captured_selection', { token, text });
      if (useAppStore.getState().capture?.replace_token === token) {
        useAppStore.setState({ capture: { ...capture, replace_token: null } });
        void getCurrentWindow().hide();
      }
    } catch (error) { setError(String(error)); }
    finally { setBusy(false); }
  };
  return <div className="mt-2">
    <Button size="sm" disabled={busy || node.status === 'loading' || node.status === 'streaming'}
      title="回到原输入框，确认选中文字未改变后替换为译文；可在原软件中 Ctrl+Z 撤销"
      onClick={() => void replace()}>
      {busy ? <LoaderCircle className="h-3 w-3 animate-spin" /> : <ArrowRightLeft className="h-3 w-3" />}
      {busy ? '正在替换…' : '替换选中文字'}
    </Button>
    {error && <p role="alert" className="mt-1 text-[11px] text-rose-400">{error}</p>}
  </div>;
}
