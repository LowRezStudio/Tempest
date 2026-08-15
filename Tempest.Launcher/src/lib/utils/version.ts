export function compareVersions(a: string | undefined, b: string | undefined): number {
	const aParts = a?.split(".") ?? [];
	const bParts = b?.split(".") ?? [];
	const max = Math.max(aParts.length, bParts.length);
	for (let i = 0; i < max; i++) {
		const av = aParts[i] ?? "0";
		const bv = bParts[i] ?? "0";
		if (av === bv) continue;
		const an = Number(av);
		const bn = Number(bv);
		if (!Number.isNaN(an) && !Number.isNaN(bn)) return an - bn;
		return av < bv ? -1 : 1;
	}
	return 0;
}
