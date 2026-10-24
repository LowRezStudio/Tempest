import maps from "$lib/data/maps.json";
import versions from "$lib/data/versions.json";
import { m } from "$lib/paraglide/messages";
import { getLocale } from "$lib/paraglide/runtime";
import type { Instance } from "$lib/types/instance";

export type Patch = (typeof versions)[number];

// 8.2 was never released, so there is nothing to download; a folder can still be one.
export const downloadablePatches = versions.filter((v) => v.version !== "8.2");

export const patchOf = (version: string | undefined): Patch | undefined =>
	version ? versions.find((v) => v.version === version) : undefined;

/**
 * An instance's patch in one line: version, name and release month.
 * @param instance - The instance.
 * @returns E.g. `Patch 0.57 “Bird of Prey” · August 2017`.
 */
export function patchSummary(instance: Pick<Instance, "label" | "version">): string {
	const version = instance.version;
	if (!version) return m.library_unknown_build();
	const patch = patchOf(version);
	if (!patch) return m.library_patch({ version });
	const date = new Date(patch.date).toLocaleDateString(getLocale(), {
		month: "long",
		year: "numeric",
	});
	// Instances are named after their patch unless renamed; no need to say it twice.
	const name =
		patch.name === instance.label
			? m.library_patch({ version })
			: m.library_patch_named({ version, name: patch.name });
	return `${name} · ${date}`;
}

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
	return `/patches/${code}_PatchBanner.avif`;
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

export const loadingScreens = Object.keys(import.meta.glob("/static/loading-screens/*.avif")).map(
	(path) => path.replace("/static", ""),
);

/**
 * The loading screens of the patch's own maps, so the art is what that game shows.
 * @param version - The instance's game version.
 * @returns Their URLs, or every loading screen for a patch without any.
 */
export function loadingScreensOf(version: string | undefined): string[] {
	const own = version
		? getMapsForVersion(version)
				.map((map) => map.iconPath)
				.filter((path) => loadingScreens.includes(path))
		: [];
	return own.length > 0 ? [...new Set(own)] : loadingScreens;
}
