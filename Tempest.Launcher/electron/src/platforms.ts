// Game platform detection, done entirely on the host: the renderer asks two
// questions and gets answers instead of walking directories over IPC.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { handle } from "./ipc/handle.js";
import type { GamePlatform } from "./ipc/contract.js";

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

async function paladinsDirectories(commonPath: string): Promise<string[]> {
	const candidates = new Set<string>();
	for (const name of ["Paladins", "paladins"]) {
		candidates.add(path.join(commonPath, name));
	}
	try {
		const entries = await fs.promises.readdir(commonPath, { withFileTypes: true });
		for (const entry of entries) {
			if (entry.isDirectory() && entry.name.toLowerCase() === "paladins") {
				candidates.add(path.join(commonPath, entry.name));
			}
		}
	} catch {
		// A missing or unreadable Steam library is not a detection failure.
	}
	return [...candidates];
}

async function isPaladinsInstallation(gamePath: string): Promise<boolean> {
	return (
		(await exists(path.join(gamePath, "Binaries", "Win64", "Paladins.exe"))) ||
		(await exists(path.join(gamePath, "Binaries", "Win32", "Paladins.exe")))
	);
}

handle("platforms:find-steam-paladins", async () => {
	const libraries = new Set<string>();

	for (const root of getSteamRoots()) {
		if (!(await exists(root))) continue;
		libraries.add(root);
		for (const library of await readSteamLibraries(root)) {
			libraries.add(library);
		}
	}

	for (const library of libraries) {
		const commonPath = path.join(library, "steamapps", "common");
		for (const candidate of await paladinsDirectories(commonPath)) {
			if (await isPaladinsInstallation(candidate)) return candidate;
		}
	}

	return null;
});

function unescapeVdf(value: string): string {
	return value.replaceAll(/\\(["\\])/g, "$1");
}
