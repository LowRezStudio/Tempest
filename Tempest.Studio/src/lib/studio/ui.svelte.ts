// Window chrome state: which view the activity bar shows, pane sizes and
// collapsed panes (remembered per window size class), the one dialog open at
// a time, the palette, and the zoom level.
import { invoke } from "$lib/platform/api";

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
};

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
