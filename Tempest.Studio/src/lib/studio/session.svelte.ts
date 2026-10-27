// Workspace files (`.twrk`): the session's unsaved work and where the user
// was, saved to a file and reopened later, independent of saving packages in
// place or "Save as". The backend writes and applies the file (`twrk.*`);
// this store collects and restores the renderer's state around it, and keeps
// a crash-recovery copy per game folder.
import { invoke, pickFile, pickSave } from "$lib/platform/api";
import { assembly } from "./assembly.svelte";
import { cooker } from "./cooker.svelte";
import { basename, dirname } from "./format";
import { nav, type Place } from "./nav.svelte";
import { rpc } from "./rpc";
import { studio, type IdRange, type StudioMode } from "./state.svelte";
import { toasts } from "./toast.svelte";
import { ui } from "./ui.svelte";
import type { McsPath, TwrkMode } from "../../../electron/src/ipc/contract";

export const TWRK_FILTERS = [{ name: "Tempest Studio workspace", extensions: ["twrk"] }];
const MODE_KEY = "studio.twrkMode";
const RECENT_KEY = "studio.twrkRecents";
const AUTOSAVE_MS = 2 * 60 * 1000;

/** What the renderer keeps in a workspace file besides the files themselves. */
type SavedState = {
	version: 1;
	mode: StudioMode;
	pkg: string | null;
	selected: number | null;
	extras: string[];
	idRange: IdRange;
	assembly: { doc: string; table: string; path: McsPath; selected: number | null } | null;
	nav: { history: Place[]; pos: number };
	ui?: unknown;
	/** Set when the workspace file is a mod project. */
	project?: Project | null;
};

/** A mod project: a folder holding the workspace file, `mod-files/` and `builds/`. */
export type Project = { dir: string; name: string };

class Session {
	/** The workspace file this window saves to, once saved or opened. */
	file = $state<{ path: string; mode: TwrkMode } | null>(null);
	/** Mode for new workspace files (remembered). */
	defaultMode = $state<TwrkMode>((localStorage.getItem(MODE_KEY) as TwrkMode | null) ?? "linked");
	saving = $state(false);
	/** The mod project open in this window (its workspace file is `file`). */
	project = $state<Project | null>(null);
	/** Project mode: whether the project file holds every edit. */
	projectState = $state<"saved" | "pending" | "saving" | "error">("saved");
	private projectTimer: ReturnType<typeof setTimeout> | null = null;
	private lastSeen = "";
	/** Workspace files saved or opened lately, newest first. */
	recents = $state<string[]>(JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]") as string[]);
	/** When the workspace file was last written. */
	savedAt = $state<number | null>(null);
	/** Anything unsaved anywhere (packages or `.dat` files). */
	hasChanges = $derived(studio.dirty.length > 0 || assembly.dirtyDocs.length > 0);
	/** A recovery copy of an earlier session of this game folder, offered on open. */
	recovery = $state<{ path: string; mtime: number } | null>(null);
	/** Extra renderer state (pane layout…) the shell contributes. */
	extraState: () => unknown = () => undefined;
	extraRestore: (ui: unknown) => void = () => {};

	/** A recovery copy was written by this window (cleared once nothing is unsaved). */
	private wroteRecovery = false;

	constructor() {
		setInterval(() => void this.autosave(), AUTOSAVE_MS);
		studio.openWorkspaceFile = (path) => this.open(path).then(() => {});
		$effect.root(() => {
			$effect(() => {
				if (studio.workspace?.dir) void this.checkRecovery();
			});
			// A project keeps every edit: it saves itself a moment after each change.
			$effect(() => {
				const seen = `${studio.edits}|${assembly.edits}|${JSON.stringify(cooker.snapshot())}`;
				if (!this.project || !this.file) {
					this.lastSeen = seen;
					return;
				}
				if (seen === this.lastSeen) return;
				this.lastSeen = seen;
				this.projectState = "pending";
				if (this.projectTimer) clearTimeout(this.projectTimer);
				this.projectTimer = setTimeout(() => void this.saveProject(), 1500);
			});
			$effect(() => {
				// Everything saved or discarded: the recovery copy would bring back stale changes.
				if (!this.hasChanges && this.wroteRecovery) {
					this.wroteRecovery = false;
					void this.clearRecovery();
				}
			});
		});
	}

	private remember(path: string): void {
		this.recents = [path, ...this.recents.filter((r) => r !== path)].slice(0, 8);
		localStorage.setItem(RECENT_KEY, JSON.stringify(this.recents));
	}

	/** Take projects off the recent list (all of them without a path); the files stay. */
	forgetRecent(path?: string): void {
		this.recents = path ? this.recents.filter((r) => r !== path) : [];
		localStorage.setItem(RECENT_KEY, JSON.stringify(this.recents));
	}

	private collect(): SavedState {
		const page = assembly.page;
		return {
			version: 1,
			mode: studio.mode,
			pkg: studio.pkg?.path ?? null,
			selected: studio.selected,
			extras: studio.extraPackages.map((p) => p.path),
			idRange: studio.idRange,
			assembly: page
				? {
						doc: page.doc,
						table: page.table,
						path: $state.snapshot(page.path) as McsPath,
						selected: assembly.selected,
					}
				: null,
			nav: { history: $state.snapshot(nav.history) as Place[], pos: nav.pos },
			ui: this.extraState(),
			project: this.project ? { ...this.project } : null,
		};
	}

	/**
	 * Save the workspace file: to the current file, else (or with `as`) to a chosen one.
	 * @param as Ask for a new file and mode even when one is set.
	 * @param mode The mode for a new file; the remembered default otherwise.
	 */
	async save(as = false, mode?: TwrkMode, quiet = false): Promise<boolean> {
		if (!studio.workspace) return false;
		let target = this.file;
		if (as || !target) {
			const name = `${basename(dirname(dirname(studio.workspace.dir))) || "workspace"}.twrk`;
			const out = await pickSave(
				"Save workspace",
				this.file?.path ?? name,
				TWRK_FILTERS,
				"twrk",
			);
			if (!out) return false;
			target = {
				path: out.endsWith(".twrk") ? out : `${out}.twrk`,
				mode: mode ?? this.defaultMode,
			};
		}
		this.saving = true;
		try {
			const call = () =>
				rpc("twrk.save", {
					out: target.path,
					mode: target.mode,
					state: this.collect(),
					include: studio.extraPackages.map((p) => p.path),
				});
			const r = quiet
				? await call()
				: await studio.run(`Saving ${basename(target.path)}…`, call);
			this.file = target;
			this.savedAt = Date.now();
			this.remember(target.path);
			localStorage.setItem(MODE_KEY, target.mode);
			this.defaultMode = target.mode;
			if (this.project) this.projectState = "saved";
			const whole = r.files.filter((f) => f.stored === "full" && !f.unchanged).length;
			const deltas = r.files.filter((f) => f.stored === "delta").length;
			if (!quiet) {
				toasts.push(
					r.warnings.length > 0 ? "warning" : "success",
					`${r.files.length} file${r.files.length === 1 ? "" : "s"} (${deltas} as changes, ${whole} whole), ${fmtSize(r.size)}${r.warnings.length > 0 ? `; ${r.warnings.join("; ")}` : ""}`,
					`Workspace saved (${r.mode})`,
				);
			}
			void this.clearRecovery();
			return true;
		} catch (error) {
			if (this.project) this.projectState = "error";
			toasts.error(
				error,
				this.project ? "Could not save the project" : "Could not save the workspace",
			);
			return false;
		} finally {
			this.saving = false;
		}
	}

	/** Save the project file now (also runs by itself after each change). */
	async saveProject(): Promise<void> {
		if (!this.project || !this.file) return;
		// Files edited since the last save join the project's list before it is written.
		await cooker.refresh();
		if (this.saving) {
			this.projectState = "pending";
			if (this.projectTimer) clearTimeout(this.projectTimer);
			this.projectTimer = setTimeout(() => void this.saveProject(), 1000);
			return;
		}
		this.projectState = "saving";
		await this.save(false, undefined, true);
	}

	/**
	 * Create a mod project: its folder (workspace file, `mod-files/`, `builds/`), the game
	 * folder open, the mod's details set; the current edits of that game folder come along.
	 */
	async createProject(p: {
		dir: string;
		name: string;
		id: string;
		author: string;
		version: string;
		gameDir: string;
	}): Promise<boolean> {
		if (this.project && !(await this.closeProject())) return false;
		try {
			await invoke("fs:mkdir", { path: `${p.dir}/mod-files` });
			await invoke("fs:mkdir", { path: `${p.dir}/builds` });
			await invoke("fs:write-text", {
				path: `${p.dir}/mod-files/README.txt`,
				text: [
					"Files in this folder go into the mod as they are laid out here:",
					"",
					"  scripts/main.lua        Lua scripts (needs tempest-scripting)",
					"  dlls/64/your.dll        DLLs injected into the 64-bit client (dlls/32/ for 32-bit)",
					"  files/ChaosGame/...     any game file, shipped whole",
					"",
					"Packages and .dat files you edit in Tempest Studio are not copied here:",
					"the mod gets only what you changed in them.",
					"",
				].join("\n"),
			});
			if (studio.workspace?.dir !== p.gameDir && !(await studio.openWorkspace(p.gameDir))) {
				return false;
			}
			cooker.startProject(
				{ name: p.name, id: p.id, author: p.author, version: p.version },
				p.dir,
			);
			this.project = { dir: p.dir, name: p.name };
			this.file = { path: `${p.dir}/${p.id}.twrk`, mode: "linked" };
			this.projectState = "saving";
			const ok = await this.save(false, undefined, true);
			if (ok) {
				toasts.push(
					"success",
					`Saved in ${p.dir}. Everything you change is kept there.`,
					`Project ${p.name} created`,
				);
			}
			return ok;
		} catch (error) {
			toasts.error(error, "Could not create the project");
			return false;
		}
	}

	/** Write a pending project save now (before closing it or opening another). */
	async flush(): Promise<void> {
		if (!this.projectTimer && this.projectState !== "pending") return;
		if (this.projectTimer) clearTimeout(this.projectTimer);
		this.projectTimer = null;
		if (this.project && this.file) {
			this.projectState = "saving";
			await cooker.refresh();
			await this.save(false, undefined, true);
		}
	}

	/**
	 * Close the project: its edits are unloaded from the Studio (they live in the
	 * project file and come back when it is opened again), so nothing of the
	 * project can be saved into the game folder by mistake. Asks first: Save and
	 * close or Discard and close when a save is pending or failed, else Close.
	 * @returns false when the user cancelled or the project could not be saved.
	 */
	async closeProject(): Promise<boolean> {
		const project = this.project;
		if (!project) return true;
		// A save already running finishes first: there is nothing to ask about it.
		for (let i = 0; i < 600 && this.projectState === "saving"; i++) {
			await new Promise((r) => setTimeout(r, 100));
		}
		const files = [...studio.dirty, ...assembly.dirtyDocs.map((d) => d.path)].map(basename);
		const unsaved = this.projectState !== "saved";
		if (unsaved) {
			const answer = await ui.confirm({
				title: `Save the changes to ${project.name}?`,
				message:
					this.projectState === "error"
						? "The project could not be saved. Try again, or discard the changes made since it was last saved."
						: "Some changes are not saved in the project yet. Save them, or discard the changes made since the last save.",
				items: files,
				confirm: "Save and close",
				alt: "Discard and close",
			});
			if (answer === false) return false;
			if (answer === true) {
				await this.flush();
				if (this.projectState !== "saved" && !(await this.save(false, undefined, true))) {
					return false;
				}
			} else {
				if (this.projectTimer) clearTimeout(this.projectTimer);
				this.projectTimer = null;
			}
		} else if (files.length > 0) {
			const answer = await ui.confirm({
				title: `Close ${project.name}?`,
				message:
					"Your edits are saved in the project and come back when you open it again. They are unloaded from the Studio, so the game folder's files show as they are on disk.",
				items: files,
				confirm: "Close the project",
			});
			if (answer !== true) return false;
		}
		this.project = null;
		this.file = null;
		this.projectState = "saved";
		cooker.projectDir = null;
		cooker.projectFiles = [];
		cooker.targets = [];
		await this.unload();
		return true;
	}

	/** Drop every edit held in memory (they are kept elsewhere: a project file). */
	private async unload(): Promise<void> {
		if (studio.dirty.length > 0) await studio.discardPaths([...studio.dirty]);
		for (const d of assembly.dirtyDocs) await assembly.discard(d.id);
	}

	/** Choose a workspace file and open it. */
	async pickAndOpen(): Promise<void> {
		const file = await pickFile("Open workspace", TWRK_FILTERS, undefined, "twrk");
		if (file) await this.open(file);
	}

	/**
	 * Open a workspace file: its game folder (or another one, for a portable file whose
	 * folder is not on this machine), its changes, then where the user was.
	 * @param path The `.twrk` file.
	 * @param recovered A recovery copy: not adopted as this window's workspace file.
	 */
	async open(path: string, recovered = false): Promise<boolean> {
		// The project being left is closed first (it asks when a save is pending);
		// nothing of it may land in the next one.
		if (this.project && !(await this.closeProject())) return false;
		try {
			const info = await rpc("twrk.read", { path });
			if (
				(studio.dirty.length > 0 || assembly.dirtyDocs.length > 0) &&
				!recovered &&
				studio.workspace
			) {
				toasts.push(
					"warning",
					"Save or discard the unsaved changes first, or open the workspace in a new window.",
					"Unsaved changes",
				);
				return false;
			}
			let dir = info.gameDir;
			const exists = await invoke("fs:exists", { path: `${dir}/Core.upk` });
			if (!exists) {
				if (info.mode === "linked") {
					toasts.push(
						"info",
						`${dir} is not on this machine; pick the same game version's CookedPCConsole folder (a linked workspace only holds the changes).`,
						"Game folder",
					);
				}
				const picked = await invoke("dialog:open", {
					title: "CookedPCConsole folder for this workspace",
					directory: true,
				});
				if (!picked?.[0]) return false;
				dir = picked[0];
			}
			if (studio.workspace?.dir !== dir || studio.dirty.length > 0) {
				if (!(await studio.openWorkspace(dir))) return false;
			}
			const r = await studio.run(`Applying ${basename(path)}…`, () =>
				rpc("twrk.apply", { path }),
			);
			for (const p of r.packages) {
				if (p.dirty && !studio.dirty.includes(p.path)) studio.dirty.push(p.path);
			}
			for (const e of r.extras) studio.addExternal(e);
			if (r.docs.length > 0) await assembly.open();
			await this.restore(r.state as SavedState | null);
			if (!recovered) {
				this.file = { path, mode: r.mode };
				this.remember(path);
			}
			const n = r.packages.length + r.docs.length;
			toasts.push(
				r.problems.length > 0 ? "warning" : "success",
				`${n} file${n === 1 ? "" : "s"} with unsaved changes${r.problems.length > 0 ? `; ${r.problems.join("; ")}` : ""}`,
				recovered ? "Session recovered" : "Workspace opened",
			);
			return true;
		} catch (error) {
			if (!recovered && !(await invoke("fs:exists", { path }))) {
				this.forgetRecent(path);
				toasts.push(
					"warning",
					`${path} was moved or deleted, so it was removed from the recent projects.`,
					"Project not found",
				);
				return false;
			}
			toasts.error(error, "Could not open the workspace");
			return false;
		}
	}

	private async restore(state: SavedState | null): Promise<void> {
		this.project = state?.project ?? null;
		this.projectState = "saved";
		if (!state || state.version !== 1) return;
		if (state.idRange !== undefined) studio.idRange = state.idRange;
		if (state.assembly) {
			if (!assembly.summary) await assembly.open();
			if (assembly.summary) {
				await assembly.openTable(
					state.assembly.doc,
					state.assembly.table,
					state.assembly.path,
					state.assembly.selected,
				);
			}
		}
		if (state.pkg) await studio.openPackage(state.pkg, state.selected ?? undefined);
		studio.mode = state.mode;
		if (state.nav?.history?.length) {
			nav.history = state.nav.history;
			nav.pos = Math.min(state.nav.pos, state.nav.history.length - 1);
		}
		if (state.ui !== undefined) this.extraRestore(state.ui);
	}

	// ----- crash recovery ------------------------------------------------------

	/** Every two minutes with unsaved changes: a linked copy in the app's data folder. */
	private async autosave(): Promise<void> {
		if (!studio.workspace || !this.hasChanges || this.saving || studio.busy) return;
		try {
			const r = await invoke("workspace:recovery", { dir: studio.workspace.dir });
			await rpc("twrk.save", { out: r.path, mode: "linked", state: this.collect() });
			this.wroteRecovery = true;
		} catch {
			// A failed autosave must not interrupt the work; the next one retries.
		}
	}

	/** Look for a recovery copy of the open game folder (called when a folder opens). */
	async checkRecovery(): Promise<void> {
		this.recovery = null;
		if (!studio.workspace || this.wroteRecovery) return;
		const r = await invoke("workspace:recovery", { dir: studio.workspace.dir });
		if (r.exists && r.mtime) this.recovery = { path: r.path, mtime: r.mtime };
	}

	async restoreRecovery(): Promise<void> {
		if (!this.recovery) return;
		const ok = await this.open(this.recovery.path, true);
		if (ok) this.recovery = null;
	}

	async clearRecovery(): Promise<void> {
		this.recovery = null;
		if (studio.workspace) {
			await invoke("workspace:recovery", { dir: studio.workspace.dir, clear: true });
		}
	}
}

function fmtSize(n: number): string {
	if (n < 1024) return `${n} B`;
	if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
	return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export const session = new Session();
