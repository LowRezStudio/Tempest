import { getVersion } from "$lib/platform/api";

let cached: Promise<string> | undefined;

// The launcher's own version (package.json), what mods' `tempest` requirements are checked against.
export function getAppVersion(): Promise<string> {
	cached ??= getVersion().catch((error: unknown) => {
		cached = undefined;
		throw error;
	});
	return cached;
}
