let currentAudio: HTMLAudioElement | null = null;
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
  isSpeaking = false;
  if (typeof window !== 'undefined' && 'speechSynthesis' in window) {
    window.speechSynthesis.cancel();
  }
}

/**
 * Play a pronunciation, stopping whatever was playing first.
 *
 * A single module-level audio instance means deep lookup stacks can never
 * overlap their audio.
 */
export function playPronunciation(
  word: string,
  audioUrl: string | undefined,
  onEnd: () => void,
): void {
  stopPronunciation();

  if (audioUrl) {
    const audio = new Audio(audioUrl);
    currentAudio = audio;
    const finish = () => {
      if (currentAudio === audio) currentAudio = null;
      onEnd();
    };
    // `onerror` and the rejected `play()` can both report one failure.
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
  utterance.lang = 'en-US';
  utterance.rate = 0.9;
  const handleEnd = () => {
    isSpeaking = false;
    onEnd();
  };
  utterance.onend = handleEnd;
  utterance.onerror = handleEnd;
  window.speechSynthesis.speak(utterance);
}
