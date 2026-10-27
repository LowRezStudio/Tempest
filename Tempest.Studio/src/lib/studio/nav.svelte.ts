import { basename } from "./format";
// One Back/Forward history for the whole window: package selections and
// assembly rows alike, so following a reference from a row into a package
// (or back) can be retraced. The stores register how to go to their own
// kind of place; `navigating` keeps a replayed visit out of the history.
import type { McsPath } from "../../../electron/src/ipc/contract";

export type PackagePlace = { kind: "package"; path: string; index: number; label?: string };
export type AssemblyPlace = {
	kind: "assembly";
	doc: string;
	path: McsPath;
	table: string;
	index: number | null;
};
export type Place = PackagePlace | AssemblyPlace;

type Handlers = {
	package?: (p: PackagePlace) => Promise<void>;
	assembly?: (p: AssemblyPlace) => Promise<void>;
};

function same(a: Place, b: Place): boolean {
	if (a.kind === "package" && b.kind === "package") {
		return a.path === b.path && a.index === b.index;
	}
	if (a.kind === "assembly" && b.kind === "assembly") {
		return (
			a.doc === b.doc &&
			a.table === b.table &&
			a.index === b.index &&
			JSON.stringify(a.path) === JSON.stringify(b.path)
		);
	}
	return false;
}

class Nav {
	history = $state<Place[]>([]);
	pos = $state(-1);
	canBack = $derived(this.pos > 0);
	canForward = $derived(this.pos < this.history.length - 1);
	navigating = false;
	private handlers: Handlers = {};
	private listeners: ((p: Place) => void)[] = [];

	/** Called with every place recorded (tabs follow the window's moves). */
	onRecord(listener: (p: Place) => void): void {
		this.listeners.push(listener);
	}

	/** Go to a place without touching the history (tab switches). */
	async visit(p: Place): Promise<void> {
		const before = this.navigating;
		this.navigating = true;
		try {
			await (p.kind === "package" ? this.handlers.package?.(p) : this.handlers.assembly?.(p));
		} finally {
			this.navigating = before;
		}
	}

	register(handlers: Handlers): void {
		this.handlers = { ...this.handlers, ...handlers };
	}

	/** Append a visit, dropping the forward entries. */
	record(place: Place): void {
		for (const l of this.listeners) l(place);
		if (this.navigating) return;
		const current = this.history[this.pos];
		if (current && same(current, place)) {
			// Same place: keep the freshest label.
			if (place.kind === "package" && place.label) this.history[this.pos] = place;
			return;
		}
		this.history = [...this.history.slice(0, this.pos + 1), place].slice(-200);
		this.pos = this.history.length - 1;
	}

	clear(): void {
		this.history = [];
		this.pos = -1;
	}

	/** Where Back (-1) / Forward (1) would go, as text. */
	label(offset: -1 | 1): string | null {
		const p = this.history[this.pos + offset];
		if (!p) return null;
		if (p.kind === "package") return `${basename(p.path)} › ${p.label ?? `export #${p.index}`}`;
		const table = p.table.replace(/^DATA_SET_/, "");
		return `${p.doc} › ${table}${p.index === null ? "" : ` #${p.index}`}`;
	}

	private async goTo(pos: number): Promise<void> {
		const p = this.history[pos];
		if (!p) return;
		this.pos = pos;
		this.navigating = true;
		try {
			await (p.kind === "package" ? this.handlers.package?.(p) : this.handlers.assembly?.(p));
		} finally {
			this.navigating = false;
		}
	}

	back(): Promise<void> {
		return this.canBack ? this.goTo(this.pos - 1) : Promise.resolve();
	}

	forward(): Promise<void> {
		return this.canForward ? this.goTo(this.pos + 1) : Promise.resolve();
	}
}

export const nav = new Nav();
