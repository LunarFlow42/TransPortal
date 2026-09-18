import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { currentMonitor, getCurrentWindow, PhysicalPosition } from '@tauri-apps/api/window';
import { suppressAutoHide } from './autoHide';

/**
 * Bring the dedicated settings window to the front.
 *
 * Suppresses the overlay's auto-hide first so focusing the settings window
 * does not dismiss the overlay that triggered it.
 */
export async function openSettingsWindow(): Promise<void> {
  suppressAutoHide();
  const settings = await WebviewWindow.getByLabel('settings');
  if (!settings) return;
  await settings.show();
  await settings.setFocus();
}

/** Growing an OCR result/card must not push its bottom below the work area. */
export async function clampCurrentWindow(): Promise<void> {
  const window = getCurrentWindow();
  const [monitor, position, size] = await Promise.all([
    currentMonitor(), window.outerPosition(), window.outerSize(),
  ]);
  if (!monitor) return;
  const area = monitor.workArea ?? monitor;
  const minX = area.position.x + 8;
  const minY = area.position.y + 8;
  const maxX = Math.max(minX, area.position.x + area.size.width - size.width - 8);
  const maxY = Math.max(minY, area.position.y + area.size.height - size.height - 8);
  const x = Math.min(maxX, Math.max(minX, position.x));
  const y = Math.min(maxY, Math.max(minY, position.y));
  if (x !== position.x || y !== position.y) await window.setPosition(new PhysicalPosition(x, y));
}
