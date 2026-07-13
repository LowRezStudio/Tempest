import { ipcMain, app } from "electron";

ipcMain.handle("app:version", () => app.getVersion());
ipcMain.handle("relaunch", () => {
	app.relaunch();
	app.exit();
});
