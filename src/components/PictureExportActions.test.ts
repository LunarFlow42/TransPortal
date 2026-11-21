import React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { PictureExportActions } from './PictureExportActions';
import { renderPicture } from '../utils/picture';
import { resumeAutoHide, suppressAutoHide } from '../utils/autoHide';
import type { OcrPicture, PictureTranslation } from '../types';

const lifecycle = vi.hoisted(() => ({ cursor: 0, values: [] as unknown[] }));
vi.mock('react', async (original) => ({
  ...await original<typeof import('react')>(),
  useState: (initial: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = initial;
    return [lifecycle.values[index], (value: unknown) => { lifecycle.values[index] = value; }];
  },
  useRef: (current: unknown) => {
    const index = lifecycle.cursor++;
    if (!(index in lifecycle.values)) lifecycle.values[index] = { current };
    return lifecycle.values[index];
  },
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: vi.fn() }));
vi.mock('../utils/picture', () => ({ renderPicture: vi.fn() }));
vi.mock('../utils/autoHide', () => ({ suppressAutoHide: vi.fn(), resumeAutoHide: vi.fn() }));

const picture: OcrPicture = {
  width: 1920, height: 1080, data_url: 'data:image/png;base64,original',
  blocks: [{ id: 0, text: 'Hello', x: 10, y: 10, width: 100, height: 30 }],
};
const translation: PictureTranslation = { target_lang: 'zh-CN', blocks: [{ id: 0, text: '你好', error: null }] };
const png = 'data:image/png;base64,translated';
const canvas = { toDataURL: vi.fn(() => png) };
type Props = React.ComponentProps<typeof PictureExportActions>;
interface ElementProps { children?: React.ReactNode; onClick?: () => void; disabled?: boolean; role?: string }
function elements(node: React.ReactNode): React.ReactElement<ElementProps>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!React.isValidElement<ElementProps>(node)) return [];
  return [node, ...elements(node.props.children)];
}
function render(overrides: Partial<Props> = {}) {
  lifecycle.cursor = 0;
  const tree = PictureExportActions({ picture, translation, ...overrides });
  if (tree instanceof Promise) throw new Error('Expected a synchronous component');
  return elements(tree);
}
const button = (index: number, overrides: Partial<Props> = {}) => render(overrides).filter((element) => element.props.onClick)[index];
const finish = () => vi.waitFor(() => expect(button(0).props.disabled).toBe(false));

beforeEach(() => {
  vi.clearAllMocks();
  lifecycle.values.length = 0;
  lifecycle.cursor = 0;
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
  vi.mocked(save).mockReset().mockResolvedValue('D:\\translated.png');
  vi.mocked(renderPicture).mockReset().mockResolvedValue(undefined);
  vi.stubGlobal('document', { createElement: vi.fn(() => canvas) });
});

afterEach(() => vi.unstubAllGlobals());

describe('translated screenshot exports', () => {
  it('copies a freshly rendered full-resolution translation through the native clipboard command', async () => {
    button(0).props.onClick!();
    await finish();
    expect(renderPicture).toHaveBeenCalledExactlyOnceWith(canvas, picture, translation);
    expect(canvas.toDataURL).toHaveBeenCalledWith('image/png');
    expect(invoke).toHaveBeenCalledExactlyOnceWith('export_ocr_picture', { dataUrl: png, path: null });
    expect(save).not.toHaveBeenCalled();
    expect(render().find((element) => element.props.role === 'status')?.props.children).toBe('译图已复制');
    expect(suppressAutoHide).toHaveBeenCalledWith(Infinity);
    expect(resumeAutoHide).toHaveBeenCalledOnce();
  });

  it('saves the translation as PNG to the path selected in the native dialog', async () => {
    button(1).props.onClick!();
    await finish();
    expect(save).toHaveBeenCalledWith({ defaultPath: 'TransPortal-translation.png', filters: [{ name: 'PNG 图片', extensions: ['png'] }] });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('export_ocr_picture', { dataUrl: png, path: 'D:\\translated.png' });
    expect(render().find((element) => element.props.role === 'status')?.props.children).toBe('译图已保存');
  });

  it('cancelling the save dialog never renders or exports an image and releases the focus hold', async () => {
    vi.mocked(save).mockResolvedValue(null);
    button(1).props.onClick!();
    await finish();
    expect(renderPicture).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
    expect(resumeAutoHide).toHaveBeenCalledOnce();
    expect(render().some((element) => element.props.role === 'status')).toBe(false);
  });

  it('prevents duplicate operations even before the next render', async () => {
    let resolve!: () => void;
    vi.mocked(invoke).mockReturnValue(new Promise<void>((accept) => { resolve = accept; }));
    const click = button(0).props.onClick!;
    click();
    click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    expect(button(0).props.disabled).toBe(true);
    expect(button(1).props.disabled).toBe(true);
    resolve();
    await finish();
  });

  it.each(['render', 'native', 'dialog'])('reports a %s failure and allows retrying', async (stage) => {
    if (stage === 'render') vi.mocked(renderPicture).mockRejectedValueOnce(new Error('failed'));
    if (stage === 'native') vi.mocked(invoke).mockRejectedValueOnce(new Error('failed'));
    if (stage === 'dialog') vi.mocked(save).mockRejectedValueOnce(new Error('failed'));
    button(stage === 'dialog' ? 1 : 0).props.onClick!();
    await finish();
    expect(render().find((element) => element.props.role === 'alert')?.props.children).toContain('failed');
    expect(resumeAutoHide).toHaveBeenCalledOnce();
    button(0).props.onClick!();
    await finish();
    expect(render().find((element) => element.props.role === 'status')?.props.children).toBe('译图已复制');
  });

  it.each([
    { picture: null },
    { translation: null },
    { busy: true },
    { translation: { ...translation, blocks: [{ id: 0, text: '', error: 'unavailable' }] } },
  ])('disables exports when an image or completed translation is unavailable (%j)', async (overrides) => {
    expect(button(0, overrides).props.disabled).toBeTruthy();
    expect(button(1, overrides).props.disabled).toBeTruthy();
    button(0, overrides).props.onClick!();
    await Promise.resolve();
    expect(invoke).not.toHaveBeenCalled();
  });
});
