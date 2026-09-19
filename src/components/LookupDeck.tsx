import React from 'react';
import { motion } from 'framer-motion';
import { LookupCard } from './LookupCard';
import { useAppStore } from '../store/useAppStore';
import { cn } from '../utils/cn';

/**
 * How many frames are physically mounted. Deeper history lives only in the
 * store, so a dozen-level drill-down costs exactly the same DOM as three.
 */
const VISIBLE_DEPTH = 3;

interface LookupDeckProps {
  fillHeight?: boolean;
}

/**
 * Renders the drill-down lookup stack as a deck of cards: the top frame is
 * interactive, the ones beneath peek out from behind with a scale/dim offset
 * to convey depth.
 */
export const LookupDeck: React.FC<LookupDeckProps> = ({ fillHeight = false }) => {
  const stack = useAppStore((state) => state.lookupStack);
  const visible = stack.slice(-VISIBLE_DEPTH);
  const offset = stack.length - visible.length;

  return (
    <div className={cn('relative w-full pt-3', fillHeight && 'h-full flex flex-col')}>
      {visible.map((node, i) => {
        const depth = visible.length - 1 - i; // 0 = top frame
        const stackIndex = offset + i;
        const isTop = depth === 0;

        return (
          <motion.div
            key={node.id}
            initial={isTop ? { opacity: 0, y: 14, scale: 0.98 } : false}
            animate={{
              opacity: isTop ? 1 : Math.max(0.16, 0.5 - depth * 0.17),
              y: -depth * 8,
              scale: 1 - depth * 0.04,
            }}
            transition={{ type: 'spring', stiffness: 380, damping: 32 }}
            style={{
              zIndex: 30 - depth,
              transformOrigin: 'top center',
              pointerEvents: isTop ? 'auto' : 'none',
            }}
            className={cn(
              isTop ? 'relative' : 'absolute inset-x-0 top-3',
              fillHeight && isTop && 'flex-1 flex flex-col min-h-0',
            )}
            aria-hidden={!isTop}
          >
            <LookupCard
              node={node}
              stack={stack}
              stackIndex={stackIndex}
              isTop={isTop}
              fillHeight={fillHeight}
            />
          </motion.div>
        );
      })}
    </div>
  );
};
