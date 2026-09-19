import { invoke } from '@tauri-apps/api/core';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { currentMonitor, getCurrentWindow, PhysicalPosition } from '@tauri-apps/api/window';
import { suppressAutoHide } from './autoHide';

/**
 * Open or bring the dedicated settings window to the front dynamically.
 *
 * Suppresses the overlay's auto-hide first so focusing the settings window
 * does not dismiss the overlay that triggered it.
 */
export async function openSettingsWindow(): Promise<void> {
  suppressAutoHide();
  try {
    await invoke('open_settings_window');
  } catch (error) {
    console.error('Failed to open settings window:', error);
  }
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

/**
 * Atomically resize the current window and clamp it within the screen's safe work area.
 * Executes natively in a single DWM compositor transaction, eliminating edge jitter and overflow.
 */
export async function resizeAndClampWindow(width: number, height: number): Promise<void> {
  try {
    await invoke('resize_and_clamp_window', { width, height });
  } catch {
    // Fallback for mocked/unsupported environments
    try {
      await getCurrentWindow().setSize(new LogicalSize(width, height));
      await clampCurrentWindow();
    } catch (error) {
      console.error('Failed to resize and clamp window:', error);
    }
  }
}

/**
 * Configure OS-level DWM Acrylic frosted glass effect on the current window.
 */
export async function setWindowAcrylic(enabled: boolean): Promise<void> {
  try {
    await invoke('set_window_acrylic', { enabled });
  } catch (error) {
    console.warn('Failed to set window acrylic effect:', error);
  }
}

/**
 * Configure DWM theme (dark/light) to coordinate backdrop material tinting.
 */
export async function setWindowTheme(dark: boolean): Promise<void> {
  try {
    await invoke('set_window_theme', { dark });
  } catch (error) {
    console.warn('Failed to set window theme:', error);
  }
}
