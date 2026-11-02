// Matinee playback for the preview: sample a track's curves at a time and
// turn a movement key into a matrix in the map view's space. Matrices here
// are Unreal row-vector 4×4 in row-major arrays, which is also three.js'
// column-major `fromArray` order once moved to the view (see map.rs).

export type Key = { time: number; value: number | number[] | string | null; mode?: string | null };

/** The value of a curve at `t`: linear between keys (constant modes step), held at the ends. */
export function sample(keys: Key[], t: number): number[] | null {
	const vals = keys.filter((k) => k.value !== null && typeof k.value !== "string");
	if (vals.length === 0) return null;
	const v = (k: Key) => (Array.isArray(k.value) ? k.value : [k.value as number]);
	if (t <= vals[0].time) return v(vals[0]);
	for (let i = 0; i < vals.length - 1; i++) {
		const a = vals[i];
		const b = vals[i + 1];
		if (t >= a.time && t <= b.time) {
			if (a.mode && /constant/i.test(a.mode)) return v(a);
			const f = b.time > a.time ? (t - a.time) / (b.time - a.time) : 0;
			const va = v(a);
			const vb = v(b);
			return va.map((x, k) => x + ((vb[k] ?? x) - x) * f);
		}
	}
	return v(vals.at(-1)!);
}

type Mat = number[];

const IDENTITY: Mat = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

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

/** `FRotationMatrix` from degrees. */
function rotation(pitch: number, yaw: number, roll: number): Mat {
	const r = Math.PI / 180;
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

const P: Mat = [0.01, 0, 0, 0, 0, 0, 0.01, 0, 0, 0.01, 0, 0, 0, 0, 0, 1];
const P_INV: Mat = [100, 0, 0, 0, 0, 0, 100, 0, 0, 100, 0, 0, 0, 0, 0, 1];

/** An Unreal matrix in the view's space. */
export function toView(m: Mat): Mat {
	return mul(mul(P_INV, m), P);
}

/**
 * A movement key as a view matrix: position (Unreal units) and Euler
 * angles in degrees (X roll, Y pitch, Z yaw, as `EulerTrack` stores them).
 * Relative to the actor's initial placement (`IMF_RelativeToInitial`) when
 * `initial` (a view matrix) is given, else in world space.
 */
export function movePose(pos: number[], euler: number[] | null, initial: Mat | null): Mat {
	const e = euler ?? [0, 0, 0];
	const m = rotation(e[1] ?? 0, e[2] ?? 0, e[0] ?? 0);
	m[12] = pos[0] ?? 0;
	m[13] = pos[1] ?? 0;
	m[14] = pos[2] ?? 0;
	const view = toView(m);
	return initial ? mul(view, initial) : view;
}

export { IDENTITY };
