<script lang="ts">
	import { onMount } from "svelte";
	import type { Snippet } from "svelte";

	interface Props {
		/** Hopping after the cursor, or waddling along the ground */
		gait: "hop" | "waddle";
		/** The art's width, in px */
		width: number;
		/** The art; `look` points toward the cursor, -1 to 1 on each axis, for its eyes */
		children: Snippet<[{ x: number; y: number }]>;
	}

	let { gait, width, children }: Props = $props();

	const MARGIN = 12;
	const HOP_DISTANCE = 80;
	const WADDLE_SPEED = 150;

	let layer = $state<HTMLDivElement>();
	const pose = $state({ x: -1000, y: 0, tilt: 0, sx: 1, sy: 1, facing: 1 });
	const look = $state({ x: 0, y: 0 });

	const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

	onMount(() => {
		const ground = layer;
		if (!ground) return;
		// Starts in the bottom-right corner and stays there when motion is reduced
		pose.x = ground.clientWidth - width - MARGIN;
		if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;

		let pointer: { x: number; y: number } | undefined;
		const onMove = (e: PointerEvent) => (pointer = { x: e.clientX, y: e.clientY });
		window.addEventListener("pointermove", onMove);

		let hop:
			| { from: number; to: number; height: number; start: number; duration: number }
			| undefined;
		let landedAt = 0;
		let stride = 0;
		let last = performance.now();

		function hopTowards(now: number, target: number, above: number) {
			if (hop) {
				const t = Math.min(1, (now - hop.start) / hop.duration);
				const arc = Math.sin(Math.PI * t);
				pose.x = hop.from + (hop.to - hop.from) * t;
				pose.y = -hop.height * 4 * t * (1 - t);
				pose.sx = 1 - 0.08 * arc;
				pose.sy = 1 + 0.12 * arc;
				pose.tilt = Math.sign(hop.to - hop.from) * 8 * arc;
				if (t === 1) {
					hop = undefined;
					landedAt = now;
				}
				return;
			}
			const squash = Math.max(0, 1 - (now - landedAt) / 140);
			pose.y = 0;
			pose.tilt = 0;
			pose.sx = 1 + 0.12 * squash;
			pose.sy = 1 - 0.16 * squash;
			const dx = target - pose.x;
			const travelling = Math.abs(dx) > 8;
			if (now - landedAt < (travelling ? 140 : 800)) return;
			if (travelling) pose.facing = Math.sign(dx);
			// Under the cursor it keeps jumping up at it, higher the closer it is
			const height = travelling ? 26 : above < 260 ? clamp(above - 50, 10, 90) : 12;
			hop = {
				from: pose.x,
				to: travelling
					? pose.x + Math.sign(dx) * Math.min(Math.abs(dx), HOP_DISTANCE)
					: pose.x,
				height,
				start: now,
				duration: 300 + height * 2,
			};
		}

		function waddleTowards(dt: number, target: number) {
			const dx = target - pose.x;
			if (Math.abs(dx) > 4) {
				pose.x += Math.sign(dx) * Math.min(Math.abs(dx), WADDLE_SPEED * dt);
				pose.facing = Math.sign(dx);
				stride += dt * 10;
				pose.tilt = Math.sin(stride) * 7;
				pose.y = -Math.abs(Math.sin(stride)) * 3;
			} else {
				pose.tilt *= 0.85;
				pose.y *= 0.85;
			}
		}

		let frame = requestAnimationFrame(function tick(now) {
			const dt = Math.min(0.05, (now - last) / 1000);
			last = now;
			const rect = ground.getBoundingClientRect();
			const max = rect.width - width - MARGIN;
			pose.x = clamp(pose.x, MARGIN, max);
			const target = pointer ? clamp(pointer.x - rect.left - width / 2, MARGIN, max) : pose.x;
			const above = pointer ? rect.bottom - pointer.y : Infinity;
			if (gait === "hop") hopTowards(now, target, above);
			else waddleTowards(dt, target);

			if (pointer) {
				const dx = pointer.x - (rect.left + pose.x + width / 2);
				const dy = pointer.y - (rect.bottom + pose.y - width / 2);
				const distance = Math.hypot(dx, dy) || 1;
				const reach = Math.min(1, distance / 120);
				look.x = (dx / distance) * reach * pose.facing;
				look.y = (dy / distance) * reach;
			}
			frame = requestAnimationFrame(tick);
		});

		return () => {
			cancelAnimationFrame(frame);
			window.removeEventListener("pointermove", onMove);
		};
	});
</script>

<div bind:this={layer} class="absolute inset-0">
	<div
		class="absolute bottom-0 left-0 origin-bottom"
		style="width: {width}px; transform: translate({pose.x}px, {pose.y}px) rotate({pose.tilt}deg) scale({pose.sx *
			pose.facing}, {pose.sy});"
	>
		{@render children(look)}
	</div>
</div>
