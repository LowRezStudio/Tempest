// The export table arrives as columns (`ExportColumns`); the rows the
// components work with, with their object paths and class names, are built
// here once per package open.
import type {
	ExportRow,
	ImportRow,
	PackageTables,
	PackageTablesWire,
} from "../../../electron/src/ipc/contract";

/** The name a package reference points at: an export, an import, or "Class" for the null class. */
function refName(ref: number, names: string[], imports: ImportRow[]): string {
	if (ref > 0) return names[ref - 1] ?? "?";
	if (ref < 0) return imports[-ref - 1]?.name ?? "?";
	return "Class";
}

export function expandTables(wire: PackageTablesWire): PackageTables {
	const c = wire.exports;
	const n = c.names.length;
	const paths = new Array<string>(n);
	const pathOf = (i: number): string => {
		const cached = paths[i];
		if (cached !== undefined) return cached;
		const outer = c.outers[i];
		let p: string;
		if (outer > 0 && outer - 1 < n) {
			p = `${pathOf(outer - 1)}.${c.names[i]}`;
		} else if (outer < 0) {
			p = `${wire.imports[-outer - 1]?.path ?? "?"}.${c.names[i]}`;
		} else {
			p = c.names[i];
		}
		paths[i] = p;
		return p;
	};
	const exports: ExportRow[] = new Array(n);
	for (let i = 0; i < n; i++) {
		exports[i] = {
			i,
			name: c.names[i],
			path: pathOf(i),
			class: refName(c.classes[i], c.names, wire.imports),
			classRef: c.classes[i],
			outer: c.outers[i],
			super: c.supers[i],
			archetype: c.archetypes[i],
			size: c.sizes[i],
			offset: c.offsets[i],
			flags: "",
			flagsHex: `0x${c.objectFlags[i]}`,
			exportFlags: c.exportFlags[i],
		};
	}
	return { ...wire, exports };
}
