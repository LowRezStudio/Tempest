// Tempest Studio updates. Studio releases share the repository with the
// launcher (tags `studio-v*`) and are never the "latest" release, so the
// newest one is found through the GitHub API, then electron-updater reads its
// update files (`latest.yml`, `latest-mac.yml`, `latest-linux.yml`, attached
// by .github/workflows/studio-release.yml) from that release's download folder
// and installs like the launcher does: Windows (NSIS) and Linux (AppImage,
// deb/rpm with a graphical sudo) in place, macOS by downloading the new build.
import fs from "node:fs";
import path from "node:path";
import { app, BrowserWindow, net } from "electron";
import electronUpdater from "electron-updater";
import { handle } from "./ipc/handle.js";
import type { StudioUpdate } from "./ipc/contract.js";

const { autoUpdater } = electronUpdater;

const REPO = "LowRezStudio/Tempest";
const PREFIX = "studio-v";

autoUpdater.autoDownload = false;
autoUpdater.autoInstallOnAppQuit = false;
// An "error" event without a listener throws inside electron-updater; failures
// reach the renderer through the promises below.
autoUpdater.on("error", (err) => console.error("[updater]", err.message));

/** Set on the process an update relaunches, which waits for this one to exit. */
const UPDATE_PARENT_ENV = "TEMPEST_STUDIO_UPDATE_PARENT_PID";

/** The release electron-updater was pointed at last (by `updater:check`). */
let pointedAt: string | null = null;
let downloaded = false;

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
 * Whether electron-updater can install here: Windows, an AppImage, or a
 * deb/rpm install with its package manager and a graphical sudo.
 * @returns True when "Update now" can work on this install.
 */
function canInstall(): boolean {
	if (!app.isPackaged) return false;
	if (process.platform === "win32") return true;
	if (process.platform !== "linux") return false;
	if (process.env.APPIMAGE) return true;
	let type: string;
	try {
		type = fs.readFileSync(path.join(process.resourcesPath, "package-type"), "utf8").trim();
	} catch {
		return false;
	}
	const managers =
		type === "deb" ? ["dpkg", "apt"] : type === "rpm" ? ["zypper", "dnf", "yum", "rpm"] : [];
	const elevate =
		process.getuid?.() === 0 || ["pkexec", "kdesudo", "gksudo", "beesu"].some(hasCommand);
	return managers.some(hasCommand) && elevate;
}

/**
 * Compares `x.y.z[-pre]` versions; a pre-release sorts below its release.
 * @param a Candidate version.
 * @param b Version to compare against.
 * @returns Whether `a` is newer than `b`.
 */
function isNewer(a: string, b: string): boolean {
	const parse = (v: string) => {
		const [core, pre] = v.split("-", 2);
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

type Release = {
	tag_name: string;
	draft: boolean;
	prerelease: boolean;
	body?: string;
	html_url: string;
};

/** The newest published Studio release, or null (offline, rate limited, none). */
async function newestRelease(): Promise<Release | null> {
	const controller = new AbortController();
	const timeout = setTimeout(() => controller.abort(), 8000);
	try {
		const response = await net.fetch(
			`https://api.github.com/repos/${REPO}/releases?per_page=40`,
			{
				signal: controller.signal,
				headers: { Accept: "application/vnd.github+json" },
			},
		);
		if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
		const releases = (await response.json()) as Release[];
		let best: Release | null = null;
		for (const r of releases) {
			if (r.draft || r.prerelease || !r.tag_name.startsWith(PREFIX)) continue;
			if (
				!best ||
				isNewer(r.tag_name.slice(PREFIX.length), best.tag_name.slice(PREFIX.length))
			) {
				best = r;
			}
		}
		return best;
	} finally {
		clearTimeout(timeout);
	}
}

handle("updater:check", async (): Promise<StudioUpdate> => {
	const current = app.getVersion();
	const release = await newestRelease();
	if (!release) return { available: false, current };
	const version = release.tag_name.slice(PREFIX.length);
	if (!isNewer(version, current)) return { available: false, current };
	const info: StudioUpdate = {
		available: true,
		current,
		version,
		notes: (release.body ?? "").trim() || null,
		page: release.html_url,
		canInstall: false,
	};
	if (!canInstall()) return info;
	// The release's update files say what to download; older releases have none.
	try {
		autoUpdater.setFeedURL({
			provider: "generic",
			url: `https://github.com/${REPO}/releases/download/${release.tag_name}`,
		});
		const result = await autoUpdater.checkForUpdates();
		pointedAt = result?.isUpdateAvailable ? release.tag_name : null;
		downloaded = false;
		return { ...info, canInstall: !!result?.isUpdateAvailable };
	} catch {
		return info;
	}
});

let progressTo: Electron.WebContents | null = null;
autoUpdater.on("download-progress", (p) => {
	if (progressTo && !progressTo.isDestroyed()) {
		progressTo.send("updater:progress", {
			percent: p.percent,
			transferred: p.transferred,
			total: p.total,
		});
	}
});

handle("updater:download", async (event) => {
	if (!pointedAt) throw new Error("check for updates first");
	progressTo = event.sender;
	await autoUpdater.downloadUpdate();
	downloaded = true;
});

/**
 * Applies the downloaded update and restarts. On Linux the install runs inside
 * `quitAndInstall` (behind a password prompt the user can cancel), so the
 * windows are only closed once it worked; a failure is thrown.
 */
handle("updater:install", () => {
	if (!downloaded) throw new Error("download the update first");
	if (process.platform !== "linux") {
		for (const w of BrowserWindow.getAllWindows()) w.destroy();
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
	for (const w of BrowserWindow.getAllWindows()) w.destroy();
});

/**
 * First thing at startup: a process relaunched by an update waits (up to 15 s)
 * for the one that installed it to exit, or it would lose the single-instance
 * lock to it and quit.
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
