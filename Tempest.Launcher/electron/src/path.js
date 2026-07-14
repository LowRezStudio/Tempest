import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { ipcMain, app } from "electron";

ipcMain.handle("path:join", (_event, { paths }) => path.join(...paths));
ipcMain.handle("path:dirname", (_event, { path: p }) => path.dirname(p));
ipcMain.handle("path:app-config-dir", () => app.getPath("userData"));
ipcMain.handle("path:home-dir", () => os.homedir());
ipcMain.handle("path:resolve-resource", (_event, { paths }) => {
	const r = path.join(process.resourcesPath, ...paths);
	if (!app.isPackaged && !fs.existsSync(r)) {
		const dev = path.join(import.meta.dirname, "../../src-tauri/resources", ...paths);
		if (fs.existsSync(dev)) return dev;
	}
	return r;
});
