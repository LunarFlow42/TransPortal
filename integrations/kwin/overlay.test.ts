import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./transportal-overlay/contents/code/main.js', import.meta.url), 'utf8');
function signal() {
  const listeners: (() => void)[] = [];
  return { connect: (callback: () => void) => listeners.push(callback), emit: () => listeners.forEach((callback) => callback()) };
}
function fixture(app = 'com.transportal.app', caption = 'TransPortal Overlay [1]', area = { x: -1920, y: 0, width: 1920, height: 800 }) {
  const window = {
    resourceClass: app, captionNormal: caption, minimized: false, deleted: false,
    frameGeometry: { x: 0, y: 0, width: 300, height: 200 },
    captionChanged: signal(), minimizedChanged: signal(),
  };
  const workspace = {
    cursorPos: { x: -20, y: 780 }, currentDesktop: {}, activeWindow: { id: 'editor' },
    screenAt: () => ({}),
    clientArea: () => area,
    windowAdded: signal(), windowList: () => [window],
  };
  runInNewContext(source, { workspace, KWin: { PlacementArea: 0 } });
  return { window, workspace };
}
describe('optional Plasma 6 cursor overlay script', () => {
  it('flips at negative-coordinate output edges and does not activate windows', () => {
    const { window, workspace } = fixture();
    expect(window.frameGeometry).toEqual({ x: -332, y: 568, width: 300, height: 200 });
    expect(workspace.activeWindow).toEqual({ id: 'editor' });
  });
  it('repositions on a fresh request marker, not on every cursor move', () => {
    const { window, workspace } = fixture();
    workspace.cursorPos = { x: -1800, y: 20 };
    expect(window.frameGeometry.x).toBe(-332);
    window.captionNormal = 'TransPortal Overlay [2]';
    window.captionChanged.emit();
    expect(window.frameGeometry).toEqual({ x: -1788, y: 32, width: 300, height: 200 });
  });
  it('never changes settings, manual main windows, OCR, or other applications', () => {
    for (const [app, caption] of [
      ['com.transportal.app', 'TransPortal Settings'], ['com.transportal.app', 'TransPortal'],
      ['com.transportal.app', 'TransPortal Region'], ['other', 'TransPortal Overlay [1]'],
      ['transportal', 'TransPortal Overlay [1] extra'],
    ]) {
      const { window } = fixture(app, caption);
      expect(window.frameGeometry.x).toBe(0);
    }
  });
  it('fits a large overlay into a small output work area', () => {
    const { window } = fixture('com.transportal.app', 'TransPortal Overlay [3]', { x: 10, y: 20, width: 180, height: 140 });
    expect(window.frameGeometry).toEqual({ x: 18, y: 28, width: 164, height: 124 });
  });
});
