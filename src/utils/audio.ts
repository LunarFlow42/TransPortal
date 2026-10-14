import { invoke } from '@tauri-apps/api/core';
import { useAppStore } from '../store/useAppStore';

let currentAudio: HTMLAudioElement | null = null;
let currentBlobUrl: string | null = null;
let isSpeaking = false;

/**
 * Whether audio pronunciation is actively playing.
 *
 * Checks the live media/speech state instead of trusting our flags alone: a
 * failed audio element never fires `ended`, and WebView2 sometimes skips an
 * utterance's `end` event, which would otherwise block auto-dismiss forever.
 */
export function isAudioPlaying(): boolean {
  const audioBusy =
    currentAudio !== null && !currentAudio.paused && !currentAudio.ended && currentAudio.error === null;
  const speechBusy =
    isSpeaking && typeof window !== 'undefined' && 'speechSynthesis' in window &&
    (window.speechSynthesis.speaking || window.speechSynthesis.pending);
  return audioBusy || speechBusy;
}

/** Stop any pronunciation currently playing (audio element or speech synthesis). */
export function stopPronunciation(): void {
  if (currentAudio) {
    currentAudio.onended = null;
    currentAudio.onerror = null;
    currentAudio.pause();
    currentAudio.src = '';
    currentAudio = null;
  }
  if (currentBlobUrl) {
    URL.revokeObjectURL(currentBlobUrl);
    currentBlobUrl = null;
  }
  isSpeaking = false;
  if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
    window.speechSynthesis.cancel();
  }
}

/**
 * Play a pronunciation, stopping whatever was playing first.
 *
 * If `ttsEngine` is configured to `kokoro_onnx`, attempts local offline neural synthesis first.
 * If the model is not installed or the text contains non-English characters, automatically
 * falls back to remote audioUrl or Web Speech API.
 */
export function playPronunciation(
  word: string,
  audioUrl: string | null | undefined,
  onEnd: () => void,
  options?: { voice?: string; speed?: number },
): void {
  stopPronunciation();

  const state = useAppStore.getState();
  const voice = options?.voice ?? state.ttsVoice ?? 'af_heart';
  const speed = options?.speed ?? state.ttsSpeed ?? 1.0;
  const ttsEngine = state.ttsEngine;

  if (ttsEngine === 'kokoro_onnx') {
    void invoke<ArrayBuffer>('tts_synthesize', {
      text: word,
      voice,
      speed,
    })
      .then((buffer) => {
        playWavBuffer(buffer, onEnd, () => {
          // If blob playback somehow fails, fall back to remote or Web Speech
          fallbackPronunciation(word, audioUrl, onEnd);
        });
      })
      .catch(() => {
        // Fallback gracefully on UnsupportedLanguageOrEmpty or missing model
        fallbackPronunciation(word, audioUrl, onEnd);
      });
    return;
  }

  fallbackPronunciation(word, audioUrl, onEnd);
}

function playWavBuffer(
  buffer: ArrayBuffer,
  onEnd: () => void,
  onError: () => void,
): void {
  const blob = new Blob([buffer], { type: 'audio/wav' });
  const blobUrl = URL.createObjectURL(blob);
  currentBlobUrl = blobUrl;

  const audio = new Audio(blobUrl);
  currentAudio = audio;

  const finish = () => {
    if (currentAudio === audio) {
      currentAudio = null;
    }
    if (currentBlobUrl === blobUrl) {
      URL.revokeObjectURL(blobUrl);
      currentBlobUrl = null;
    }
    onEnd();
  };

  const fail = () => {
    if (currentAudio === audio) {
      currentAudio = null;
    }
    if (currentBlobUrl === blobUrl) {
      URL.revokeObjectURL(blobUrl);
      currentBlobUrl = null;
    }
    onError();
  };

  audio.onended = finish;
  audio.onerror = fail;
  void audio.play().catch(fail);
}

function fallbackPronunciation(
  word: string,
  audioUrl: string | null | undefined,
  onEnd: () => void,
): void {
  if (audioUrl) {
    const audio = new Audio(audioUrl);
    currentAudio = audio;
    const finish = () => {
      if (currentAudio === audio) currentAudio = null;
      onEnd();
    };
    const fallBack = () => {
      if (currentAudio !== audio) return;
      currentAudio = null;
      speak(word, onEnd);
    };
    audio.onended = finish;
    audio.onerror = fallBack;
    void audio.play().catch(fallBack);
    return;
  }

  speak(word, onEnd);
}

function speak(word: string, onEnd: () => void): void {
  if (typeof window === 'undefined' || !('speechSynthesis' in window)) {
    isSpeaking = false;
    onEnd();
    return;
  }
  window.speechSynthesis.cancel();
  isSpeaking = true;
  const utterance = new SpeechSynthesisUtterance(word);
  // Detect if text contains Chinese
  if (/[\u4e00-\u9fff]/.test(word)) {
    utterance.lang = 'zh-CN';
  } else {
    utterance.lang = 'en-US';
  }
  utterance.rate = 0.9;
  const handleEnd = () => {
    isSpeaking = false;
    onEnd();
  };
  utterance.onend = handleEnd;
  utterance.onerror = handleEnd;
  window.speechSynthesis.speak(utterance);
}
