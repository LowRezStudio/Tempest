// Every action of the Studio as a command: one place that defines its title,
// its shortcut and whether it can run now. The global key handler, the
// command palette (Ctrl+K) and the shortcut list (F1) all read from here.
import { untrack, type Component } from "svelte";

export type Command = {
	id: string;
	title: string;
	/** Palette section. */
	group: string;
	/** Shortcut as written in the UI: `Mod+S`, `Mod+Shift+P`, `Alt+ArrowLeft`, `F1`. */
	keys?: string;
	icon?: Component;
	/** Whether the command can run now (greyed out otherwise). */
	enabled?: () => boolean;
	/** Runs even when focus is in a text field (e.g. Save, the palette). */
	global?: boolean;
	/** Hidden from the palette (shortcut-only helpers). */
	hidden?: boolean;
	run: () => unknown;
};

const isMac = navigator.platform.toLowerCase().includes("mac");

/** `Mod+Shift+S` → `Ctrl+Shift+S` (or `⌘⇧S` on macOS). */
export function keyLabel(keys: string): string {
	if (!isMac) {
		return keys.replace("Mod", "Ctrl").replace("ArrowLeft", "←").replace("ArrowRight", "→");
	}
	return keys
		.replace("Mod+", "⌘")
		.replace("Shift+", "⇧")
		.replace("Alt+", "⌥")
		.replace("Ctrl+", "⌃")
		.replace("ArrowLeft", "←")
		.replace("ArrowRight", "→");
}

function matches(keys: string, e: KeyboardEvent): boolean {
	const parts = keys.split("+");
	const key = parts.at(-1)!.toLowerCase();
	const want = new Set(parts.slice(0, -1).map((p) => p.toLowerCase()));
	const mod = isMac ? e.metaKey : e.ctrlKey;
	if (want.has("ctrl")) {
		// Explicit Ctrl (Ctrl+Tab): the Control key on every platform.
		if (!e.ctrlKey) return false;
	} else if (want.has("mod") !== mod) {
		return false;
	}
	if (want.has("shift") !== e.shiftKey) return false;
	if (want.has("alt") !== e.altKey) return false;
	if (!isMac && e.metaKey) return false;
	if (isMac && !want.has("ctrl") && e.ctrlKey) return false;
	const k = e.key.toLowerCase();
	// Shift changes e.key for symbols; compare the physical key for those.
	if (key === "/" || key === "?") return e.code === "Slash";
	if (key === "=" || key === "+") return e.code === "Equal" || k === "+" || k === "=";
	if (key === "-") return e.code === "Minus" || k === "-";
	if (key === "tab") return k === "tab";
	return k === key;
}

function inField(target: EventTarget | null): boolean {
	const el = target as HTMLElement | null;
	return !!el?.closest?.("input, textarea, select, [contenteditable=true], .cm-editor");
}

class Commands {
	list = $state<Command[]>([]);

	/** Register commands (replacing those with the same id); returns an unregister function. */
	register(commands: Command[]): () => void {
		const ids = new Set(commands.map((c) => c.id));
		this.list = [...untrack(() => this.list).filter((c) => !ids.has(c.id)), ...commands];
		return () => {
			this.list = this.list.filter((c) => !ids.has(c.id) || !commands.includes(c));
		};
	}

	get(id: string): Command | undefined {
		return this.list.find((c) => c.id === id);
	}

	enabled(c: Command): boolean {
		return c.enabled ? c.enabled() : true;
	}

	run(id: string): void {
		const c = this.get(id);
		if (c && this.enabled(c)) void c.run();
	}

	/** The window's key handler: the first enabled command whose shortcut matches. */
	handle(e: KeyboardEvent): boolean {
		const field = inField(e.target);
		for (const c of this.list) {
			if (!c.keys || !matches(c.keys, e)) continue;
			if (field && !c.global) continue;
			if (!this.enabled(c)) continue;
			e.preventDefault();
			void c.run();
			return true;
		}
		return false;
	}
}

export const commands = new Commands();
