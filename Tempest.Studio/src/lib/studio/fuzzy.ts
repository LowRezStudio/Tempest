/**
 * Fuzzy score of `text` for `query`: every query character in order, with
 * bonuses for consecutive runs, word starts and an exact substring; null when
 * it does not match. Case-insensitive.
 * @param query What the user typed.
 * @param text The candidate.
 */
export function fuzzy(query: string, text: string): number | null {
	const q = query.toLowerCase().trim();
	if (!q) return 0;
	const t = text.toLowerCase();
	const sub = t.indexOf(q);
	if (sub >= 0) return 1000 - sub - t.length / 100;
	let score = 0;
	let ti = 0;
	let run = 0;
	for (const ch of q) {
		if (ch === " ") continue;
		const at = t.indexOf(ch, ti);
		if (at < 0) return null;
		run = at === ti ? run + 1 : 0;
		const boundary = at === 0 || /[\s._/\\-]/.test(t[at - 1] ?? "");
		score += 1 + run * 2 + (boundary ? 3 : 0) - (at - ti) * 0.05;
		ti = at + 1;
	}
	return score - t.length / 100;
}
