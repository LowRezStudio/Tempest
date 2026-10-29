// CodeMirror languages for the movie editor's scripts: ActionScript 2 on the
// legacy JavaScript mode, and the backend's P-code (`swf/as2/pcode.rs`).
import { StreamLanguage, type StringStream } from "@codemirror/language";
import { javascript } from "@codemirror/legacy-modes/mode/javascript";

export const actionscript = StreamLanguage.define(javascript);

/** Line-oriented: `Label:`, an action name, then values (`"…"`, numbers, `r1`, `c3:"…"`); `;` comments. */
export const pcode = StreamLanguage.define<{ start: boolean }>({
	name: "pcode",
	startState: () => ({ start: true }),
	token(stream: StringStream, state) {
		if (stream.sol()) state.start = true;
		if (stream.eatSpace()) return null;
		if (stream.peek() === ";") {
			stream.skipToEnd();
			return "comment";
		}
		if (stream.peek() === '"') {
			stream.next();
			let escaped = false;
			let ch: string | void;
			while ((ch = stream.next()) !== undefined) {
				if (ch === '"' && !escaped) break;
				escaped = !escaped && ch === "\\";
			}
			state.start = false;
			return "string";
		}
		// `StringStream.match` consumes what it matches (not `String#match`).
		const number = stream.match(/^-?(\d+\.?\d*(e[+-]?\d+)?f?|NaN|Infinity)/i);
		if (number) {
			state.start = false;
			return "number";
		}
		const word = stream.match(/^[A-Za-z_$][\w$]*/);
		if (word) {
			const text = stream.current();
			if (state.start) {
				state.start = false;
				if (stream.peek() === ":") return "labelName";
				return "keyword";
			}
			if (/^r\d+$/.test(text)) return "variableName.special";
			if (/^cw?\d+$/.test(text)) return "atom";
			if (/^(true|false|null|undefined)$/.test(text)) return "atom";
			if (/^L\d+$/.test(text)) return "labelName";
			return "propertyName";
		}
		stream.next();
		return null;
	},
});

/** The line an error message names (`line 12: …`), if any. */
export function errorLine(message: string): number | null {
	// Inside a `__pcode { … }` block the assembler counts from the block's first line.
	const raw = /at line (\d+): line (\d+)/.exec(message);
	if (raw) return Number(raw[1]) + Number(raw[2]) - 1;
	const m = /line (\d+)/.exec(message);
	return m ? Number(m[1]) : null;
}
