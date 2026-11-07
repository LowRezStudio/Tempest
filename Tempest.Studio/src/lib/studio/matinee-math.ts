// Matinee playback for the preview, ported from the engine so the view
// matches the game: `FInterpCurve::Eval` / `AutoSetTangents` (UnMath.h) and
// the move track's `GetKeyTransformAtTime` / `ComputeWorldSpaceKeyTransform`
// / `CalcInitialTransform` (UnInterpolation.cpp). The Rust reference is
// `matinee_curve.rs` in tempest-assets; both give the same numbers on its
// test curves. Matrices are Unreal row-vector 4×4 in row-major arrays, which
// is also three.js' column-major `fromArray` order once moved to the view
// (see map.rs).

export type Mode =
	| "CIM_Linear"
	| "CIM_CurveAuto"
	| "CIM_Constant"
	| "CIM_CurveUser"
	| "CIM_CurveBreak"
	| "CIM_CurveAutoClamped";

export const MODES: { id: Mode; label: string }[] = [
	{ id: "CIM_CurveAutoClamped", label: "Auto clamped" },
	{ id: "CIM_CurveAuto", label: "Auto" },
	{ id: "CIM_CurveUser", label: "User" },
	{ id: "CIM_CurveBreak", label: "Break" },
	{ id: "CIM_Linear", label: "Linear" },
	{ id: "CIM_Constant", label: "Constant" },
];

/** A key as `matinee.get` sends it (curves carry tangents and a mode). */
export type Key = {
	time: number;
	value: number | number[] | string | null;
	mode?: string | null;
	arrive?: number | number[] | null;
	leave?: number | number[] | null;
};

/** `FInterpCurvePoint`. */
export type Point = { time: number; out: number[]; arrive: number[]; leave: number[]; mode: Mode };

const KINDA_SMALL_NUMBER = 1e-4;

export function parseMode(m: string | null | undefined): Mode {
	const l = (m ?? "").toLowerCase();
	return MODES.find((x) => x.id.toLowerCase() === l)?.id ?? "CIM_Linear";
}

const isCurve = (m: Mode) =>
	m === "CIM_CurveAuto" ||
	m === "CIM_CurveAutoClamped" ||
	m === "CIM_CurveUser" ||
	m === "CIM_CurveBreak";
const isAuto = (m: Mode) => m === "CIM_CurveAuto" || m === "CIM_CurveAutoClamped";

const arr = (v: number | number[] | string | null | undefined, width: number): number[] =>
	Array.isArray(v) ? v : typeof v === "number" ? [v] : Array.from({ length: width }, () => 0);

/** The points of a key list (numeric keys only). */
export function points(keys: Key[]): Point[] {
	const width = keys.reduce(
		(w, k) => Math.max(w, Array.isArray(k.value) ? k.value.length : 1),
		1,
	);
	return keys
		.filter((k) => k.value !== null && typeof k.value !== "string")
		.map((k) => ({
			time: k.time,
			out: arr(k.value, width),
			arrive: arr(k.arrive, width),
			leave: arr(k.leave, width),
			mode: parseMode(k.mode),
		}));
}

/** `CubicInterp` (UnTemplate.h). */
function cubic(p0: number, t0: number, p1: number, t1: number, a: number): number {
	const a2 = a * a;
	const a3 = a2 * a;
	return (
		(2 * a3 - 3 * a2 + 1) * p0 +
		(a3 - 2 * a2 + a) * t0 +
		(a3 - a2) * t1 +
		(-2 * a3 + 3 * a2) * p1
	);
}

/** `FInterpCurve::Eval` (`IMT_UseFixedTangentEvalAndNewAutoTangents`). */
export function evalCurve(pts: Point[], t: number, def: number[] = [0]): number[] {
	const n = pts.length;
	if (n === 0) return def;
	if (n < 2 || t <= pts[0].time) return pts[0].out;
	if (t >= pts[n - 1].time) return pts[n - 1].out;
	for (let i = 1; i < n; i++) {
		if (t < pts[i].time) {
			const a = pts[i - 1];
			const b = pts[i];
			const diff = b.time - a.time;
			if (diff > 0 && a.mode !== "CIM_Constant") {
				const alpha = (t - a.time) / diff;
				return a.out.map((x, k) =>
					a.mode === "CIM_Linear"
						? x + ((b.out[k] ?? 0) - x) * alpha
						: cubic(
								x,
								(a.leave[k] ?? 0) * diff,
								b.out[k] ?? 0,
								(b.arrive[k] ?? 0) * diff,
								alpha,
							),
				);
			}
			return a.out;
		}
	}
	return pts[n - 1].out;
}

/** The value of a key list at `t` (null without numeric keys). */
export function sample(keys: Key[], t: number): number[] | null {
	const p = points(keys);
	return p.length ? evalCurve(p, t) : null;
}

/** `FClampFloatTangent` (UnMath.cpp). */
export function clampFloatTangent(
	prev: number,
	prevT: number,
	cur: number,
	curT: number,
	next: number,
	nextT: number,
): number {
	const prevToNextT = Math.max(KINDA_SMALL_NUMBER, nextT - prevT);
	const prevToCurT = Math.max(KINDA_SMALL_NUMBER, curT - prevT);
	const curToNextT = Math.max(KINDA_SMALL_NUMBER, nextT - curT);
	const prevToNextH = next - prev;
	const prevToCurH = cur - prev;
	const curToNextH = next - cur;
	if ((prevToCurH >= 0 && curToNextH <= 0) || (prevToCurH <= 0 && curToNextH >= 0)) return 0;
	const CLAMP = 0.333;
	const curToNextTan = curToNextH / curToNextT;
	const prevToCurTan = prevToCurH / prevToCurT;
	const prevToNextTan = prevToNextH / prevToNextT;
	let clamped = prevToNextTan;
	const lower = CLAMP;
	const upper = 1 - CLAMP;
	const alpha = prevToCurH / prevToNextH;
	const lerp = (a: number, b: number, x: number) => a + (b - a) * x;
	const pick = prevToNextH > 0 ? Math.min : Math.max;
	if (alpha < lower) {
		clamped = pick(clamped, lerp(prevToNextTan, prevToCurTan, 1 - alpha / CLAMP));
	}
	if (alpha > upper) {
		clamped = pick(clamped, lerp(prevToNextTan, curToNextTan, (alpha - upper) / CLAMP));
	}
	return clamped;
}

/** `FInterpCurve::AutoSetTangents`; `clampable` is false for linear colours (no clamping overload). */
export function autoSetTangents(pts: Point[], tension = 0, clampable = true): Point[] {
	const n = pts.length;
	return pts.map((p, i) => {
		const zero = p.out.map(() => 0);
		let arrive = p.arrive;
		let leave = p.leave;
		if (i === 0) {
			if (n === 1 || isAuto(p.mode)) leave = zero;
		} else if (i < n - 1) {
			if (isAuto(p.mode)) {
				const prev = pts[i - 1];
				const next = pts[i + 1];
				if (isCurve(prev.mode) && isCurve(p.mode)) {
					const clamp = clampable && p.mode === "CIM_CurveAutoClamped";
					const diff = Math.max(KINDA_SMALL_NUMBER, next.time - prev.time);
					arrive = p.out.map((c, k) =>
						clamp
							? (1 - tension) *
								clampFloatTangent(
									prev.out[k] ?? 0,
									prev.time,
									c,
									p.time,
									next.out[k] ?? 0,
									next.time,
								)
							: ((1 - tension) *
									(c - (prev.out[k] ?? 0) + ((next.out[k] ?? 0) - c))) /
								diff,
					);
					leave = arrive;
				} else if (prev.mode === "CIM_Constant") {
					// The engine also tests the key itself here, which an auto key never is.
					arrive = zero;
					leave = zero;
				}
			}
		} else if (isAuto(p.mode)) {
			arrive = zero;
		}
		return { ...p, arrive, leave };
	});
}

// ----- matrices --------------------------------------------------------------------

type Mat = number[];

export const IDENTITY: Mat = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

export function mul(a: Mat, b: Mat): Mat {
	const out = Array.from({ length: 16 }, () => 0);
	for (let i = 0; i < 4; i++) {
		for (let j = 0; j < 4; j++) {
			let s = 0;
			for (let k = 0; k < 4; k++) s += a[i * 4 + k] * b[k * 4 + j];
			out[i * 4 + j] = s;
		}
	}
	return out;
}

/** Inverse of an affine row-vector matrix. */
export function inverse(m: Mat): Mat {
	const a = [
		[m[0], m[1], m[2]],
		[m[4], m[5], m[6]],
		[m[8], m[9], m[10]],
	];
	const det =
		a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) -
		a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0]) +
		a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
	const d = Math.abs(det) < 1e-12 ? 1e-12 : det;
	const inv = [
		[
			(a[1][1] * a[2][2] - a[1][2] * a[2][1]) / d,
			(a[0][2] * a[2][1] - a[0][1] * a[2][2]) / d,
			(a[0][1] * a[1][2] - a[0][2] * a[1][1]) / d,
		],
		[
			(a[1][2] * a[2][0] - a[1][0] * a[2][2]) / d,
			(a[0][0] * a[2][2] - a[0][2] * a[2][0]) / d,
			(a[0][2] * a[1][0] - a[0][0] * a[1][2]) / d,
		],
		[
			(a[1][0] * a[2][1] - a[1][1] * a[2][0]) / d,
			(a[0][1] * a[2][0] - a[0][0] * a[2][1]) / d,
			(a[0][0] * a[1][1] - a[0][1] * a[1][0]) / d,
		],
	];
	const t = [m[12], m[13], m[14]];
	const out = [...IDENTITY];
	for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) out[i * 4 + j] = inv[i][j];
	for (let j = 0; j < 3; j++) {
		out[12 + j] = -(t[0] * inv[0][j] + t[1] * inv[1][j] + t[2] * inv[2][j]);
	}
	return out;
}

/** `FRotationMatrix` from a rotator (pitch, yaw, roll in 65536ths of a turn). */
export function rotation(pitch: number, yaw: number, roll: number): Mat {
	const r = Math.PI / 32768;
	const [sp, cp] = [Math.sin(pitch * r), Math.cos(pitch * r)];
	const [sy, cy] = [Math.sin(yaw * r), Math.cos(yaw * r)];
	const [sr, cr] = [Math.sin(roll * r), Math.cos(roll * r)];
	return [
		cp * cy,
		cp * sy,
		sp,
		0,
		sr * sp * cy - cr * sy,
		sr * sp * sy + cr * cy,
		-sr * cp,
		0,
		-(cr * sp * cy + sr * sy),
		cy * sr - cr * sp * sy,
		cr * cp,
		0,
		0,
		0,
		0,
		1,
	];
}

/** `FRotationTranslationMatrix`. */
export function rotationTranslation(rot: number[], pos: number[]): Mat {
	const m = rotation(rot[0] ?? 0, rot[1] ?? 0, rot[2] ?? 0);
	m[12] = pos[0] ?? 0;
	m[13] = pos[1] ?? 0;
	m[14] = pos[2] ?? 0;
	return m;
}

/** `FMatrix::Rotator` (map.rs `decompose`), from the matrix's axes. */
export function rotator(m: Mat): number[] {
	const len = (i: number) => Math.hypot(m[i * 4], m[i * 4 + 1], m[i * 4 + 2]) || 1;
	const ax = (i: number) => [m[i * 4] / len(i), m[i * 4 + 1] / len(i), m[i * 4 + 2] / len(i)];
	const [x, y, z] = [ax(0), ax(1), ax(2)];
	const k = 32768 / Math.PI;
	const pitch = Math.atan2(x[2], Math.hypot(x[0], x[1])) * k;
	const yaw = Math.atan2(x[1], x[0]) * k;
	const r = rotation(pitch, yaw, 0);
	const sy = [r[4], r[5], r[6]];
	const dot = (a: number[]) => a[0] * sy[0] + a[1] * sy[1] + a[2] * sy[2];
	return [pitch, yaw, Math.atan2(dot(z), dot(y)) * k];
}

const P: Mat = [0.01, 0, 0, 0, 0, 0, 0.01, 0, 0, 0.01, 0, 0, 0, 0, 0, 1];
const P_INV: Mat = [100, 0, 0, 0, 0, 0, 100, 0, 0, 100, 0, 0, 0, 0, 0, 1];

/** An Unreal matrix in the view's space. */
export function toView(m: Mat): Mat {
	return mul(mul(P_INV, m), P);
}

/** A view matrix back in Unreal space. */
export function fromView(m: Mat): Mat {
	return mul(mul(P, m), P_INV);
}

// ----- move tracks -------------------------------------------------------------------

/** `FRotator::MakeFromEuler`: Euler degrees (X roll, Y pitch, Z yaw) to a truncated rotator. */
export function rotatorFromEuler(e: number[]): number[] {
	const k = 32768 / 180;
	return [Math.trunc((e[1] ?? 0) * k), Math.trunc((e[2] ?? 0) * k), Math.trunc((e[0] ?? 0) * k)];
}

/** `FRotator::Euler`. */
export function eulerFromRotator(r: number[]): number[] {
	const k = 180 / 32768;
	return [(r[2] ?? 0) * k, (r[0] ?? 0) * k, (r[1] ?? 0) * k];
}

/** What a move track's keys give at a time: position, and a rotator. */
export type MoveCurves = { pos: Point[]; euler: Point[]; axes: Point[][] | null };

/** `UInterpTrackMove::GetKeyTransformAtTime` (no lookup keys, no quaternion interpolation). */
export function keyTransform(c: MoveCurves, t: number): { pos: number[]; rot: number[] } {
	if (c.axes) {
		const v = (k: number) => evalCurve(c.axes![k] ?? [], t, [0])[0] ?? 0;
		return { pos: [v(0), v(1), v(2)], rot: rotatorFromEuler([v(3), v(4), v(5)]) };
	}
	return {
		pos: evalCurve(c.pos, t, [0, 0, 0]),
		rot: rotatorFromEuler(evalCurve(c.euler, t, [0, 0, 0])),
	};
}

function removeScaling(m: Mat): Mat {
	const out = [...m];
	for (let i = 0; i < 3; i++) {
		const l = Math.hypot(m[i * 4], m[i * 4 + 1], m[i * 4 + 2]);
		if (l > 1e-8) for (let j = 0; j < 3; j++) out[i * 4 + j] = m[i * 4 + j] / l;
	}
	return out;
}

/** `UInterpTrackInstMove::CalcInitialTransform` (no base). */
export function initialTM(
	actorRot: number[],
	actorLoc: number[],
	start: { pos: number[]; rot: number[] },
	raw: boolean,
): Mat {
	const actor = rotationTranslation(actorRot, actorLoc);
	return removeScaling(
		raw ? actor : mul(inverse(rotationTranslation(start.rot, start.pos)), actor),
	);
}

/** `ComputeWorldSpaceKeyTransform` with `GetMoveRefFrame` (no base): world when `initial` is null. */
export function worldTransform(pos: number[], rot: number[], initial: Mat | null): Mat {
	const local = rotationTranslation(rot, pos);
	return initial ? mul(local, initial) : local;
}

/**
 * Where a driven actor is drawn at time `t`, as a view matrix: the actor's
 * own scale (what its placement has besides rotation and location) times
 * the track's world transform. `actor` is its placement in the view.
 */
export function movePose(
	c: MoveCurves,
	t: number,
	actor: Mat,
	relative: boolean,
	raw: boolean,
	startTime = 0,
): Mat {
	const placed = fromView(actor);
	const rot = rotator(placed);
	const loc = [placed[12], placed[13], placed[14]];
	const pre = mul(placed, inverse(rotationTranslation(rot, loc)));
	const initial = relative ? initialTM(rot, loc, keyTransform(c, startTime), raw) : null;
	const k = keyTransform(c, t);
	return toView(mul(pre, worldTransform(k.pos, k.rot, initial)));
}

/**
 * The key that puts the actor at view matrix `target` (`UpdateKeyframe`):
 * position and Euler angles in the track's reference frame, the angles
 * unwound to the nearest of `near` (the key's previous angles).
 */
export function keyFromPose(
	c: MoveCurves,
	target: Mat,
	actor: Mat,
	relative: boolean,
	raw: boolean,
	near: number[] = [0, 0, 0],
): { pos: number[]; euler: number[] } {
	const placed = fromView(actor);
	const rot = rotator(placed);
	const loc = [placed[12], placed[13], placed[14]];
	const pre = mul(placed, inverse(rotationTranslation(rot, loc)));
	const world = removeScaling(mul(inverse(pre), fromView(target)));
	const rel = relative
		? mul(world, inverse(initialTM(rot, loc, keyTransform(c, 0), raw)))
		: world;
	const e = eulerFromRotator(rotator(rel));
	const unwound = e.map((a, i) => a + Math.round(((near[i] ?? 0) - a) / 360) * 360);
	return { pos: [rel[12], rel[13], rel[14]], euler: unwound };
}
