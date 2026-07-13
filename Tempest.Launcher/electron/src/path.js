const { ipcMain, app } = require("electron");
const path = require("node:path");
const os = require("node:os");

ipcMain.handle("path:join", (_event, { paths }) => path.join(...paths));
ipcMain.handle("path:dirname", (_event, { path: p }) => path.dirname(p));
ipcMain.handle("path:app-config-dir", () => app.getPath("userData"));
ipcMain.handle("path:home-dir", () => os.homedir());
ipcMain.handle("path:resolve-resource", (_event, { paths }) =>
	path.join(process.resourcesPath, ...paths),
);
