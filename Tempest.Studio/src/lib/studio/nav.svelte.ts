import { basename } from "./format";
// One Back/Forward history for the whole window: package selections and
// assembly rows alike, so following a reference from a row into a package
// (or back) can be retraced. The stores register how to go to their own
// kind of place; `navigating` keeps a replayed visit out of the history.
import type { McsPath } from "../../../electron/src/ipc/contract";

export type PackagePlace = {
	kind: "package";
	path: string;
	index: number;
	label?: string;
	/** A view inside the export's editor (the movie editor's node); none: as the editor left it. */
	view?: { node: string; label: string };
};
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
	/** Opens a place's `view` once its export is shown. */
	view?: (p: PackagePlace) => void;
};

function same(a: Place, b: Place): boolean {
	if (a.kind === "package" && b.kind === "package") {
		// A selection without a view is the export as its editor shows it.
		const view = !a.view || !b.view || a.view.node === b.view.node;
		return a.path === b.path && a.index === b.index && view;
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
			await this.go(p);
		} finally {
			this.navigating = before;
		}
	}

	private async go(p: Place): Promise<void> {
		if (p.kind === "assembly") return this.handlers.assembly?.(p);
		await this.handlers.package?.(p);
		if (p.view) this.handlers.view?.(p);
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
			// Same place: keep the freshest label, and the view a plain selection does not name.
			if (place.kind === "package" && current.kind === "package") {
				this.history[this.pos] = {
					...place,
					label: place.label ?? current.label,
					view: place.view ?? current.view,
				};
			}
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
		if (p.kind === "package") {
			const view = p.view ? ` › ${p.view.label}` : "";
			return `${basename(p.path)} › ${p.label ?? `export #${p.index}`}${view}`;
		}
		const table = p.table.replace(/^DATA_SET_/, "");
		return `${p.doc} › ${table}${p.index === null ? "" : ` #${p.index}`}`;
	}

	private async goTo(pos: number): Promise<void> {
		const p = this.history[pos];
		if (!p) return;
		this.pos = pos;
		this.navigating = true;
		try {
			await this.go(p);
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
