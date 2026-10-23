// The export table as an outer hierarchy. Roots are exports whose outer is
// not an export of this package (none, or an import).
import type { ExportRow } from "../../../electron/src/ipc/contract";

export type Tree = {
	roots: number[];
	children: Map<number, number[]>;
	/** Transitive descendant count, for the badge. */
	descendants: Map<number, number>;
};

function compareRows(rows: ExportRow[]) {
	return (a: number, b: number) => {
		const ra = rows[a]!;
		const rb = rows[b]!;
		const pa = ra.class.toLowerCase() === "package" ? 0 : 1;
		const pb = rb.class.toLowerCase() === "package" ? 0 : 1;
		if (pa !== pb) return pa - pb;
		return ra.name.localeCompare(rb.name, undefined, { sensitivity: "base", numeric: true });
	};
}

export function buildTree(rows: ExportRow[]): Tree {
	const children = new Map<number, number[]>();
	const roots: number[] = [];
	for (const row of rows) {
		const outer = row.outer - 1;
		if (row.outer > 0 && outer < rows.length) {
			let list = children.get(outer);
			if (!list) {
				list = [];
				children.set(outer, list);
			}
			list.push(row.i);
		} else {
			roots.push(row.i);
		}
	}
	const cmp = compareRows(rows);
	roots.sort(cmp);
	for (const list of children.values()) list.sort(cmp);
	const descendants = new Map<number, number>();
	const count = (i: number): number => {
		const cached = descendants.get(i);
		if (cached !== undefined) return cached;
		let n = 0;
		for (const c of children.get(i) ?? []) n += 1 + count(c);
		descendants.set(i, n);
		return n;
	};
	for (const row of rows) count(row.i);
	return { roots, children, descendants };
}

/** Every ancestor export index of `i`, nearest first. */
export function ancestors(rows: ExportRow[], i: number): number[] {
	const out: number[] = [];
	let cur = rows[i]?.outer ?? 0;
	let guard = 0;
	while (cur > 0 && cur - 1 < rows.length && guard++ < 64) {
		out.push(cur - 1);
		cur = rows[cur - 1]!.outer;
	}
	return out;
}

/** Distinct class names with counts, most common first. */
export function classCounts(rows: ExportRow[]): { name: string; count: number }[] {
	const counts = new Map<string, number>();
	for (const row of rows) counts.set(row.class, (counts.get(row.class) ?? 0) + 1);
	return [...counts.entries()]
		.map(([name, count]) => ({ name, count }))
		.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}
