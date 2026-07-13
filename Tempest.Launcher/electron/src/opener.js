const { ipcMain, shell } = require("electron");

ipcMain.handle("opener:open-path", (_event, { path: p }) => shell.openPath(p));
ipcMain.handle("opener:open-url", (_event, { url }) => shell.openExternal(url));
