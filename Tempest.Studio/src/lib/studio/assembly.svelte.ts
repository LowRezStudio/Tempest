// Assembly editor state: the game's `assembly.dat` (content database) and
// `Lang_*.dat` (messages) marshals opened by the backend (`mcts.*`), the
// table and page on screen, the selected row, and the reference rules that
// turn `*_ID` columns into links.
import { invoke } from "$lib/platform/api";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type {
	McsCell,
	McsColumn,
	McsDoc,
	McsField,
	McsMessage,
	McsPath,
	McsSummary,
	McsValidValue,
} from "../../../electron/src/ipc/contract";

/** Where a `*_ID` column points: the table and its key column. */
export type Ref = { table: string; key: string };

/** Key columns of the top-level tables, most specific name first. */
const REFS: [string, Ref][] = [
	["DEVICE_PROJECTILE_ID", { table: "DATA_SET_PROJECTILES", key: "DEVICE_PROJECTILE_ID" }],
	["BOT_SPAWN_TABLE_ID", { table: "DATA_SET_BOT_SPAWN_TABLES", key: "BOT_SPAWN_TABLE_ID" }],
	["MATERIAL_RES_GROUP_ID", { table: "DATA_SET_MAT_RES_GROUPS", key: "MATERIAL_RES_GROUP_ID" }],
	["LOOT_TABLE_GROUP_ID", { table: "DATA_SET_LOOT_TABLE_GROUPS", key: "LOOT_TABLE_GROUP_ID" }],
	["LOOT_TABLE_ID", { table: "DATA_SET_LOOT_TABLES", key: "LOOT_TABLE_ID" }],
	["EFFECT_GROUP_ID", { table: "DATA_SET_EFFECT_GROUPS", key: "EFFECT_GROUP_ID" }],
	["GAME_TIP_ID", { table: "DATA_SET_TIPS", key: "GAME_TIP_ID" }],
	["DEPLOYABLE_ID", { table: "DATA_SET_DEPLOYABLES", key: "DEPLOYABLE_ID" }],
	["ITEM_ID", { table: "DATA_SET_ITEMS", key: "ITEM_ID" }],
	["DEVICE_ID", { table: "DATA_SET_DEVICES", key: "DEVICE_ID" }],
	["SKIN_ID", { table: "DATA_SET_SKINS", key: "SKIN_ID" }],
	["BOT_ID", { table: "DATA_SET_BOTS", key: "BOT_ID" }],
	["FX_ID", { table: "DATA_SET_SPECIAL_FX", key: "FX_ID" }],
	["RES_ID", { table: "DATA_SET_RESOURCES", key: "RES_ID" }],
	["ICON_ID", { table: "DATA_SET_ICONS", key: "ICON_ID" }],
	["ASM_ID", { table: "DATA_SET_ASSEMBLY_MESHES", key: "ASM_ID" }],
	["VALUE_ID", { table: "DATA_SET_VALID_VALUES", key: "VALUE_ID" }],
	["PROP_ID", { table: "DATA_SET_PROPERTIES", key: "PROP_ID" }],
	["XP_ID", { table: "DATA_SET_XP_TABLES", key: "XP_ID" }],
	["VGS_ID", { table: "DATA_SET_VGS", key: "VGS_ID" }],
	["LANG_ID", { table: "DATA_SET_LANGUAGES", key: "LANG_ID" }],
	["BEHAVIOR_ID", { table: "DATA_SET_BOT_BEHAVIORS", key: "BEHAVIOR_ID" }],
];

/** The table a column refers to, by naming convention (`CARD_RES_ID` → resources). */
export function refOf(column: string): Ref | null {
	if (column.endsWith("_MSG_ID")) return null;
	for (const [suffix, ref] of REFS) {
		if (column === suffix || column.endsWith(`_${suffix}`)) return ref;
	}
	return null;
}

export function isMessageColumn(column: string): boolean {
	return column.endsWith("_MSG_ID");
}

/** `GAME_VERSION_RAW` packs four 16-bit numbers: 8.1.5839.0. */
export function versionText(raw: string | number): string {
	const v = BigInt(raw);
	return [48n, 32n, 16n, 0n].map((s) => String((v >> s) & 0xffffn)).join(".");
}

export function versionRaw(text: string): string | null {
	const parts = text.trim().split(".");
	if (parts.length !== 4 || parts.some((p) => !/^\d+$/.test(p) || Number(p) > 0xffff)) {
		return null;
	}
	let v = 0n;
	for (const p of parts) v = (v << 16n) | BigInt(p);
	return v.toString();
}

export type Page = {
	doc: string;
	path: McsPath;
	table: string;
	columns: McsColumn[];
	total: number;
	offset: number;
	rows: [number, Record<string, McsCell>][];
};

type Place = { doc: string; path: McsPath; table: string; index: number | null };

const LIMIT = 200;

class Assembly {
	summary = $state<McsSummary | null>(null);
	loading = $state(false);
	/** The row set on screen. */
	page = $state<Page | null>(null);
	filter = $state("");
	sort = $state<string>("");
	desc = $state(false);
	/** Index (within the row set) of the selected row, and its fields. */
	selected = $state<number | null>(null);
	fields = $state<McsField[] | null>(null);
	/** Message texts by id and language, filled on demand. */
	messages = $state<Record<string, Record<string, McsMessage>>>({});
	validValues = $state<Record<string, McsValidValue> | null>(null);
	/** Back/forward over the places visited (table + row). */
	history = $state<Place[]>([]);
	historyPos = $state(-1);
	private navigating = false;
	private requestId = 0;

	docs = $derived<McsDoc[]>(this.summary?.docs ?? []);
	doc = $derived<McsDoc | null>(
		this.page
			? (this.docs.find((d) => d.id === this.page?.doc) ?? null)
			: (this.docs[0] ?? null),
	);
	dirtyDocs = $derived(this.docs.filter((d) => d.dirty));
	isDirty = $derived(this.doc?.dirty ?? false);
	undoLabels = $derived(this.doc?.undo ?? []);
	languages = $derived(this.docs.filter((d) => d.id !== "assembly").map((d) => d.id));
	canBack = $derived(this.historyPos > 0);
	canForward = $derived(this.historyPos < this.history.length - 1);

	/** Tell the backend which id range new content takes its ids from (the user's setting). */
	async applyIdRange(): Promise<void> {
		const r = studio.idRange;
		try {
			await rpc("mcts.idRange", r ? { start: r.start, end: r.end } : {});
		} catch (error) {
			toasts.error(error, "Could not apply the id range");
		}
	}

	/** Open (or reload) the assembly and languages of a cooked folder. */
	async open(reload = false): Promise<boolean> {
		this.loading = true;
		try {
			this.summary = await rpc("mcts.open", { reload });
			await this.applyIdRange();
			this.page = null;
			this.selected = null;
			this.fields = null;
			this.messages = {};
			this.validValues = null;
			this.history = [];
			this.historyPos = -1;
			const first = this.summary.docs[0];
			if (first?.tables[0]) await this.openTable(first.id, first.tables[0].name, []);
			return true;
		} catch (error) {
			toasts.error(error, "Could not open the assembly");
			return false;
		} finally {
			this.loading = false;
		}
	}

	close(): void {
		this.summary = null;
		this.page = null;
		this.selected = null;
		this.fields = null;
	}

	/** Refresh one document's summary (dirty flag, undo labels, row counts). */
	private async refreshDoc(id: string): Promise<void> {
		const doc = await rpc("mcts.doc", { doc: id });
		if (this.summary) {
			this.summary.docs = this.summary.docs.map((d) => (d.id === id ? doc : d));
		}
	}

	/** Show a row set: a top-level table, or a nested one under `path`. */
	async openTable(
		doc: string,
		table: string,
		statePath: McsPath,
		select: number | null = null,
		keepFilter = false,
	): Promise<void> {
		// Proxied state does not survive the IPC's structured clone.
		const path = $state.snapshot(statePath) as McsPath;
		if (!keepFilter) {
			this.filter = "";
			this.sort = "";
			this.desc = false;
		}
		const id = ++this.requestId;
		try {
			const info = await rpc("mcts.table", { doc, path, table });
			let offset = 0;
			if (select !== null && !this.filter && !this.sort) {
				offset = Math.floor(select / LIMIT) * LIMIT;
			}
			const rows = await rpc("mcts.rows", {
				doc,
				path,
				table,
				offset,
				limit: LIMIT,
				filter: this.filter,
				sort: this.sort,
				desc: this.desc,
			});
			if (id !== this.requestId) return;
			this.page = {
				doc,
				path,
				table,
				columns: info.columns,
				total: rows.total,
				offset: rows.offset,
				rows: rows.rows,
			};
			this.selected = null;
			this.fields = null;
			this.record({ doc, path, table, index: select });
			if (select !== null) await this.select(select);
		} catch (error) {
			toasts.error(error, "Could not load the table");
		}
	}

	/** Reload the current page (after a filter, sort, or edit). */
	async reload(offset = this.page?.offset ?? 0): Promise<void> {
		const p = this.page;
		if (!p) return;
		const id = ++this.requestId;
		try {
			const rows = await rpc("mcts.rows", {
				doc: p.doc,
				path: $state.snapshot(p.path) as McsPath,
				table: p.table,
				offset,
				limit: LIMIT,
				filter: this.filter,
				sort: this.sort,
				desc: this.desc,
			});
			if (id !== this.requestId) return;
			this.page = { ...p, total: rows.total, offset: rows.offset, rows: rows.rows };
		} catch (error) {
			toasts.error(error, "Could not load the rows");
		}
	}

	async setFilter(text: string): Promise<void> {
		this.filter = text;
		await this.reload(0);
	}

	async setSort(column: string): Promise<void> {
		if (this.sort === column) {
			if (this.desc) {
				this.sort = "";
				this.desc = false;
			} else {
				this.desc = true;
			}
		} else {
			this.sort = column;
			this.desc = false;
		}
		await this.reload(0);
	}

	get limit(): number {
		return LIMIT;
	}

	async nextPage(): Promise<void> {
		if (this.page && this.page.offset + LIMIT < this.page.total) {
			await this.reload(this.page.offset + LIMIT);
		}
	}

	async prevPage(): Promise<void> {
		if (this.page && this.page.offset > 0) {
			await this.reload(Math.max(0, this.page.offset - LIMIT));
		}
	}

	/** Path of a row of the current table. */
	rowPath(index: number): McsPath {
		const p = this.page;
		if (!p) return [];
		return [...($state.snapshot(p.path) as McsPath), [p.table, index]];
	}

	async select(index: number | null): Promise<void> {
		this.selected = index;
		this.fields = null;
		if (index === null || !this.page) return;
		this.record({ doc: this.page.doc, path: this.page.path, table: this.page.table, index });
		try {
			const r = await rpc("mcts.row", { doc: this.page.doc, path: this.rowPath(index) });
			if (this.selected !== index) return;
			this.fields = r.fields;
			const ids = r.fields
				.filter((f) => isMessageColumn(f.name) && typeof f.value === "number")
				.map((f) => f.value as number);
			await this.loadMessages(ids);
		} catch (error) {
			toasts.error(error, "Could not load the row");
		}
	}

	/** Fetch the texts of message ids not seen yet. */
	async loadMessages(ids: number[]): Promise<void> {
		const missing = [...new Set(ids)].filter(
			(id) => id !== 0 && !(String(id) in this.messages),
		);
		if (missing.length === 0) return;
		const got = await rpc("mcts.messages", { ids: missing });
		const next = { ...this.messages };
		for (const id of missing) next[String(id)] = got[String(id)] ?? {};
		this.messages = next;
	}

	async loadValidValues(): Promise<void> {
		if (this.validValues) return;
		try {
			this.validValues = await rpc("mcts.validValues", {});
		} catch {
			this.validValues = {};
		}
	}

	/** Text of a message in a language, when loaded. */
	message(id: McsCell, lang = "INT"): string | null {
		if (typeof id !== "number") return null;
		return this.messages[String(id)]?.[lang]?.text ?? null;
	}

	// ----- edits -------------------------------------------------------------

	async setCell(index: number, field: string, text: string | null): Promise<void> {
		const p = this.page;
		if (!p) return;
		try {
			const r =
				text === null
					? await rpc("mcts.set", {
							doc: p.doc,
							path: this.rowPath(index),
							field,
							value: null,
						})
					: await rpc("mcts.set", { doc: p.doc, path: this.rowPath(index), field, text });
			this.page = {
				...p,
				rows: p.rows.map(([i, cells]) => (i === index ? [i, r.row] : [i, cells])),
			};
			await this.refreshDoc(p.doc);
			if (this.selected === index) {
				const row = await rpc("mcts.row", { doc: p.doc, path: this.rowPath(index) });
				this.fields = row.fields;
			}
		} catch (error) {
			toasts.error(error, `Could not set ${field}`);
		}
	}

	async setMessage(id: number, lang: string, text: string): Promise<void> {
		try {
			await rpc("mcts.setMessage", { lang, id, text });
			const next = { ...this.messages };
			const entry = { ...(next[String(id)] ?? {}) };
			entry[lang] = { ...(entry[lang] ?? { name: "", path: [] }), text };
			next[String(id)] = entry;
			this.messages = next;
			await this.refreshDoc(lang);
		} catch (error) {
			toasts.error(error, "Could not set the message");
		}
	}

	async setHeader(doc: string, field: string, text: string): Promise<void> {
		try {
			await rpc("mcts.setHeader", { doc, field, text });
			await this.refreshDoc(doc);
		} catch (error) {
			toasts.error(error, `Could not set ${field}`);
		}
	}

	/** Add a row after the selected one (a copy of it), or at the end. */
	async insertRow(clone: number | null): Promise<void> {
		const p = this.page;
		if (!p) return;
		try {
			const r = await rpc("mcts.insert", {
				doc: p.doc,
				path: $state.snapshot(p.path) as McsPath,
				table: p.table,
				index: clone === null ? undefined : clone + 1,
				clone: clone ?? undefined,
			});
			await this.refreshDoc(p.doc);
			await this.reload();
			await this.select(r.index);
		} catch (error) {
			toasts.error(error, "Could not add the row");
		}
	}

	async deleteRow(index: number): Promise<void> {
		const p = this.page;
		if (!p) return;
		try {
			await rpc("mcts.delete", { doc: p.doc, path: this.rowPath(index) });
			await this.refreshDoc(p.doc);
			this.selected = null;
			this.fields = null;
			await this.reload();
		} catch (error) {
			toasts.error(error, "Could not delete the row");
		}
	}

	async undo(): Promise<void> {
		const id = this.doc?.id;
		if (!id) return;
		try {
			const r = await rpc("mcts.undo", { doc: id });
			if (r.undone) toasts.push("info", r.undone, "Undone");
			await this.refreshDoc(id);
			await this.reload();
			if (this.selected !== null) await this.select(this.selected);
		} catch (error) {
			toasts.error(error, "Undo failed");
		}
	}

	/** Save one document (in place, or into a folder keeping its name). */
	async save(id: string, outDir?: string): Promise<boolean> {
		const doc = this.docs.find((d) => d.id === id);
		if (!doc) return false;
		try {
			const out = outDir ? `${outDir}/${doc.path.split(/[/\\]/).at(-1)}` : undefined;
			const r = await rpc("mcts.save", { doc: id, out });
			await this.refreshDoc(id);
			toasts.push("success", r.out, "Saved");
			return true;
		} catch (error) {
			toasts.error(error, "Save failed");
			return false;
		}
	}

	async saveAll(outDir?: string): Promise<void> {
		for (const d of this.dirtyDocs) await this.save(d.id, outDir);
	}

	async discard(id: string): Promise<void> {
		try {
			await rpc("mcts.discard", { doc: id });
			await this.refreshDoc(id);
			this.messages = {};
			await this.reload();
			if (this.selected !== null) await this.select(this.selected);
		} catch (error) {
			toasts.error(error, "Could not discard");
		}
	}

	// ----- references ----------------------------------------------------------

	/** Jump to the row a `*_ID` value refers to. */
	async follow(column: string, value: McsCell): Promise<void> {
		const ref = refOf(column);
		if (!ref || typeof value !== "number") return;
		try {
			const found = await rpc("mcts.find", {
				doc: "assembly",
				table: ref.table,
				key: ref.key,
				value,
			});
			if (found.length === 0) {
				toasts.push(
					"warning",
					`${ref.table} has no row with ${ref.key} = ${value}`,
					"Not found",
				);
				return;
			}
			await this.openTable("assembly", ref.table, [], found[0]);
		} catch (error) {
			toasts.error(error, "Could not follow the reference");
		}
	}

	/** Jump to a message row in a language file. */
	async openMessage(id: number, lang: string): Promise<void> {
		const m = this.messages[String(id)]?.[lang];
		if (!m) return;
		const [[table, index]] = m.path;
		await this.openTable(lang, table, [], index);
	}

	// ----- history ------------------------------------------------------------

	private record(place: Place): void {
		if (this.navigating) return;
		const last = this.history[this.historyPos];
		if (
			last &&
			last.doc === place.doc &&
			last.table === place.table &&
			last.index === place.index &&
			JSON.stringify(last.path) === JSON.stringify(place.path)
		) {
			return;
		}
		this.history = [...this.history.slice(0, this.historyPos + 1), place].slice(-100);
		this.historyPos = this.history.length - 1;
	}

	private async goTo(pos: number): Promise<void> {
		const place = this.history[pos];
		if (!place) return;
		this.historyPos = pos;
		this.navigating = true;
		try {
			await this.openTable(place.doc, place.table, place.path, place.index);
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

	/** Reveal a saved file in the file manager. */
	async reveal(path: string): Promise<void> {
		await invoke("shell:show-item", { path });
	}
}

export const assembly = new Assembly();
