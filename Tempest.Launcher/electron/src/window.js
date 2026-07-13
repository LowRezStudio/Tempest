const { ipcMain, BrowserWindow } = require("electron");

ipcMain.handle("window:destroy", (event) => {
	BrowserWindow.fromWebContents(event.sender)?.destroy();
});
