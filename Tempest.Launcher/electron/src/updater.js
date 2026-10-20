import { ipcMain, BrowserWindow } from "electron";
import electronUpdater from "electron-updater";
const { autoUpdater } = electronUpdater;

autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = false;

let updateDownloaded = false;

export function isUpdateDownloaded() {
	return updateDownloaded;
}

// Progress is forwarded to whichever webContents started the download. One
// module-level listener instead of one per `updater:download` call: stacking
// listeners would resend every progress chunk once per retry.
let downloadSender = null;

autoUpdater.on("download-progress", (p) => {
	const sender = downloadSender;
	if (!sender || sender.isDestroyed()) return;
	// bytesPerSecond is throughput, not a chunk size; the renderer accumulates
	// chunkLength, so send the actual delta of transferred bytes.
	sender.send("updater:download-progress", {
		chunkLength: p.transferred - lastTransferred,
		contentLength: p.total,
	});
});

let lastTransferred = 0;

/**
 * Applies a downloaded update: silent install, app restarted afterwards.
 * Destroys windows directly so the renderer's close guard (background work
 * in the tray) can't block the quit.
 */
export function quitAndInstallUpdate() {
	for (const window of BrowserWindow.getAllWindows()) window.destroy();
	autoUpdater.quitAndInstall(true, true);
}

ipcMain.handle("updater:check", () => {
	return new Promise((resolve) => {
		const listeners = [
			[
				"update-available",
				(info) => {
					cleanup();
					resolve({ available: true, version: info.version });
				},
			],
			[
				"update-not-available",
				() => {
					cleanup();
					resolve({ available: false });
				},
			],
			[
				"error",
				(err) => {
					cleanup();
					// Releases up to v1.0.3 were built with Tauri and have no
					// electron-updater metadata (latest.yml). Those update checks can
					// never succeed, so treat the missing metadata as "up to date"
					// instead of surfacing a 404 to the user.
					if (/latest[^ ]*\.yml/i.test(err.message)) {
						resolve({ available: false });
					} else {
						resolve({ error: err.message });
					}
				},
			],
		];
		const cleanup = () => {
			for (const [event, fn] of listeners) autoUpdater.removeListener(event, fn);
		};
		for (const [event, fn] of listeners) autoUpdater.on(event, fn);
		void autoUpdater.checkForUpdates();
	});
});

ipcMain.handle("updater:download", (event) => {
	return new Promise((resolve, reject) => {
		downloadSender = event.sender;
		lastTransferred = 0;
		autoUpdater.once("update-downloaded", () => {
			updateDownloaded = true;
			resolve();
		});
		autoUpdater.once("error", (err) => reject(err));
		void autoUpdater.downloadUpdate();
	});
});
