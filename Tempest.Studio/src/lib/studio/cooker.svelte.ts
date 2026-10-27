// The mod cooker. A mod is a list of game files it changes; for each one the
// user picks where the edited content comes from (the unsaved edits in the
// Studio, the file saved in place, or an exported copy) and the cooker makes a
// delta against the file's original (the Studio's record, the launcher's
// backup, or an original the user confirms or picks). Plus other files (Lua,
// DLLs, loose files). Cooked into a signed `.tempest` with tempest-cli, then
// saved, installed or published like the launcher's cooker does.
import { invoke, pickDirectory, pickFile, pickFiles, pickSave } from "$lib/platform/api";
import { basename, dirname } from "./format";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import { ui } from "./ui.svelte";
import type { CookItem, CookPreview, CookSource } from "../../../electron/src/ipc/contract";

export const MOD_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{1,63}$/;
const PROJECT_KEY = "studio.cooker";

export type Extra = { src: string; dest: string };
export type Author = { name: string; link: string };
export type LogLine = { kind: "info" | "ok" | "error"; text: string };
/** The user's choice for one game file of the mod. */
export type Choice = { on: boolean; source: string | null; mode: "delta" | "whole" };
/** One game file the mod can change, with every source of edited content for it. */
export type Entry = {
	target: string;
	rel: string;
	kind: "upk" | "mcts";
	sources: CookSource[];
	choice: Choice;
	chosen: CookSource | null;
};

export type Meta = {
	id: string;
	name: string;
	version: string;
	authors: Author[];
	readme: string;
	dependencies: string;
	icon: string | null;
	patchFrom: string;
	patchTo: string;
	summary: string;
	changelog: string;
	tags: string;
};

/** What a workspace's mod project keeps (per game folder, and in workspace files). */
type Project = { meta: Meta; extras: Extra[]; choices: Record<string, Choice> };

const emptyMeta = (): Meta => ({
	id: "",
	name: "",
	version: "1.0.0",
	authors: [{ name: "", link: "" }],
	readme: "",
	dependencies: "",
	icon: null,
	patchFrom: "",
	patchTo: "",
	summary: "",
	changelog: "",
	tags: "",
});

/** Where a loose file goes inside the mod by default. */
export function defaultDest(src: string): string {
	const name = basename(src);
	const lower = name.toLowerCase();
	if (lower.endsWith(".lua")) return `scripts/${name}`;
	if (lower.endsWith(".dll")) return `dlls/64/${name}`;
	return `files/${name}`;
}

const isGameFile = (p: string) => /\.(upk|dat)$/i.test(p);

/** The default source of a game file: unsaved edits, else the newest copy, else the in-place save. */
function preferred(sources: CookSource[]): CookSource | null {
	const unsaved = sources.find((s) => s.origin === "unsaved");
	if (unsaved) return unsaved;
	const copies = sources
		.filter((s) => s.origin === "copy")
		.sort((a, b) => (b.when ?? 0) - (a.when ?? 0));
	return copies[0] ?? sources.find((s) => s.origin === "saved") ?? null;
}

class Cooker {
	meta = $state<Meta>(emptyMeta());
	extras = $state<Extra[]>([]);
	choices = $state<Record<string, Choice>>({});
	sources = $state<CookSource[]>([]);
	/** Preview by source id (what the delta would hold), filled for the chosen sources. */
	previews = $state<Record<string, CookPreview>>({});
	previewing = $state(false);
	cli = $state<{
		available: boolean;
		path: string | null;
		hasKey: boolean;
		signingKey: string;
	} | null>(null);
	busy = $state<string | null>(null);
	log = $state<LogLine[]>([]);
	/** The last package cooked: where it is and who signed it. */
	cooked = $state<{ path: string; signer: string | null } | null>(null);

	/** The game folder the project belongs to. */
	private dir: string | null = null;
	private previewTimer: ReturnType<typeof setTimeout> | null = null;

	entries = $derived.by<Entry[]>(() => {
		const byTarget = new Map<string, CookSource[]>();
		for (const s of this.sources) {
			const list = byTarget.get(s.target) ?? [];
			list.push(s);
			byTarget.set(s.target, list);
		}
		return [...byTarget.entries()]
			.map(([target, sources]) => {
				const saved = this.choices[target];
				const fallback = preferred(sources);
				const choice: Choice = saved ?? {
					on: true,
					source: fallback?.id ?? null,
					mode: sources.some((s) => s.script) ? "whole" : "delta",
				};
				const chosen = sources.find((s) => s.id === choice.source) ?? fallback;
				return {
					target,
					rel: sources[0]!.rel,
					kind: sources[0]!.kind,
					sources,
					choice,
					chosen,
				};
			})
			.sort((a, b) => a.rel.localeCompare(b.rel));
	});
	included = $derived(this.entries.filter((e) => e.choice.on && e.chosen));
	/** Included delta entries whose original is not known yet: cooking waits for them. */
	missingOriginals = $derived(
		this.included.filter((e) => e.choice.mode === "delta" && e.chosen?.base.kind === "none"),
	);
	idValid = $derived(MOD_ID_PATTERN.test(this.meta.id));
	canCook = $derived(
		this.idValid &&
			this.meta.name.trim() !== "" &&
			(this.included.length > 0 || this.extras.length > 0) &&
			this.missingOriginals.length === 0 &&
			!!this.cli?.available,
	);

	constructor() {
		$effect.root(() => {
			$effect(() => {
				const dir = studio.workspace?.dir ?? null;
				if (dir !== this.dir) {
					this.dir = dir;
					this.load();
				}
			});
			$effect(() => {
				const snapshot = JSON.stringify(this.snapshot());
				if (this.dir) localStorage.setItem(`${PROJECT_KEY}:${this.dir}`, snapshot);
			});
		});
	}

	// ----- project ---------------------------------------------------------------

	private load(): void {
		this.cooked = null;
		this.log = [];
		this.previews = {};
		let p: Partial<Project> = {};
		try {
			p = JSON.parse(
				localStorage.getItem(`${PROJECT_KEY}:${this.dir}`) ?? "{}",
			) as Partial<Project>;
		} catch {
			// a damaged project starts over
		}
		this.meta = { ...emptyMeta(), ...p.meta };
		this.extras = p.extras ?? [];
		this.choices = p.choices ?? {};
	}

	/** Start a new mod: empty details, every file included again, no other files. */
	async newMod(): Promise<void> {
		const ok = await ui.confirm({
			title: "Start a new mod?",
			message:
				"The details, the choices and the other files of the current mod are cleared. Your edits and files are not touched.",
			confirm: "New mod",
		});
		if (ok !== true) return;
		this.meta = emptyMeta();
		this.extras = [];
		this.choices = {};
		this.cooked = null;
		this.log = [];
		this.schedulePreview();
	}

	setChoice(target: string, patch: Partial<Choice>): void {
		const e = this.entries.find((x) => x.target === target);
		if (!e) return;
		this.choices = { ...this.choices, [target]: { ...e.choice, ...patch } };
		this.schedulePreview();
	}

	// ----- sources ---------------------------------------------------------------

	async refresh(): Promise<void> {
		this.cli = await invoke("cli:status", {});
		if (!studio.workspace) return;
		try {
			this.sources = (await rpc("mod.changes", {})).files;
		} catch (error) {
			toasts.error(error, "Could not list the changes");
		}
		this.schedulePreview(0);
	}

	schedulePreview(delay = 300): void {
		if (this.previewTimer) clearTimeout(this.previewTimer);
		this.previewTimer = setTimeout(() => void this.preview(), delay);
	}

	private items(): CookItem[] {
		return this.included.map((e) => ({
			id: e.chosen!.id,
			source: e.chosen!.source,
			target: e.target,
			kind: e.kind,
			mode: e.choice.mode,
		}));
	}

	/** What each included file would put in the mod. */
	async preview(): Promise<void> {
		const items = this.items().filter((i) => {
			const e = this.included.find((x) => x.chosen?.id === i.id);
			return !(i.mode === "delta" && e?.chosen?.base.kind === "none");
		});
		if (items.length === 0) return;
		this.previewing = true;
		try {
			const r = await rpc("mod.preview", { items });
			const next = { ...this.previews };
			for (const p of r.items) if (p.id) next[p.id] = p;
			this.previews = next;
		} catch (error) {
			toasts.error(error, "Could not preview the mod");
		} finally {
			this.previewing = false;
		}
	}

	/** "The game file is unmodified": take it as the original from now on. */
	async useGameFile(target: string): Promise<void> {
		const ok = await ui.confirm({
			title: "Use the game file as the original?",
			message: `${basename(target)} in the game folder becomes the original this mod is compared with. Only do this if the file was never modified (by you, another tool or a mod installed as a whole file); otherwise pick a clean copy instead.`,
			confirm: "It is unmodified",
			alt: "Pick the original…",
		});
		if (ok === "alt") return this.pickOriginal(target);
		if (ok !== true) return;
		try {
			await rpc("delta.record", { path: target });
			await this.refresh();
		} catch (error) {
			toasts.error(error, "Could not record the original");
		}
	}

	/** Pick a clean copy of a game file (a backup, another install) as its original. */
	async pickOriginal(target: string): Promise<void> {
		const ext = target.toLowerCase().endsWith(".dat") ? "dat" : "upk";
		const file = await pickFile(
			`The original ${basename(target)} (from a backup or a clean install)`,
			[{ name: basename(target), extensions: [ext] }],
			undefined,
			"originals",
		);
		if (!file) return;
		if (basename(file).toLowerCase() !== basename(target).toLowerCase()) {
			const ok = await ui.confirm({
				title: "Different file name",
				message: `${basename(file)} does not have the name of ${basename(target)}. Use it as its original anyway?`,
				confirm: "Use it",
			});
			if (ok !== true) return;
		}
		try {
			await studio.run("Recording the original…", () =>
				rpc("delta.record", { path: target, from: file }),
			);
			await this.refresh();
			toasts.push(
				"success",
				`${basename(file)} is the original of ${basename(target)}`,
				"Original set",
			);
		} catch (error) {
			toasts.error(error, "Could not read the original");
		}
	}

	/** Add modified game files from anywhere: each becomes a source of the game file of the same name. */
	async addModified(files?: string[]): Promise<void> {
		const picked =
			files ??
			(await pickFiles(
				"Modified packages or .dat files",
				[{ name: "Packages and .dat files", extensions: ["upk", "dat"] }],
				"cooker",
			));
		for (const file of picked) {
			let target = (await rpc("mod.match", { path: file })).target;
			if (!target) {
				const ok = await ui.confirm({
					title: `Which game file is ${basename(file)}?`,
					message:
						"No file of this name is in the game folder. Pick the game file it is a modified version of, or ship it as a new file.",
					confirm: "Pick the game file…",
					alt: "Ship it as a new file",
				});
				if (ok === "alt") {
					this.extras = [
						...this.extras,
						{ src: file, dest: `files/ChaosGame/CookedPCConsole/${basename(file)}` },
					];
					continue;
				}
				if (ok !== true) continue;
				target = await pickFile(
					"The game file it replaces",
					[],
					studio.workspace?.dir,
					"game-file",
				);
				if (!target) continue;
			}
			await rpc("mod.addCopy", { path: file, target });
			this.choices = { ...this.choices, [target]: { on: true, source: null, mode: "delta" } };
		}
		await this.refresh();
		// The new copy becomes the chosen source of its game file.
		for (const file of picked) {
			const s = this.sources.find(
				(x) => x.origin === "copy" && x.source.toLowerCase() === file.toLowerCase(),
			);
			if (s) this.setChoice(s.target, { on: true, source: s.id });
		}
	}

	async forgetCopy(s: CookSource): Promise<void> {
		await rpc("mod.forgetCopy", { path: s.source });
		await this.refresh();
	}

	// ----- other files ----------------------------------------------------------

	async addFiles(): Promise<void> {
		const files = await pickFiles("Files to ship in the mod", [], "cooker");
		const modified = files.filter(isGameFile);
		for (const f of files.filter((x) => !isGameFile(x))) {
			this.extras = [...this.extras, { src: f, dest: defaultDest(f) }];
		}
		if (modified.length > 0) {
			toasts.push(
				"info",
				`${modified.map((f) => basename(f)).join(", ")}: added as modified game files, so the mod gets a delta.`,
				"Modified game files",
			);
			await this.addModified(modified);
		}
	}

	async addFolder(): Promise<void> {
		const dir = await pickDirectory("Folder to ship in the mod (for example scripts/)");
		if (dir) this.extras = [...this.extras, { src: dir, dest: basename(dir) }];
	}

	async pickIcon(): Promise<void> {
		const f = await pickFile("Mod icon (PNG, square)", [
			{ name: "PNG image", extensions: ["png"] },
		]);
		if (f) this.meta.icon = f;
	}

	// ----- cook, install, publish -------------------------------------------------

	private say(kind: LogLine["kind"], text: string): void {
		this.log = [...this.log, { kind, text }].slice(-200);
	}

	private async cli$(args: string[]): Promise<{ code: number; stdout: string; stderr: string }> {
		this.say(
			"info",
			`tempest mod ${args.map((a) => (a.includes(" ") ? `"${a}"` : a)).join(" ")}`,
		);
		const r = await invoke("cli:mod", { args });
		const out = `${r.stdout}${r.stderr}`.trim();
		if (out) this.say(r.code === 0 ? "ok" : "error", out);
		return r;
	}

	/** Create a signing key (`mod gen-key`), trusted on this machine. */
	async createKey(id: string): Promise<void> {
		this.busy = "Creating a signing key…";
		try {
			const r = await this.cli$(["gen-key", "--id", id || "tempest-studio"]);
			if (r.code !== 0) throw new Error(r.stderr || r.stdout);
			await this.refresh();
		} catch (error) {
			toasts.error(error, "Could not create the key");
		} finally {
			this.busy = null;
		}
	}

	/** Cook into `out` (asked for when absent): mod folder, pack, sign. */
	async cook(out?: string): Promise<string | null> {
		if (!this.canCook) return null;
		const m = this.meta;
		const target =
			out ??
			(await pickSave(
				"Save the mod package",
				`${m.id}-${m.version}.tempest`,
				[{ name: "Tempest mod", extensions: ["tempest"] }],
				"cooker-out",
			));
		if (!target) return null;
		this.busy = "Cooking…";
		this.log = [];
		try {
			const staging = await invoke("cli:staging", { id: m.id });
			const r = await studio.run("Cooking the mod…", () =>
				rpc("mod.cook", {
					dir: staging,
					id: m.id,
					name: m.name.trim(),
					version: m.version.trim() || "1.0.0",
					authors: ($state.snapshot(m.authors) as Author[]).filter((a) => a.name.trim()),
					readme: m.readme,
					dependencies: m.dependencies
						.split(/[\n,]/)
						.map((d) => d.trim())
						.filter(Boolean),
					icon: m.icon ?? undefined,
					items: this.items(),
					extras: $state.snapshot(this.extras) as Extra[],
				}),
			);
			for (const e of r.entries) {
				const what =
					e.mode === "delta"
						? `delta: ${e.changed ?? 0} changed, ${e.added ?? 0} added, ${e.deleted ?? 0} removed`
						: e.mode === "unchanged"
							? "no changes, left out"
							: e.mode;
				this.say(e.mode === "unchanged" ? "info" : "ok", `${e.rel}: ${what}`);
			}
			const packed = await this.cli$(["pack", staging, target]);
			if (packed.code !== 0) throw new Error("tempest mod pack failed (see the log)");
			let signer: string | null = null;
			if (this.cli?.hasKey) {
				const signed = await this.cli$(["sign", target]);
				if (signed.code !== 0) throw new Error("tempest mod sign failed (see the log)");
				signer = signed.stdout.match(/with (.+)$/m)?.[1]?.trim() ?? "your key";
			} else {
				this.say(
					"info",
					"No signing key: the package is unsigned (fine for assets, refused for code).",
				);
			}
			this.cooked = { path: target, signer };
			toasts.push(
				"success",
				`${basename(target)}${signer ? `, signed by ${signer}` : ", unsigned"}`,
				"Mod cooked",
			);
			return target;
		} catch (error) {
			toasts.error(error, "Cooking failed");
			return null;
		} finally {
			this.busy = null;
		}
	}

	/** Install the cooked package into the open game folder (or another one). */
	async install(other = false): Promise<void> {
		if (!this.cooked) return;
		let game = studio.workspace ? dirname(dirname(studio.workspace.dir)) : null;
		if (other || !game) {
			game = await pickDirectory(
				"Game folder to install the mod into (the one holding ChaosGame)",
			);
		} else {
			// Files saved in place here already hold the edits: the mod lands on top of them.
			const edited = this.included.filter((e) =>
				this.sources.some((s) => s.target === e.target && s.origin === "saved"),
			);
			if (edited.length > 0) {
				const ok = await ui.confirm({
					title: "This game folder already has your edits",
					message:
						"These files were saved in place, so they already hold your changes. The mod would be applied on top of them, and testing here would not show the mod alone. A clean copy of the game is the better place to test.",
					items: edited.map((e) => e.rel),
					confirm: "Install here anyway",
					alt: "Install elsewhere…",
					danger: true,
				});
				if (ok === "alt") {
					game = await pickDirectory(
						"Game folder to install the mod into (the one holding ChaosGame)",
					);
				} else if (ok !== true) {
					return;
				}
			}
		}
		if (!game) return;
		this.busy = "Installing…";
		try {
			const r = await this.cli$([
				"install",
				game,
				this.cooked.path,
				"--replace",
				"--allow-untrusted",
			]);
			if (r.code !== 0) throw new Error("tempest mod install failed (see the log)");
			toasts.push("success", `Installed into ${game}`, "Mod installed");
		} catch (error) {
			toasts.error(error, "Install failed");
		} finally {
			this.busy = null;
		}
	}

	async login(key: string): Promise<boolean> {
		const r = await this.cli$(["login", key.trim()]);
		if (r.code !== 0) {
			toasts.push("error", r.stderr || r.stdout, "Login failed");
			return false;
		}
		return true;
	}

	/** Upload the cooked (signed) package to the mod repository. */
	async publish(): Promise<void> {
		if (!this.cooked) return;
		const m = this.meta;
		if (!m.patchFrom.trim()) {
			toasts.push("warning", "Give the first compatible patch (for example 8.1).", "Publish");
			return;
		}
		const args = ["publish", this.cooked.path, "--from", m.patchFrom.trim()];
		if (m.patchTo.trim()) args.push("--to", m.patchTo.trim());
		if (m.summary.trim()) args.push("--summary", m.summary.trim());
		if (m.changelog.trim()) args.push("--changelog", m.changelog.trim());
		if (m.tags.trim()) args.push("--tags", m.tags.trim());
		this.busy = "Publishing…";
		try {
			const r = await this.cli$(args);
			if (r.code !== 0) throw new Error("tempest mod publish failed (see the log)");
			toasts.push("success", `${m.name} ${m.version} uploaded`, "Published");
		} catch (error) {
			toasts.error(error, "Publish failed");
		} finally {
			this.busy = null;
		}
	}

	// ----- workspace files ------------------------------------------------------

	snapshot(): Project {
		return {
			meta: $state.snapshot(this.meta) as Meta,
			extras: $state.snapshot(this.extras) as Extra[],
			choices: $state.snapshot(this.choices) as Record<string, Choice>,
		};
	}

	restore(v: unknown): void {
		const s = v as Partial<Project> | null;
		if (!s) return;
		if (s.meta) this.meta = { ...emptyMeta(), ...s.meta };
		if (s.extras) this.extras = s.extras;
		// Workspace files of 0.0.15 kept a list of choices; they start over.
		if (s.choices && !Array.isArray(s.choices)) this.choices = s.choices;
	}
}

export const cooker = new Cooker();
