import fs from "node:fs";
import path from "node:path";
import { app, BrowserWindow, net } from "electron";
import electronUpdater from "electron-updater";
import { handle } from "./ipc/handle.js";

const { autoUpdater } = electronUpdater;

autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = false;

// An "error" event without a listener throws inside electron-updater; the
// failures themselves reach the renderer through the promises below.
autoUpdater.on("error", (err) => console.error("[updater]", err.message));

/**
 * Set on the environment of the process an update relaunches (AppImage spawns
 * the new file itself, deb/rpm use `app.relaunch()`), so it can wait for this
 * one to exit before taking the single-instance and profile locks.
 */
export const UPDATE_PARENT_ENV = "TEMPEST_UPDATE_PARENT_PID";

let updateDownloaded = false;

export function isUpdateDownloaded(): boolean {
	return updateDownloaded;
}

function hasCommand(name: string): boolean {
	return (process.env.PATH ?? "").split(path.delimiter).some((dir) => {
		try {
			fs.accessSync(path.join(dir, name), fs.constants.X_OK);
			return true;
		} catch {
			return false;
		}
	});
}

/**
 * Whether electron-updater can install an update on this Linux install: an
 * AppImage replaces its own file; a deb/rpm install (electron-builder writes
 * `resources/package-type`) needs its package manager and a graphical sudo for
 * the password prompt. Anything else (an AUR repackage, an unpacked build) is
 * updated through whatever installed it.
 * @returns Whether `quitAndInstall` can apply an update here.
 */
function canInstallOnLinux(): boolean {
	if (process.env.APPIMAGE) return true;
	let packageType: string;
	try {
		packageType = fs
			.readFileSync(path.join(process.resourcesPath, "package-type"), "utf8")
			.trim();
	} catch {
		return false;
	}
	const managers =
		packageType === "deb"
			? ["dpkg", "apt"]
			: packageType === "rpm"
				? ["zypper", "dnf", "yum", "rpm"]
				: [];
	const elevate =
		process.getuid?.() === 0 || ["pkexec", "kdesudo", "gksudo", "beesu"].some(hasCommand);
	return managers.some(hasCommand) && elevate;
}

function canInstallUpdates(): boolean {
	if (!app.isPackaged) return false;
	if (process.platform === "win32") return true;
	if (process.platform === "linux") return canInstallOnLinux();
	return false;
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
	// chunkLength, so send the actual delta of transferred bytes (clamped for
	// resumed downloads where transferred can jump backwards).
	const chunk = Math.max(0, p.transferred - lastTransferred);
	lastTransferred = p.transferred;
	sender.send("updater:download-progress", {
		chunkLength: chunk,
		contentLength: p.total,
	});
});

/**
 * Applies a downloaded update: silent install, app restarted afterwards.
 * Destroys windows directly so the close guard (background work in the
 * tray) can't block the quit.
 *
 * On Linux the install runs synchronously inside `quitAndInstall` (moving the
 * AppImage, or dpkg/rpm behind a pkexec prompt the user can cancel) and the
 * quit is deferred, so the windows are only destroyed once it succeeded; a
 * failure is thrown and the launcher stays open.
 */
export function quitAndInstallUpdate(): void {
	if (process.platform !== "linux") {
		for (const window of BrowserWindow.getAllWindows()) window.destroy();
		autoUpdater.quitAndInstall(true, true);
		return;
	}
	let failure: Error | null = null;
	const onError = (err: Error) => {
		failure ??= err;
	};
	autoUpdater.on("error", onError);
	process.env[UPDATE_PARENT_ENV] = String(process.pid);
	try {
		autoUpdater.quitAndInstall(true, true);
	} finally {
		autoUpdater.removeListener("error", onError);
	}
	if (failure) {
		delete process.env[UPDATE_PARENT_ENV];
		throw failure;
	}
	for (const window of BrowserWindow.getAllWindows()) window.destroy();
}

/**
 * Called first thing at startup: a process relaunched by an update waits (up
 * to 15 s) for the launcher that installed it to exit, since it would
 * otherwise lose the single-instance lock to it and quit.
 */
export function waitForUpdatedParent(): void {
	const pid = Number(process.env[UPDATE_PARENT_ENV]);
	delete process.env[UPDATE_PARENT_ENV];
	if (!pid || pid === process.pid) return;
	const sleeper = new Int32Array(new SharedArrayBuffer(4));
	const deadline = Date.now() + 15_000;
	while (Date.now() < deadline) {
		try {
			process.kill(pid, 0);
		} catch {
			return;
		}
		Atomics.wait(sleeper, 0, 0, 100);
	}
}

async function fetchGitHub<T>(endpoint: string): Promise<T | null> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 2000);
	try {
		const response = await net.fetch(
			`https://api.github.com/repos/LowRezStudio/Tempest/releases/${endpoint}`,
			{
				signal: controller.signal,
				headers: { Accept: "application/vnd.github+json" },
			},
		);
		if (!response.ok) return null;
		return (await response.json()) as T;
	} catch {
		return null;
	} finally {
		clearTimeout(timeout);
	}
}

/**
 * Markdown body of a GitHub release, fetched host-side.
 * @param tag Release tag to fetch (e.g. `v1.1.0`).
 * @returns The trimmed release body, or `null` when unavailable.
 */
async function fetchReleaseNotes(tag: string): Promise<string | null> {
	const data = await fetchGitHub<{ body?: string }>(`tags/${encodeURIComponent(tag)}`);
	return (data?.body ?? "").trim() || null;
}

/**
 * Compares `x.y.z[-pre]` versions; a pre-release sorts below its release.
 * @param a Candidate version (a leading `v` is ignored).
 * @param b Version to compare against.
 * @returns Whether `a` is newer than `b`.
 */
function isNewer(a: string, b: string): boolean {
	const parse = (v: string) => {
		const [core, pre] = v.replace(/^v/, "").split("-", 2);
		return { parts: core.split(".").map((n) => Number(n) || 0), pre: pre ?? null };
	};
	const x = parse(a);
	const y = parse(b);
	for (let i = 0; i < 3; i++) {
		const d = (x.parts[i] ?? 0) - (y.parts[i] ?? 0);
		if (d !== 0) return d > 0;
	}
	if (x.pre === y.pre) return false;
	if (x.pre === null) return true;
	if (y.pre === null) return false;
	return x.pre.localeCompare(y.pre, undefined, { numeric: true }) > 0;
}

handle("updater:check", async () => {
	const canInstall = canInstallUpdates();
	try {
		const result = await autoUpdater.checkForUpdates();
		if (result == null) {
			// The updater is inactive: a dev build, or a Linux install it
			// can't update (not an AppImage, no package type). Packaged builds
			// still learn about new versions from the latest GitHub release.
			if (!app.isPackaged) return { available: false };
			const latest = await fetchGitHub<{ tag_name?: string; body?: string }>("latest");
			if (!latest?.tag_name || !isNewer(latest.tag_name, app.getVersion())) {
				return { available: false };
			}
			return {
				available: true,
				version: latest.tag_name.replace(/^v/, ""),
				releaseNotes: (latest.body ?? "").trim() || null,
				canInstall: false,
			};
		}
		if (!result.isUpdateAvailable) return { available: false };
		// Release notes come from the GitHub release body (the CI release job
		// sets it to the annotated tag's message). Fetched host-side so the
		// update dialog doesn't depend on any renderer query having run.
		const { version } = result.updateInfo;
		const tag = (result.updateInfo as { tag?: string }).tag ?? `v${version}`;
		const releaseNotes = await fetchReleaseNotes(tag);
		return { available: true, version, releaseNotes, canInstall };
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		// Releases up to v1.0.3 were built with Tauri and have no
		// electron-updater metadata (latest.yml). Those update checks can
		// never succeed, so treat the missing metadata as "up to date"
		// instead of surfacing a 404 to the user.
		return /latest[^ ]*\.yml/i.test(message) ? { available: false } : { error: message };
	}
});

handle("updater:download", async (event) => {
	downloadSender = event.sender;
	lastTransferred = 0;
	await autoUpdater.downloadUpdate();
	updateDownloaded = true;
});
