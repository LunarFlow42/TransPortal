import React, { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  ChevronLeft,
  ChevronRight,
  Maximize2,
  Minimize2,
  Volume2,
  Copy,
  Check,
  Zap,
  Download,
  Sparkles,
  BookOpen,
} from 'lucide-react';
import { useAppStore } from '../store/useAppStore';
import { InteractiveSentence } from './InteractiveSentence';

export const MorphingContainer: React.FC = () => {
  const {
    mode,
    setMode,
    queryText,
    wordData,
    translation,
    streamingText,
    isStreaming,
    isLoading,
    history,
    historyIndex,
    goBack,
    goForward,
    ankiStatus,
    ankiMessage,
    syncToAnki,
    exportTsv,
  } = useAppStore();

  const [copied, setCopied] = useState(false);
  const [isPlayingAudio, setIsPlayingAudio] = useState(false);

  // Play audio pronunciation via Web Speech API or audio URL
  const playAudio = (word: string, audioUrl?: string) => {
    setIsPlayingAudio(true);
    if (audioUrl) {
      const audio = new Audio(audioUrl);
      audio.onended = () => setIsPlayingAudio(false);
      audio.onerror = () => {
        speakWebSpeech(word);
      };
      audio.play().catch(() => speakWebSpeech(word));
    } else {
      speakWebSpeech(word);
    }
  };

  const speakWebSpeech = (word: string) => {
    if ('speechSynthesis' in window) {
      window.speechSynthesis.cancel();
      const utterance = new SpeechSynthesisUtterance(word);
      utterance.lang = 'en-US';
      utterance.rate = 0.9;
      utterance.onend = () => setIsPlayingAudio(false);
      utterance.onerror = () => setIsPlayingAudio(false);
      window.speechSynthesis.speak(utterance);
    } else {
      setIsPlayingAudio(false);
    }
  };

  const handleCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const currentDisplayWord = wordData?.word || queryText;
  const displayText = streamingText || translation || 'Translating...';

  return (
    <motion.div
      layout
      transition={{
        type: 'spring',
        stiffness: 350,
        damping: 28,
        mass: 0.8,
      }}
      className={`
        relative overflow-hidden rounded-2xl border border-white/10
        bg-[#141416]/85 backdrop-blur-xl text-slate-100 shadow-2xl shadow-black/70
        transition-colors duration-200
        ${mode === 'bubble' ? 'w-full max-w-[420px] p-3.5' : 'w-full max-w-[460px] p-4'}
      `}
    >
      {/* ========================================================================= */}
      {/* 1. GHOST BUBBLE MODE                                                      */}
      {/* ========================================================================= */}
      {mode === 'bubble' ? (
        <motion.div
          key="bubble-content"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.15 }}
          className="flex flex-col gap-2.5"
        >
          {/* Header Bar */}
          <div
            data-tauri-drag-region
            className="flex items-center justify-between cursor-move pb-1 border-b border-white/5"
          >
            <div className="flex items-center gap-2">
              <span className="relative flex h-2 w-2">
                <span
                  className={`animate-ping absolute inline-flex h-full w-full rounded-full ${
                    isStreaming ? 'bg-cyan-400 opacity-75' : 'bg-emerald-400 opacity-50'
                  }`}
                />
                <span
                  className={`relative inline-flex rounded-full h-2 w-2 ${
                    isStreaming ? 'bg-cyan-500' : 'bg-emerald-500'
                  }`}
                />
              </span>
              <span className="text-[11px] font-medium tracking-wide uppercase text-slate-400">
                Ghost Bubble
              </span>
            </div>

            {/* Bubble Action Controls */}
            <div className="flex items-center gap-1 text-slate-400">
              <button
                onClick={() => handleCopy(displayText)}
                title="Copy Translation"
                className="p-1 hover:text-white hover:bg-white/10 rounded transition-colors"
              >
                {copied ? (
                  <Check className="w-3.5 h-3.5 text-emerald-400" />
                ) : (
                  <Copy className="w-3.5 h-3.5" />
                )}
              </button>

              {/* Morph to Card */}
              <button
                onClick={() => setMode('card')}
                title="Expand to Word Card"
                className="p-1 hover:text-white hover:bg-white/10 rounded transition-colors"
              >
                <Maximize2 className="w-3.5 h-3.5 text-cyan-400" />
              </button>
            </div>
          </div>

          {/* Source Snippet */}
          <p className="text-xs text-slate-400 line-clamp-1 italic px-0.5">
            "{queryText || 'Select or enter text...'}"
          </p>

          {/* Translation Result / Streaming Stream */}
          <div className="text-sm font-medium text-white leading-relaxed select-text px-0.5 min-h-[22px]">
            {isLoading && !streamingText ? (
              <span className="text-slate-500 animate-pulse">Analyzing context...</span>
            ) : (
              <span>
                {displayText}
                {isStreaming && (
                  <span className="inline-block w-1.5 h-3.5 ml-1 bg-cyan-400 animate-pulse align-middle" />
                )}
              </span>
            )}
          </div>
        </motion.div>
      ) : (
        /* ========================================================================= */
        /* 2. DEEP WORD CARD MODE                                                    */
        /* ========================================================================= */
        <motion.div
          key="card-content"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
          className="flex flex-col gap-3 max-h-[380px]"
        >
          {/* Top Navigation & Action Header */}
          <div
            data-tauri-drag-region
            className="flex items-center justify-between cursor-move pb-2 border-b border-white/10"
          >
            {/* History Navigation Stack */}
            <div className="flex items-center gap-1">
              <button
                onClick={goBack}
                disabled={historyIndex <= 0}
                title="Go Back"
                className="p-1 rounded text-slate-400 hover:text-white hover:bg-white/10 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              >
                <ChevronLeft className="w-4 h-4" />
              </button>
              <button
                onClick={goForward}
                disabled={historyIndex >= history.length - 1}
                title="Go Forward"
                className="p-1 rounded text-slate-400 hover:text-white hover:bg-white/10 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
              >
                <ChevronRight className="w-4 h-4" />
              </button>

              {history.length > 0 && (
                <span className="text-[11px] text-slate-400 px-1.5 py-0.5 bg-white/5 rounded-md font-mono">
                  {historyIndex + 1}/{history.length}
                </span>
              )}
            </div>

            {/* Actions: Anki Sync & Minimize */}
            <div className="flex items-center gap-1.5">
              {/* Anki Sync */}
              <button
                onClick={() => syncToAnki()}
                disabled={ankiStatus === 'syncing'}
                title="Sync to Anki deck"
                className={`
                  flex items-center gap-1 px-2 py-1 rounded-md text-xs font-medium transition-colors
                  ${
                    ankiStatus === 'success'
                      ? 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/40'
                      : ankiStatus === 'error'
                      ? 'bg-rose-500/20 text-rose-300 border border-rose-500/40'
                      : 'bg-indigo-600/30 hover:bg-indigo-600/50 text-indigo-200 border border-indigo-500/30'
                  }
                `}
              >
                <Zap className="w-3 h-3" />
                <span>
                  {ankiStatus === 'syncing'
                    ? 'Syncing...'
                    : ankiStatus === 'success'
                    ? 'Synced!'
                    : 'Anki'}
                </span>
              </button>

              {/* TSV Export */}
              <button
                onClick={() => exportTsv()}
                title="Export TSV for Anki"
                className="p-1 text-slate-400 hover:text-white hover:bg-white/10 rounded transition-colors"
              >
                <Download className="w-3.5 h-3.5" />
              </button>

              {/* Collapse to Bubble */}
              <button
                onClick={() => setMode('bubble')}
                title="Collapse to Ghost Bubble"
                className="p-1 text-slate-400 hover:text-white hover:bg-white/10 rounded transition-colors"
              >
                <Minimize2 className="w-3.5 h-3.5 text-cyan-400" />
              </button>
            </div>
          </div>

          {/* Anki Status Toast (if any) */}
          <AnimatePresence>
            {ankiMessage && (
              <motion.div
                initial={{ opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -4 }}
                className={`text-[11px] px-2.5 py-1 rounded-md ${
                  ankiStatus === 'error'
                    ? 'bg-rose-500/20 text-rose-300 border border-rose-500/30'
                    : 'bg-emerald-500/20 text-emerald-300 border border-emerald-500/30'
                }`}
              >
                {ankiMessage}
              </motion.div>
            )}
          </AnimatePresence>

          {/* Scrollable Dictionary Details Body */}
          <div className="flex flex-col gap-3 overflow-y-auto pr-1 select-text">
            {/* Headword, Phonetic & Audio */}
            <div className="flex items-baseline justify-between gap-2">
              <div className="flex items-baseline gap-2.5 flex-wrap">
                <h1 className="text-2xl font-bold tracking-tight text-white capitalize">
                  {currentDisplayWord}
                </h1>
                {wordData?.phonetic && (
                  <span className="text-sm font-mono text-cyan-300/90">
                    {wordData.phonetic}
                  </span>
                )}
                <button
                  onClick={() => playAudio(currentDisplayWord, wordData?.audio_url)}
                  title="Pronounce"
                  className={`p-1 rounded-full text-slate-400 hover:text-cyan-300 hover:bg-white/10 transition-colors ${
                    isPlayingAudio ? 'text-cyan-300 animate-pulse' : ''
                  }`}
                >
                  <Volume2 className="w-4 h-4" />
                </button>
              </div>

              {/* Tags */}
              {wordData?.tags && (
                <div className="flex items-center gap-1 flex-wrap">
                  {wordData.tags.map((tag) => (
                    <span
                      key={tag}
                      className="text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded bg-white/10 text-slate-300"
                    >
                      {tag}
                    </span>
                  ))}
                </div>
              )}
            </div>

            {/* Exchange Forms (if available) */}
            {wordData?.exchange && (
              <div className="text-[11px] text-slate-400 font-mono">
                {wordData.exchange.split('/').map((item, idx) => (
                  <span key={idx} className="mr-2">
                    {item}
                  </span>
                ))}
              </div>
            )}

            {/* Definitions with POS Pill Badges */}
            <div className="flex flex-col gap-2">
              {wordData?.definitions && wordData.definitions.length > 0 ? (
                wordData.definitions.map((def, idx) => (
                  <div key={idx} className="flex items-start gap-2 text-sm leading-snug">
                    <span className="shrink-0 px-2 py-0.5 text-[11px] font-bold rounded-md bg-indigo-500/20 text-indigo-300 border border-indigo-500/30">
                      {def.pos}
                    </span>
                    <span className="text-slate-200">{def.meanings.join('；')}</span>
                  </div>
                ))
              ) : (
                <div className="text-sm text-slate-200">
                  {translation || displayText}
                </div>
              )}
            </div>

            {/* Context Sentences with Interactive Tokenizer */}
            {wordData?.examples && wordData.examples.length > 0 && (
              <div className="flex flex-col gap-2 pt-2 border-t border-white/10">
                <div className="flex items-center justify-between text-[11px] text-slate-400 font-medium">
                  <div className="flex items-center gap-1 text-cyan-300/80">
                    <BookOpen className="w-3 h-3" />
                    <span>Context & Drill-down</span>
                  </div>
                  <span className="text-[10px] text-slate-500 italic">Click words to drill down</span>
                </div>

                <div className="flex flex-col gap-2.5">
                  {wordData.examples.map((ex, idx) => (
                    <div
                      key={idx}
                      className="p-2 rounded-lg bg-white/5 hover:bg-white/[0.07] border border-white/5 transition-colors"
                    >
                      <InteractiveSentence
                        sentence={ex.en}
                        currentWord={currentDisplayWord}
                      />
                      <p className="text-xs text-slate-400 mt-1">{ex.zh}</p>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* AI Async Enhancement & Context Extension */}
            {(isStreaming || streamingText) && (
              <div className="flex flex-col gap-1.5 p-2.5 rounded-lg bg-gradient-to-br from-indigo-950/30 to-purple-950/30 border border-indigo-500/20 mt-1">
                <div className="flex items-center gap-1.5 text-xs font-semibold text-indigo-300">
                  <Sparkles className="w-3.5 h-3.5 text-indigo-400 animate-spin-slow" />
                  <span>AI Context Enhancement</span>
                  {isStreaming && (
                    <span className="inline-block w-1.5 h-1.5 rounded-full bg-indigo-400 animate-ping" />
                  )}
                </div>
                <p className="text-xs text-slate-300 leading-relaxed select-text">
                  {streamingText}
                  {isStreaming && (
                    <span className="inline-block w-1 h-3 ml-0.5 bg-indigo-400 animate-pulse align-middle" />
                  )}
                </p>
              </div>
            )}
          </div>
        </motion.div>
      )}
    </motion.div>
  );
};
