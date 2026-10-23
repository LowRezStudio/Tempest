// Strip escape sequences and map supported ANSI styles to theme classes.

export interface AnsiRun {
	text: string;
	/** Space-separated class names for the run's SGR state ("" when default). */
	cls: string;
}

const ESC = "\x1b";

const DEFAULT_SGR = {
	fg: -1,
	bold: false,
	dim: false,
	italic: false,
	underline: false,
	strike: false,
};

type Sgr = typeof DEFAULT_SGR;

function sgrClass(sgr: Sgr): string {
	const parts: string[] = [];
	if (sgr.fg >= 0) parts.push(`ansi-fg-${sgr.fg}`);
	if (sgr.bold) parts.push("ansi-bold");
	if (sgr.dim) parts.push("ansi-dim");
	if (sgr.italic) parts.push("ansi-italic");
	if (sgr.underline) parts.push("ansi-underline");
	if (sgr.strike) parts.push("ansi-strike");
	return parts.join(" ");
}

function applySgr(raw: string, state: Sgr): Sgr {
	const params = raw.split(";").map(Number);
	let next = { ...state };

	for (let i = 0; i < params.length; i++) {
		const code = params[i];
		if (code === 0) {
			next = { ...DEFAULT_SGR };
		} else if (code === 1) {
			next.bold = true;
		} else if (code === 2) {
			next.dim = true;
		} else if (code === 3) {
			next.italic = true;
		} else if (code === 4) {
			next.underline = true;
		} else if (code === 9) {
			next.strike = true;
		} else if (code === 22) {
			next.bold = false;
			next.dim = false;
		} else if (code === 23) {
			next.italic = false;
		} else if (code === 24) {
			next.underline = false;
		} else if (code === 29) {
			next.strike = false;
		} else if ((code >= 30 && code <= 37) || (code >= 90 && code <= 97)) {
			// The theme has one colour per slot, so bright reuses the normal one.
			next.fg = code % 10;
		} else if (code === 39) {
			next.fg = -1;
		} else if (code === 38 || code === 48) {
			// Skip unsupported extended foreground/background colors without treating their values as styles.
			const mode = params[i + 1];
			if (mode === 5) i += 2;
			else if (mode === 2) i += 4;
		}
	}

	return next;
}

// Returns where the escape sequence ends and, for SGR sequences, its payload.
function readEscape(input: string, start: number): { end: number; sgr?: string } {
	const kind = input[start + 1];

	if (kind === "[") {
		for (let i = start + 2; i < input.length; i++) {
			const code = input.charCodeAt(i);
			if (code < 0x40 || code > 0x7e) continue;
			return { end: i + 1, sgr: input[i] === "m" ? input.slice(start + 2, i) : undefined };
		}
		return { end: input.length };
	}

	if (kind === "]") {
		// OSC is terminated by BEL or ST (ESC \).
		for (let i = start + 2; i < input.length; i++) {
			if (input.charCodeAt(i) === 0x07) return { end: i + 1 };
			if (input[i] === ESC && input[i + 1] === "\\") return { end: i + 2 };
		}
		return { end: input.length };
	}

	// Charset selection (ESC ( B) carries one extra byte, everything else two.
	const width = kind === "(" || kind === ")" || kind === "*" || kind === "+" ? 3 : 2;
	return { end: Math.min(input.length, start + width) };
}

// Keep the final progress-bar update, ignoring trailing carriage returns.
export function normalizeLine(text: string): string {
	let end = text.length;
	while (end > 0 && text[end - 1] === "\r") end--;
	const line = text.slice(0, end);
	return line.slice(line.lastIndexOf("\r") + 1);
}

// Splits a line into styled runs, dropping escape sequences.
export function ansiRuns(input: string): AnsiRun[] {
	const runs: AnsiRun[] = [];
	let state = DEFAULT_SGR;
	let index = 0;

	const emit = (text: string): void => {
		if (!text) return;
		const cls = sgrClass(state);
		const previous = runs.at(-1);
		if (previous && previous.cls === cls) {
			runs[runs.length - 1] = { text: previous.text + text, cls };
			return;
		}
		runs.push({ text, cls });
	};

	while (index < input.length) {
		const escapeAt = input.indexOf(ESC, index);
		if (escapeAt < 0) {
			emit(input.slice(index));
			break;
		}
		emit(input.slice(index, escapeAt));
		const sequence = readEscape(input, escapeAt);
		if (sequence.sgr !== undefined) state = applySgr(sequence.sgr, state);
		index = sequence.end;
	}

	return runs;
}
