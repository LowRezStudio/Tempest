// Shared window registry: the main window is created in main.ts but also
// needed by tray.ts (show/quit/navigate) without import cycles at call time.
import type { BrowserWindow } from "electron";

let mainWindow: BrowserWindow | null = null;

export function setMainWindow(window: BrowserWindow | null): void {
	mainWindow = window;
}

export function getMainWindow(): BrowserWindow | null {
	return mainWindow;
}

/** Shows and focuses the main window, restoring it from the tray/minimize. */
export function showMainWindow(): void {
	const window = getMainWindow();
	if (!window || window.isDestroyed()) return;
	if (window.isMinimized()) window.restore();
	window.show();
	window.focus();
}
