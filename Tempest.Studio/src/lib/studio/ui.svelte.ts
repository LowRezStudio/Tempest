// Window chrome state: which view the activity bar shows, pane sizes and
// collapsed panes (remembered per window size class), the one dialog open at
// a time, the palette, and the zoom level.
import { invoke } from "$lib/platform/api";
import type { MenuItem } from "$lib/components/shell/MenuItems.svelte";

export type DialogId =
	| "search"
	| "replace"
	| "batch"
	| "verify"
	| "rewrite"
	| "modFolder"
	| "wwise"
	| "mcp"
	| "modExport"
	| "newContent"
	| "vendor"
	| "shortcuts"
	| "saveWorkspace"
	| "idRange"
	| "newProject";

export type PaletteMode = "commands" | "open" | "objects";

const PANES_KEY = "studio.panes";
const ZOOM_KEY = "studio.zoom";

function loadPanes(): Record<string, number> {
	try {
		return JSON.parse(localStorage.getItem(PANES_KEY) ?? "{}") as Record<string, number>;
	} catch {
		return {};
	}
}

/** An object action the inspector carries out for the selected export (asked from a context menu). */
export type ObjectAction = "export" | "replace" | "raw" | "rename" | "copy" | "delete";

/** A question for the confirmation dialog (`ui.confirm`). */
export type ConfirmRequest = {
	title: string;
	message: string;
	/** Lines listed under the message (files, rows…). */
	items?: string[];
	confirm: string;
	cancel?: string;
	danger?: boolean;
	/** A third choice (for example "Save first"); resolves to "alt". */
	alt?: string;
	/**
	 * Offers "Don't ask me again": once ticked and confirmed, the question
	 * (kept under this key) answers itself with yes from then on.
	 */
	remember?: string;
};

const REMEMBER_PREFIX = "studio.confirmed:";

/** Whether the user ticked "Don't ask me again" for a remembered question. */
export function remembered(key: string): boolean {
	try {
		return localStorage.getItem(REMEMBER_PREFIX + key) === "1";
	} catch {
		return false;
	}
}

/** Remember (or forget) a "Don't ask me again". */
export function remember(key: string, on: boolean): void {
	try {
		if (on) localStorage.setItem(REMEMBER_PREFIX + key, "1");
		else localStorage.removeItem(REMEMBER_PREFIX + key);
	} catch {
		// storage unavailable: it asks again
	}
}

class Ui {
	/** Window width in CSS pixels (zoom included). */
	width = $state(window.innerWidth);
	height = $state(window.innerHeight);
	/** Narrow window: side panels overlay the editor instead of taking room. */
	compact = $derived(this.width < 1100);
	/** Pane sizes in CSS pixels, by pane id. */
	panes = $state<Record<string, number>>(loadPanes());
	/** Collapsed panes, by id (side bar, record pane…). */
	collapsed = $state<Record<string, boolean>>({});
	dialog = $state<DialogId | null>(null);
	palette = $state<PaletteMode | null>(null);
	/** Text the palette opens with. */
	paletteQuery = $state("");
	/** Text the package search opens with (and runs). */
	searchQuery = $state("");
	zoom = $state(Number(localStorage.getItem(ZOOM_KEY) ?? "1") || 1);
	/** The open context menu: where, and its items. */
	contextMenu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	/** An action for the inspector to run on export `index` once it shows it. */
	objectAction = $state<{ kind: ObjectAction; index: number } | null>(null);
	/** The question the confirmation dialog shows, with its answer callback. */
	asking = $state<(ConfirmRequest & { answer: (v: boolean | "alt") => void }) | null>(null);

	constructor() {
		window.addEventListener("resize", () => {
			this.width = window.innerWidth;
			this.height = window.innerHeight;
		});
		$effect.root(() => {
			$effect(() => {
				localStorage.setItem(PANES_KEY, JSON.stringify(this.panes));
			});
			$effect(() => {
				localStorage.setItem(ZOOM_KEY, String(this.zoom));
				void invoke("window:zoom", { factor: this.zoom });
			});
		});
	}

	size(id: string, fallback: number): number {
		return this.panes[id] ?? fallback;
	}

	setSize(id: string, px: number): void {
		this.panes[id] = Math.round(px);
	}

	isCollapsed(id: string): boolean {
		return this.collapsed[id] ?? false;
	}

	toggle(id: string, value?: boolean): void {
		this.collapsed[id] = value ?? !this.isCollapsed(id);
	}

	open(dialog: DialogId): void {
		this.palette = null;
		this.dialog = dialog;
	}

	close(): void {
		this.dialog = null;
	}

	/**
	 * Show a context menu for a right-click (or the keyboard's menu key: then
	 * under the element). Items that are null or false are left out.
	 * @param event The `contextmenu` event.
	 * @param items The menu.
	 */
	openContextMenu(event: MouseEvent, items: (MenuItem | null | false | undefined)[]): void {
		event.preventDefault();
		event.stopPropagation();
		const list = items.filter((i): i is MenuItem => !!i);
		// No separator at either end or twice in a row.
		const clean = list.filter(
			(it, k) => it !== "-" || (k > 0 && k < list.length - 1 && list[k - 1] !== "-"),
		);
		if (clean.length === 0) return;
		let { clientX: x, clientY: y } = event;
		if (x === 0 && y === 0 && event.target instanceof Element) {
			const r = event.target.getBoundingClientRect();
			x = r.left + 12;
			y = r.bottom;
		}
		this.contextMenu = { x, y, items: clean };
	}

	openPalette(mode: PaletteMode, query = ""): void {
		this.dialog = null;
		this.paletteQuery = query;
		this.palette = mode;
	}

	/**
	 * Ask before something destructive; resolves true (confirmed), false (cancelled) or
	 * "alt" (the optional third button).
	 * @param req The question.
	 */
	confirm(req: ConfirmRequest): Promise<boolean | "alt"> {
		if (req.remember && remembered(req.remember)) return Promise.resolve(true);
		this.asking?.answer(false);
		return new Promise((resolve) => {
			this.asking = {
				...req,
				answer: (v) => {
					this.asking = null;
					resolve(v);
				},
			};
		});
	}

	setZoom(z: number): void {
		this.zoom = Math.min(2, Math.max(0.6, Math.round(z * 10) / 10));
	}
}

export const ui = new Ui();
