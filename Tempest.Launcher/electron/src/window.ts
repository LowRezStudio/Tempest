import { BrowserWindow, Menu, type MenuItemConstructorOptions } from "electron";
import { handle } from "./ipc/handle.js";
import type { ContextMenuAction } from "./ipc/contract.js";

const windowFrom = (event: Electron.IpcMainInvokeEvent) =>
	BrowserWindow.fromWebContents(event.sender);

handle("window:destroy", (event) => {
	windowFrom(event)?.destroy();
});

handle("window:hide", (event) => {
	windowFrom(event)?.hide();
});

handle("window:show", (event) => {
	windowFrom(event)?.show();
});

handle("window:set-focus", (event) => {
	windowFrom(event)?.focus();
});

handle("window:unminimize", (event) => {
	windowFrom(event)?.restore();
});

// Native roles handle editable fields; virtualized logs need renderer actions to include hidden rows.
handle("window:context-menu", (event, { editable, canCopy, canSelectAll, labels }) => {
	const window = windowFrom(event);
	if (!window) return null;

	if (editable) {
		const template: MenuItemConstructorOptions[] = [
			{ role: "cut", enabled: canCopy },
			{ role: "copy", enabled: canCopy },
			{ role: "paste" },
			{ type: "separator" },
			{ role: "selectAll" },
		];
		Menu.buildFromTemplate(template).popup({ window });
		return null;
	}

	return new Promise<ContextMenuAction | null>((resolve) => {
		const menu = Menu.buildFromTemplate([
			{ label: labels.copy, enabled: canCopy, click: () => resolve("copy") },
			{ label: labels.selectAll, enabled: canSelectAll, click: () => resolve("selectAll") },
		]);

		menu.popup({
			window,
			// Let the click handler resolve before dismissal, regardless of native event ordering.
			callback: () => setTimeout(() => resolve(null), 0),
		});
	});
});
