// The Matinee editor's state and actions (`MatineePanel` and its parts, the
// MCP tool): what is selected (group, track, key), playback (loop, section,
// rate), the curve editor, and every edit, each one `matinee.*` call (one
// undo step of the level, `map.matineeEdit`). The open timeline itself lives
// in the map store (`map.matinee`, `map.matineeAt`, `map.matineeTime`).
import { map } from "./map.svelte";
import { keyFromPose, type Mode } from "./matinee-math";
import { isMove, moveCurves } from "./matinee-preview";
import { rpc } from "./rpc";
import { studio } from "./state.svelte";
import { toasts } from "./toast.svelte";
import type {
	MapActor,
	MatineeCurve,
	MatineeGroup,
	MatineeTrack,
	UpkMethods,
} from "../../../electron/src/ipc/contract";

/** The track classes the game's maps use, with what they need. */
export const TRACK_CLASSES: {
	class: string;
	label: string;
	property?: string[];
	director?: boolean;
}[] = [
	{ class: "InterpTrackMove", label: "Movement" },
	{ class: "InterpTrackEvent", label: "Event" },
	{ class: "InterpTrackAkEvent", label: "Wwise event" },
	{ class: "InterpTrackSound", label: "Sound" },
	{ class: "InterpTrackAnimControl", label: "Animation" },
	{ class: "InterpTrackToggle", label: "Toggle" },
	{ class: "InterpTrackVisibility", label: "Visibility" },
	{ class: "InterpTrackFade", label: "Fade", director: true },
	{ class: "InterpTrackDirector", label: "Director", director: true },
	{
		class: "InterpTrackFloatProp",
		label: "Float property",
		property: ["Brightness", "Radius", "FOVAngle", "DrawScale"],
	},
	{ class: "InterpTrackVectorProp", label: "Vector property", property: ["DrawScale3D"] },
	{ class: "InterpTrackColorProp", label: "Colour property", property: ["LightColor"] },
	{ class: "InterpTrackFloatMaterialParam", label: "Material scalar", property: [] },
	{ class: "InterpTrackVectorMaterialParam", label: "Material vector", property: [] },
];

export type KeyRef = { track: number; curve: string; key: number };

type EditMethod = Parameters<typeof map.matineeEdit>[0];

class MatineeEditor {
	group = $state<number | null>(null);
	track = $state<number | null>(null);
	loop = $state(true);
	/** Play only `EdSectionStart`..`EdSectionEnd`. */
	sectionOnly = $state(false);
	/** Preview speed (the action's `PlayRate` by default). */
	rate = $state<number | null>(null);
	/** The curve editor's track and curve. */
	curve = $state<{ track: number; path: string } | null>(null);
	list = $state.raw<UpkMethods["matinee.list"]["result"]["matinees"] | null>(null);
	/** The level `list` is of. */
	listFor = $state<string | null>(null);

	constructor() {
		map.captureMove = (level, actor, matrix) => this.captureMove(level, actor, matrix);
	}

	get tl() {
		return map.matinee;
	}

	groupOf(track: number): MatineeGroup | null {
		return this.tl?.groups.find((g) => g.tracks.some((t) => t.export === track)) ?? null;
	}

	trackOf(track: number | null): MatineeTrack | null {
		if (track === null) return null;
		for (const g of this.tl?.groups ?? []) {
			const t = g.tracks.find((x) => x.export === track);
			if (t) return t;
		}
		return null;
	}

	curveOf(ref: { track: number; curve: string } | null): MatineeCurve | null {
		if (!ref) return null;
		return this.trackOf(ref.track)?.curves.find((c) => c.path === ref.curve) ?? null;
	}

	private edit(method: EditMethod, params: Record<string, unknown>) {
		return map.matineeEdit(method, params);
	}

	// ----- keys ----------------------------------------------------------------------

	selectKey(ref: KeyRef | null) {
		map.matineeKey = ref;
		if (ref) {
			this.track = ref.track;
			this.group = this.groupOf(ref.track)?.export ?? this.group;
		}
	}

	async setKey(
		ref: KeyRef,
		change: {
			time?: number;
			value?: number | number[];
			mode?: Mode;
			arrive?: number | number[];
			leave?: number | number[];
			fields?: Record<string, string | number | boolean | { path: string; index: number }>;
			values?: Record<string, number | number[]>;
		},
	) {
		const r = await this.edit("matinee.setKey", { ...ref, ...change });
		if (r?.key !== undefined) map.matineeKey = { ...ref, key: r.key };
		return r;
	}

	async addKey(
		track: number,
		curve: string,
		time: number,
		extra: {
			value?: number | number[];
			mode?: Mode;
			values?: Record<string, number | number[]>;
			fields?: Record<string, string | number | boolean>;
		} = {},
	) {
		const r = await this.edit("matinee.addKey", { track, curve, time, ...extra });
		if (r?.key !== undefined) this.selectKey({ track, curve, key: r.key });
		return r;
	}

	async removeKey(ref: KeyRef) {
		await this.edit("matinee.removeKey", { ...ref });
		map.matineeKey = null;
	}

	/** Recompute a key's tangents: back to an auto mode. */
	autoTangents(ref: KeyRef) {
		return this.setKey(ref, { mode: "CIM_CurveAutoClamped" });
	}

	/** The actor a group drives, as placed in the map. */
	drivenActor(g: MatineeGroup): MapActor | null {
		const at = map.matineeAt;
		const a = g.actors.find((x) => x.export !== null);
		return at && a ? map.actor({ level: at.level, export: a.export! }) : null;
	}

	/**
	 * A key at the playhead on a move track from where its actor is now
	 * drawn (`AddKeyframe` + `UpdateKeyframe`); on another track, a key with
	 * the value the curve has there.
	 */
	async keyAtPlayhead(track: number) {
		const t = this.trackOf(track);
		const g = this.groupOf(track);
		if (!t || !g) return;
		const time = Math.round(map.matineeTime * 1000) / 1000;
		if (isMove(t)) {
			const actor = this.drivenActor(g);
			const pose = actor
				? (map.matineePreview.poses.get(`${map.matineeAt!.level}#${actor.export}`) ??
					actor.matrix)
				: null;
			if (!actor || !pose) {
				await this.addKey(track, "PosTrack.Points", time);
				return;
			}
			const k = keyFromPose(
				moveCurves(g, t),
				pose,
				actor.matrix,
				/relative/i.test(t.moveFrame ?? ""),
				t.rawInitial,
			);
			await this.addKey(track, "PosTrack.Points", time, {
				value: k.pos,
				values: { "EulerTrack.Points": k.euler },
			});
			return;
		}
		const c = t.curves[0];
		if (c) await this.addKey(track, c.path, time);
	}

	/**
	 * A gizmo move of an actor a move track drives, in the Matinee view:
	 * the key under the playhead takes the new placement, or a new key is
	 * made there; the actor's own placement is left alone.
	 */
	async captureMove(level: string, actor: MapActor, matrix: number[]): Promise<boolean> {
		const tl = this.tl;
		if (!tl || map.matineeAt?.level !== level) return false;
		for (const g of tl.groups) {
			if (!g.actors.some((a) => a.export === actor.export)) continue;
			const move = g.tracks.find((t) => isMove(t) && !t.disabled && t.subTracks.length === 0);
			const keys = move?.curves.find((c) => c.name.toLowerCase() === "postrack")?.keys ?? [];
			if (!move) continue;
			const k = keys.findIndex((x) => Math.abs(x.time - map.matineeTime) < 1 / 60);
			const eulers =
				move.curves.find((c) => c.name.toLowerCase() === "eulertrack")?.keys ?? [];
			// The angles of the key there, else of the key before the playhead.
			const prev =
				k >= 0 ? eulers[k] : eulers.filter((e) => e.time <= map.matineeTime).at(-1);
			const near = Array.isArray(prev?.value) ? prev.value : [0, 0, 0];
			const pose = keyFromPose(
				moveCurves(g, move),
				matrix,
				actor.matrix,
				/relative/i.test(move.moveFrame ?? ""),
				move.rawInitial,
				near,
			);
			const change = { value: pose.pos, values: { "EulerTrack.Points": pose.euler } };
			// Between keys: a new key there (the editor's key-on-move).
			await (k >= 0
				? this.setKey({ track: move.export, curve: "PosTrack.Points", key: k }, change)
				: this.addKey(
						move.export,
						"PosTrack.Points",
						Math.round(map.matineeTime * 1000) / 1000,
						change,
					));
			map.version++;
			return true;
		}
		return false;
	}

	// ----- groups and tracks -------------------------------------------------------

	async addGroup(name: string, director = false) {
		const r = await this.edit("matinee.addGroup", { name, director });
		if (r?.group !== undefined) this.group = r.group;
		return r;
	}

	async removeGroup(group: number) {
		await this.edit("matinee.removeGroup", { group });
		if (this.group === group) this.group = null;
		map.matineeKey = null;
	}

	setGroup(group: number, change: { name?: string; color?: number[] }) {
		return this.edit("matinee.setGroup", { group, ...change });
	}

	async addTrack(group: number, cls: string, property?: string) {
		const r = await this.edit("matinee.addTrack", { group, class: cls, property });
		if (r?.track !== undefined) this.track = r.track;
		return r;
	}

	async removeTrack(track: number) {
		await this.edit("matinee.removeTrack", { track });
		if (this.track === track) this.track = null;
		if (this.curve?.track === track) this.curve = null;
		map.matineeKey = null;
	}

	/** Properties of the action, the data, a group or a track (`matinee.set`). */
	set(object: number, values: Record<string, string | number | boolean>) {
		return this.edit("matinee.set", { object, values });
	}

	setLength(length: number) {
		const tl = this.tl;
		if (!tl || !(length > 0)) return;
		return this.set(tl.data, { InterpLength: length });
	}

	/** Make the selected actor (of the timeline's level) driven by a group, or not. */
	async connect(group: number, actor: number, remove = false) {
		await this.edit("matinee.connect", { group, actor, remove });
		map.version++;
	}

	// ----- timelines -----------------------------------------------------------------

	async loadList(level: string) {
		try {
			this.list = (await rpc("matinee.list", { path: level })).matinees;
			this.listFor = level;
		} catch (error) {
			toasts.error(error, "Could not list the Matinees");
		}
	}

	/** Open the first Matinee of a level (the timeline's picker lists the others). */
	async openFirst(level: string) {
		await this.loadList(level);
		const first = this.list?.[0];
		if (first) await map.openMatinee(level, first.export);
		else toasts.push("info", "This level has no Matinee: New Matinee makes one", "Matinee");
	}

	/** A new Matinee in `level`'s top sequence (or `sequence`), driving `actor` when given. */
	async create(level: string, opts: { actor?: number; length?: number; sequence?: number } = {}) {
		try {
			const r = await rpc("matinee.create", { path: level, length: 5, ...opts });
			map.noteEdit(level);
			studio.markDirty(level);
			this.listFor = null;
			if (r.action !== null) await map.openMatinee(level, r.action);
			return r;
		} catch (error) {
			toasts.error(error, "Could not create the Matinee");
			return null;
		}
	}
}

export const matineeEd = new MatineeEditor();
