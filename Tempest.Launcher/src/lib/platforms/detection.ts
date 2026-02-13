import { path as tauriPath } from "@tauri-apps/api";
import { exists, readDir } from "@tauri-apps/plugin-fs";
import { allowScopeDirectory } from "$lib/tauri/scopes";
import { instancePlatforms, type InstancePlatform } from "$lib/types/instance";

const platformCache = new Map<string, InstancePlatform[]>();

export async function detectAvailablePlatforms(
	instancePath: string,
): Promise<InstancePlatform[]> {
	console.debug("[platforms] detectAvailablePlatforms", { instancePath });
	await allowScopeDirectory(instancePath, true);
	const cached = platformCache.get(instancePath);
	if (cached) {
		console.debug("[platforms] cached result", { instancePath, cached });
		return cached;
	}

	const resolvedPath = (await resolveGameFolder(instancePath)) ?? instancePath;
	console.debug("[platforms] resolved game folder", { instancePath, resolvedPath });
	await allowScopeDirectory(resolvedPath, true);

	const binariesRoot = await tauriPath.join(resolvedPath, "Binaries");
	console.debug("[platforms] binaries root", { binariesRoot });

	const detected: InstancePlatform[] = [];
	for (const platform of instancePlatforms) {
		const platformRoot = await tauriPath.join(binariesRoot, platform);
		const hasBinaries = await hasGameExecutableInDir(platformRoot);
		console.debug("[platforms] platform check", {
			platform,
			platformRoot,
			hasBinaries,
		});
		if (hasBinaries) {
			detected.push(platform);
		}
	}

	const resolved = detected.length ? detected : [...instancePlatforms];
	console.debug("[platforms] resolved platforms", { resolved });
	platformCache.set(instancePath, resolved);
	return resolved;
}

const gameExecutableName = "Paladins.exe";
const fileExtensions = [".exe", ".dll"];

async function hasGameExecutableInDir(directoryPath: string): Promise<boolean> {
	try {
		await allowScopeDirectory(directoryPath, false);
		const dirExists = await exists(directoryPath);
		if (!dirExists) {
			console.debug("[platforms] binaries dir missing", { directoryPath });
			return false;
		}
		const entries = await readDir(directoryPath);
		const executable = gameExecutableName.toLowerCase();
		const hasExecutable = entries.some(
			(entry) =>
				!entry.isDirectory && (entry.name ?? "").toLowerCase() === executable,
		);
		console.debug("[platforms] binaries dir entries", {
			directoryPath,
			entryCount: entries.length,
			hasExecutable,
		});
		return hasExecutable;
	} catch (error) {
		console.debug("[platforms] failed to read binaries dir", {
			directoryPath,
			error,
		});
		return false;
	}
}

const resolveGameFolder = async (instancePath: string): Promise<string | null> => {
	let current = isFilePath(instancePath) ? await tauriPath.dirname(instancePath) : instancePath;

	while (true) {
		await allowScopeDirectory(current, false);
		const binariesPath = await tauriPath.join(current, "Binaries");
		const enginePath = await tauriPath.join(current, "Engine");

		if ((await exists(binariesPath)) && (await exists(enginePath))) return current;

		const parent = await tauriPath.dirname(current);
		if (parent === current) return null;
		current = parent;
	}
};

const isFilePath = (value: string): boolean => {
	const lower = value.toLowerCase();
	return fileExtensions.some((ext) => lower.endsWith(ext));
};
