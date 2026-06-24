export type SlotRect = { top: number; mid: number };

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
