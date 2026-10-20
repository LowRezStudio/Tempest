import { BrowserWindow, net } from "electron";
import electronUpdater from "electron-updater";
import { handle } from "./ipc/handle.js";

const { autoUpdater } = electronUpdater;

autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = false;

let updateDownloaded = false;

export function isUpdateDownloaded(): boolean {
	return updateDownloaded;
}

// Progress is forwarded to whichever webContents started the download. One
// module-level listener instead of one per `updater:download` call: stacking
// listeners would resend every progress chunk once per retry.
let downloadSender: Electron.WebContents | null = null;
let lastTransferred = 0;

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

/**
 * Applies a downloaded update: silent install, app restarted afterwards.
 * Destroys windows directly so the close guard (background work in the
 * tray) can't block the quit.
 */
export function quitAndInstallUpdate(): void {
	for (const window of BrowserWindow.getAllWindows()) window.destroy();
	autoUpdater.quitAndInstall(true, true);
}

/**
 * Markdown body of a GitHub release, fetched host-side.
 * @param tag Release tag to fetch (e.g. `v1.1.0`).
 * @returns The trimmed release body, or `null` when unavailable.
 */
async function fetchReleaseNotes(tag: string): Promise<string | null> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 2000);
	try {
		const response = await net.fetch(
			`https://api.github.com/repos/LowRezStudio/Tempest/releases/tags/${encodeURIComponent(tag)}`,
			{
				signal: controller.signal,
				headers: { Accept: "application/vnd.github+json" },
			},
		);
		if (!response.ok) return null;
		const data = (await response.json()) as { body?: string };
		return (data.body ?? "").trim() || null;
	} catch {
		return null;
	} finally {
		clearTimeout(timeout);
	}
}

handle("updater:check", () => {
	return new Promise((resolve) => {
		let done = false;
		const finish = (result: {
			available?: boolean;
			version?: string;
			releaseNotes?: string | null;
			error?: string;
		}) => {
			if (done) return;
			done = true;
			autoUpdater.removeListener("update-available", onUpdateAvailable);
			autoUpdater.removeListener("update-not-available", onUpdateNotAvailable);
			autoUpdater.removeListener("error", onError);
			resolve(result);
		};
		// Release notes come from the GitHub release body (the CI release job
		// sets it to the annotated tag's message). Fetched host-side so the
		// update dialog doesn't depend on any renderer query having run.
		const onUpdateAvailable = (info: { version: string; tag?: string }) => {
			const version = info.version;
			const tag = info.tag ?? `v${version}`;
			fetchReleaseNotes(tag)
				.then((releaseNotes) => finish({ available: true, version, releaseNotes }))
				.catch(() => finish({ available: true, version, releaseNotes: null }));
		};
		const onUpdateNotAvailable = () => {
			finish({ available: false });
		};
		const onError = (err: Error) => {
			// Releases up to v1.0.3 were built with Tauri and have no
			// electron-updater metadata (latest.yml). Those update checks can
			// never succeed, so treat the missing metadata as "up to date"
			// instead of surfacing a 404 to the user.
			finish(
				/latest[^ ]*\.yml/i.test(err.message)
					? { available: false }
					: { error: err.message },
			);
		};

		autoUpdater.on("update-available", onUpdateAvailable);
		autoUpdater.on("update-not-available", onUpdateNotAvailable);
		autoUpdater.on("error", onError);
		void autoUpdater.checkForUpdates();
	});
});

handle("updater:download", (event) => {
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
