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

type Compat = { patches: string[]; ranges: { from?: string; to?: string }[] };

function patchOrder(version: string): number {
	const [a = "0", b = "0", c = "0"] = version.split(".");
	return Number(a) * 1_000_000 + Number(b) * 10_000 + Number(c);
}

// Mirrors the repository's compat rules: explicit patches or inclusive ranges, numeric by component.
export function isCompatible(compat: Compat, patch: string | undefined): boolean {
	if (!patch) return true;
	if (compat.patches.includes(patch)) return true;
	const n = patchOrder(patch);
	return compat.ranges.some(
		(r) =>
			(r.from == null || patchOrder(r.from) <= n) && (r.to == null || n <= patchOrder(r.to)),
	);
}

export function describeCompat(compat: Compat): string {
	const parts = compat.ranges.map((r) =>
		r.from && r.to ? `${r.from} – ${r.to}` : r.from ? `${r.from}+` : r.to ? `≤ ${r.to}` : "all",
	);
	return [...parts, ...compat.patches].join(", ");
}
