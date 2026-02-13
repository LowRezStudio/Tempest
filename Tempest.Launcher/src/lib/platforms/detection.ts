import { path as tauriPath } from "@tauri-apps/api";
import { exists, readDir } from "@tauri-apps/plugin-fs";
import { instancePlatforms, type InstancePlatform } from "$lib/types/instance";

const platformCache = new Map<string, InstancePlatform[]>();

export async function detectAvailablePlatforms(
	instancePath: string,
): Promise<InstancePlatform[]> {
	const cached = platformCache.get(instancePath);
	if (cached) return cached;

	const platformRoots = [
		await tauriPath.join(instancePath, "ChaosGame", "Binaries"),
		await tauriPath.join(instancePath, "Binaries"),
	];

	const detected: InstancePlatform[] = [];
	for (const platform of instancePlatforms) {
		const hasBinaries = await hasFilesInAnyDir(
			platformRoots.map((root) => tauriPath.join(root, platform)),
		);
		if (hasBinaries) {
			detected.push(platform);
		}
	}

	const resolved = detected.length ? detected : [...instancePlatforms];
	platformCache.set(instancePath, resolved);
	return resolved;
}

async function hasFilesInAnyDir(directoryPaths: Promise<string>[]): Promise<boolean> {
	for (const dirPromise of directoryPaths) {
		const dirPath = await dirPromise;
		if (await hasFilesInDir(dirPath)) return true;
	}
	return false;
}

async function hasFilesInDir(directoryPath: string): Promise<boolean> {
	try {
		if (!(await exists(directoryPath))) return false;
		const entries = await readDir(directoryPath);
		return entries.length > 0;
	} catch {
		return false;
	}
}
