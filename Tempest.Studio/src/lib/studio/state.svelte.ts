// Studio state: the open workspace (a cooked directory), the package whose
// tables are loaded, the selected export, and which packages have unsaved
// changes. Every mutation goes through `upk serve`; this store mirrors it.
import { invoke, listen } from "$lib/platform/api";
import { basename, dirname, stem } from "./format";
import { nav } from "./nav.svelte";
import { rpc } from "./rpc";
import { warmRuffle } from "./ruffle";
import { toasts } from "./toast.svelte";
import type {
	ExportRow,
	PackageTables,
	ParsedObject,
	Workspace,
	WorkspacePackage,
} from "../../../electron/src/ipc/contract";

const RECENTS_KEY = "studio.recents";
const SESSION_KEY = "studio.session";
const THEME_KEY = "studio.theme";
const ID_RANGE_KEY = "studio.idRange";

/** The editor the window shows. */
export type StudioMode = "packages" | "assembly" | "mod";

/** The id range new content takes its ids from, `null` for "past the largest in use". */
export type IdRange = { start: number; end: number } | null;

function loadIdRange(): IdRange {
	try {
		const raw = localStorage.getItem(ID_RANGE_KEY);
		const v = raw ? (JSON.parse(raw) as IdRange) : null;
		return v && Number.isFinite(v.start) && Number.isFinite(v.end) && v.end > v.start
			? v
			: null;
	} catch {
		return null;
	}
}

function loadRecents(): string[] {
	try {
		const raw = localStorage.getItem(RECENTS_KEY);
		return raw ? (JSON.parse(raw) as string[]) : [];
	} catch {
		return [];
	}
}

class Studio {
	workspace = $state<Workspace | null>(null);
	/** Bumped on every (re)opened game folder: the backend dropped everything it held. */
	workspaceEpoch = $state(0);
	/** Busy message shown in the status bar while a long call runs. */
	busy = $state<string | null>(null);
	/** Raw state: 65k export rows must not become proxies; the tables are replaced, never mutated. */
	pkg = $state.raw<PackageTables | null>(null);
	selected = $state<number | null>(null);
	/** Package paths with unsaved edits. */
	dirty = $state<string[]>([]);
	recents = $state<string[]>(loadRecents());
	theme = $state<"mocha" | "latte">(
		(localStorage.getItem(THEME_KEY) as "mocha" | "latte" | null) ?? "mocha",
	);
	/**
	 * Ids for new content (devices, items, champions, messages…) come from this
	 * range, so two mods made with the Studio do not allocate the same ids; a
	 * per-user setting, applied to the backend when the assembly opens.
	 */
	idRange = $state<IdRange>(loadIdRange());
	backendError = $state<string | null>(null);
	/** What the window shows: the package editor, or the assembly/lang editor. */
	mode = $state<StudioMode>("packages");
	/** Bumped whenever the export table of `pkg` changed (rename, delete, copy). */
	tablesVersion = $state(0);
	/** Undo labels of the open package, newest first. */
	undoLabels = $state<string[]>([]);
	/** Edited class sources not yet compiled, keyed `path|class`. */
	drafts = $state<Record<string, string>>({});
	/** Object currently shown by the inspector (cache by export index). */
	object = $state<ParsedObject | null>(null);
	objectLoading = $state(false);

	/** Game files saved in place (their original kept by the Studio), and exported copies by game file. */
	marks = $state<{ inPlace: string[]; copies: Record<string, string[]> }>({
		inPlace: [],
		copies: {},
	});
	/** Packages opened from outside the workspace folder (double-clicked files, mod folders). */
	extraPackages = $state<WorkspacePackage[]>([]);
	/** Opens a `.twrk` workspace file (set by the session store). */
	openWorkspaceFile: ((path: string) => Promise<void>) | null = null;
	/** Files to open once a workspace exists (opened from the OS before any game folder was chosen). */
	pendingFiles = $state<string[]>([]);
	packages = $derived<WorkspacePackage[]>([
		...this.extraPackages,
		...(this.workspace?.packages ?? []),
	]);
	selectedRow = $derived<ExportRow | null>(
		this.pkg && this.selected !== null ? (this.pkg.exports[this.selected] ?? null) : null,
	);
	isDirty = $derived(this.pkg ? this.dirty.includes(this.pkg.path) : false);

	constructor() {
		$effect.root(() => {
			$effect(() => {
				document.documentElement.setAttribute("data-theme", this.theme);
				localStorage.setItem(THEME_KEY, this.theme);
			});
			$effect(() => {
				try {
					if (this.idRange) {
						localStorage.setItem(ID_RANGE_KEY, JSON.stringify(this.idRange));
					} else {
						localStorage.removeItem(ID_RANGE_KEY);
					}
				} catch {
					// Storage unavailable: the range lives for this session only.
				}
			});
			$effect(() => {
				const title = this.pkg
					? `${this.pkg.name}${this.isDirty ? " •" : ""} — Tempest Studio`
					: "Tempest Studio";
				void invoke("window:set-title", { title });
			});
		});
		listen("upk:exit", () => {
			this.backendError = "the upk backend stopped; reopen the workspace";
			this.pkg = null;
			this.object = null;
			this.dirty = [];
		});
		listen("open-files", (files) => {
			void this.openFromOs(files);
		});
		// Survive a page reload (dev server, Ctrl+R): reopen what was open.
		$effect.root(() => {
			$effect(() => {
				const session = {
					dir: this.workspace?.dir ?? null,
					pkg: this.pkg?.path ?? null,
					selected: this.selected,
					mode: this.mode,
				};
				try {
					sessionStorage.setItem(SESSION_KEY, JSON.stringify(session));
				} catch {
					// ignore
				}
			});
		});
		void this.restoreSession().then(() => this.applyStartup());
	}

	/** Open what this window was created for (a folder, files); kept by the
	 * host until applied, so a dev reload mid-way replays it. */
	private async applyStartup(): Promise<void> {
		const startup = await invoke("window:startup", {});
		if (!startup) return;
		if (startup.dir && !this.workspace) await this.openWorkspace(startup.dir);
		if (startup.files?.length) await this.openFiles(startup.files);
		await invoke("window:startup", { done: true });
	}

	private async restoreSession(): Promise<void> {
		let session: {
			dir: string | null;
			pkg: string | null;
			selected: number | null;
			mode?: StudioMode;
		} | null = null;
		try {
			session = JSON.parse(sessionStorage.getItem(SESSION_KEY) ?? "null");
		} catch {
			return;
		}
		if (!session?.dir) return;
		await this.openWorkspace(session.dir);
		if (session.mode) this.mode = session.mode;
		if (this.workspace && session.pkg) {
			await this.openPackage(session.pkg, session.selected ?? undefined);
		}
	}

	async run<T>(message: string, work: () => Promise<T>): Promise<T> {
		this.busy = message;
		try {
			return await work();
		} finally {
			this.busy = null;
		}
	}

	/** Another Studio window, with its own backend: a second workspace side by side. */
	async newWindow(dir?: string): Promise<void> {
		await invoke("window:new", dir ? { dir } : {});
	}

	/** Take game folders off the recent list (all of them without a path); the folders stay. */
	forgetRecent(dir?: string): void {
		this.recents = dir ? this.recents.filter((r) => r !== dir) : [];
		localStorage.setItem(RECENTS_KEY, JSON.stringify(this.recents));
	}

	async openWorkspace(dir: string, quiet = false): Promise<boolean> {
		try {
			const ws = await this.run(`Loading class database from ${basename(dir)}…`, () =>
				rpc("workspace.open", { dir }),
			);
			this.workspace = ws;
			this.workspaceEpoch++;
			this.backendError = null;
			this.pkg = null;
			this.object = null;
			this.selected = null;
			this.dirty = [];
			nav.clear();
			this.extraPackages = [];
			this.recents = [dir, ...this.recents.filter((r) => r !== dir)].slice(0, 8);
			localStorage.setItem(RECENTS_KEY, JSON.stringify(this.recents));
			toasts.push(
				"success",
				`${ws.packages.length} packages, ${ws.scriptPackages.length} script packages (${ws.loadMs} ms)`,
				"Workspace open",
			);
			void this.refreshMarks();
			if (this.pendingFiles.length > 0) {
				const files = this.pendingFiles;
				this.pendingFiles = [];
				await this.openFiles(files);
			}
			return true;
		} catch (error) {
			if (!quiet) toasts.error(error, "Could not open workspace");
			return false;
		}
	}

	async refreshPackages(): Promise<void> {
		if (!this.workspace) return;
		this.workspace.packages = await rpc("workspace.packages", {});
	}

	async openPackage(path: string, select?: number): Promise<void> {
		try {
			const tables = await this.run(`Opening ${basename(path)}…`, () =>
				rpc("package.open", { path }),
			);
			this.pkg = tables;
			this.selected = null;
			this.object = null;
			this.tablesVersion++;
			if (tables.dirty && !this.dirty.includes(path)) this.dirty.push(path);
			if (tables.exports.some((e) => e.class.toLowerCase() === "swfmovie")) {
				requestIdleCallback(() => void warmRuffle(), { timeout: 2000 });
			}
			void this.refreshUndo();
			// An export added by an edit that was since discarded is gone: select nothing.
			if (select !== undefined && select < tables.exports.length) await this.select(select);
		} catch (error) {
			toasts.error(error, `Could not open ${basename(path)}`);
		}
	}

	async reloadTables(): Promise<void> {
		if (!this.pkg) return;
		const tables = await rpc("package.tables", { path: this.pkg.path });
		tables.path = this.pkg.path;
		this.pkg = tables;
		this.tablesVersion++;
	}

	async select(index: number | null): Promise<void> {
		this.selected = index;
		this.object = null;
		if (index === null || !this.pkg) return;
		const path = this.pkg.path;
		this.record(path, index);
		this.objectLoading = true;
		try {
			const obj = await rpc("object.get", { path, index });
			if (this.selected === index && this.pkg?.path === path) this.object = obj;
		} catch (error) {
			toasts.error(error, "Could not parse object");
		} finally {
			this.objectLoading = false;
		}
	}

	/** Append a selection to the window's history. */
	private record(path: string, index: number): void {
		const label = this.pkg?.path === path ? this.pkg.exports[index]?.path : undefined;
		nav.record({ kind: "package", path, index, label });
	}

	/** Go to a package place of the history (registered with `nav`). */
	async visit(path: string, index: number): Promise<void> {
		this.mode = "packages";
		await (this.pkg?.path === path ? this.select(index) : this.openPackage(path, index));
	}

	/** Bumped on every edit (and discard): a project saves itself when it changes. */
	edits = $state(0);

	markDirty(path = this.pkg?.path): void {
		this.edits++;
		if (path && !this.dirty.includes(path)) this.dirty.push(path);
		void this.refreshUndo();
	}

	async refreshUndo(): Promise<void> {
		if (!this.pkg) {
			this.undoLabels = [];
			return;
		}
		const path = this.pkg.path;
		try {
			const r = await rpc("package.undoList", { path });
			if (this.pkg?.path === path) {
				this.undoLabels = r.labels;
				if (!r.dirty) this.dirty = this.dirty.filter((d) => d !== path);
			}
		} catch {
			this.undoLabels = [];
		}
	}

	/** Revert the last operation on the open package. */
	async undo(): Promise<void> {
		if (!this.pkg) return;
		const path = this.pkg.path;
		const selected = this.selected;
		try {
			const r = await rpc("package.undo", { path });
			if (r.label === null) {
				toasts.push("info", "Nothing to undo", "Undo");
				return;
			}
			toasts.push("success", r.label, "Undone");
			this.undoLabels = r.remaining;
			if (!r.dirty) this.dirty = this.dirty.filter((d) => d !== path);
			this.clearDrafts(path);
			await this.reloadTables();
			await this.select(
				selected !== null && selected < (this.pkg?.exports.length ?? 0) ? selected : null,
			);
		} catch (error) {
			toasts.error(error, "Undo failed");
		}
	}

	/** Throw away every unsaved change of the open package and reload it from disk. */
	async discard(): Promise<void> {
		if (!this.pkg) return;
		const path = this.pkg.path;
		const selected = this.selected;
		try {
			await rpc("package.discard", { path });
			this.edits++;
			this.dirty = this.dirty.filter((d) => d !== path);
			this.clearDrafts(path);
			this.undoLabels = [];
			await this.openPackage(path, selected ?? undefined);
			toasts.push("success", "Reloaded from disk", "Changes discarded");
		} catch (error) {
			toasts.error(error, "Discard failed");
		}
	}

	/** Save the open package in place (or to `out`). */
	async save(out?: string): Promise<boolean> {
		if (!this.pkg) return false;
		const path = this.pkg.path;
		const selected = this.selected;
		try {
			const r = await this.run(`Saving ${basename(out ?? path)}…`, () =>
				rpc("package.save", { path, out }),
			);
			if (!r.dirty) this.dirty = this.dirty.filter((d) => d !== path);
			this.undoLabels = r.undo;
			if (r.reopened) {
				this.clearDrafts(path);
				await this.openPackage(path, selected ?? undefined);
			} else if (!out || out === path) {
				// Saved in place: the package (and its undo history) stays open.
				await this.reloadTables();
			}
			if (out && out !== path) {
				toasts.push(
					"success",
					`${r.out}. The edits stay unsaved here (Save writes them into the game folder); the mod cooker lists the copy.`,
					"Copy exported",
				);
			} else {
				toasts.push("success", r.out, "Saved");
			}
			void this.refreshMarks();
			return true;
		} catch (error) {
			toasts.error(error, "Save failed");
			return false;
		}
	}

	/** Duplicate the selected export next to it and select the copy. */
	async duplicate(): Promise<void> {
		if (!this.pkg || this.selected === null) return;
		const path = this.pkg.path;
		try {
			const r = await rpc("object.duplicate", { path, index: this.selected });
			this.markDirty(path);
			await this.reloadTables();
			await this.select(r.index);
			toasts.push("success", r.path, "Duplicated");
		} catch (error) {
			toasts.error(error, "Could not duplicate");
		}
	}

	/** Refresh which game files were saved in place or exported as copies (package list badges). */
	async refreshMarks(): Promise<void> {
		if (!this.workspace) return;
		try {
			const r = await rpc("mod.changes", {});
			const copies: Record<string, string[]> = {};
			for (const f of r.files) {
				if (f.origin === "copy") (copies[f.target] ??= []).push(f.source);
			}
			this.marks = {
				inPlace: r.files.filter((f) => f.origin === "saved").map((f) => f.target),
				copies,
			};
		} catch {
			// badges only
		}
	}

	/** Throw away the unsaved edits of packages (reloaded from disk). */
	async discardPaths(paths: string[]): Promise<void> {
		this.edits++;
		for (const path of paths) {
			try {
				await rpc("package.discard", { path });
				this.dirty = this.dirty.filter((d) => d !== path);
				this.clearDrafts(path);
			} catch (error) {
				toasts.error(error, `Could not discard ${basename(path)}`);
			}
		}
		if (this.pkg && paths.includes(this.pkg.path)) {
			this.undoLabels = [];
			await this.openPackage(this.pkg.path, this.selected ?? undefined);
		}
	}

	/** Save every package with unsaved changes in place. */
	async saveAll(): Promise<void> {
		const paths = [...this.dirty];
		let saved = 0;
		for (const path of paths) {
			try {
				const r = await this.run(`Saving ${basename(path)}…`, () =>
					rpc("package.save", { path }),
				);
				if (!r.dirty) this.dirty = this.dirty.filter((d) => d !== path);
				saved++;
			} catch (error) {
				toasts.error(error, `Could not save ${basename(path)}`);
			}
		}
		if (this.pkg && paths.includes(this.pkg.path)) {
			await this.reloadTables();
			await this.refreshUndo();
		}
		void this.refreshMarks();
		if (saved > 0) {
			toasts.push(
				"success",
				`${saved} package${saved === 1 ? "" : "s"} saved in place`,
				"Saved",
			);
		}
	}

	async closePackage(): Promise<void> {
		if (!this.pkg) return;
		const path = this.pkg.path;
		await rpc("package.close", { path });
		this.dirty = this.dirty.filter((d) => d !== path);
		this.clearDrafts(path);
		this.pkg = null;
		this.object = null;
		this.selected = null;
	}

	clearDrafts(path: string): void {
		const prefix = `${path}|`;
		for (const k of Object.keys(this.drafts)) {
			if (k.startsWith(prefix)) delete this.drafts[k];
		}
	}

	/**
	 * Packages opened from the OS (file association, argv, drag) or from
	 * anywhere outside the workspace folder. The class database comes from a
	 * game folder: the file's own folder when it is one, else the last one
	 * used; without either, the files wait until a folder is picked.
	 */
	async openFiles(files: string[]): Promise<void> {
		const twrk = files.find((f) => f.toLowerCase().endsWith(".twrk"));
		if (twrk && this.openWorkspaceFile) {
			await this.openWorkspaceFile(twrk);
			return;
		}
		const file = files[0];
		if (!file) return;
		if (!this.workspace) {
			const dir = dirname(file);
			let opened = await this.openWorkspace(dir, true);
			if (!opened && this.recents[0] && this.recents[0] !== dir) {
				opened = await this.openWorkspace(this.recents[0], true);
			}
			if (!opened) {
				this.pendingFiles = files;
				toasts.push(
					"info",
					"Pick the game's CookedPCConsole folder first; the file opens right after.",
					"Open package",
				);
				return;
			}
		}
		for (const f of files) this.addExternal(f);
		await this.openPackage(file);
	}

	/** List a package from outside the workspace folder in the package pane. */
	addExternal(file: string): void {
		const inWorkspace = this.workspace?.packages.some((p) => p.path === file) ?? false;
		if (inWorkspace || this.extraPackages.some((p) => p.path === file)) return;
		this.extraPackages = [
			{ name: stem(file), path: file, size: 0, script: false, map: false, open: true },
			...this.extraPackages,
		];
	}

	private openFromOs(files: string[]): Promise<void> {
		return this.openFiles(files);
	}
}

export const studio = new Studio();
