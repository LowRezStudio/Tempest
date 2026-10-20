// Platform detection runs entirely on the host (`platforms:detect`): the
// game-folder walk and Binaries checks used to be dozens of renderer-driven
// fs round trips, now it's one call. Results are cached per instance path.
import { invoke } from "$lib/platform/api";
import { instancePlatforms } from "$lib/types/instance";
import type { InstancePlatform } from "$lib/types/instance";

const platformCache = new Map<string, InstancePlatform[]>();

export async function detectAvailablePlatforms(instancePath: string): Promise<InstancePlatform[]> {
	const cached = platformCache.get(instancePath);
	if (cached) return cached;

	const detected = await invoke("platforms:detect", { instancePath });
	const resolved = detected.length > 0 ? detected : [...instancePlatforms];
	platformCache.set(instancePath, resolved);
	return resolved;
}
