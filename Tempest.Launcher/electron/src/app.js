import { ipcMain, app } from "electron";

let pendingOpenFiles = [];

export function queueOpenFiles(files) {
	pendingOpenFiles.push(...files);
}

ipcMain.handle("app:version", () => app.getVersion());
ipcMain.handle("take_pending_open_files", () => {
	const files = pendingOpenFiles;
	pendingOpenFiles = [];
	return files;
});
ipcMain.handle("relaunch", () => {
	app.relaunch();
	app.exit();
});
