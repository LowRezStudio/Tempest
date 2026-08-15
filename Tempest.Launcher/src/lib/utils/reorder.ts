export type SlotRect = {
	top: number;
	bottom: number;
	left: number;
	mid: number;
	cx: number;
};

export function reorderArray<T>(array: readonly T[], from: number, to: number): T[] {
	if (from === to || from < 0 || from >= array.length) return [...array];
	const next = [...array];
	const [item] = next.splice(from, 1);
	next.splice(Math.max(0, Math.min(to, next.length)), 0, item);
	return next;
}

export function computeDropIndex(
	orderedIds: readonly string[],
	rects: ReadonlyMap<string, SlotRect>,
	clientY: number,
): number {
	let target = 0;
	for (let i = 0; i < orderedIds.length; i++) {
		const r = rects.get(orderedIds[i]);
		if (r && clientY >= r.mid) target = i;
	}
	return Math.max(0, Math.min(target, orderedIds.length - 1));
}

export function computeGridDropIndex(
	orderedIds: readonly string[],
	rects: ReadonlyMap<string, SlotRect>,
	clientX: number,
	clientY: number,
): number {
	let target = 0;
	for (let i = 0; i < orderedIds.length; i++) {
		const r = rects.get(orderedIds[i]);
		if (!r) continue;
		if (clientY > r.bottom || (clientY >= r.top && clientX > r.cx)) target++;
	}
	return Math.min(target, orderedIds.length);
}

export function measureGrid(
	orderedIds: readonly string[],
	rects: ReadonlyMap<string, SlotRect>,
): {
	columns: number;
	xPos: number[];
	yPos: number[];
} {
	const ROW_TOLERANCE = 8;
	const rows: string[][] = [];
	for (let i = 0; i < orderedIds.length; i++) {
		const r = rects.get(orderedIds[i]);
		const row = rows.find((group) => {
			const first = group[0] ? rects.get(group[0]) : undefined;
			return !!first && !!r && Math.abs(r.top - first.top) <= ROW_TOLERANCE;
		});
		if (row && r) {
			row.push(orderedIds[i]);
		} else if (r) {
			rows.push([orderedIds[i]]);
		}
	}
	let columns = 1;
	for (const row of rows) {
		if (row.length > columns) columns = row.length;
	}

	const xPos: number[] = [];
	const yPos: number[] = [];
	const xCount: number[] = [];
	const yCount: number[] = [];
	for (let i = 0; i < orderedIds.length; i++) {
		const r = rects.get(orderedIds[i]);
		if (!r) continue;
		const col = i % columns;
		const row = Math.floor(i / columns);
		xPos[col] = (xPos[col] ?? 0) + r.left;
		xCount[col] = (xCount[col] ?? 0) + 1;
		yPos[row] = (yPos[row] ?? 0) + r.top;
		yCount[row] = (yCount[row] ?? 0) + 1;
	}
	for (let c = 0; c < columns; c++) xPos[c] = xPos[c]! / xCount[c]!;
	for (let r = 0; r < yCount.length; r++) yPos[r] = yPos[r]! / yCount[r]!;

	return { columns, xPos, yPos };
}
