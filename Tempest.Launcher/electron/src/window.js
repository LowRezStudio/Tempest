import { ipcMain, BrowserWindow } from "electron";

ipcMain.handle("window:destroy", (event) => {
	BrowserWindow.fromWebContents(event.sender)?.destroy();
});
