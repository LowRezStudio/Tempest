import maps from "$lib/data/maps.json";
import versions from "$lib/data/versions.json";

export type Patch = (typeof versions)[number];

export const patchOf = (version: string | undefined): Patch | undefined =>
	version ? versions.find((v) => v.version === version) : undefined;

/**
 * The patch's banner in static/patches, named after the game's own patch notes: CB33 (closed
 * beta 0.33), OB57 (open beta 0.57), R81 (release 8.1). Some patches have none; callers fall
 * back when the image fails to load.
 * @param version - The instance's game version.
 * @returns The banner's URL, or undefined for versions not shaped like a patch.
 */
export function patchBannerOf(version: string | undefined): string | undefined {
	const match = version?.match(/^(\d+)\.(\d+)$/);
	if (!match) return undefined;
	const [, major, minor] = match;
	const code =
		major === "0" ? `${Number(minor) <= 33 ? "CB" : "OB"}${minor}` : `R${major}${minor}`;
	return `/patches/${code}_PatchBanner.webp`;
}

export function compareVersions(a: string, b: string) {
	const partsA = a.split(".").map(Number);
	const partsB = b.split(".").map(Number);

	const maxLength = Math.max(partsA.length, partsB.length);

	for (let i = 0; i < maxLength; i++) {
		const valA = partsA[i] ?? 0;
		const valB = partsB[i] ?? 0;

		if (valA > valB) return 1;
		if (valA < valB) return -1;
	}

	return 0;
}

export function getMapsForVersion(gameVersion: string) {
	return maps.filter((m) => {
		return (
			compareVersions(gameVersion, m.versionStart) >= 0 &&
			compareVersions(gameVersion, m.versionEnd) <= 0
		);
	});
}
