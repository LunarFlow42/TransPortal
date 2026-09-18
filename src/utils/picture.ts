import type { OcrBlock, OcrPicture, PictureTranslation } from '../types';

/** Wrap without dropping code points, keeping ordinary words together when possible. */
export function wrapPictureText(text: string, width: number, measure: (text: string) => number): string[] {
  const lines: string[] = [];
  for (const paragraph of text.replace(/\r\n?/g, '\n').split('\n')) {
    let line = '';
    for (const character of Array.from(paragraph)) {
      if (line && measure(line + character) > width) {
        const space = line.lastIndexOf(' ');
        if (space > 0 && character !== ' ' && measure(line.slice(space + 1) + character) <= width) {
          lines.push(line.slice(0, space));
          line = line.slice(space + 1) + character;
        } else {
          lines.push(line.trimEnd());
          line = character === ' ' ? '' : character;
        }
      } else line += character;
    }
    lines.push(line.trimEnd());
  }
  return lines;
}

function backgroundColor(pixels: ImageData, block: OcrBlock): [number, number, number] {
  const { width, height, data } = pixels;
  const bins = new Map<number, { count: number; r: number; g: number; b: number }>();
  const sample = (x: number, y: number) => {
    x = Math.max(0, Math.min(width - 1, Math.round(x)));
    y = Math.max(0, Math.min(height - 1, Math.round(y)));
    const offset = (y * width + x) * 4;
    const alpha = data[offset + 3] / 255;
    const r = Math.round(data[offset] * alpha + 255 * (1 - alpha));
    const g = Math.round(data[offset + 1] * alpha + 255 * (1 - alpha));
    const b = Math.round(data[offset + 2] * alpha + 255 * (1 - alpha));
    const key = (r >> 4) * 256 + (g >> 4) * 16 + (b >> 4);
    const bin = bins.get(key) ?? { count: 0, r: 0, g: 0, b: 0 };
    bin.count++; bin.r += r; bin.g += g; bin.b += b;
    bins.set(key, bin);
  };
  // A thin ring outside the detected ink estimates the local background.
  for (let step = 0; step <= 32; step++) {
    const x = block.x + block.width * step / 32;
    const y = block.y + block.height * step / 32;
    sample(x, block.y - 2); sample(x, block.y + block.height + 2);
    sample(block.x - 2, y); sample(block.x + block.width + 2, y);
  }
  const most = [...bins.values()].sort((a, b) => b.count - a.count)[0];
  return [Math.round(most.r / most.count), Math.round(most.g / most.count), Math.round(most.b / most.count)];
}

export async function renderPicture(
  canvas: HTMLCanvasElement, picture: OcrPicture, translation: PictureTranslation | null,
): Promise<void> {
  const source = new Image();
  source.src = picture.data_url;
  await source.decode();
  canvas.width = picture.width;
  canvas.height = picture.height;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('无法绘制图片。');
  context.drawImage(source, 0, 0, picture.width, picture.height);
  if (!translation) return;
  const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
  const translated = new Map(translation.blocks.map((block) => [block.id, block]));
  for (const block of picture.blocks) {
    const result = translated.get(block.id);
    if (!result?.text.trim() || result.error || block.width < 1 || block.height < 1) continue;
    const color = backgroundColor(pixels, block);
    context.save();
    context.beginPath();
    context.rect(Math.max(0, block.x - 1), Math.max(0, block.y - 1), block.width + 2, block.height + 2);
    context.clip();
    context.fillStyle = `rgb(${color.join(',')})`;
    context.fillRect(block.x - 1, block.y - 1, block.width + 2, block.height + 2);
    context.fillStyle = color[0] * 0.299 + color[1] * 0.587 + color[2] * 0.114 > 145 ? '#121212' : '#fafafa';
    const font = (size: number) => `${size}px "Segoe UI", "Microsoft YaHei", sans-serif`;
    let low = 1;
    let high = Math.min(64, block.height / 1.15);
    let lines = [result.text];
    for (let iteration = 0; iteration < 14; iteration++) {
      const size = (low + high) / 2;
      context.font = font(size);
      const wrapped = wrapPictureText(result.text, block.width, (text) => context.measureText(text).width);
      if (wrapped.length * size * 1.15 <= block.height) low = size;
      else high = size;
    }
    context.font = font(low);
    lines = wrapPictureText(result.text, block.width, (text) => context.measureText(text).width);
    const lineHeight = Math.min(low * 1.15, block.height / lines.length);
    context.textBaseline = 'middle';
    const top = block.y + (block.height - lineHeight * lines.length) / 2;
    lines.forEach((line, index) => context.fillText(line, block.x, top + (index + 0.5) * lineHeight, block.width));
    context.restore();
  }
}
