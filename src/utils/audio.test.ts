import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { isAudioPlaying, playPronunciation, stopPronunciation } from './audio';
import { useAppStore } from '../store/useAppStore';

class MockSpeechSynthesisUtterance {
  text: string;
  lang = '';
  rate = 1;
  voice: { name: string; lang: string } | null = null;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(text: string) {
    this.text = text;
  }
}

describe('Audio playback & pronunciation tests', () => {
  const originalWindow = globalThis.window;

  beforeEach(() => {
    stopPronunciation();
    useAppStore.setState({
      ttsEngine: 'web_speech',
      ttsVoice: 'af_heart',
      ttsSpeed: 1.0,
    });
  });

  afterEach(() => {
    globalThis.window = originalWindow;
  });

  it('reports false when no audio or speech is playing', () => {
    expect(isAudioPlaying()).toBe(false);
  });

  it('stops active pronunciation without throwing', () => {
    expect(() => stopPronunciation()).not.toThrow();
    expect(isAudioPlaying()).toBe(false);
  });

  it('falls back to Web Speech with female voice preference', () => {
    const speakMock = vi.fn();
    const cancelMock = vi.fn();
    const mockVoices = [
      { name: 'Microsoft David Desktop - English (United States)', lang: 'en-US' },
      { name: 'Microsoft Zira Desktop - English (United States)', lang: 'en-US' },
    ];

    const mockWindow = {
      speechSynthesis: {
        speak: speakMock,
        cancel: cancelMock,
        speaking: false,
        pending: false,
        getVoices: () => mockVoices,
      },
    };

    // @ts-expect-error Mock window and utterance in test environment
    globalThis.window = mockWindow;
    // @ts-expect-error Mock SpeechSynthesisUtterance
    globalThis.SpeechSynthesisUtterance = MockSpeechSynthesisUtterance;

    const onEnd = vi.fn();
    playPronunciation('Hello world', null, onEnd, { voice: 'af_heart' });

    expect(cancelMock).toHaveBeenCalled();
    expect(speakMock).toHaveBeenCalledTimes(1);

    const utterance = speakMock.mock.calls[0][0] as MockSpeechSynthesisUtterance;
    expect(utterance.text).toBe('Hello world');
    expect(utterance.lang).toBe('en-US');
    expect(utterance.voice?.name).toBe('Microsoft Zira Desktop - English (United States)');
  });

  it('falls back to male Web Speech when male voice is selected', () => {
    const speakMock = vi.fn();
    const cancelMock = vi.fn();
    const mockVoices = [
      { name: 'Microsoft David Desktop - English (United States)', lang: 'en-US' },
      { name: 'Microsoft Zira Desktop - English (United States)', lang: 'en-US' },
    ];

    const mockWindow = {
      speechSynthesis: {
        speak: speakMock,
        cancel: cancelMock,
        speaking: false,
        pending: false,
        getVoices: () => mockVoices,
      },
    };

    // @ts-expect-error Mock window and utterance in test environment
    globalThis.window = mockWindow;
    // @ts-expect-error Mock SpeechSynthesisUtterance
    globalThis.SpeechSynthesisUtterance = MockSpeechSynthesisUtterance;

    const onEnd = vi.fn();
    playPronunciation('Hello world', null, onEnd, { voice: 'am_adam' });

    expect(speakMock).toHaveBeenCalledTimes(1);
    const utterance = speakMock.mock.calls[0][0] as MockSpeechSynthesisUtterance;
    expect(utterance.voice?.name).toBe('Microsoft David Desktop - English (United States)');
  });
});
