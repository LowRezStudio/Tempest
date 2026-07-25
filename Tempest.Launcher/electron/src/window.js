import { ipcMain, BrowserWindow } from "electron";

const windowFrom = (event) => BrowserWindow.fromWebContents(event.sender);

ipcMain.handle("window:destroy", (event) => {
	windowFrom(event)?.destroy();
});

ipcMain.handle("window:hide", (event) => {
	windowFrom(event)?.hide();
});

ipcMain.handle("window:show", (event) => {
	windowFrom(event)?.show();
});

ipcMain.handle("window:set-focus", (event) => {
	windowFrom(event)?.focus();
});

ipcMain.handle("window:unminimize", (event) => {
	windowFrom(event)?.restore();
});
