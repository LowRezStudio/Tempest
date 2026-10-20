// App-level IPC: version, relaunch, queued open-file events, instance path
// defaults and the localStorage settings.json snapshot.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { app, ipcMain } from "electron";
import { handle } from "./ipc/handle.js";
import { isUpdateDownloaded, quitAndInstallUpdate } from "./updater.js";

let pendingOpenFiles: string[] = [];

export function queueOpenFiles(files: string[]): void {
	pendingOpenFiles.push(...files);
}

handle("app:version", () => app.getVersion());

handle("app:take-pending-open-files", () => {
	const files = pendingOpenFiles;
	pendingOpenFiles = [];
	return files;
});

handle("app:relaunch", () => {
	// If an update was downloaded, install it silently instead of just
	// restarting the same version; the installer relaunches the app after.
	if (isUpdateDownloaded()) {
		quitAndInstallUpdate();
		return;
	}
	app.relaunch();
	app.exit();
});

handle("app:default-instance-path", () =>
	process.platform === "win32"
		? path.join("C:", "Games", "Tempest")
		: path.join(os.homedir(), "Games", "Tempest"),
);

// ---- settings.json snapshot ----
// The Tauri build periodically dumped localStorage to settings.json next to
// this profile as a recoverable snapshot; the renderer sends the serialized
// JSON and the host owns the file.

const snapshotPath = () => path.join(app.getPath("userData"), "settings.json");

handle("settings:write-snapshot", async (_event, { json }) => {
	await fs.promises.writeFile(snapshotPath(), json, "utf8");
});

handle("settings:delete-snapshot", async () => {
	await fs.promises.rm(snapshotPath(), { force: true });
});

// One-time localStorage migration: hand the snapshot to the preload script
// (which exposes it before any page script runs) so the store bootstrap can
// import it before persisted state initializes. Synchronous by necessity:
// sendSync from the preload script.
ipcMain.on("migration:dump", (event) => {
	try {
		event.returnValue = fs.readFileSync(snapshotPath(), "utf8");
	} catch {
		event.returnValue = null;
	}
});
