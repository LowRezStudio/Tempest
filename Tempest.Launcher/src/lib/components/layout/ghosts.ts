// The Halloween ghosts, simulated a frame at a time: each has its own shape, pace, tricks and
// moods. They float after a point gliding along a smooth path, so their motion has weight and
// lag, and their sheets, arms and accessories follow on springs. HalloweenGhosts.svelte draws them.

export type Kind = "boo" | "lumen" | "zip" | "baby";
export type Expression = "smile" | "joy" | "wow" | "calm";

type Point = { x: number; y: number };
type Waypoint = Point & {
	trick?: "loop" | "orbit";
	/** Circling another ghost: the point moves along with it */
	around?: { host: Ghost; angle: number; r: number };
};

type Character = {
	size: number;
	/** px/s along its path; the baby has none of its own, it tags along after Lumen */
	speed: number;
	head: number;
	length: number;
	/** How much wider than the head the hem gets */
	flare: number;
	/** How much narrower the hem gets, as a share of the head: Zip's comet tail */
	taper: number;
	waves: number;
	wave: number;
	/** How far the hem streams behind, per px/s */
	trail: number;
	stiffness: number;
	damping: number;
	lean: number;
	bob: { size: number; speed: number };
	shade: string;
	face: { gap: number; eye: number; cheeks: number; grin: boolean; blinkEvery: number };
	arms: boolean;
};

const characters: Record<Kind, Character> = {
	// Sweet and a little shy: wanders lazily and stops for happy twirls
	boo: {
		size: 1.15,
		speed: 58,
		head: 14,
		length: 28,
		flare: 7,
		taper: 0,
		waves: 4,
		wave: 2.8,
		trail: 0.2,
		stiffness: 10,
		damping: 5.7,
		lean: 0.08,
		bob: { size: 3, speed: 2.2 },
		shade: "#f4d7ec",
		face: { gap: 5.5, eye: 1, cheeks: 0.9, grin: false, blinkEvery: 3.8 },
		arms: true,
	},
	// Round and unhurried: drifts with its lantern, stops now and then to look around
	lumen: {
		size: 1.2,
		speed: 36,
		head: 16,
		length: 21,
		flare: 9,
		taper: 0,
		waves: 5,
		wave: 2.4,
		trail: 0.16,
		stiffness: 8,
		damping: 5.4,
		lean: 0.05,
		bob: { size: 2.4, speed: 1.6 },
		shade: "#d3e3f5",
		face: { gap: 6, eye: 0.95, cheeks: 0.55, grin: false, blinkEvery: 5.2 },
		arms: true,
	},
	// Fast and playful: a comet with a streaming tail that loops and circles the others
	zip: {
		size: 1,
		speed: 125,
		head: 12,
		length: 30,
		flare: 0,
		taper: 0.55,
		waves: 2,
		wave: 2.2,
		trail: 0.32,
		stiffness: 26,
		damping: 8.7,
		lean: 0.09,
		bob: { size: 2, speed: 3.1 },
		shade: "#c8efe3",
		face: { gap: 5, eye: 0.95, cheeks: 0.6, grin: true, blinkEvery: 2.9 },
		arms: false,
	},
	// Lumen's little one, bobbing along behind it
	baby: {
		size: 1,
		speed: 0,
		head: 9,
		length: 12,
		flare: 4,
		taper: 0,
		waves: 3,
		wave: 1.6,
		trail: 0.18,
		stiffness: 18,
		damping: 5.5,
		lean: 0.1,
		bob: { size: 3.5, speed: 3.8 },
		shade: "#e4dcf8",
		face: { gap: 4.8, eye: 1.15, cheeks: 0.8, grin: false, blinkEvery: 3.3 },
		arms: true,
	},
};

type Ghost = {
	kind: Kind;
	c: Character;
	seed: number;
	x: number;
	y: number;
	vx: number;
	vy: number;
	ax: number;
	carrot: Point;
	path: Waypoint[];
	plan: Waypoint[];
	u: number;
	pace: number;
	paceTarget: number;
	heading: number;
	looping: boolean;
	angle: number;
	angleSpeed: number;
	/** Extra turns on top of the lean, from twirls and loops; settles back to zero */
	spin: number;
	tail: number;
	tailSpeed: number;
	/** A pendulum in the page's frame, for the lantern, bow, hat and curl */
	swing: number;
	swingSpeed: number;
	phase: number;
	face: Expression;
	lastFace: Expression;
	faceSince: number;
	busyUntil: number;
	nextTrick: number;
	waveUntil: number;
	greetedAt: number;
	sparkles: (Point & { at: number })[];
	lastSparkle: number;
	history: (Point & { at: number })[];
};

export type GhostPose = {
	kind: Kind;
	transform: string;
	d: string;
	left: Point;
	right: Point;
	shade: string;
	shadeX: number;
	face: {
		expression: Expression;
		open: number;
		look: Point;
		scale: number;
		gap: number;
		eye: number;
		cheeks: number;
		grin: boolean;
	};
	arms?: { left: number; right: number };
	/** The lantern's angle against the ghost's own turn, so it hangs straight down */
	hang: number;
	wiggle: number;
	flicker: number;
	sparkles: { x: number; y: number; size: number; opacity: number }[];
};

const TAU = Math.PI * 2;
const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));
const rand = (min: number, max: number) => min + Math.random() * (max - min);
const sign = () => (Math.random() < 0.5 ? -1 : 1);
const lerp = (a: Point, b: Point, k: number): Point => ({
	x: a.x + (b.x - a.x) * k,
	y: a.y + (b.y - a.y) * k,
});
const distance = (a: Point, b: Point) => Math.hypot(b.x - a.x, b.y - a.y);
const wrap = (angle: number) => Math.atan2(Math.sin(angle), Math.cos(angle));
const where = (w: Waypoint): Point =>
	w.around
		? {
				x: w.around.host.x + w.around.r * Math.cos(w.around.angle),
				y: w.around.host.y + w.around.r * Math.sin(w.around.angle),
			}
		: w;

// Centripetal Catmull-Rom: smooth through every waypoint, without the overshoot the uniform
// kind gets where a tight loop meets a long stretch.
function spline(p0: Point, p1: Point, p2: Point, p3: Point, u: number): Point {
	const t1 = Math.sqrt(distance(p0, p1)) || 1e-3;
	const t2 = t1 + (Math.sqrt(distance(p1, p2)) || 1e-3);
	const t3 = t2 + (Math.sqrt(distance(p2, p3)) || 1e-3);
	const t = t1 + (t2 - t1) * u;
	const a1 = lerp(p0, p1, t / t1);
	const a2 = lerp(p1, p2, (t - t1) / (t2 - t1));
	const a3 = lerp(p2, p3, (t - t2) / (t3 - t2));
	return lerp(lerp(a1, a2, t / t2), lerp(a2, a3, (t - t1) / (t3 - t1)), (t - t1) / (t2 - t1));
}

// A closed curve through the points, as cubic Béziers.
function smooth(points: Point[]): string {
	const n = points.length;
	const at = (i: number) => points[(i + n) % n]!;
	const f = (value: number) => value.toFixed(1);
	let d = `M${f(at(0).x)} ${f(at(0).y)}`;
	for (let i = 0; i < n; i++) {
		const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
		d += `C${f(p1.x + (p2.x - p0.x) / 6)} ${f(p1.y + (p2.y - p0.y) / 6)} ${f(p2.x - (p3.x - p1.x) / 6)} ${f(p2.y - (p3.y - p1.y) / 6)} ${f(p2.x)} ${f(p2.y)}`;
	}
	return `${d}Z`;
}

// The sheet around a head at the origin: the hem streams `tail` px behind, bending more the lower
// it goes, and ripples as `phase` moves.
function sheet(c: Character, tail: number, stretch: number, phase: number, t: number) {
	const length = c.length * stretch;
	const spine = (s: number) => tail * s * s + Math.sin(t * 2.1 + s * 2.4) * 1.1 * s;
	const half = (s: number) => c.head * (1 - c.taper * s) + c.flare * s ** 1.4;
	const edge = (s: number, side: number) => ({ x: spine(s) + side * half(s), y: length * s });
	const points: Point[] = [];
	for (let i = 0; i < 12; i++) {
		const a = Math.PI * (1 + i / 12);
		points.push({ x: c.head * Math.cos(a), y: c.head * Math.sin(a) });
	}
	for (let i = 0; i < 6; i++) points.push(edge(i / 6, 1));
	const samples = c.waves * 6;
	for (let i = 0; i <= samples; i++) {
		const u = i / samples;
		const ripple = c.wave * Math.sin(Math.PI * u) ** 0.5 * Math.sin(TAU * c.waves * u + phase);
		points.push({ x: spine(1) + half(1) * (1 - 2 * u), y: length + ripple });
	}
	for (let i = 5; i > 0; i--) points.push(edge(i / 6, -1));
	return { d: smooth(points), left: edge(0.42, -1), right: edge(0.42, 1) };
}

export class Flock {
	private ghosts: Ghost[];
	private width: number;
	private height: number;
	private time = 0;

	constructor(width: number, height: number) {
		this.width = width;
		this.height = height;
		const spots: [Kind, number][] = [
			["boo", 0.22],
			["lumen", 0.58],
			["baby", 0.52],
			["zip", 0.86],
		];
		this.ghosts = spots.map(([kind, at], i) => this.spawn(kind, at, i));
	}

	resize(width: number, height: number) {
		this.width = width;
		this.height = height;
	}

	private get band() {
		return {
			left: this.width * 0.04,
			right: this.width * 0.96,
			top: this.height - 165,
			bottom: this.height - 48,
		};
	}

	private inBand(p: Point, slack = 0): Point {
		const { left, right, top, bottom } = this.band;
		return { x: clamp(p.x, left, right), y: clamp(p.y, top - slack, bottom + slack) };
	}

	private spawn(kind: Kind, at: number, i: number): Ghost {
		const { top, bottom } = this.band;
		const start = this.inBand({
			x: this.width * at,
			y: bottom - (bottom - top) * (0.25 + 0.2 * i),
		});
		const ghost: Ghost = {
			kind,
			c: characters[kind],
			seed: i * 1.7,
			...start,
			vx: 0,
			vy: 0,
			ax: 0,
			carrot: start,
			path: [start, start],
			plan: [],
			u: 0,
			pace: 1,
			paceTarget: 1,
			heading: 0,
			looping: false,
			angle: 0,
			angleSpeed: 0,
			spin: 0,
			tail: 0,
			tailSpeed: 0,
			swing: 0,
			swingSpeed: 0,
			phase: i,
			face: "smile",
			lastFace: "smile",
			faceSince: -1,
			busyUntil: 0,
			nextTrick: 2 + i * 1.5,
			waveUntil: 0,
			greetedAt: -10,
			sparkles: [],
			lastSparkle: 0,
			history: [],
		};
		ghost.path.push(this.wander(ghost), this.wander(ghost));
		return ghost;
	}

	// The next stretch: onward from the last waypoint, turning a little, mostly sideways.
	private wander(g: Ghost): Waypoint {
		const from = where(g.path.at(-1)!);
		const before = where(g.path.at(-2) ?? from);
		const heading =
			distance(from, before) > 1
				? Math.atan2(from.y - before.y, from.x - before.x)
				: Math.random() * TAU;
		const turn = heading + rand(-1.1, 1.1);
		const reach = rand(110, 240);
		const { left, right } = this.band;
		let x = from.x + Math.cos(turn) * reach;
		// Turns back off the sides rather than piling up against them
		if (x < left || x > right) x = from.x - Math.cos(turn) * reach;
		return this.inBand({ x, y: from.y + Math.sin(turn) * reach * 0.3 + rand(-25, 25) });
	}

	// A loop-de-loop off the end of the path, to whichever side has room.
	private loop(g: Ghost): Waypoint[] {
		const end = where(g.path.at(-1)!);
		const before = where(g.path.at(-2)!);
		const length = distance(before, end) || 1;
		const dx = (end.x - before.x) / length;
		const dy = (end.y - before.y) / length;
		const r = rand(28, 40);
		const { left, right, top, bottom } = this.band;
		const first = sign();
		for (const dir of [first, -first]) {
			const cx = end.x - dir * dy * r;
			const cy = end.y + dir * dx * r;
			const fits =
				cx - r >= left && cx + r <= right && cy - r >= top - 30 && cy + r <= bottom + 20;
			if (!fits) continue;
			const a0 = Math.atan2(end.y - cy, end.x - cx);
			const points: Waypoint[] = [];
			for (let k = 1; k <= 8; k++) {
				const a = a0 + (dir * TAU * k) / 8;
				points.push({ x: cx + r * Math.cos(a), y: cy + r * Math.sin(a), trick: "loop" });
			}
			points.push(this.inBand({ x: end.x + dx * 100, y: end.y + dy * 100 }));
			return points;
		}
		return [];
	}

	// Once and a quarter round another ghost, keeping up as it moves.
	private orbit(g: Ghost, host: Ghost): Waypoint[] {
		const end = where(g.path.at(-1)!);
		const start = Math.atan2(end.y - host.y, end.x - host.x);
		const dir = sign();
		const points: Waypoint[] = [];
		for (let k = 1; k <= 10; k++) {
			const angle = start + (dir * TAU * 1.25 * k) / 10;
			points.push({ x: 0, y: 0, trick: "orbit", around: { host, angle, r: 52 } });
		}
		return points;
	}

	private schedule(g: Ghost) {
		const t = this.time;
		if (t >= g.busyUntil && g.paceTarget < 1) g.paceTarget = 1;
		if (t < g.nextTrick || t < g.busyUntil || g.plan.length > 0) return;
		if (g.kind === "lumen") {
			// Drifts to a stop, looks around, sets off again
			g.paceTarget = 0;
			g.busyUntil = t + rand(1.8, 3.2);
			g.nextTrick = g.busyUntil + rand(5, 9);
			return;
		}
		if (g.kind === "boo" && Math.random() < 0.55) {
			g.paceTarget = 0.15;
			g.spin += 360 * sign();
			g.busyUntil = t + 1.4;
			g.nextTrick = t + rand(7, 11);
			return;
		}
		const lumen = this.ghosts.find((other) => other.kind === "lumen");
		const circling = g.kind === "zip" && lumen && Math.random() < 0.4;
		g.plan.push(...(circling ? this.orbit(g, lumen) : this.loop(g)));
		g.nextTrick = t + (g.kind === "zip" ? rand(3.5, 6) : rand(7, 11));
	}

	private travel(g: Ghost, dt: number) {
		this.schedule(g);
		g.pace += (g.paceTarget - g.pace) * Math.min(1, dt * 2.5);
		const span = Math.max(12, distance(where(g.path[1]!), where(g.path[2]!)));
		g.u += (g.c.speed * g.pace * dt) / span;
		while (g.u >= 1) {
			g.u -= 1;
			g.path.shift();
			g.path.push(g.plan.shift() ?? this.wander(g));
		}
		const [p0, p1, p2, p3] = g.path.map(where);
		g.carrot = spline(p0!, p1!, p2!, p3!, g.u);
		g.looping = g.path[2]!.trick === "loop";
	}

	private follow(g: Ghost) {
		const t = this.time;
		const lumen = this.ghosts.find((other) => other.kind === "lumen");
		if (!lumen) return;
		const past = lumen.history.find((h) => h.at >= t - 0.55) ?? lumen;
		const behind = lumen.vx >= 0 ? -1 : 1;
		g.carrot = { x: past.x + behind * 46, y: past.y + 16 + Math.sin(t * 1.7) * 6 };
	}

	private mood(g: Ghost): Expression {
		const t = this.time;
		if (g.looping || g.path[2]?.trick === "orbit" || t < g.waveUntil) return "joy";
		if (g.kind === "boo" && t < g.busyUntil) return "joy";
		if (g.kind === "lumen" && t < g.busyUntil) return "calm";
		if (g.kind === "baby") {
			const lumen = this.ghosts.find((other) => other.kind === "lumen");
			return lumen && t < lumen.busyUntil ? "joy" : "smile";
		}
		if (g.kind === "zip" && Math.hypot(g.vx, g.vy) > 150) return "wow";
		return "smile";
	}

	step(dt: number) {
		if (dt <= 0) return;
		this.time += dt;
		const t = this.time;
		for (const g of this.ghosts) {
			const before = g.carrot;
			if (g.kind === "baby") this.follow(g);
			else this.travel(g, dt);

			// Loops turn the ghost all the way round with its heading; afterwards the turn
			// rounds to whole circles and settles
			const cvx = (g.carrot.x - before.x) / dt;
			const cvy = (g.carrot.y - before.y) / dt;
			if (Math.hypot(cvx, cvy) > 5) {
				const heading = Math.atan2(cvy, cvx);
				if (g.looping) g.spin += (wrap(heading - g.heading) * 180) / Math.PI;
				g.heading = heading;
			}
			if (!g.looping) g.spin = Math.round(g.spin / 360) * 360;

			const { stiffness, damping } = g.c;
			const ax = (g.carrot.x - g.x) * stiffness - g.vx * damping;
			const ay = (g.carrot.y - g.y) * stiffness - g.vy * damping;
			g.vx += ax * dt;
			g.vy += ay * dt;
			g.x += g.vx * dt;
			g.y += g.vy * dt;
			g.ax = ax;

			const target = clamp(g.vx * g.c.lean, -20, 20) + g.spin;
			g.angleSpeed += ((target - g.angle) * 45 - g.angleSpeed * 11) * dt;
			g.angle += g.angleSpeed * dt;
			if (g.spin !== 0 && !g.looping && Math.abs(target - g.angle) < 0.5) {
				g.angle -= g.spin;
				g.spin = 0;
			}

			const { lx } = this.local(g);
			const streaming = clamp(-lx * g.c.trail, -18, 18);
			g.tailSpeed += ((streaming - g.tail) * 55 - g.tailSpeed * 7) * dt;
			g.tail += g.tailSpeed * dt;
			g.phase += (1.4 - lx * 0.07) * dt;
			g.swingSpeed += (-g.swing * 38 - g.swingSpeed * 3.2 + ax * 1.6) * dt;
			g.swing = clamp(g.swing + g.swingSpeed * dt, -75, 75);

			const mood = this.mood(g);
			if (mood !== g.face) {
				g.lastFace = g.face;
				g.face = mood;
				g.faceSince = t;
			}

			if (g.kind === "lumen") {
				g.history.push({ x: g.x, y: g.y, at: t });
				while (g.history.length > 0 && g.history[0]!.at < t - 1) g.history.shift();
			}
			if (g.kind === "zip") this.sparkle(g);
		}
		this.greet();
	}

	// Ghosts that meet wave hello; Lumen and its little one are always together, so they don't.
	private greet() {
		const t = this.time;
		for (const [i, a] of this.ghosts.entries()) {
			for (const b of this.ghosts.slice(i + 1)) {
				const kinds = new Set([a.kind, b.kind]);
				const family = kinds.has("lumen") && kinds.has("baby");
				if (family || distance(a, b) > 70) continue;
				if (t < a.greetedAt + 6 || t < b.greetedAt + 6) continue;
				for (const g of [a, b]) {
					g.waveUntil = t + 1.3;
					g.greetedAt = t;
				}
			}
		}
	}

	private sparkle(g: Ghost) {
		const t = this.time;
		g.sparkles = g.sparkles.filter((s) => t - s.at < 0.7);
		if (Math.hypot(g.vx, g.vy) < 70 || t - g.lastSparkle < 0.05) return;
		g.lastSparkle = t;
		// From the tip of the tail
		const rad = (g.angle * Math.PI) / 180;
		const tip = { x: g.tail * g.c.size, y: g.c.length * 0.9 * g.c.size };
		g.sparkles.push({
			x: g.x + tip.x * Math.cos(rad) - tip.y * Math.sin(rad) + rand(-4, 4),
			y: g.y + tip.x * Math.sin(rad) + tip.y * Math.cos(rad) + rand(-4, 4),
			at: t,
		});
	}

	// The ghost's velocity as it sees it, turned with its body
	private local(g: Ghost) {
		const rad = (g.angle * Math.PI) / 180;
		return {
			lx: g.vx * Math.cos(rad) + g.vy * Math.sin(rad),
			ly: -g.vx * Math.sin(rad) + g.vy * Math.cos(rad),
		};
	}

	poses(): GhostPose[] {
		const t = this.time;
		const order: Kind[] = ["baby", "lumen", "boo", "zip"];
		return order.flatMap((kind) => {
			const g = this.ghosts.find((ghost) => ghost.kind === kind);
			return g ? [this.pose(g, t)] : [];
		});
	}

	private pose(g: Ghost, t: number): GhostPose {
		const { c } = g;
		const { lx, ly } = this.local(g);
		const bob = c.bob.size * Math.sin(t * c.bob.speed + g.seed);
		const breath = Math.sin(t * 2.3 + g.seed);
		const sx = c.size * (1 - 0.02 * breath);
		const sy = c.size * (1 + 0.025 * breath);
		const stretch = 1 + clamp(-ly * 0.0022, -0.12, 0.2);

		// Expressions change with a quick blink; the ordinary blinks come on their own timer
		const change = (t - g.faceSince) / 0.18;
		const blinking = (t + g.seed * 1.7) % c.face.blinkEvery;
		const blink = blinking < 0.16 ? 1 - Math.sin((Math.PI * blinking) / 0.16) * 0.9 : 1;
		const open = Math.min(blink, change < 1 ? Math.abs(Math.cos(Math.PI * change)) : 1);
		const expression = change < 0.5 ? g.lastFace : g.face;

		const resting = g.kind === "lumen" && t < g.busyUntil;
		const sweep = clamp(-lx * 0.18, -28, 28);
		const flap = Math.sin(t * 2.6 + g.seed) * 8;
		const waving = t < g.waveUntil || (g.kind === "boo" && t < g.busyUntil);

		return {
			kind: g.kind,
			transform: `translate(${g.x.toFixed(1)} ${(g.y + bob).toFixed(1)}) rotate(${g.angle.toFixed(1)}) scale(${sx.toFixed(3)} ${sy.toFixed(3)})`,
			...sheet(c, g.tail, stretch, g.phase, t),
			shade: c.shade,
			shadeX: clamp(-g.tail * 0.25, -5, 5),
			face: {
				expression,
				open,
				look: resting
					? { x: Math.sin(t * 0.9) * 2.4, y: -0.6 }
					: { x: clamp(lx * 0.025, -2.5, 2.5), y: clamp(ly * 0.02, -1.5, 1.5) },
				scale: c.head / 14,
				gap: c.face.gap,
				eye: c.face.eye,
				cheeks: c.face.cheeks,
				grin: c.face.grin,
			},
			arms: c.arms
				? {
						left: -25 + sweep - flap,
						right: waving ? -60 + Math.sin(t * 16) * 26 : 25 + sweep + flap,
					}
				: undefined,
			hang: g.swing - g.angle,
			wiggle: g.swing * 0.4 + Math.sin(t * 3 + g.seed) * 3,
			flicker: 0.75 + 0.25 * Math.sin(t * 13) * Math.sin(t * 7.3),
			sparkles: g.sparkles.map((s) => {
				const age = (t - s.at) / 0.7;
				return { x: s.x, y: s.y + age * 6, size: 1 - age * 0.7, opacity: 1 - age };
			}),
		};
	}
}
