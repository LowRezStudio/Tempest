// Game platform detection, done entirely on the host: the renderer asks two
// questions and gets answers instead of walking directories over IPC.
import { execFile } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { handle } from "./ipc/handle.js";
import type { FoundInstallation, GamePlatform, InstallationSource } from "./ipc/contract.js";

const ALL_PLATFORMS: GamePlatform[] = ["Win64", "Win32"];
const GAME_EXECUTABLE = "paladins.exe";

async function exists(p: string): Promise<boolean> {
	try {
		await fs.promises.access(p);
		return true;
	} catch {
		return false;
	}
}

/**
 * Instance paths can point at the game exe itself; peel back to the folder.
 * @param value Path to check.
 * @returns True when the path looks like a game executable/DLL file.
 */
function isFilePath(value: string): boolean {
	const lower = value.toLowerCase();
	return lower.endsWith(".exe") || lower.endsWith(".dll");
}

/**
 * Walks up from the instance path until a folder with Binaries and Engine is found.
 * @param instancePath Instance path to start from.
 * @returns The game folder, or `null` when none exists.
 */
export async function resolveGameFolder(instancePath: string): Promise<string | null> {
	let current = isFilePath(instancePath) ? path.dirname(instancePath) : instancePath;
	while (true) {
		if (
			(await exists(path.join(current, "Binaries"))) &&
			(await exists(path.join(current, "Engine")))
		) {
			return current;
		}
		const parent = path.dirname(current);
		if (parent === current) return null;
		current = parent;
	}
}

async function hasGameExecutableInDir(directoryPath: string): Promise<boolean> {
	try {
		const entries = await fs.promises.readdir(directoryPath, { withFileTypes: true });
		return entries.some(
			(entry) => entry.isFile() && entry.name.toLowerCase() === GAME_EXECUTABLE,
		);
	} catch {
		return false;
	}
}

handle("platforms:detect", async (_event, { instancePath }) => {
	const resolved = (await resolveGameFolder(instancePath)) ?? instancePath;
	const binariesRoot = path.join(resolved, "Binaries");

	const detected: GamePlatform[] = [];
	for (const platform of ALL_PLATFORMS) {
		if (await hasGameExecutableInDir(path.join(binariesRoot, platform))) {
			detected.push(platform);
		}
	}

	// If nothing was detected, report all platforms as a safe default.
	return detected.length > 0 ? detected : [...ALL_PLATFORMS];
});

// ---- Steam / Paladins discovery ----

function getSteamRoots(): string[] {
	const home = os.homedir();
	if (process.platform === "win32") {
		return [
			"C:\\Program Files (x86)\\Steam",
			"C:\\Program Files\\Steam",
			path.join(home, "AppData", "Local", "Steam"),
		];
	}
	if (process.platform === "darwin") {
		return [path.join(home, "Library", "Application Support", "Steam")];
	}
	return [
		path.join(home, ".steam", "root"),
		path.join(home, ".steam", "steam"),
		path.join(home, ".local", "share", "Steam"),
		path.join(home, ".var", "app", "com.valvesoftware.Steam", "data", "Steam"),
	];
}

async function readSteamLibraries(root: string): Promise<string[]> {
	const manifestPath = path.join(root, "steamapps", "libraryfolders.vdf");
	if (!(await exists(manifestPath))) return [];
	try {
		const contents = await fs.promises.readFile(manifestPath, "utf8");
		return [...contents.matchAll(/"path"\s+"((?:\\.|[^"\\])*)"/g)]
			.map((match) => unescapeVdf(match[1] ?? ""))
			.filter(Boolean);
	} catch {
		// An unreadable libraryfolders.vdf is not a detection failure.
		return [];
	}
}

const PALADINS = /paladins/i;
/** Live and the Battlegrounds PTS */
const STEAM_APP_IDS = [444090, 596350];

async function readSteamInstallDir(manifestPath: string): Promise<string | null> {
	try {
		const contents = await fs.promises.readFile(manifestPath, "utf8");
		const match = contents.match(/"installdir"\s+"((?:\\.|[^"\\])*)"/);
		return match?.[1] ? unescapeVdf(match[1]) : null;
	} catch {
		return null;
	}
}

function unescapeVdf(value: string): string {
	return value.replaceAll(/\\(["\\])/g, "$1");
}

async function subfolders(dir: string): Promise<string[]> {
	try {
		const entries = await fs.promises.readdir(dir, { withFileTypes: true });
		// Dirents of symlinks and junctions aren't directories, which keeps walks out of loops.
		return entries.filter((e) => e.isDirectory()).map((e) => path.join(dir, e.name));
	} catch {
		return [];
	}
}

async function namedSubfolders(dir: string): Promise<string[]> {
	return (await subfolders(dir)).filter((p) => PALADINS.test(path.basename(p)));
}

async function readJson<T>(file: string): Promise<T | null> {
	try {
		return JSON.parse(await fs.promises.readFile(file, "utf8")) as T;
	} catch {
		return null;
	}
}

async function isPaladinsInstallation(gamePath: string): Promise<boolean> {
	const binaries = path.join(gamePath, "Binaries");
	if (!(await exists(binaries))) return false;
	return (
		(await exists(path.join(binaries, "Win64", "Paladins.exe"))) ||
		(await exists(path.join(binaries, "Win32", "Paladins.exe")))
	);
}

const keyOf = (p: string) => {
	const resolved = path.resolve(p).replace(/[\\/]+$/, "");
	return process.platform === "win32" ? resolved.toLowerCase() : resolved;
};

const toArray = <T>(value: T[] | T | null | undefined): T[] =>
	value == null ? [] : Array.isArray(value) ? value : [value];

type WindowsHints = {
	steam: string[];
	installs: { key: string; publisher?: string | null; path: string }[];
	drives: string[];
};

// One PowerShell run for what the registry knows: Steam's own path, the uninstall entries (Steam
// writes one per game, standalone launchers do too) and the fixed drives worth sweeping.
const WINDOWS_QUERY = [
	"$uninstall = 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*', 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*', 'HKCU:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'",
	"@{",
	"  steam = @(@((Get-ItemProperty 'HKCU:\\Software\\Valve\\Steam' -ErrorAction SilentlyContinue).SteamPath, (Get-ItemProperty 'HKLM:\\SOFTWARE\\WOW6432Node\\Valve\\Steam' -ErrorAction SilentlyContinue).InstallPath) | Where-Object { $_ })",
	"  installs = @(Get-ItemProperty $uninstall -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -like '*Paladins*' -and $_.InstallLocation } | ForEach-Object { @{ key = $_.PSChildName; publisher = $_.Publisher; path = $_.InstallLocation } })",
	"  drives = @([System.IO.DriveInfo]::GetDrives() | Where-Object { $_.DriveType -eq 'Fixed' -and $_.IsReady } | ForEach-Object { $_.Name })",
	"} | ConvertTo-Json -Compress -Depth 4",
].join("\n");

function queryWindows(): Promise<WindowsHints> {
	const none: WindowsHints = { steam: [], installs: [], drives: [] };
	if (process.platform !== "win32") return Promise.resolve(none);
	return new Promise((resolve) => {
		execFile(
			"powershell.exe",
			["-NoProfile", "-Command", WINDOWS_QUERY],
			{ windowsHide: true, timeout: 15_000 },
			(err, stdout) => {
				if (err) return resolve(none);
				try {
					const parsed = JSON.parse(stdout) as Partial<WindowsHints>;
					resolve({
						steam: toArray(parsed.steam),
						installs: toArray(parsed.installs),
						drives: toArray(parsed.drives),
					});
				} catch {
					resolve(none);
				}
			},
		);
	});
}

async function steamCandidates(roots: string[]): Promise<string[]> {
	const libraries = new Map<string, string>();
	const addLibrary = (library: string) => {
		if (!libraries.has(keyOf(library))) libraries.set(keyOf(library), library);
	};
	for (const root of roots) {
		if (!(await exists(root))) continue;
		addLibrary(root);
		for (const library of await readSteamLibraries(root)) addLibrary(library);
	}

	const candidates: string[] = [];
	for (const library of libraries.values()) {
		const steamapps = path.join(library, "steamapps");
		for (const id of STEAM_APP_IDS) {
			const dir = await readSteamInstallDir(path.join(steamapps, `appmanifest_${id}.acf`));
			if (dir) candidates.push(path.join(steamapps, "common", dir));
		}
		candidates.push(...(await namedSubfolders(path.join(steamapps, "common"))));
	}
	return candidates;
}

async function epicCandidates(): Promise<string[]> {
	const candidates: string[] = [];
	if (process.platform === "win32") {
		const programData = process.env.ProgramData ?? "C:\\ProgramData";
		const manifests = path.join(programData, "Epic", "EpicGamesLauncher", "Data", "Manifests");
		let files: string[] = [];
		try {
			files = (await fs.promises.readdir(manifests)).filter((f) => f.endsWith(".item"));
		} catch {
			// No Epic launcher.
		}
		for (const file of files) {
			const item = await readJson<{ DisplayName?: string; InstallLocation?: string }>(
				path.join(manifests, file),
			);
			if (item?.InstallLocation && PALADINS.test(item.DisplayName ?? "")) {
				candidates.push(item.InstallLocation);
			}
		}
		return candidates;
	}
	// Heroic, the usual Epic launcher on Linux, keeps legendary's install list.
	const home = os.homedir();
	const configs = [
		path.join(home, ".config", "heroic"),
		path.join(home, ".var", "app", "com.heroicgameslauncher.hgl", "config", "heroic"),
	];
	for (const config of configs) {
		const installed = await readJson<Record<string, { title?: string; install_path?: string }>>(
			path.join(config, "legendaryConfig", "legendary", "installed.json"),
		);
		for (const game of Object.values(installed ?? {})) {
			if (game.install_path && PALADINS.test(game.title ?? "")) {
				candidates.push(game.install_path);
			}
		}
	}
	return candidates;
}

async function hirezCandidates(): Promise<string[]> {
	if (process.platform !== "win32") return [];
	const programFiles = [
		process.env["ProgramFiles(x86)"] ?? "C:\\Program Files (x86)",
		process.env.ProgramFiles ?? "C:\\Program Files",
	];
	const roots = programFiles.map((p) => path.join(p, "Hi-Rez Studios", "HiRezGames"));
	return (await Promise.all(roots.map(namedSubfolders))).flat();
}

// The sweep checks every folder it meets but only enters ones named like a place games live,
// so a drive costs a few hundred directory reads rather than a full walk.
const PROMISING = /paladins|games?|steam|epic|hi-?rez|tempest|program ?files/i;
const SKIPPED =
	/^(\$|\.|windows$|system volume information$|recovery$|perflogs$|programdata$|users$|appdata$|node_modules$)/i;
const SWEEP_DEPTH = 4;
const SWEEP_BUDGET_MS = 8_000;

async function sweepRoots(drives: string[]): Promise<string[]> {
	const home = os.homedir();
	const roots = [
		...drives,
		home,
		...["Desktop", "Downloads", "Documents", "Games"].map((d) => path.join(home, d)),
	];
	if (process.platform === "linux") {
		const user = os.userInfo().username;
		for (const mounts of ["/mnt", `/media/${user}`, `/run/media/${user}`]) {
			roots.push(...(await subfolders(mounts)));
		}
	} else if (process.platform === "darwin") {
		roots.push(...(await subfolders("/Volumes")));
	}
	return roots;
}

async function sweep(
	dir: string,
	depth: number,
	deadline: number,
	visit: (dir: string) => Promise<boolean>,
): Promise<void> {
	// A game folder holds no other installs.
	if (Date.now() > deadline || (await visit(dir))) return;
	if (depth >= SWEEP_DEPTH || (depth > 0 && !PROMISING.test(path.basename(dir)))) return;
	const children = (await subfolders(dir)).filter((child) => !SKIPPED.test(path.basename(child)));
	await Promise.all(children.map((child) => sweep(child, depth + 1, deadline, visit)));
}

handle("platforms:find-paladins", async (event, args) => {
	const { folders, sweep: thorough = true, progressId } = args;
	const found = new Map<string, FoundInstallation>();
	// Sources run from most to least specific, so a folder keeps the first source that found it.
	const add = async (candidate: string, source: InstallationSource): Promise<boolean> => {
		const key = keyOf(candidate);
		if (found.has(key)) return true;
		if (!(await isPaladinsInstallation(candidate))) return false;
		const install = { path: path.resolve(candidate), source };
		found.set(key, install);
		if (progressId && !event.sender.isDestroyed()) {
			event.sender.send("platforms:found-paladins", { id: progressId, install });
		}
		return true;
	};

	// The registry query takes a while, so Steam's usual folders are checked meanwhile.
	const windows = queryWindows();
	const others = Promise.all([
		epicCandidates(),
		hirezCandidates(),
		Promise.all(folders.map(subfolders)).then((lists) => lists.flat()),
	]);
	for (const candidate of await steamCandidates(getSteamRoots())) await add(candidate, "steam");
	const hints = await windows;
	for (const candidate of await steamCandidates(hints.steam)) await add(candidate, "steam");

	const [epic, hirez, tempest] = await others;
	for (const candidate of epic) await add(candidate, "epic");
	for (const install of hints.installs) {
		const source = install.key.startsWith("Steam App")
			? "steam"
			: /hi-?rez/i.test(install.publisher ?? "")
				? "hirez"
				: "folder";
		await add(install.path, source);
	}
	for (const candidate of hirez) await add(candidate, "hirez");
	for (const candidate of tempest) await add(candidate, "tempest");

	if (thorough) {
		const deadline = Date.now() + SWEEP_BUDGET_MS;
		for (const root of await sweepRoots(hints.drives)) {
			await sweep(root, 0, deadline, (dir) => add(dir, "folder"));
		}
	}

	return [...found.values()];
});
