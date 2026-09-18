import React, { useMemo } from 'react';
import { useAppStore } from '../store/useAppStore';
import { cn } from '../utils/cn';
import { tokenizeSentence } from '../utils/tokenize';

interface InteractiveSentenceProps {
  sentence: string;
  currentWord?: string;
  onWordClick?: (word: string) => void;
  className?: string;
  /** Render word tokens in the muted colour (used for the source snippet). */
  muted?: boolean;
}

export const InteractiveSentence: React.FC<InteractiveSentenceProps> = ({
  sentence,
  currentWord,
  onWordClick,
  className,
  muted = false,
}) => {
  const pushLookup = useAppStore((state) => state.pushLookup);

  // Tokenize into ASCII words, punctuation, and whitespace. CJK text simply
  // falls into the non-word runs and is rendered verbatim.
  const tokens = useMemo(() => tokenizeSentence(sentence), [sentence]);

  const handleWordClick = (token: string, e: React.MouseEvent) => {
    e.stopPropagation();
    // A non-empty selection means the user is selecting/copying, not tapping.
    if (window.getSelection()?.toString()) return;

    const word = token.replace(/^[^\w]+|[^\w]+$/g, '').trim();
    if (!word) return;

    if (onWordClick) {
      onWordClick(word);
    } else {
      void pushLookup(word, { text: sentence, word });
    }
  };

  return (
    <p className={cn('select-text text-sm leading-relaxed tracking-normal', className)}>
      {tokens.map((token, index) => {
        if (!token.isWord) {
          // Punctuation / whitespace / CJK runs: keep the inherited colour so
          // translated Chinese is never dimmed.
          return <span key={index}>{token.text}</span>;
        }

        const isCurrent =
          currentWord && token.text.toLowerCase() === currentWord.toLowerCase();

        return (
          <span
            key={index}
            onClick={(e) => handleWordClick(token.text, e)}
            title={`Click to look up "${token.text}"`}
            className={cn(
              '-mx-0.5 inline-block cursor-pointer rounded px-1 py-0.5 transition-all duration-150',
              isCurrent
                ? 'bg-[var(--pos-bg)] font-semibold text-[var(--pos-fg)] underline decoration-[var(--pos-stroke)] underline-offset-4'
                : muted
                  ? 'text-[var(--muted)] hover:bg-[var(--surface-3)] hover:text-[var(--accent-b)]'
                  : 'text-[var(--text)] hover:bg-[var(--surface-3)] hover:text-[var(--accent-b)] active:scale-95',
            )}
          >
            {token.text}
          </span>
        );
      })}
    </p>
  );
};
