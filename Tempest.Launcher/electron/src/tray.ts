// Host-owned tray. The renderer pushes localized, declarative menu state via
// `tray:ensure`/`tray:set-state`; the host owns the TrayIcon and Menu, handles
// the fixed actions (show window, quit) itself and forwards navigation to the
// renderer as a `tray:navigate` event.
import path from "node:path";
import { app, Menu, nativeImage, Tray, type MenuItemConstructorOptions } from "electron";
import { getMainWindow, showMainWindow } from "./app-state.js";
import { handle } from "./ipc/handle.js";
import type { TrayEntry, TrayState } from "./ipc/contract.js";

let tray: Tray | null = null;

function trayImage() {
	const iconPath = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "resources", "icons", "icon.png");
	const size = process.platform === "darwin" ? 22 : 24;
	return nativeImage.createFromPath(iconPath).resize({ width: size, height: size });
}

function quitApp(): void {
	// destroy() bypasses the close guard (hide-to-tray), so this always quits;
	// fall back to app.quit() when the window is already gone.
	const window = getMainWindow();
	if (window && !window.isDestroyed()) {
		window.destroy();
	} else {
		app.quit();
	}
}

function navigate(route: string): void {
	showMainWindow();
	const window = getMainWindow();
	if (window && !window.isDestroyed()) {
		window.webContents.send("tray:navigate", { route });
	}
}

function menuClickAction(entry: Extract<TrayEntry, { kind: "item" }>): () => void {
	return () => {
		if (entry.id === "quit") quitApp();
		else if (entry.id === "open") showMainWindow();
		else if (entry.id.startsWith("nav:")) navigate(entry.id.slice("nav:".length));
	};
}

function applyState(state: TrayState): void {
	if (!tray) return;
	const template: MenuItemConstructorOptions[] = state.entries.map((entry) =>
		entry.kind === "separator"
			? { type: "separator" }
			: {
					label: entry.text,
					enabled: entry.enabled,
					click: menuClickAction(entry),
				},
	);
	tray.setContextMenu(Menu.buildFromTemplate(template));
}

handle("tray:ensure", (_event, { tooltip }) => {
	// Create once and reuse: on Linux, Tray#destroy() never unregisters the
	// StatusNotifierItem (electron/electron#49517), so every dev hot reload
	// would leave a duplicate icon behind if we destroyed and recreated.
	if (!tray) {
		tray = new Tray(trayImage());
		tray.on("click", () => showMainWindow());
	}
	tray.setToolTip(tooltip);
});

handle("tray:set-state", (_event, state) => {
	applyState(state);
});
