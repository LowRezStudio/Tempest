import { exists, homeDir, path as sysPath, platform, readDir, readFile } from "$lib/platform/api";
import { allowScopeDirectory, allowScopeFile } from "$lib/platform/scopes";

/** Finds the first Steam library containing a usable Paladins installation.
 * @returns The installation path, or `null` when Steam does not contain Paladins.
 */
export async function findSteamPaladinsInstallation(): Promise<string | null> {
	const libraries = new Set<string>();

	for (const root of await getSteamRoots()) {
		if (!(await pathExists(root))) continue;

		libraries.add(root);
		for (const library of await readSteamLibraries(root)) {
			libraries.add(library);
		}
	}

	for (const library of libraries) {
		const commonPath = await sysPath.join(library, "steamapps", "common");
		const candidates = await paladinsDirectories(commonPath);

		for (const candidate of candidates) {
			if (await isPaladinsInstallation(candidate)) return candidate;
		}
	}

	return null;
}

async function getSteamRoots(): Promise<string[]> {
	const home = await homeDir();
	const os = platform();

	if (os === "windows") {
		return [
			"C:\\Program Files (x86)\\Steam",
			"C:\\Program Files\\Steam",
			await sysPath.join(home, "AppData", "Local", "Steam"),
		];
	}

	if (os === "macos") {
		return [await sysPath.join(home, "Library", "Application Support", "Steam")];
	}

	return [
		await sysPath.join(home, ".steam", "root"),
		await sysPath.join(home, ".steam", "steam"),
		await sysPath.join(home, ".local", "share", "Steam"),
		await sysPath.join(home, ".var", "app", "com.valvesoftware.Steam", "data", "Steam"),
	];
}

async function readSteamLibraries(root: string): Promise<string[]> {
	const manifestPath = await sysPath.join(root, "steamapps", "libraryfolders.vdf");
	if (!(await pathExists(manifestPath, true))) return [];

	try {
		await allowScopeFile(manifestPath);
		const contents = new TextDecoder().decode(await readFile(manifestPath));
		return [...contents.matchAll(/"path"\s+"((?:\\.|[^"\\])*)"/g)]
			.map((match) => unescapeVdf(match[1] ?? ""))
			.filter(Boolean);
	} catch (error) {
		console.debug("[steam] failed to read libraryfolders.vdf", { manifestPath, error });
		return [];
	}
}

async function paladinsDirectories(commonPath: string): Promise<string[]> {
	const candidates = new Set<string>();
	for (const name of ["Paladins", "paladins"]) {
		candidates.add(await sysPath.join(commonPath, name));
	}

	try {
		await allowScopeDirectory(commonPath, false);
		for (const entry of await readDir(commonPath)) {
			if (entry.isDirectory && entry.name?.toLowerCase() === "paladins") {
				candidates.add(await sysPath.join(commonPath, entry.name));
			}
		}
	} catch {
		// A missing or unreadable Steam library is not a detection failure.
	}

	return [...candidates];
}

async function isPaladinsInstallation(gamePath: string): Promise<boolean> {
	return (
		(await pathExists(
			await sysPath.join(gamePath, "Binaries", "Win64", "Paladins.exe"),
			true,
		)) ||
		(await pathExists(await sysPath.join(gamePath, "Binaries", "Win32", "Paladins.exe"), true))
	);
}

async function pathExists(target: string, file = false): Promise<boolean> {
	try {
		await (file ? allowScopeFile(target) : allowScopeDirectory(target, false));
		return await exists(target);
	} catch {
		return false;
	}
}

function unescapeVdf(value: string): string {
	return value.replaceAll(/\\(["\\])/g, "$1");
}
