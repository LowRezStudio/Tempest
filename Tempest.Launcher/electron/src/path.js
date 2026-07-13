import os from "node:os";
import path from "node:path";
import { ipcMain, app } from "electron";

ipcMain.handle("path:join", (_event, { paths }) => path.join(...paths));
ipcMain.handle("path:dirname", (_event, { path: p }) => path.dirname(p));
ipcMain.handle("path:app-config-dir", () => app.getPath("userData"));
ipcMain.handle("path:home-dir", () => os.homedir());
ipcMain.handle("path:resolve-resource", (_event, { paths }) =>
	path.join(process.resourcesPath, ...paths),
);
