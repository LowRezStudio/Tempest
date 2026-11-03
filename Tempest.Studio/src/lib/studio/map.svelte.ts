// The map view: a persistent map and the sublevels it streams, drawn in
// three.js (`MapViewport`), with an outliner, an inspector and a Kismet
// graph per level. Moving an actor writes its Location/Rotation/DrawScale3D
// (or a collection mesh's component transform) through `map.setTransform`,
// an unsaved edit of that level's package like any other.
import { untrack } from "svelte";
import { movePose, sample } from "./matinee-math";
import { nav, type MapPlace } from "./nav.svelte";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type {
	KismetGraph,
	MatineeTimeline,
	MapActor,
	MapLevel,
	ParsedObject,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

export type MapEntry = UpkMethods["map.list"]["result"]["maps"][number];
export type Shading = "lit" | "unlit" | "untextured" | "wireframe";

/** Categories of actors the view can hide, with their labels and colours. */
export const CATEGORIES: { id: string; label: string; color: string }[] = [
	{ id: "mesh", label: "Meshes", color: "#a6adc8" },
	{ id: "playerstart", label: "Player starts", color: "#89b4fa" },
	{ id: "spawn", label: "Spawn points", color: "#94e2d5" },
	{ id: "botfactory", label: "Factories", color: "#fab387" },
	{ id: "gameplay", label: "Gameplay", color: "#f9e2af" },
	{ id: "trigger", label: "Triggers", color: "#f5c2e7" },
	{ id: "blocking", label: "Blocking volumes", color: "#f38ba8" },
	{ id: "volume", label: "Other volumes", color: "#cba6f7" },
	{ id: "light", label: "Lights", color: "#f9e2af" },
	{ id: "sound", label: "Sounds", color: "#a6e3a1" },
	{ id: "emitter", label: "Emitters", color: "#eba0ac" },
	{ id: "navigation", label: "Navigation", color: "#74c7ec" },
	{ id: "info", label: "Level info", color: "#7f849c" },
	{ id: "other", label: "Other", color: "#bac2de" },
];

export const categoryColor = (id: string) =>
	CATEGORIES.find((c) => c.id === id)?.color ?? "#bac2de";

/** Team colours of player starts (TeamIndex). */
export const TEAM_COLORS = ["#89b4fa", "#f38ba8", "#a6e3a1", "#f9e2af"];

export type Selection = { level: string; export: number };

const HIDDEN_KEY = "studio.map.hidden";

function loadHidden(): Record<string, boolean> {
	try {
		return JSON.parse(localStorage.getItem(HIDDEN_KEY) ?? "") as Record<string, boolean>;
	} catch {
		// Collision and navigation clutter the view: off until asked for.
		return { blocking: true, navigation: true, info: true };
	}
}

class MapStore {
	list = $state.raw<MapEntry[] | null>(null);
	listLoading = $state(false);
	/** Show only persistent maps (those that stream sublevels) in the picker. */
	persistentOnly = $state(true);
	path = $state<string | null>(null);
	levels = $state.raw<MapLevel[]>([]);
	missing = $state<string[]>([]);
	loading = $state(false);
	error = $state<string | null>(null);
	selected = $state<Selection | null>(null);
	/** The selected actor's object (inspector). */
	object = $state.raw<ParsedObject | null>(null);
	view = $state<"scene" | "kismet" | "matinee">("scene");
	shading = $state<Shading>("lit");
	hiddenCategories = $state<Record<string, boolean>>(loadHidden());
	hiddenLevels = $state<Record<string, boolean>>({});
	/** The outliner lists only actors flagged `m_bCanBaseOn` False (any category). */
	rooftopsOnly = $state(false);
	/** Outliner filter text. */
	filter = $state("");
	/** Colour the surfaces of actors flagged `m_bCanBaseOn` False (rooftops, heavy walls). */
	heavyWalls = $state(false);
	/** How flagged actors are drawn: faces by slope, or each object one flat colour seen through walls ("X-ray"). */
	heavyStyle = $state<"slope" | "xray">("slope");
	rooftopCount = $derived(
		this.levels.reduce((n, l) => n + l.actors.filter((a) => a.canBaseOn === false).length, 0),
	);
	/** Gizmo mode. */
	tool = $state<"select" | "translate" | "rotate" | "scale">("translate");
	/** Bumped when actors moved (the viewport re-reads their matrices). */
	version = $state(0);
	/** Asks the viewport to frame the selection. */
	focusRequest = $state(0);
	/** Edited levels in order, for undo. */
	private edits: string[] = [];

	// Kismet
	kismetLevel = $state<string | null>(null);
	sequences = $state.raw<UpkMethods["kismet.sequences"]["result"]["sequences"]>([]);
	sequence = $state<number | null>(null);
	graph = $state.raw<KismetGraph | null>(null);
	/** Node positions the user moved, by `path#sequence` then export (kept in the Studio, not the package). */
	layout = $state<Record<string, Record<number, { x: number; y: number }>>>({});
	kismetSelected = $state<number | null>(null);

	constructor() {
		$effect.root(() => {
			$effect(() => {
				localStorage.setItem(HIDDEN_KEY, JSON.stringify(this.hiddenCategories));
			});
		});
	}

	actor(sel: Selection | null = this.selected): MapActor | null {
		if (!sel) return null;
		return (
			this.levels
				.find((l) => l.path === sel.level)
				?.actors.find((a) => a.export === sel.export) ?? null
		);
	}

	level(path: string | null = this.selected?.level ?? null): MapLevel | null {
		return this.levels.find((l) => l.path === path) ?? null;
	}

	async loadList(): Promise<void> {
		if (this.listLoading) return;
		this.listLoading = true;
		try {
			const r = await rpc("map.list", {});
			this.list = r.maps.sort((a, b) => a.name.localeCompare(b.name));
		} catch (error) {
			toasts.error(error, "Could not list the maps");
		} finally {
			this.listLoading = false;
		}
	}

	place(): MapPlace | null {
		if (!this.path) return null;
		return {
			kind: "map",
			path: this.path,
			level: this.selected?.level,
			index: this.selected?.export,
			view: this.view,
			sequence: this.view === "kismet" ? (this.sequence ?? undefined) : undefined,
			matinee: this.view === "matinee" ? (this.matineeAt ?? undefined) : undefined,
		};
	}

	record(): void {
		const p = this.place();
		if (p) nav.record(p);
	}

	/** Open a map (its sublevels too). */
	async open(path: string, select?: Selection, record = true): Promise<void> {
		studio.mode = "map";
		if (this.path === path && this.levels.length > 0) {
			if (select) await this.select(select, record);
			else if (record) this.record();
			return;
		}
		this.loading = true;
		this.error = null;
		this.path = path;
		this.levels = [];
		this.selected = null;
		this.object = null;
		this.graph = null;
		this.sequence = null;
		this.kismetLevel = null;
		try {
			const r = await rpc("map.scene", { path });
			if (this.path !== path) return;
			this.levels = r.levels;
			this.missing = r.missing;
			this.hiddenLevels = {};
			this.version++;
			if (select) await this.select(select, false);
			if (record) this.record();
		} catch (error) {
			this.error = error instanceof Error ? error.message : String(error);
		} finally {
			this.loading = false;
		}
	}

	/** Re-read one level after an edit or undo (the others stay as drawn). */
	async reloadLevel(level: string): Promise<void> {
		try {
			const r = await rpc("map.scene", { path: level, streamed: false });
			const fresh = r.levels[0];
			if (!fresh) return;
			this.levels = this.levels.map((l) =>
				l.path === level ? { ...fresh, name: l.name } : l,
			);
			this.version++;
			if (this.selected?.level === level) await this.loadObject();
		} catch (error) {
			toasts.error(error, "Could not reload the level");
		}
	}

	async select(sel: Selection | null, record = true): Promise<void> {
		this.selected = sel;
		this.object = null;
		if (record) this.record();
		await this.loadObject();
	}

	async loadObject(): Promise<void> {
		const sel = this.selected;
		if (!sel) return;
		try {
			const o = await rpc("object.get", { path: sel.level, index: sel.export });
			if (this.selected === sel) this.object = o;
		} catch {
			this.object = null;
		}
	}

	/** An actor moved in the viewport: write it. */
	async move(level: string, actor: MapActor, matrix: number[]): Promise<void> {
		try {
			const r = await rpc("map.setTransform", {
				path: level,
				index: actor.export,
				matrix,
				parent: actor.edit === "component" ? actor.parent : undefined,
			});
			// Keep the drawn matrix; record the numbers the actor now stores.
			this.levels = this.levels.map((l) =>
				l.path !== level
					? l
					: {
							...l,
							dirty: true,
							actors: l.actors.map((a) =>
								a.export !== actor.export
									? a
									: {
											...a,
											matrix,
											location:
												actor.edit === "actor" ? r.translation : a.location,
											rotation: r.rotation,
										},
							),
						},
			);
			this.edits.push(level);
			studio.markDirty(level);
			if (this.selected?.level === level && this.selected.export === actor.export) {
				await this.loadObject();
			}
		} catch (error) {
			toasts.error(error, `Could not move ${actor.name}`);
			this.version++;
		}
	}

	/** A property changed in the inspector: the actor may have moved. */
	async changed(updated: ParsedObject): Promise<void> {
		this.object = updated;
		const level = this.selected?.level;
		if (!level) return;
		this.edits.push(level);
		await this.reloadLevel(level);
	}

	/** Set `m_bCanBaseOn` of the selected actor (or of the collection holding the selected mesh). */
	async setCanBaseOn(value: boolean): Promise<void> {
		const sel = this.selected;
		const actor = this.actor();
		if (!sel || !actor) return;
		const index =
			actor.edit === "component" && actor.parent !== undefined ? actor.parent : actor.export;
		try {
			await rpc("object.set", {
				path: sel.level,
				index,
				prop: "m_bCanBaseOn",
				value: value ? "True" : "False",
			});
			this.noteEdit(sel.level);
			await this.reloadLevel(sel.level);
		} catch (error) {
			toasts.error(error, "Could not change m_bCanBaseOn");
		}
	}

	/** An edit of a level made elsewhere (the MCP's property writes): undo takes it back. */
	noteEdit(level: string): void {
		this.edits.push(level);
		studio.markDirty(level);
	}

	/** The map's level packages with unsaved edits. */
	get dirtyLevels(): string[] {
		return this.levels.map((l) => l.path).filter((p) => studio.dirty.includes(p));
	}

	get canUndo(): boolean {
		return this.edits.length > 0;
	}

	async undo(): Promise<void> {
		const level = this.edits.pop();
		if (!level) {
			toasts.push("info", "Nothing to undo in this map", "Undo");
			return;
		}
		try {
			const r = await rpc("package.undo", { path: level });
			if (r.label === null) return;
			toasts.push("success", r.label, "Undone");
			if (!r.dirty) studio.dirty = studio.dirty.filter((d) => d !== level);
			await this.reloadLevel(level);
			if (this.view === "kismet" && this.kismetLevel === level) await this.loadGraph();
		} catch (error) {
			toasts.error(error, "Undo failed");
		}
	}

	// ----- Kismet -------------------------------------------------------------

	async openKismet(level: string, sequence?: number): Promise<void> {
		this.view = "kismet";
		try {
			if (this.kismetLevel !== level || this.sequences.length === 0) {
				const r = await rpc("kismet.sequences", { path: level });
				this.sequences = r.sequences;
				this.kismetLevel = level;
			}
			const seq = sequence ?? this.sequences[0]?.export ?? null;
			this.sequence = seq;
			this.kismetSelected = null;
			await this.loadGraph();
		} catch (error) {
			toasts.error(error, "Could not read the Kismet sequences");
		}
		this.record();
	}

	async loadGraph(): Promise<void> {
		if (!this.kismetLevel || this.sequence === null) {
			this.graph = null;
			return;
		}
		this.graph = await rpc("kismet.graph", { path: this.kismetLevel, index: this.sequence });
	}

	layoutKey(): string {
		return `${this.kismetLevel}#${this.sequence}`;
	}

	moveNode(node: number, x: number, y: number): void {
		const k = this.layoutKey();
		this.layout = { ...this.layout, [k]: { ...this.layout[k], [node]: { x, y } } };
	}

	async link(p: {
		from: number;
		to: number;
		output?: number;
		input?: number;
		variable?: number;
		remove?: boolean;
	}): Promise<void> {
		const level = this.kismetLevel;
		if (!level || this.sequence === null) return;
		try {
			const g = await rpc("kismet.link", { path: level, sequence: this.sequence, ...p });
			this.graph = g;
			if (g.changed) {
				this.edits.push(level);
				studio.markDirty(level);
			}
		} catch (error) {
			toasts.error(error, p.remove ? "Could not unlink" : "Could not link");
		}
	}

	// ----- Matinee -------------------------------------------------------------

	/** The open timeline: its level package and SeqAct_Interp (or InterpData) export. */
	matineeAt = $state<{ level: string; index: number } | null>(null);
	matinee = $state.raw<MatineeTimeline | null>(null);
	matineeTime = $state(0);
	matineePlaying = $state(false);
	matineeKey = $state<{ track: number; curve: string; key: number } | null>(null);

	async openMatinee(level: string, index: number): Promise<void> {
		try {
			this.matinee = await rpc("matinee.get", { path: level, index });
			this.matineeAt = { level, index };
			this.matineeTime = 0;
			this.matineeKey = null;
			this.view = "matinee";
			this.record();
		} catch (error) {
			toasts.error(error, "Could not read the Matinee");
		}
	}

	private async matineeEdit(
		method: "matinee.setKey" | "matinee.addKey" | "matinee.removeKey",
		params: Record<string, unknown>,
	): Promise<MatineeTimeline | null> {
		const at = this.matineeAt;
		if (!at) return null;
		try {
			const r = (await rpc(method, {
				path: at.level,
				index: at.index,
				...params,
			} as never)) as MatineeTimeline;
			this.matinee = r;
			this.noteEdit(at.level);
			return r;
		} catch (error) {
			toasts.error(error, "Could not change the key");
			return null;
		}
	}

	setKey(
		track: number,
		curve: string,
		key: number,
		change: { time?: number; value?: number | number[] },
	) {
		return this.matineeEdit("matinee.setKey", { track, curve, key, ...change });
	}

	async addKey(track: number, curve: string, time: number): Promise<void> {
		const r = await this.matineeEdit("matinee.addKey", { track, curve, time });
		if (r?.key !== undefined) this.matineeKey = { track, curve, key: r.key };
	}

	async removeKey(track: number, curve: string, key: number): Promise<void> {
		await this.matineeEdit("matinee.removeKey", { track, curve, key });
		this.matineeKey = null;
	}

	/**
	 * Where the timeline puts things at the current time: the view matrix of
	 * each driven actor (`level#export`), and the path of each movement.
	 */
	matineePreview = $derived.by(() => {
		const tl = this.matinee;
		const at = this.matineeAt;
		const poses = new Map<string, number[]>();
		const paths: { color: string; points: number[][] }[] = [];
		if (!tl || !at || this.view !== "matinee") return { poses, paths };
		const t = this.matineeTime;
		for (const g of tl.groups) {
			const move = g.tracks.find((x) => x.class === "interptrackmove" && !x.disabled);
			if (!move) continue;
			const actor = g.actors.find((a) => a.export !== null);
			const placed =
				actor && actor.export !== null
					? this.actor({ level: at.level, export: actor.export })
					: null;
			const relative = /relative/i.test(move.moveFrame ?? "");
			const initial = relative && placed ? placed.matrix : null;
			const curve = (name: string) =>
				move.curves.find((c) => c.name.toLowerCase() === name)?.keys ?? [];
			// Split axes: one float curve per axis.
			const posKeys = curve("postrack");
			const eulerKeys = curve("eulertrack");
			const axes = g.tracks.filter((x) => x.parent === move.export);
			const axisValue = (name: string, time: number) => {
				const tr = axes.find((x) => (x.moveAxis ?? "").toLowerCase() === name);
				const keys = tr?.curves.find((c) => c.kind === "curve")?.keys ?? [];
				return keys.length ? (sample(keys, time)?.[0] ?? 0) : 0;
			};
			const poseAt = (time: number) => {
				let pos: number[];
				let euler: number[] | null;
				if (axes.length > 0) {
					pos = ["x", "y", "z"].map((a) => axisValue(`axis_translation${a}`, time));
					euler = ["x", "y", "z"].map((a) => axisValue(`axis_rotation${a}`, time));
				} else {
					pos = sample(posKeys, time) ?? [0, 0, 0];
					euler = sample(eulerKeys, time);
				}
				return movePose(pos, euler, initial);
			};
			if (axes.length === 0 && posKeys.length === 0) continue;
			if (placed && actor?.export != null) {
				poses.set(`${at.level}#${actor.export}`, poseAt(t));
			}
			const color = g.color ? `rgb(${g.color[0]},${g.color[1]},${g.color[2]})` : "#f9e2af";
			const steps = 48;
			const points = Array.from({ length: steps + 1 }, (_, i) => {
				const m = poseAt((tl.length * i) / steps);
				return [m[12], m[13], m[14]];
			});
			paths.push({ color, points });
		}
		return { poses, paths };
	});

	palette = $state.raw<UpkMethods["kismet.palette"]["result"]["classes"] | null>(null);

	async loadPalette(): Promise<void> {
		if (this.palette) return;
		try {
			this.palette = (
				await rpc("kismet.palette", { path: this.kismetLevel ?? undefined })
			).classes;
		} catch (error) {
			toasts.error(error, "Could not list the Kismet nodes");
		}
	}

	/** Add a node (a copy of `example`) at canvas point (x, y). Events fire from, and object variables hold, the actor selected in the 3D view when it is in this level. */
	async addNode(
		example: { path: string; index: number },
		kind: string,
		x: number,
		y: number,
	): Promise<void> {
		const level = this.kismetLevel;
		if (!level || this.sequence === null) return;
		const actor = this.selected?.level === level ? this.actor() : null;
		const target = actor
			? actor.edit === "component"
				? actor.parent
				: actor.export
			: undefined;
		try {
			const g = await rpc("kismet.add", {
				path: level,
				sequence: this.sequence,
				// A plain object: IPC cannot clone the palette's state proxies.
				example: { path: example.path, index: example.index },
				x: Math.round(x),
				y: Math.round(y),
				originator: kind === "event" ? target : undefined,
				object: kind === "variable" ? target : undefined,
			});
			this.graph = g;
			this.moveNode(g.added, Math.round(x), Math.round(y));
			this.kismetSelected = g.added;
			this.noteEdit(level);
		} catch (error) {
			toasts.error(error, "Could not add the node");
		}
	}

	async removeNode(node: number): Promise<void> {
		const level = this.kismetLevel;
		if (!level || this.sequence === null) return;
		try {
			this.graph = await rpc("kismet.remove", { path: level, sequence: this.sequence, node });
			if (this.kismetSelected === node) this.kismetSelected = null;
			this.noteEdit(level);
		} catch (error) {
			toasts.error(error, "Could not delete the node");
		}
	}

	/** Go to a recorded place (Back/Forward, tabs). */
	async visit(p: MapPlace): Promise<void> {
		studio.mode = "map";
		const sel =
			p.level !== undefined && p.index !== undefined
				? { level: p.level, export: p.index }
				: undefined;
		await this.open(p.path, sel, false);
		if (p.view === "kismet") await this.openKismetQuiet(p.level ?? p.path, p.sequence);
		else if (p.view === "matinee" && p.matinee) {
			const before = nav.navigating;
			nav.navigating = true;
			try {
				await this.openMatinee(p.matinee.level, p.matinee.index);
			} finally {
				nav.navigating = before;
			}
		} else {
			this.view = "scene";
			// The history ignores it while navigating; the active tab follows.
			this.record();
		}
	}

	private async openKismetQuiet(level: string, sequence?: number): Promise<void> {
		const before = nav.navigating;
		nav.navigating = true;
		try {
			await this.openKismet(level, sequence);
		} finally {
			nav.navigating = before;
		}
	}
}

export const map = new MapStore();

nav.register({ map: (p) => map.visit(p) });

// Switching to the map view is a move: the tab shows the map again.
$effect.root(() => {
	$effect(() => {
		if (studio.mode !== "map") return;
		untrack(() => map.record());
	});
});
