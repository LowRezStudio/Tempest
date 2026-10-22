import type { AnsiRun } from "$lib/utils/ansi";

// East Asian wide/fullwidth characters and common emoji occupy two monospace columns.
const WIDE =
	/[\u1100-\u115f\u2e80-\u303e\u3041-\u33ff\u3400-\u4dbf\u4e00-\u9fff\ua000-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe10-\ufe19\ufe30-\ufe6f\uff00-\uff60\uffe0-\uffe6]|[\u{1f300}-\u{1f64f}\u{1f680}-\u{1f6ff}\u{1f900}-\u{1f9ff}\u{1fa70}-\u{1faff}\u{20000}-\u{3fffd}]/u;

const columnWidth = (char: string): number => (WIDE.test(char) ? 2 : 1);

// Wrap before rendering so virtualization can use fixed-height rows.
export function wrapRuns(runs: readonly AnsiRun[], width: number, firstWidth = width): AnsiRun[][] {
	const rows: AnsiRun[][] = [];
	const maxWidth = Math.max(1, Math.floor(width));
	let current: AnsiRun[] = [];
	let used = 0;
	let limit = Math.max(1, Math.floor(firstWidth));

	const append = (text: string, cls: string): void => {
		const previous = current.at(-1);
		if (previous && previous.cls === cls) {
			previous.text += text;
		} else {
			current.push({ text, cls });
		}
	};

	const nextRow = (): void => {
		rows.push(current);
		current = [];
		used = 0;
		limit = maxWidth;
	};

	for (const run of runs) {
		for (const char of run.text) {
			if (char === "\t") {
				let spaces = 8 - (used % 8);
				while (spaces > 0) {
					if (used >= limit) nextRow();
					const take = Math.min(limit - used, spaces);
					append(" ".repeat(take), run.cls);
					used += take;
					spaces -= take;
				}
			} else {
				const cells = columnWidth(char);
				if (used + cells > limit && used > 0) nextRow();
				append(char, run.cls);
				used += cells;
			}
		}
	}

	rows.push(current);
	return rows;
}
