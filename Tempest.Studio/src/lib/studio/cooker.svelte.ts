// The mod cooker: turns the workspace's changes (unsaved edits and files saved
// since their base was recorded) plus extra files (Lua, DLLs, loose files)
// into a signed `.tempest`, then saves, installs or publishes it through
// tempest-cli, the same way the launcher's cooker does.
import { invoke, pickDirectory, pickFile, pickFiles, pickSave } from "$lib/platform/api";
import { basename, dirname } from "./format";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";

export const MOD_ID_PATTERN = /^[a-z0-9][a-z0-9._-]{1,63}$/;
const META_KEY = "studio.cookerMeta";

export type Change = {
	path: string;
	kind: "upk" | "mcts";
	doc?: string;
	state: "unsaved" | "saved";
	rel: string;
	base: boolean;
	script: boolean;
};
export type Item = Change & { on: boolean; mode: "delta" | "whole" };
export type Extra = { src: string; dest: string };
export type Author = { name: string; link: string };
export type LogLine = { kind: "info" | "ok" | "error"; text: string };

type Meta = {
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

function loadMeta(): Meta {
	const fallback: Meta = {
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
	};
	try {
		return {
			...fallback,
			...(JSON.parse(localStorage.getItem(META_KEY) ?? "{}") as Partial<Meta>),
		};
	} catch {
		return fallback;
	}
}

/** Where a loose file goes inside the mod by default. */
export function defaultDest(src: string): string {
	const name = basename(src);
	const lower = name.toLowerCase();
	if (lower.endsWith(".lua")) return `scripts/${name}`;
	if (lower.endsWith(".dll")) return `dlls/64/${name}`;
	if (lower.endsWith(".upk") || lower.endsWith(".pck") || lower.endsWith(".tfc")) {
		return `files/ChaosGame/CookedPCConsole/${name}`;
	}
	return `files/${name}`;
}

class Cooker {
	meta = $state<Meta>(loadMeta());
	items = $state<Item[]>([]);
	extras = $state<Extra[]>([]);
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

	idValid = $derived(MOD_ID_PATTERN.test(this.meta.id));
	selected = $derived(this.items.filter((i) => i.on));
	canCook = $derived(
		this.idValid &&
			this.meta.name.trim() !== "" &&
			(this.selected.length > 0 || this.extras.length > 0) &&
			!!this.cli?.available,
	);

	constructor() {
		$effect.root(() => {
			$effect(() => {
				localStorage.setItem(META_KEY, JSON.stringify(this.meta));
			});
		});
	}

	private say(kind: LogLine["kind"], text: string): void {
		this.log = [...this.log, { kind, text }].slice(-200);
	}

	async refresh(): Promise<void> {
		this.cli = await invoke("cli:status", {});
		if (!studio.workspace) return;
		try {
			const r = await rpc("mod.changes", {});
			const previous = new Map(this.items.map((i) => [i.path, i]));
			this.items = r.files.map((f) => {
				const p = previous.get(f.path);
				// Script packages are compared as whole files by default: a recompiled
				// package's delta is large and the launcher handles whole files fine.
				const mode = p?.mode ?? (f.script ? "whole" : "delta");
				return { ...f, on: p?.on ?? true, mode };
			});
		} catch (error) {
			toasts.error(error, "Could not list the changes");
		}
	}

	/**
	 * Open the cooker on the workspace's changes with one file chosen as a
	 * delta (the movie editor's "Make a mod"): the file must be listed, so the
	 * changes are read again first.
	 */
	async propose(path: string): Promise<void> {
		studio.mode = "mod";
		await this.refresh();
		const same = (p: string) =>
			p.replaceAll("\\", "/").toLowerCase() === path.replaceAll("\\", "/").toLowerCase();
		this.items = this.items.map((i) => (same(i.path) ? { ...i, on: true, mode: "delta" } : i));
		if (!this.items.some((i) => same(i.path))) {
			toasts.push(
				"info",
				"Apply the movie edits first: the package has no changes yet",
				"Make a mod",
			);
		}
	}

	async addFiles(): Promise<void> {
		const files = await pickFiles("Files to ship in the mod", [], "cooker");
		for (const f of files) this.extras = [...this.extras, { src: f, dest: defaultDest(f) }];
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
			this.say("info", `Mod folder: ${staging}`);
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
					items: this.selected.map((i) => ({
						path: i.path,
						kind: i.kind,
						doc: i.doc,
						mode: i.mode,
					})),
					extras: $state.snapshot(this.extras) as Extra[],
				}),
			);
			for (const e of r.entries) {
				this.say(
					e.mode === "unchanged" ? "info" : "ok",
					`${e.file}: ${e.mode === "delta" ? `delta, ${e.ops} change(s)` : e.mode}${e.warnings?.length ? ` (${e.warnings.join("; ")})` : ""}`,
				);
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

	/** State carried by workspace files. */
	snapshot(): unknown {
		return {
			meta: $state.snapshot(this.meta),
			extras: $state.snapshot(this.extras),
			choices: this.items.map((i) => ({ path: i.path, on: i.on, mode: i.mode })),
		};
	}

	restore(v: unknown): void {
		const s = v as {
			meta?: Meta;
			extras?: Extra[];
			choices?: { path: string; on: boolean; mode: "delta" | "whole" }[];
		} | null;
		if (!s) return;
		if (s.meta) this.meta = { ...this.meta, ...s.meta };
		if (s.extras) this.extras = s.extras;
		if (s.choices) {
			const byPath = new Map(s.choices.map((c) => [c.path, c]));
			this.items = this.items.map((i) => ({ ...i, ...(byPath.get(i.path) ?? {}) }));
		}
	}
}

export const cooker = new Cooker();
