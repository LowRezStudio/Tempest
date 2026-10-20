import { ipcMain, app } from "electron";
import { isUpdateDownloaded, quitAndInstallUpdate } from "./updater.js";

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
	// If an update was downloaded, install it silently instead of just
	// restarting the same version; the installer relaunches the app after.
	if (isUpdateDownloaded()) {
		quitAndInstallUpdate();
		return;
	}
	app.relaunch();
	app.exit();
});
