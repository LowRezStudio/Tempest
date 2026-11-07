// What a Matinee timeline does to the map view at a time: the pose of every
// actor a move track drives (`matinee-math.ts`, the engine's evaluation),
// the path each movement draws, and the property tracks the view can show:
// `DrawScale3D` scales the pose, `LightColor`/`Brightness` tint a light's
// marker, toggle and visibility tracks hide what they switch off.
import { evalCurve, movePose, points, type MoveCurves, type Point } from "./matinee-math";
import type {
	MapActor,
	MatineeGroup,
	MatineeTimeline,
	MatineeTrack,
} from "../../../electron/src/ipc/contract";

export type MatineePreview = {
	/** View matrix per driven actor (`level#export`). */
	poses: Map<string, number[]>;
	paths: { color: string; points: number[][] }[];
	/** Actors switched off (toggle / visibility tracks). */
	hidden: Set<string>;
	/** Light colour (0..1 RGB) and brightness per actor. */
	tints: Map<string, { color?: number[]; brightness?: number }>;
};

const curveOf = (t: MatineeTrack, name: string) =>
	t.curves.find((c) => c.name.toLowerCase() === name.toLowerCase());

/** The curves of a move track (its split axes when it has `SubTracks`). */
export function moveCurves(group: MatineeGroup, move: MatineeTrack): MoveCurves {
	const pts = (t: MatineeTrack | undefined, name: string): Point[] =>
		t ? points(curveOf(t, name)?.keys ?? []) : [];
	const subs = group.tracks.filter((x) => x.parent === move.export);
	const axisNames = [
		"axis_translationx",
		"axis_translationy",
		"axis_translationz",
		"axis_rotationx",
		"axis_rotationy",
		"axis_rotationz",
	];
	return {
		pos: pts(move, "PosTrack"),
		euler: pts(move, "EulerTrack"),
		axes:
			subs.length > 0
				? axisNames.map((a) =>
						pts(
							subs.find((s) => (s.moveAxis ?? "").toLowerCase() === a),
							"FloatTrack",
						),
					)
				: null,
	};
}

export const isMove = (t: MatineeTrack) => t.class.toLowerCase() === "interptrackmove";

/** The state a timed key list (`ETTA_On`, `EVTA_Hide`…) leaves at `t`, or null before its first key. */
function lastKey(t: MatineeTrack, time: number): string | null {
	const keys = t.curves[0]?.keys ?? [];
	let v: string | null = null;
	for (const k of keys) if (k.time <= time && typeof k.value === "string") v = k.value;
	return v;
}

export function timelinePreview(
	tl: MatineeTimeline | null,
	level: string | null,
	t: number,
	actorOf: (exp: number) => MapActor | null,
): MatineePreview {
	const out: MatineePreview = {
		poses: new Map(),
		paths: [],
		hidden: new Set(),
		tints: new Map(),
	};
	if (!tl || !level) return out;
	const startTime = tl.settings?.forceStartPos ? tl.settings.forceStartPosition : 0;
	for (const g of tl.groups) {
		const driven = g.actors.filter((a) => a.export !== null);
		const move = g.tracks.find((x) => isMove(x) && !x.disabled);
		const color = g.color ? `rgb(${g.color[0]},${g.color[1]},${g.color[2]})` : "#f9e2af";
		// Property tracks first: a scale changes the pose.
		let scale: number[] | null = null;
		const tint: { color?: number[]; brightness?: number } = {};
		let hide = false;
		for (const tr of g.tracks) {
			if (tr.disabled) continue;
			const cls = tr.class.toLowerCase();
			const prop = (tr.property ?? "").toLowerCase();
			const curve = tr.curves.find((c) => c.kind === "curve");
			const value = () =>
				curve && curve.keys.length ? evalCurve(points(curve.keys), t) : null;
			if (cls === "interptrackvectorprop" && prop === "drawscale3d") scale = value();
			else if (cls === "interptrackcolorprop" && prop === "lightcolor") {
				const v = value();
				if (v) tint.color = v;
			} else if (cls === "interptracklinearcolorprop" && prop === "lightcolor") {
				const v = value();
				if (v) tint.color = v.slice(0, 3);
			} else if (cls === "interptrackfloatprop" && prop === "brightness") {
				const v = value();
				if (v) tint.brightness = v[0];
			} else if (cls === "interptracktoggle") {
				const s = lastKey(tr, t)?.toLowerCase();
				if (s === "etta_off") hide = true;
				else if (s === "etta_on") hide = false;
			} else if (cls === "interptrackvisibility") {
				const s = lastKey(tr, t)?.toLowerCase();
				if (s === "evta_hide") hide = true;
				else if (s === "evta_show") hide = false;
			}
		}
		for (const a of driven) {
			const key = `${level}#${a.export}`;
			if (hide) out.hidden.add(key);
			if (tint.color || tint.brightness !== undefined) out.tints.set(key, tint);
		}
		const actor = driven.length ? actorOf(driven[0].export!) : null;
		if (!move) {
			if (scale && actor) {
				const m = [...actor.matrix];
				const base = actor.drawScale3D ?? [1, 1, 1];
				// View axes are (x, z, y) of Unreal's.
				const f = [
					scale[0] / (base[0] || 1),
					scale[2] / (base[2] || 1),
					scale[1] / (base[1] || 1),
				];
				for (let r = 0; r < 3; r++) for (let c = 0; c < 3; c++) m[r * 4 + c] *= f[r];
				for (const a of driven) out.poses.set(`${level}#${a.export}`, m);
			}
			continue;
		}
		const c = moveCurves(g, move);
		if (!c.axes && c.pos.length === 0) continue;
		const relative = /relative/i.test(move.moveFrame ?? "");
		for (const a of driven) {
			const placed = actorOf(a.export!);
			if (!placed) continue;
			let m = movePose(c, t, placed.matrix, relative, move.rawInitial, startTime);
			if (scale) {
				const base = placed.drawScale3D ?? [1, 1, 1];
				const f = [
					scale[0] / (base[0] || 1),
					scale[2] / (base[2] || 1),
					scale[1] / (base[1] || 1),
				];
				m = [...m];
				for (let r = 0; r < 3; r++) for (let k = 0; k < 3; k++) m[r * 4 + k] *= f[r];
			}
			out.poses.set(`${level}#${a.export}`, m);
		}
		// The path: through the first driven actor, or from the world origin.
		const base = actor?.matrix ?? [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
		const steps = Math.max(48, Math.min(400, Math.round(tl.length * 30)));
		const pts = Array.from({ length: steps + 1 }, (_, i) => {
			const m = movePose(
				c,
				(tl.length * i) / steps,
				base,
				relative,
				move.rawInitial,
				startTime,
			);
			return [m[12], m[13], m[14]];
		});
		out.paths.push({ color, points: pts });
	}
	return out;
}
