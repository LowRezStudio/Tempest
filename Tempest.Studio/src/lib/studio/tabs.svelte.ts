// Tabs over the places of the window: each tab remembers one place (a
// package object or an assembly table/row). Moving around changes the
// active tab's place; Ctrl/middle-click on a package, export or reference
// opens a new tab; clicking a tab goes back to its place.
import { basename } from "./format";
import { nav, type Place } from "./nav.svelte";

export type Tab = { id: number; place: Place };

let next = 1;

class Tabs {
	list = $state<Tab[]>([]);
	active = $state<number | null>(null);
	/** Set while a tab switch replays its place, so it does not move the tab. */
	private switching = false;
	/** The next recorded place opens a new tab instead of moving the active one. */
	private openNext = false;

	constructor() {
		nav.onRecord((place) => this.track(place));
	}

	private track(place: Place): void {
		if (this.switching) return;
		const current = this.list.find((t) => t.id === this.active);
		if (!current || this.openNext) {
			this.openNext = false;
			const tab = { id: next++, place };
			const at = current ? this.list.indexOf(current) + 1 : this.list.length;
			this.list = [...this.list.slice(0, at), tab, ...this.list.slice(at)];
			this.active = tab.id;
			return;
		}
		current.place = place;
	}

	/** The next place visited goes to a new tab (Ctrl/middle-click). */
	openInNewTab(): void {
		this.openNext = true;
	}

	async activate(id: number): Promise<void> {
		const tab = this.list.find((t) => t.id === id);
		if (!tab) return;
		this.active = id;
		this.switching = true;
		try {
			await nav.visit(tab.place);
		} finally {
			this.switching = false;
		}
	}

	duplicate(): void {
		const current = this.list.find((t) => t.id === this.active);
		if (!current) return;
		const tab = { id: next++, place: $state.snapshot(current.place) as Place };
		const at = this.list.indexOf(current) + 1;
		this.list = [...this.list.slice(0, at), tab, ...this.list.slice(at)];
		this.active = tab.id;
	}

	async close(id = this.active): Promise<void> {
		const i = this.list.findIndex((t) => t.id === id);
		if (i < 0) return;
		const wasActive = this.active === id;
		this.list = this.list.filter((t) => t.id !== id);
		if (!wasActive) return;
		const neighbour = this.list[i] ?? this.list[i - 1];
		this.active = neighbour?.id ?? null;
		if (neighbour) await this.activate(neighbour.id);
	}

	async cycle(step: 1 | -1): Promise<void> {
		if (this.list.length < 2) return;
		const i = this.list.findIndex((t) => t.id === this.active);
		const target = this.list[(i + step + this.list.length) % this.list.length];
		if (target) await this.activate(target.id);
	}

	clear(): void {
		this.list = [];
		this.active = null;
	}

	label(place: Place): { title: string; detail: string } {
		if (place.kind === "package") {
			const leaf = place.label?.split(".").at(-1) ?? `#${place.index}`;
			return {
				title: leaf,
				detail: `${basename(place.path)} › ${place.label ?? place.index}`,
			};
		}
		const table = place.table.replace(/^DATA_SET_/, "");
		return {
			title: place.index === null ? table : `${table} #${place.index}`,
			detail: `${place.doc} › ${place.table}${place.index === null ? "" : ` row ${place.index}`}`,
		};
	}
}

export const tabs = new Tabs();
