import { describe, expect, it, vi } from 'vitest';
import type { RegionSelection } from '../types';
import { paintFrozenPreview } from './frozenPreview';

const selection = (id: number): RegionSelection => ({ id, purpose: 'text', bounds: { x: -80, y: 40, width: 2, height: 2 } });

describe('game screenshot preview', () => {
  it('keeps the current scene when an equally sized previous snip finishes its IPC copy late', async () => {
    let current = 1;
    let release!: (frame: ArrayBuffer) => void;
    const late = new Promise<ArrayBuffer>((resolve) => { release = resolve; });
    const paint = vi.fn();
    const a = paintFrozenPreview(selection(1), () => late, () => current === 1, paint);
    current = 2; // Esc, then a new snip after the dialogue advances
    const sceneB = new Uint8ClampedArray(16).fill(0xb);
    expect(await paintFrozenPreview(selection(2), async () => sceneB.buffer, () => current === 2, paint)).toBe(true);
    release(new Uint8ClampedArray(16).fill(0xa).buffer);
    expect(await a).toBe(false);
    expect(paint.mock.calls).toEqual([[sceneB]]);
  });

  it('never enables selection on a partial frame', async () => {
    const paint = vi.fn();
    await expect(paintFrozenPreview(selection(2), async () => new ArrayBuffer(12), () => true, paint)).rejects.toThrow();
    expect(paint).not.toHaveBeenCalled();
  });
});
