// Studio state: the open workspace (a cooked directory), the package whose
// tables are loaded, the selected export, and which packages have unsaved
// changes. Every mutation goes through `upk serve`; this store mirrors it.
import { invoke, listen } from "$lib/platform/api";
import { basename, dirname, stem } from "./format";
import { rpc } from "./rpc";
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
	backendError = $state<string | null>(null);
	/** What the window shows: the package editor, or the assembly/lang editor. */
	mode = $state<"packages" | "assembly">("packages");
	/** Bumped whenever the export table of `pkg` changed (rename, delete, copy). */
	tablesVersion = $state(0);
	/** Navigation history of selections (package path + export), browser style. */
	history = $state<{ path: string; index: number }[]>([]);
	historyPos = $state(-1);
	private navigating = false;
	/** Undo labels of the open package, newest first. */
	undoLabels = $state<string[]>([]);
	/** Edited class sources not yet compiled, keyed `path|class`. */
	drafts = $state<Record<string, string>>({});
	/** Object currently shown by the inspector (cache by export index). */
	object = $state<ParsedObject | null>(null);
	objectLoading = $state(false);

	/** Packages opened from outside the workspace folder (double-clicked files, mod folders). */
	extraPackages = $state<WorkspacePackage[]>([]);
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
	canBack = $derived(this.historyPos > 0);
	canForward = $derived(this.historyPos < this.history.length - 1);

	constructor() {
		$effect.root(() => {
			$effect(() => {
				document.documentElement.setAttribute("data-theme", this.theme);
				localStorage.setItem(THEME_KEY, this.theme);
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
			mode?: "packages" | "assembly";
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

	async openWorkspace(dir: string, quiet = false): Promise<boolean> {
		try {
			const ws = await this.run(`Loading class database from ${basename(dir)}…`, () =>
				rpc("workspace.open", { dir }),
			);
			this.workspace = ws;
			this.backendError = null;
			this.pkg = null;
			this.object = null;
			this.selected = null;
			this.dirty = [];
			this.history = [];
			this.historyPos = -1;
			this.extraPackages = [];
			this.recents = [dir, ...this.recents.filter((r) => r !== dir)].slice(0, 8);
			localStorage.setItem(RECENTS_KEY, JSON.stringify(this.recents));
			toasts.push(
				"success",
				`${ws.packages.length} packages, ${ws.scriptPackages.length} script packages (${ws.loadMs} ms)`,
				"Workspace open",
			);
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
			void this.refreshUndo();
			if (select !== undefined) await this.select(select);
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

	/** Append a selection to the history (dropping any forward entries). */
	private record(path: string, index: number): void {
		if (this.navigating) return;
		const current = this.history[this.historyPos];
		if (current && current.path === path && current.index === index) return;
		this.history = [...this.history.slice(0, this.historyPos + 1), { path, index }].slice(-200);
		this.historyPos = this.history.length - 1;
	}

	/** Where Back / Forward would go, as "package › object" text. */
	historyLabel(offset: -1 | 1): string | null {
		const entry = this.history[this.historyPos + offset];
		if (!entry) return null;
		const object =
			this.pkg?.path === entry.path
				? this.pkg.exports[entry.index]?.path
				: `export #${entry.index}`;
		return `${basename(entry.path)} › ${object ?? entry.index}`;
	}

	private async goTo(pos: number): Promise<void> {
		const entry = this.history[pos];
		if (!entry) return;
		this.historyPos = pos;
		this.navigating = true;
		try {
			await (this.pkg?.path === entry.path
				? this.select(entry.index)
				: this.openPackage(entry.path, entry.index));
		} finally {
			this.navigating = false;
		}
	}

	back(): Promise<void> {
		return this.canBack ? this.goTo(this.historyPos - 1) : Promise.resolve();
	}

	forward(): Promise<void> {
		return this.canForward ? this.goTo(this.historyPos + 1) : Promise.resolve();
	}

	markDirty(path = this.pkg?.path): void {
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
			toasts.push("success", r.out, "Saved");
			return true;
		} catch (error) {
			toasts.error(error, "Save failed");
			return false;
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
