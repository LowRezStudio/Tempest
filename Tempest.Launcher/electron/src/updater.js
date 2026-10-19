import { ipcMain } from "electron";
import electronUpdater from "electron-updater";
const { autoUpdater } = electronUpdater;

autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = false;

ipcMain.handle("updater:check", () => {
	return new Promise((resolve) => {
		autoUpdater.once("update-available", (info) =>
			resolve({ available: true, version: info.version }),
		);
		autoUpdater.once("update-not-available", () => resolve({ available: false }));
		autoUpdater.once("error", (err) => {
			// Releases up to v1.0.3 were built with Tauri and have no
			// electron-updater metadata (latest.yml). Those update checks can
			// never succeed, so treat the missing metadata as "up to date"
			// instead of surfacing a 404 to the user.
			if (/latest[^ ]*\.yml/i.test(err.message)) {
				resolve({ available: false });
			} else {
				resolve({ error: err.message });
			}
		});
		void autoUpdater.checkForUpdates();
	});
});

ipcMain.handle("updater:download", (event) => {
	return new Promise((resolve, reject) => {
		autoUpdater.on("download-progress", (p) => {
			event.sender.send("updater:download-progress", {
				chunkLength: p.bytesPerSecond,
				contentLength: p.total,
			});
		});
		autoUpdater.once("update-downloaded", () => resolve());
		autoUpdater.once("error", (err) => reject(err));
		void autoUpdater.downloadUpdate();
	});
});
