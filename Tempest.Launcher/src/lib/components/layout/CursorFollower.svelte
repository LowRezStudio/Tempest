<script lang="ts">
	import { onMount } from "svelte";
	import type { Snippet } from "svelte";

	interface Look {
		x: number;
		y: number;
		away: boolean;
		/**
		 * Wide-eyed while held or flying, "ouch" right after a hard hit, dizzy after a wild ride,
		 * happy while petted or patted
		 */
		face: "calm" | "wide" | "ouch" | "dizzy" | "happy";
	}

	interface Props {
		/** Hopping after the cursor, or waddling along the ground */
		gait: "hop" | "waddle";
		/** The art's width, in px */
		width: number;
		/**
		 * Stroking it with the cursor pets it, clicking pats it and dragging throws it; it bounces
		 * off the walls, and resizing or moving the window knocks it around. Too many hard throws
		 * blow it up: it leaves a tombstone and drops back in
		 */
		throwable?: boolean;
		/** The color of its insides, sprayed around when it blows up */
		goo?: string;
		/** The colors of the chunks it blows apart into */
		debris?: string[];
		/**
		 * The radius of a cobweb in the bottom-right corner, in px; flying into it gets it stuck
		 * until it wriggles free or is pulled out
		 */
		web?: number;
		/**
		 * The art; `look` points toward the cursor, -1 to 1 on each axis, for its eyes, says
		 * whether the cursor left the window and which face to pull
		 */
		children: Snippet<[Look]>;
	}

	let {
		gait,
		width,
		throwable = false,
		goo = "currentColor",
		debris = [goo],
		web = 0,
		children,
	}: Props = $props();

	const MARGIN = 12;
	const HOP_DISTANCE = 80;
	const WADDLE_SPEED = 150;
	const GRAVITY = 2600;
	const BOUNCE = 0.6;
	const FLOOR_BOUNCE = 0.45;
	const FRICTION = 5;
	const MAX_SPEED = 3000;
	// Slower than this it comes to rest instead of bouncing, and isn't knocked off its feet
	const SETTLE_SPEED = 160;
	// Winces at hits harder than this, and three in one flight leave it dizzy
	const HARD_HIT = 1300;
	const STUCK_TIME = 3500;
	// Share of the window's change in speed it keeps; the floor drags the rest along
	const MOMENTUM = 0.7;
	// This many throws at least this fast within the window and it blows up when it hits something
	const HARD_THROW = 1800;
	const THROWS_TO_BLOW = 4;
	const THROWS_WINDOW = 15000;
	const MAX_TOMBSTONES = 6;
	const DEG = Math.PI / 180;

	let layer = $state<HTMLDivElement>();
	let sprite = $state<HTMLDivElement>();
	let interactive = $state(false);
	let held = $state(false);
	let dead = $state(false);
	let primed = $state(false);
	let airborne = $state(false);
	// The strands holding it while it's stuck in the cobweb
	let threads = $state<{ x: number; y: number; ends: { x: number; y: number }[] }>();
	// Offset from its resting spot in the bottom-left corner; scale and rotation are about its center
	const pose = $state({ tx: -1000, ty: 0, angle: 0, sx: 1, sy: 1, facing: 1, opacity: 1 });
	const look = $state<Look>({ x: 0, y: 0, away: false, face: "calm" });
	let bursts = $state<{ id: number; x: number; y: number; angle: number; start: number }[]>([]);
	// Planted on the floor, ceiling or wall it blew up against, `at` along it
	interface Grave {
		id: number;
		side: "floor" | "ceiling" | "left" | "right";
		at: number;
	}
	let tombstones = $state<Grave[]>([]);
	let ghost = $state<{ x: number; y: number }>();
	let splatter = $state<{
		opacity: number;
		drops: {
			x: number;
			y: number;
			vx: number;
			vy: number;
			r: number;
			landed: boolean;
			// Chunks tumble as they fly and come in their own colors; drops of goo don't
			chunk?: { angle: number; spin: number; color: string };
		}[];
	}>();
	let blast = $state<{
		x: number;
		y: number;
		puffs: { dx: number; dy: number; size: number; delay: number }[];
	}>();
	let hearts = $state<{ id: number; x: number; y: number; drift: number; start: number }[]>([]);
	const soul: Look = { x: 0, y: -1, away: false, face: "calm" };

	const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

	// Its base on the surface, standing up out of it
	function graveSpot({ side, at }: Grave) {
		const along = `min(${Math.max(20, at)}px, calc(100% - 20px))`;
		if (side === "floor") return `left: ${along}; top: 100%;`;
		if (side === "ceiling") return `left: ${along}; top: 0; transform: rotate(180deg);`;
		if (side === "left") return `left: 0; top: ${along}; transform: rotate(90deg);`;
		return `left: 100%; top: ${along}; transform: rotate(-90deg);`;
	}

	onMount(() => {
		const ground = layer;
		const art = sprite;
		if (!ground || !art) return;
		// Starts in the bottom-right corner and stays there when motion is reduced
		pose.tx = ground.clientWidth - width - MARGIN;
		if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
		interactive = throwable;

		let pointer: { x: number; y: number } | undefined;
		const onMove = (e: PointerEvent) => {
			pointer = { x: e.clientX, y: e.clientY };
			look.away = false;
		};
		// Out of the window: it stays where it is instead of chasing the last spot the cursor was.
		const onOut = (e: MouseEvent) => {
			if (e.relatedTarget || grab) return;
			pointer = undefined;
			look.away = true;
		};
		window.addEventListener("pointermove", onMove);
		window.addEventListener("mouseout", onOut);

		const hx = width / 2;
		let hy = art.offsetHeight / 2;
		let W = ground.clientWidth;
		let H = ground.clientHeight;
		// Its center from the layer's top-left corner, velocity in px/s, spin in deg/s
		let x = pose.tx + hx;
		let y = H - hy;
		let vx = 0;
		let vy = 0;
		let angle = 0;
		let spin = 0;
		let mode: "ground" | "held" | "flying" | "stuck" | "dead" = "ground";

		let hop:
			| { from: number; to: number; height: number; start: number; duration: number }
			| undefined;
		let landedAt = 0;
		let stride = 0;
		let lift = 0;
		let tilt = 0;
		let squashX = 1;
		let squashY = 1;

		let splat: { amount: number; axis: "x" | "y"; side: number; start: number } | undefined;
		let hardHits = 0;
		let ouchUntil = 0;
		let happyUntil = 0;
		let dizzySince = 0;
		let dizzyUntil = 0;
		let burstId = 0;
		let diedAt = 0;
		let respawnedAt = -Infinity;
		let tombstoneId = 0;
		let hardThrows: number[] = [];
		let detonate = false;
		let wasInWeb = false;
		let stuckSince = 0;
		// From the bottom-right corner, so it moves with the cobweb when the window's resized
		let anchor = { dx: 0, dy: 0 };
		let strands: { dx: number; dy: number }[] = [];
		let blownAgainst: Grave["side"] = "floor";
		// Builds up while the cursor strokes it, and wears off
		let petting = 0;
		let lastPointer: { x: number; y: number } | undefined;
		let heartId = 0;
		let heartAt = 0;

		let grab:
			| {
					id: number;
					dragging: boolean;
					at: number;
					startX: number;
					startY: number;
					clientX: number;
					clientY: number;
					// The grabbed spot in its own unrotated frame, so it hangs from there
					lx: number;
					ly: number;
			  }
			| undefined;
		let pivot: { x: number; y: number; vx: number; vy: number } | undefined;

		let origin: { x: number; y: number } | undefined;
		let windowVX = 0;
		let windowVY = 0;
		let size: { w: number; h: number } | undefined;
		let wallV = 0;
		let floorV = 0;

		const inWeb = () => web > 0 && Math.hypot(W - x, H - y) < web * 0.95;

		function hopTowards(now: number, target: number, above: number, idle: boolean) {
			if (hop) {
				const t = Math.min(1, (now - hop.start) / hop.duration);
				const arc = Math.sin(Math.PI * t);
				x = hop.from + (hop.to - hop.from) * t;
				lift = hop.height * 4 * t * (1 - t);
				squashX = 1 - 0.08 * arc;
				squashY = 1 + 0.12 * arc;
				tilt = Math.sign(hop.to - hop.from) * 8 * arc;
				if (t === 1) {
					hop = undefined;
					landedAt = now;
				}
				return;
			}
			const squash = Math.max(0, 1 - (now - landedAt) / 140);
			lift = 0;
			tilt = 0;
			squashX = 1 + 0.12 * squash;
			squashY = 1 - 0.16 * squash;
			const dx = target - x;
			const travelling = Math.abs(dx) > 8;
			if (idle || now - landedAt < (travelling ? 140 : 800)) return;
			if (travelling) pose.facing = Math.sign(dx);
			// Under the cursor it keeps jumping up at it, higher the closer it is
			const height = travelling ? 26 : above < 260 ? clamp(above - 50, 10, 90) : 12;
			hop = {
				from: x,
				to: travelling ? x + Math.sign(dx) * Math.min(Math.abs(dx), HOP_DISTANCE) : x,
				height,
				start: now,
				duration: 300 + height * 2,
			};
		}

		function waddleTowards(dt: number, target: number) {
			const dx = target - x;
			if (Math.abs(dx) > 4) {
				x += Math.sign(dx) * Math.min(Math.abs(dx), WADDLE_SPEED * dt);
				pose.facing = Math.sign(dx);
				stride += dt * 10;
				tilt = Math.sin(stride) * 7;
				lift = Math.abs(Math.sin(stride)) * 3;
			} else {
				tilt *= 0.85;
				lift *= 0.85;
			}
		}

		function walk(now: number, dt: number, rect: DOMRect) {
			const dizzy = now < dizzyUntil;
			const target =
				pointer && !dizzy ? clamp(pointer.x - rect.left, MARGIN + hx, W - MARGIN - hx) : x;
			// Holds still with the cursor on it, to be clicked or petted
			const pointedAt =
				throwable &&
				!!pointer &&
				Math.abs(pointer.x - rect.left - x) < hx &&
				Math.abs(pointer.y - rect.top - y) < hy;
			if (pointedAt && pointer && lastPointer) {
				const stroke = Math.hypot(pointer.x - lastPointer.x, pointer.y - lastPointer.y);
				petting = Math.min(1.5, petting + stroke / 120);
			}
			if (petting > 0.6 && now - heartAt > 320) {
				heartAt = now;
				hearts.push({
					id: heartId++,
					x: x + (Math.random() - 0.5) * hx,
					y: y - hy * 0.8,
					drift: (Math.random() - 0.5) * 30,
					start: now,
				});
			}
			if (dizzy) {
				hop = undefined;
				lift = 0;
				squashX = 1;
				squashY = 1;
				// Wobbles on the spot, steadier as it comes to
				tilt =
					Math.sin((now - dizzySince) / 80) * 10 * Math.min(1, (dizzyUntil - now) / 500);
			} else if (gait === "hop") {
				hopTowards(
					now,
					target,
					pointer ? rect.bottom - pointer.y : Infinity,
					// Stays put while being petted, even when a stroke runs off its edge
					!pointer || !!grab || pointedAt || petting > 0.2,
				);
			} else {
				waddleTowards(dt, target);
			}
			const inset = throwable ? hx : hx + MARGIN;
			x = clamp(x, inset, W - inset);
			y = H - hy - lift;
			// Leans into the window's pushes too small to knock it over
			angle = tilt + clamp(vx * 0.06, -12, 12);
			wasInWeb = inWeb();
		}

		function takeOff(now: number) {
			if (hop) {
				const t = Math.min(1, (now - hop.start) / hop.duration);
				vx += ((hop.to - hop.from) / hop.duration) * 1000;
				vy -= ((hop.height * 4 * (1 - 2 * t)) / hop.duration) * 1000;
				hop = undefined;
			}
			mode = "flying";
			hardHits = 0;
			dizzyUntil = 0;
		}

		function land(now: number) {
			mode = "ground";
			angle = 0;
			vx = 0;
			vy = 0;
			spin = 0;
			landedAt = now;
			if (hardHits >= 3) {
				dizzySince = now;
				dizzyUntil = now + 1800;
			}
		}

		// A pat: it squishes, beams and puffs out a few hearts
		function tap() {
			const now = performance.now();
			landedAt = now;
			happyUntil = now + 700;
			for (let i = -2; i <= 2; i++) {
				hearts.push({
					id: heartId++,
					x: x + i * hx * 0.3,
					y: y - hy * 0.7,
					drift: i * 16 + (Math.random() - 0.5) * 8,
					start: now,
				});
			}
		}

		// Blown to bits with its insides sprayed around; its ghost floats off and a tombstone
		// comes up under it
		function explode(now: number) {
			mode = "dead";
			dead = true;
			primed = false;
			detonate = false;
			diedAt = now;
			splat = undefined;
			const drops = Array.from({ length: 26 }, (_, i) => {
				const heading = Math.random() * 360 * DEG;
				const speed = 300 + Math.random() * 700;
				const chunky = i < 12;
				return {
					x: x + Math.cos(heading) * hx * 0.4,
					y: y + Math.sin(heading) * hy * 0.4,
					vx: Math.cos(heading) * speed + vx * 0.3,
					vy: Math.sin(heading) * speed - 300 + vy * 0.3,
					r: chunky ? 3 + Math.random() * 4 : 1.5 + Math.random() * 2,
					landed: false,
					chunk: chunky
						? {
								angle: Math.random() * 360,
								spin: (Math.random() * 2 - 1) * 900,
								color: debris[i % debris.length],
							}
						: undefined,
				};
			});
			splatter = { opacity: 1, drops };
			const puffs = Array.from({ length: 6 }, () => ({
				dx: (Math.random() * 2 - 1) * 50,
				dy: -20 - Math.random() * 50,
				size: 24 + Math.random() * 24,
				delay: Math.random() * 120,
			}));
			blast = { x, y, puffs };
			vx = 0;
			vy = 0;
			spin = 0;
			ghost = { x: x - hx, y: y - hy };
			const sideways = blownAgainst === "left" || blownAgainst === "right";
			tombstones.push({ id: tombstoneId++, side: blownAgainst, at: sideways ? y : x });
			if (tombstones.length > MAX_TOMBSTONES) tombstones.shift();
			blownAgainst = "floor";
		}

		function lieDead(now: number, dt: number) {
			angle += (Math.round(angle / 360) * 360 - angle) * Math.min(1, 25 * dt);
			x = clamp(x, hx, W - hx);
			y = H - hy;
			if (splatter) {
				for (const drop of splatter.drops) {
					if (drop.landed) continue;
					drop.vy += GRAVITY * dt;
					drop.x += drop.vx * dt;
					drop.y += drop.vy * dt;
					if (drop.chunk) drop.chunk.angle += drop.chunk.spin * dt;
					if (drop.x < 0 || drop.x > W) {
						drop.x = clamp(drop.x, 0, W);
						drop.vx *= -0.3;
					}
					if (drop.y < 0) {
						drop.y = 0;
						drop.vy *= -0.3;
					}
					if (drop.y < H) continue;
					drop.y = H;
					drop.landed = true;
				}
				splatter.opacity = clamp((1800 - (now - diedAt)) / 500, 0, 1);
			}
			if (now - diedAt > 1800) respawn(now);
		}

		// Drops back in from the top, away from its grave
		function respawn(now: number) {
			const lo = MARGIN + hx;
			const hi = W - MARGIN - hx;
			const left = x - 160 - lo;
			const right = hi - x - 160;
			x = clamp(
				right >= left
					? x + 160 + Math.random() * Math.max(0, right)
					: lo + Math.random() * Math.max(0, left),
				lo,
				hi,
			);
			y = hy;
			vx = 0;
			vy = 0;
			spin = 0;
			angle = 0;
			pose.facing = x > W / 2 ? -1 : 1;
			mode = "flying";
			dead = false;
			wasInWeb = false;
			ghost = undefined;
			splatter = undefined;
			blast = undefined;
			hardHits = 0;
			respawnedAt = now;
		}

		function hit(speed: number, axis: "x" | "y", side: number, now: number) {
			if (primed) {
				detonate = true;
				if (axis === "x") blownAgainst = side > 0 ? "right" : "left";
				else blownAgainst = side > 0 ? "floor" : "ceiling";
				return;
			}
			// Squashes flat against what it hit, then jiggles back
			splat = { amount: Math.min(0.45, speed / 3500 + 0.06), axis, side, start: now };
			if (speed < HARD_HIT) return;
			hardHits++;
			ouchUntil = now + 380;
			bursts.push({
				id: burstId++,
				x: axis === "x" ? (side > 0 ? W : 0) : x,
				y: axis === "y" ? (side > 0 ? H : 0) : y,
				angle: axis === "x" ? (side > 0 ? 180 : 0) : side > 0 ? -90 : 90,
				start: now,
			});
		}

		// Spins it the way scraping along a wall at `speed` rolls it
		function rub(speed: number) {
			spin = spin * 0.4 + (speed / hy / DEG) * 0.6;
		}

		// Keeps it inside, bouncing off the walls (the right one and the floor move with resizes);
		// true when it's resting on the floor
		function bounce(now: number) {
			if (x - hx < 0) {
				x = hx;
				if (vx < 0) {
					hit(-vx, "x", -1, now);
					rub(vy);
					vx = -vx * BOUNCE;
				}
			}
			if (x + hx > W) {
				x = W - hx;
				const into = vx - wallV;
				if (into > 0) {
					hit(into, "x", 1, now);
					rub(-vy);
					vx = wallV - into * BOUNCE;
				}
			}
			if (y - hy < 0) {
				y = hy;
				if (vy < 0) {
					hit(-vy, "y", -1, now);
					rub(-vx);
					vy = -vy * BOUNCE;
				}
			}
			if (y + hy < H) return false;
			y = H - hy;
			const into = vy - floorV;
			if (into > SETTLE_SPEED) {
				hit(into, "y", 1, now);
				rub(vx);
				vy = floorV - into * FLOOR_BOUNCE;
				return false;
			}
			if (into < 0) return false;
			vy = floorV;
			return true;
		}

		function fly(dt: number, now: number) {
			vy += GRAVITY * dt;
			const speed = Math.hypot(vx, vy);
			if (speed > MAX_SPEED) {
				vx *= MAX_SPEED / speed;
				vy *= MAX_SPEED / speed;
			}
			x += vx * dt;
			y += vy * dt;
			angle += spin * dt;
			const resting = bounce(now);
			if (detonate || (primed && resting)) return explode(now);
			// Flying into the cobweb, not rolling along the floor under it, gets it stuck
			const webbed = inWeb();
			if (webbed && !wasInWeb && y + hy < H - 6) return stick(now);
			wasInWeb = webbed;
			if (!resting) return;
			vx *= Math.exp(-FRICTION * dt);
			if (Math.abs(vx) > 30) {
				// Rolls along the floor
				spin += (vx / hy / DEG - spin) * Math.min(1, 12 * dt);
				return;
			}
			// Then rocks back onto its bottom
			const upright = Math.round(angle / 360) * 360;
			spin += (-(angle - upright) * 160 - spin * 9) * dt;
			if (Math.abs(angle - upright) < 1 && Math.abs(spin) < 20) land(now);
		}

		function stick(now: number) {
			mode = "stuck";
			stuckSince = now;
			wasInWeb = true;
			spin = 0;
			// Plows a little way in; the net gives under it and springs it back there
			const speed = Math.hypot(vx, vy) || 1;
			const plow = Math.min(30, speed * 0.03);
			const ax = clamp(x + (vx / speed) * plow, hx, W - hx);
			const ay = clamp(y + (vy / speed) * plow, hy, H - hy);
			anchor = { dx: W - ax, dy: H - ay };
			if (speed > 700) {
				vx *= 700 / speed;
				vy *= 700 / speed;
			}
			// Strands out to the wall and the floor, and down into the corner
			const reach = (d: number) => clamp(d, 0, web);
			strands = [
				{ dx: 0, dy: reach(anchor.dy - 18) },
				{ dx: 0, dy: reach(anchor.dy + 24) },
				{ dx: reach(anchor.dx - 16), dy: 0 },
				{ dx: reach(anchor.dx + 28), dy: 0 },
				{ dx: 0, dy: 0 },
			];
		}

		function dangle(now: number, dt: number) {
			const ax = W - anchor.dx;
			const ay = H - anchor.dy;
			vx += (-(x - ax) * 140 - vx * 7) * dt;
			vy += (-(y - ay) * 140 - vy * 7) * dt;
			x = clamp(x + vx * dt, hx, W - hx);
			y = clamp(y + vy * dt, hy, H - hy);
			angle += (Math.round(angle / 360) * 360 - angle) * Math.min(1, 6 * dt);
			threads = {
				x,
				y,
				ends: strands.map((end) => ({ x: W - end.dx, y: H - end.dy })),
			};
			if (now - stuckSince > STUCK_TIME) breakFree(now);
		}

		// Wriggles out and drops
		function breakFree(now: number) {
			mode = "flying";
			threads = undefined;
			vx = -80;
			vy = 150;
			splat = { amount: -0.2, axis: "y", side: -1, start: now };
		}

		function lever() {
			const cos = Math.cos(angle * DEG);
			const sin = Math.sin(angle * DEG);
			if (!grab) return { rx: 0, ry: 0 };
			return {
				rx: -(grab.lx * cos - grab.ly * sin),
				ry: -(grab.lx * sin + grab.ly * cos),
			};
		}

		function hang(now: number, dt: number, rect: DOMRect) {
			if (!grab) return;
			const px = grab.clientX - rect.left;
			const py = grab.clientY - rect.top;
			pivot ??= { x: px, y: py, vx: 0, vy: 0 };
			const ease = 1 - Math.exp(-dt / 0.03);
			const pvx = pivot.vx + ((px - pivot.x) / dt - pivot.vx) * ease;
			const pvy = pivot.vy + ((py - pivot.y) / dt - pivot.vy) * ease;
			// Swings from the grabbed spot like a pendulum, pushed around as the cursor speeds up
			// and slows down
			const { rx, ry } = lever();
			const torque = rx * (GRAVITY * dt - (pvy - pivot.vy)) + ry * (pvx - pivot.vx);
			const inertia = rx * rx + ry * ry + (hy * hy) / 2;
			spin = ((spin * DEG + torque / inertia) / DEG) * Math.exp(-2 * dt);
			angle += spin * dt;
			const arm = lever();
			const tx = clamp(px + arm.rx, hx, W - hx);
			const ty = clamp(py + arm.ry, hy, H - hy);
			// Caught out of the air, it's pulled into the hand rather than jumping there
			const snap = now - grab.at < 150 ? Math.min(1, 35 * dt) : 1;
			x += (tx - x) * snap;
			y += (ty - y) * snap;
			pivot = { x: px, y: py, vx: pvx, vy: pvy };
		}

		function release(toss: boolean) {
			if (!grab) return;
			const { rx, ry } = lever();
			const { dragging } = grab;
			grab = undefined;
			held = false;
			if (!dragging) {
				if (toss) tap();
				return;
			}
			// Flies off with the cursor's speed plus its own swing
			const swing = spin * DEG;
			vx = (toss ? (pivot?.vx ?? 0) : 0) - ry * swing;
			vy = (toss ? (pivot?.vy ?? 0) : 0) + rx * swing;
			spin += vx * 0.3;
			pivot = undefined;
			mode = "flying";
			hardHits = 0;
			// Let go in the cobweb, it sticks there again
			wasInWeb = false;
			if (Math.hypot(vx, vy) < HARD_THROW) return;
			// One hard throw too many and it blows up on whatever it hits next
			const now = performance.now();
			hardThrows = [...hardThrows.filter((at) => now - at < THROWS_WINDOW), now];
			if (hardThrows.length < THROWS_TO_BLOW) return;
			hardThrows = [];
			primed = true;
		}

		const onDown = (e: PointerEvent) => {
			if (e.button !== 0 || grab) return;
			e.preventDefault();
			art.setPointerCapture(e.pointerId);
			const rect = ground.getBoundingClientRect();
			const dx = e.clientX - rect.left - x;
			const dy = e.clientY - rect.top - y;
			const cos = Math.cos(angle * DEG);
			const sin = Math.sin(angle * DEG);
			let lx = dx * cos + dy * sin;
			let ly = dy * cos - dx * sin;
			const catching = mode === "flying" || mode === "stuck";
			if (catching) {
				// Grabbed by the catch zone around it: held near its middle, not from thin air
				const reach = Math.hypot(lx, ly);
				const most = hy * 0.6;
				if (reach > most) {
					lx *= most / reach;
					ly *= most / reach;
				}
			}
			grab = {
				id: e.pointerId,
				dragging: false,
				at: e.timeStamp,
				startX: e.clientX,
				startY: e.clientY,
				clientX: e.clientX,
				clientY: e.clientY,
				lx,
				ly,
			};
			// No need to move first: in the air or the cobweb it's caught the moment it's touched
			if (catching) pickUp(e.timeStamp, e.clientX, e.clientY);
		};
		const onDrag = (e: PointerEvent) => {
			if (e.pointerId !== grab?.id) return;
			grab.clientX = e.clientX;
			grab.clientY = e.clientY;
			if (grab.dragging) return;
			if (Math.hypot(e.clientX - grab.startX, e.clientY - grab.startY) < 5) return;
			pickUp(e.timeStamp, e.clientX, e.clientY);
		};

		const pickUp = (now: number, clientX: number, clientY: number) => {
			if (!grab) return;
			grab.dragging = true;
			held = true;
			hop = undefined;
			spin = 0;
			threads = undefined;
			if (mode === "flying" || mode === "stuck") {
				// Its momentum swings it around the hand, and it squishes into it
				const { rx, ry } = lever();
				const inertia = rx * rx + ry * ry + (hy * hy) / 2;
				spin = clamp((rx * vy - ry * vx) / inertia / DEG, -720, 720);
				const speed = Math.hypot(vx, vy);
				const sideways = Math.abs(vx) >= Math.abs(vy);
				splat = {
					amount: Math.min(0.4, speed / 4000 + 0.12),
					axis: sideways ? "x" : "y",
					side: Math.sign(sideways ? vx : vy) || 1,
					start: now,
				};
				if (speed > 600) {
					const rect = ground.getBoundingClientRect();
					bursts.push({
						id: burstId++,
						x: clientX - rect.left,
						y: clientY - rect.top,
						angle: Math.atan2(-vy, -vx) / DEG,
						start: now,
					});
				}
			}
			mode = "held";
		};
		const onUp = (e: PointerEvent) => {
			if (e.pointerId === grab?.id) release(true);
		};
		const onCancel = (e: PointerEvent) => {
			if (e.pointerId === grab?.id) release(false);
		};
		if (throwable) {
			art.addEventListener("pointerdown", onDown);
			art.addEventListener("pointermove", onDrag);
			art.addEventListener("pointerup", onUp);
			art.addEventListener("pointercancel", onCancel);
			art.addEventListener("lostpointercapture", onCancel);
		}

		// How much the layer moving around the screen shoves it, opposite the way it speeds up
		function windowPush(dt: number, rect: DOMRect) {
			const ease = 1 - Math.exp(-dt / 0.06);
			const ox = window.screenX + rect.left;
			const oy = window.screenY + rect.top;
			let push = { x: 0, y: 0 };
			if (origin) {
				const dx = ox - origin.x;
				const dy = oy - origin.y;
				// Bigger leaps are the window maximizing, restoring or coming back from the tray
				if (Math.hypot(dx, dy) > 200) {
					windowVX = 0;
					windowVY = 0;
				} else {
					const nvx = windowVX + (dx / dt - windowVX) * ease;
					const nvy = windowVY + (dy / dt - windowVY) * ease;
					push = { x: (windowVX - nvx) * MOMENTUM, y: (windowVY - nvy) * MOMENTUM };
					windowVX = nvx;
					windowVY = nvy;
				}
			}
			origin = { x: ox, y: oy };
			if (size) {
				wallV += (clamp((W - size.w) / dt, -MAX_SPEED, MAX_SPEED) - wallV) * ease;
				floorV += (clamp((H - size.h) / dt, -MAX_SPEED, MAX_SPEED) - floorV) * ease;
			}
			size = { w: W, h: H };
			return push;
		}

		let last = performance.now();
		let frame = requestAnimationFrame(function tick(now) {
			const dt = clamp((now - last) / 1000, 1 / 240, 0.05);
			last = now;
			const rect = ground.getBoundingClientRect();
			W = rect.width;
			H = rect.height;
			hy = art.offsetHeight / 2;

			if (throwable) {
				const push = windowPush(dt, rect);
				if (mode === "ground") {
					vx += push.x;
					vy = Math.min(0, vy + push.y);
					const squeezed = x + hx >= W && wallV < -SETTLE_SPEED;
					if (
						Math.hypot(vx, vy) > SETTLE_SPEED ||
						squeezed ||
						Math.abs(floorV) > SETTLE_SPEED
					) {
						takeOff(now);
					} else {
						vx *= Math.exp(-10 * dt);
						vy *= Math.exp(-10 * dt);
					}
				} else if (mode === "flying" || mode === "stuck") {
					vx += push.x;
					vy += push.y;
				}
			}
			if (mode === "ground") walk(now, dt, rect);
			else if (mode === "held") hang(now, dt, rect);
			else if (mode === "stuck") dangle(now, dt);
			else if (mode === "dead") lieDead(now, dt);
			else fly(dt, now);

			let sx = 1;
			let sy = 1;
			let ox = 0;
			let oy = 0;
			if (mode === "ground") {
				// Tilts and squashes on its bottom, and squishes contentedly under a petting hand
				const cuddle = Math.min(1, petting);
				sx = squashX * (1 + 0.05 * cuddle);
				sy = squashY * (1 - 0.07 * cuddle + Math.sin(now / 90) * 0.02 * cuddle);
				ox = hy * Math.sin(angle * DEG);
				oy = hy * (1 - Math.cos(angle * DEG)) + hy * (1 - sy);
			}
			if (splat) {
				const t = (now - splat.start) / 1000;
				const wobble = splat.amount * Math.exp(-t / 0.1) * Math.cos(t * 32);
				const along = 1 - wobble;
				const across = 1 + wobble * 0.5;
				// Pinned to the side it hit
				if (splat.axis === "x") {
					sx *= along;
					sy *= across;
					ox += splat.side * hx * (1 - along);
					if (y + hy >= H - 0.5) oy += hy * (1 - across);
				} else {
					sx *= across;
					sy *= along;
					oy += splat.side * hy * (1 - along);
				}
				if (t > 0.7) splat = undefined;
			}
			// Struggles in fits against the cobweb, a little after it gets caught
			const caughtFor = now - stuckSince;
			const struggling = mode === "stuck" && caughtFor > 400 && caughtFor % 900 < 380;
			let wriggle = 0;
			if (struggling) {
				wriggle = Math.sin(caughtFor / 22) * 12;
				ox += Math.sin(caughtFor / 31) * 2;
			}
			pose.tx = x - hx + ox;
			pose.ty = y + hy - H + oy;
			pose.angle = angle + wriggle;
			pose.sx = sx;
			pose.sy = sy;
			// Gone while blown up, and fades back in when it drops in again
			pose.opacity = mode === "dead" ? 0 : clamp((now - respawnedAt) / 250, 0, 1);
			airborne = mode === "flying" || mode === "stuck";

			if (now < ouchUntil || struggling) look.face = "ouch";
			else if (mode !== "ground") look.face = "wide";
			else if (petting > 0.35 || now < happyUntil) look.face = "happy";
			else look.face = now < dizzyUntil ? "dizzy" : "calm";
			petting *= Math.exp(-2.5 * dt);
			lastPointer = pointer;
			if (pointer) {
				const dx = pointer.x - rect.left - x;
				const dy = pointer.y - rect.top - y;
				const distance = Math.hypot(dx, dy) || 1;
				const reach = Math.min(1, distance / 120) / distance;
				// Into its own frame, which turns and flips with it
				const cos = Math.cos(angle * DEG);
				const sin = Math.sin(angle * DEG);
				look.x = (dx * cos + dy * sin) * reach * pose.facing;
				look.y = (dy * cos - dx * sin) * reach;
			} else if (look.away) {
				look.x *= 0.9;
				look.y *= 0.9;
			}
			if (bursts.length && now - bursts[0].start > 450) {
				bursts = bursts.filter((burst) => now - burst.start <= 450);
			}
			if (hearts.length && now - hearts[0].start > 1000) {
				hearts = hearts.filter((heart) => now - heart.start <= 1000);
			}
			frame = requestAnimationFrame(tick);
		});

		return () => {
			cancelAnimationFrame(frame);
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("mouseout", onOut);
			art.removeEventListener("pointerdown", onDown);
			art.removeEventListener("pointermove", onDrag);
			art.removeEventListener("pointerup", onUp);
			art.removeEventListener("pointercancel", onCancel);
			art.removeEventListener("lostpointercapture", onCancel);
		};
	});
</script>

<div bind:this={layer} class="absolute inset-0">
	{#each bursts as burst (burst.id)}
		<svg
			class="text-base-content/60 absolute top-0 left-0"
			width="28"
			height="28"
			viewBox="-14 -14 28 28"
			style="transform: translate({burst.x - 14}px, {burst.y -
				14}px) rotate({burst.angle}deg);"
		>
			<path
				class="burst"
				d="M4 0h8M3.3 2.3l5.7 4M3.3-2.3l5.7-4M1.4 3.8l2.4 6.6M1.4-3.8l2.4-6.6"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				stroke-linecap="round"
			/>
		</svg>
	{/each}
	{#each tombstones as stone (stone.id)}
		<div class="absolute" style={graveSpot(stone)}>
			<svg
				class="tombstone absolute"
				style="left: -20px; top: -46px;"
				width="40"
				height="46"
				viewBox="0 0 40 46"
			>
				<ellipse cx="20" cy="45" rx="18" ry="3" fill="#57412f" />
				<path
					d="M7 45V17a13 13 0 0 1 26 0v28z"
					fill="#9ca3af"
					stroke="#6b7280"
					stroke-width="1.5"
				/>
				<path
					d="M10.5 43V17.5a9.5 9.5 0 0 1 5-8.4"
					fill="none"
					stroke="white"
					stroke-width="1.5"
					stroke-linecap="round"
					opacity="0.3"
				/>
				<text
					x="20"
					y="25"
					fill="#4b5563"
					font-family="Georgia, serif"
					font-size="8.5"
					font-weight="700"
					text-anchor="middle">RIP</text
				>
				<g fill="none" stroke="#6b7280" stroke-linecap="round">
					<path d="M14 31h12M16 35h8" stroke-width="1.2" />
					<path d="M28.5 10l-2 3.5 2 2.5" stroke-width="0.8" />
				</g>
				<path
					d="M5 45l1.5-4 1 4M8 45l1-3M32 45l1-4 2 4"
					fill="none"
					stroke="#65a30d"
					stroke-width="1.3"
					stroke-linecap="round"
				/>
			</svg>
		</div>
	{/each}
	{#if splatter}
		<svg
			class="absolute inset-0 h-full w-full overflow-visible"
			fill={goo}
			style="opacity: {splatter.opacity};"
		>
			{#each splatter.drops as drop, i (i)}
				{#if drop.chunk}
					<!-- A shard of rind -->
					<path
						d="M-1 0Q0-1.2 1 0Q0 .5-1 0z"
						fill={drop.chunk.color}
						transform="translate({drop.x} {drop.landed
							? drop.y - drop.r * 0.3
							: drop.y}) rotate({drop.landed ? 0 : drop.chunk.angle}) scale({drop.r *
							1.4})"
					/>
				{:else if drop.landed}
					<ellipse cx={drop.x} cy={drop.y} rx={drop.r * 1.8} ry={drop.r * 0.8} />
				{:else}
					<circle cx={drop.x} cy={drop.y} r={drop.r} />
				{/if}
			{/each}
		</svg>
	{/if}
	{#if threads}
		<svg
			class="text-base-content/40 absolute inset-0 h-full w-full overflow-visible"
			fill="none"
			stroke="currentColor"
			stroke-width="0.8"
		>
			{#each threads.ends as end, i (i)}
				<line x1={threads.x} y1={threads.y} x2={end.x} y2={end.y} />
			{/each}
		</svg>
	{/if}
	{#each hearts as heart (heart.id)}
		<svg
			class="absolute top-0 left-0"
			width="14"
			height="14"
			viewBox="0 0 14 14"
			style="transform: translate({heart.x - 7}px, {heart.y - 7}px);"
		>
			<path
				class="heart"
				style="--drift: {heart.drift}px;"
				d="M7 12.5C3 9.6 1 7.4 1 4.9 1 3 2.4 1.6 4.2 1.6c1.2 0 2.2.6 2.8 1.6.6-1 1.6-1.6 2.8-1.6C11.6 1.6 13 3 13 4.9c0 2.5-2 4.7-6 7.6z"
				fill="#f472b6"
			/>
		</svg>
	{/each}
	<div
		bind:this={sprite}
		class="absolute bottom-0 left-0"
		class:interactive={interactive && !dead}
		class:primed
		class:held
		style="width: {width}px; opacity: {pose.opacity}; transform: translate({pose.tx}px, {pose.ty}px) scale({pose.sx}, {pose.sy}) rotate({pose.angle}deg) scale({pose.facing}, 1);"
	>
		{@render children(look)}
		{#if airborne}
			<div class="catch-zone"></div>
		{/if}
	</div>
	{#if ghost}
		<div
			class="absolute top-0 left-0"
			style="width: {width}px; transform: translate({ghost.x}px, {ghost.y}px);"
		>
			<div class="ghost">
				{@render children(soul)}
			</div>
		</div>
	{/if}
	{#if blast}
		<div class="absolute top-0 left-0" style="transform: translate({blast.x}px, {blast.y}px);">
			{#each blast.puffs as puff, i (i)}
				<div
					class="puff"
					style="--dx: {puff.dx}px; --dy: {puff.dy}px; --size: {puff.size}px; animation-delay: {puff.delay}ms;"
				></div>
			{/each}
			<div class="flash"></div>
		</div>
	{/if}
</div>

<style>
	.interactive {
		pointer-events: auto;
		cursor: grab;
		touch-action: none;
	}

	.held {
		cursor: grabbing;
	}

	/* Much easier to grab out of the air than its own outline */
	.catch-zone {
		position: absolute;
		inset: -28px;
		border-radius: 50%;
	}

	/* Thrown once too often: glows red until it hits something */
	.primed {
		animation: fuse 0.18s ease-in-out infinite alternate;
	}

	@keyframes fuse {
		to {
			filter: brightness(1.3) saturate(1.8) drop-shadow(0 0 6px rgb(239 68 68));
		}
	}

	.heart {
		transform-box: fill-box;
		transform-origin: center;
		animation: float 1s ease-out forwards;
	}

	@keyframes float {
		from {
			opacity: 1;
			transform: translate(0, 0) scale(0.5);
		}
		30% {
			opacity: 1;
			transform: translate(calc(var(--drift) * 0.4), -16px) scale(1.1);
		}
		to {
			opacity: 0;
			transform: translate(var(--drift), -50px) scale(0.9);
		}
	}

	.flash {
		position: absolute;
		left: -60px;
		top: -60px;
		width: 120px;
		height: 120px;
		border-radius: 50%;
		background: radial-gradient(circle, #fff 0%, #fde047 30%, #f97316 55%, transparent 70%);
		animation: flash 0.35s ease-out forwards;
	}

	@keyframes flash {
		from {
			opacity: 1;
			transform: scale(0.2);
		}
		to {
			opacity: 0;
			transform: scale(1.8);
		}
	}

	.puff {
		position: absolute;
		left: calc(var(--size) / -2);
		top: calc(var(--size) / -2);
		width: var(--size);
		height: var(--size);
		border-radius: 50%;
		background: rgb(120 113 108 / 0.6);
		animation: puff 0.9s ease-out both;
	}

	@keyframes puff {
		from {
			opacity: 0.8;
			transform: translate(0, 0) scale(0.3);
		}
		to {
			opacity: 0;
			transform: translate(var(--dx), var(--dy)) scale(1.4);
		}
	}

	/* Comes up out of the ground once its ghost is on the way out */
	.tombstone {
		filter: drop-shadow(0 2px 3px rgb(0 0 0 / 0.25));
		animation: rise 0.5s 0.35s cubic-bezier(0.3, 1.5, 0.6, 1) backwards;
	}

	.ghost {
		filter: grayscale(1) brightness(1.9);
		opacity: 0;
		animation: haunt 1.6s ease-out forwards;
	}

	@keyframes rise {
		from {
			transform: translateY(100%);
		}
	}

	@keyframes haunt {
		0% {
			opacity: 0;
			transform: translate(0, 0) scale(0.9);
		}
		15% {
			opacity: 0.75;
		}
		25% {
			transform: translate(-6px, -30px) scale(0.95);
		}
		50% {
			transform: translate(6px, -60px) scale(1);
		}
		75% {
			opacity: 0.5;
			transform: translate(-6px, -90px) scale(1);
		}
		100% {
			opacity: 0;
			transform: translate(0, -120px) scale(1.05);
		}
	}

	.burst {
		transform-box: view-box;
		transform-origin: 0 0;
		animation: burst 0.45s ease-out forwards;
	}

	@keyframes burst {
		from {
			transform: scale(0.4);
			opacity: 1;
		}
		to {
			transform: scale(1.4);
			opacity: 0;
		}
	}
</style>
