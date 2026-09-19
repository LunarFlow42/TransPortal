let currentAudio: HTMLAudioElement | null = null;
let isSpeaking = false;

/** Whether audio pronunciation is actively playing. */
export function isAudioPlaying(): boolean {
  return (currentAudio !== null && !currentAudio.paused) || isSpeaking;
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
  if ('speechSynthesis' in window) {
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
    audio.onended = finish;
    audio.onerror = () => speak(word, onEnd);
    void audio.play().catch(() => speak(word, onEnd));
    return;
  }

  speak(word, onEnd);
}

function speak(word: string, onEnd: () => void): void {
  if (!('speechSynthesis' in window)) {
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
