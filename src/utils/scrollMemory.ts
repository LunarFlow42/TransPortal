/**
 * Per-frame scroll offsets for the lookup stack.
 *
 * Kept outside React state so scrolling never triggers re-renders; the offset
 * is written on scroll and restored when a frame becomes the top card again.
 */
const positions = new Map<number, number>();

export function rememberScrollTop(id: number, top: number): void {
  positions.set(id, top);
}

export function recallScrollTop(id: number): number {
  return positions.get(id) ?? 0;
}

export function forgetScrollTops(): void {
  positions.clear();
}
