import path from "node:path";
import { app, ipcMain, Menu, nativeImage, Tray } from "electron";

// id -> { tray, owner, showMenuOnLeftClick }
const trays = new Map();

function trayImage() {
	const iconPath = app.isPackaged
		? path.join(process.resourcesPath, "icon.png")
		: path.join(import.meta.dirname, "..", "..", "src-tauri", "icons", "icon.png");
	const size = process.platform === "darwin" ? 22 : 24;
	return nativeImage.createFromPath(iconPath).resize({ width: size, height: size });
}

function removeTray(id) {
	const entry = trays.get(id);
	if (!entry) return;
	entry.tray.destroy();
	trays.delete(id);
}

function sendTrayEvent(entry, event) {
	if (!entry.owner.isDestroyed()) {
		entry.owner.send("tray:event", { id: entry.id, event });
	}
}

ipcMain.handle("tray:new", (event, { id, tooltip, showMenuOnLeftClick }) => {
	removeTray(id);
	const tray = new Tray(trayImage());
	if (tooltip) tray.setToolTip(tooltip);
	const entry = { id, tray, owner: event.sender, showMenuOnLeftClick };
	trays.set(id, entry);
	tray.on("click", (_e, bounds, position) => {
		if (showMenuOnLeftClick) {
			tray.popUpContextMenu();
			return;
		}
		sendTrayEvent(entry, {
			type: "Click",
			id,
			button: "Left",
			buttonState: "Up",
			position: position ? { x: position.x, y: position.y } : undefined,
			rect: bounds
				? {
						position: { x: bounds.x, y: bounds.y },
						size: { width: bounds.width, height: bounds.height },
					}
				: undefined,
		});
	});
	tray.on("right-click", () => {
		sendTrayEvent(entry, { type: "Click", id, button: "Right", buttonState: "Up" });
	});
	tray.on("double-click", () => {
		sendTrayEvent(entry, { type: "DoubleClick", id, button: "Left" });
	});
});

ipcMain.handle("tray:remove", (_event, { id }) => {
	removeTray(id);
});

ipcMain.handle("tray:set-menu", (_event, { id, items }) => {
	const entry = trays.get(id);
	if (!entry) return;
	const template = items.map((item) =>
		item.kind === "separator"
			? { type: "separator" }
			: {
					label: item.text,
					enabled: item.enabled,
					click: () => {
						if (!entry.owner.isDestroyed()) {
							entry.owner.send("tray:menu-event", { id: item.id });
						}
					},
				},
	);
	entry.tray.setContextMenu(Menu.buildFromTemplate(template));
});

ipcMain.handle("tray:set-tooltip", (_event, { id, tooltip }) => {
	trays.get(id)?.tray.setToolTip(tooltip ?? "");
});
