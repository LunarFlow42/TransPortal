import type { RegionSelection } from '../types';

/** Raw IPC replies may arrive after Escape and a new snip. Paint only the
 * matching physical frame; the same check guards showing the selector. */
export async function paintFrozenPreview(
  selection: RegionSelection,
  load: (id: number) => Promise<ArrayBuffer>,
  isCurrent: () => boolean,
  paint: (pixels: Uint8ClampedArray<ArrayBuffer>) => void,
): Promise<boolean> {
  const buffer = await load(selection.id);
  if (!isCurrent()) return false;
  if (buffer.byteLength !== selection.bounds.width * selection.bounds.height * 4) {
    throw new Error('冻结画面的尺寸与本次截图不匹配。');
  }
  paint(new Uint8ClampedArray(buffer));
  return true;
}
