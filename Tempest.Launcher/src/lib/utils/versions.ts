import maps from "$lib/data/maps.json";

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
