import React, { useMemo } from 'react';
import { useAppStore } from '../store/useAppStore';

interface InteractiveSentenceProps {
  sentence: string;
  currentWord?: string;
  onWordClick?: (word: string) => void;
  className?: string;
}

export const InteractiveSentence: React.FC<InteractiveSentenceProps> = ({
  sentence,
  currentWord,
  onWordClick,
  className = '',
}) => {
  const drillDown = useAppStore((state) => state.drillDown);

  // Tokenize the sentence into words, punctuation, and whitespace
  const tokens = useMemo(() => {
    // Regex splits by word characters (including hyphens and apostrophes) vs non-word tokens
    const regex = /([a-zA-Z0-9'-]+|[^\s\w]+|\s+)/g;
    return sentence.match(regex) || [sentence];
  }, [sentence]);

  const handleWordClick = (token: string, e: React.MouseEvent) => {
    e.stopPropagation();
    const cleanWord = token.replace(/^[^\w]+|[^\w]+$/g, '').trim();
    if (!cleanWord) return;

    if (onWordClick) {
      onWordClick(cleanWord);
    } else {
      drillDown(cleanWord);
    }
  };

  const isWordToken = (token: string) => /^[a-zA-Z0-9'-]+$/.test(token);

  return (
    <p className={`text-sm leading-relaxed tracking-normal select-text ${className}`}>
      {tokens.map((token, index) => {
        if (!isWordToken(token)) {
          // Punctuation or whitespace
          return (
            <span key={index} className="text-slate-400">
              {token}
            </span>
          );
        }

        const isCurrent =
          currentWord && token.toLowerCase() === currentWord.toLowerCase();

        return (
          <span
            key={index}
            onClick={(e) => handleWordClick(token, e)}
            title={`Click to inspect "${token}"`}
            className={`
              inline-block rounded px-1 py-0.5 -mx-0.5 cursor-pointer transition-all duration-150
              ${
                isCurrent
                  ? 'text-indigo-300 font-semibold bg-indigo-500/20 underline decoration-indigo-400/60 underline-offset-4'
                  : 'text-slate-200 hover:text-cyan-200 hover:bg-cyan-500/20 hover:shadow-xs active:scale-95'
              }
            `}
          >
            {token}
          </span>
        );
      })}
    </p>
  );
};
