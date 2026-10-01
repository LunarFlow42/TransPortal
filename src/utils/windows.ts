import { invoke } from '@tauri-apps/api/core';
import { LogicalSize } from '@tauri-apps/api/dpi';
import { currentMonitor, getCurrentWindow, PhysicalPosition } from '@tauri-apps/api/window';

/**
 * Open or bring the dedicated settings window to the front dynamically.
 * Backend automatically hides the shell overlay while settings is open.
 */
export async function openSettingsWindow(): Promise<void> {
  try {
    await invoke('open_settings_window');
  } catch (error) {
    console.error('Failed to open settings window:', error);
  }
}

/**
 * Close or hide the dedicated settings window, automatically restoring
 * the shell overlay if it was open prior to opening settings.
 */
export async function closeSettingsWindow(): Promise<void> {
  try {
    await invoke('close_settings_window');
  } catch {
    try {
      await getCurrentWindow().hide();
    } catch (error) {
      console.error('Failed to close settings window:', error);
    }
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
 * Configure DWM theme (dark/light) to coordinate backdrop material tinting.
 */
export async function setWindowTheme(dark: boolean): Promise<void> {
  try {
    await invoke('set_window_theme', { dark });
  } catch (error) {
    console.warn('Failed to set window theme:', error);
  }
}
