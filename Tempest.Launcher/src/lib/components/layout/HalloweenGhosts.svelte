<script lang="ts">
	import { onMount } from "svelte";
	import { Flock } from "./ghosts";
	import type { GhostPose } from "./ghosts";

	const INK = "#1e1b2e";

	const uid = $props.id();
	let width = $state(0);
	let height = $state(0);
	let poses = $state.raw<GhostPose[]>([]);

	onMount(() => {
		const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
		let flock: Flock | undefined;
		let last = performance.now();
		let frame = requestAnimationFrame(function tick(now) {
			const dt = Math.min(0.05, (now - last) / 1000);
			last = now;
			if (width && !flock) flock = new Flock(width, height);
			if (flock) {
				flock.resize(width, height);
				if (!still) flock.step(dt);
				poses = flock.poses();
			}
			if (!still || !flock) frame = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(frame);
	});
</script>

{#snippet face(f: GhostPose["face"])}
	<g transform="translate({f.look.x} {f.look.y}) scale({f.scale})">
		{#each [-1, 1] as side (side)}
			{@const x = side * f.gap}
			{#if f.expression === "joy"}
				<path
					d="M{x - 2.8} 0.4q2.8 {-3.8 * f.open} 5.6 0"
					fill="none"
					stroke={INK}
					stroke-width="1.7"
					stroke-linecap="round"
				/>
			{:else if f.expression === "calm"}
				<path
					d="M{x - 2.8} -1.4q2.8 {3.2 * f.open} 5.6 0"
					fill="none"
					stroke={INK}
					stroke-width="1.6"
					stroke-linecap="round"
				/>
			{:else}
				{@const wide = f.expression === "wow" ? 1.15 : 1}
				<ellipse
					cx={x}
					cy="-1"
					rx={3.2 * f.eye * wide}
					ry={Math.max(0.5, 3.8 * f.eye * wide * f.open)}
					fill={INK}
				/>
				{#if f.open > 0.6}
					<circle cx={x + 1.1 * f.eye} cy="-2.5" r={1.3 * f.eye} fill="white" />
					<circle cx={x - 1 * f.eye} cy="0.5" r={0.6 * f.eye} fill="white" />
				{/if}
			{/if}
			<ellipse
				cx={side * (f.gap + 3.8)}
				cy="3.8"
				rx="2.5"
				ry="1.5"
				fill="#fb7185"
				opacity={f.cheeks * 0.6}
			/>
		{/each}
		{#if f.expression === "joy" || (f.grin && f.expression !== "wow")}
			<path
				d="M-3.2 3.6h6.4q0 4.6-3.2 4.6q-3.2 0-3.2-4.6z"
				fill="#3b1d2e"
				stroke={INK}
				stroke-width="1"
				stroke-linejoin="round"
			/>
			<ellipse cy="6.9" rx="1.7" ry="1.1" fill="#fb7185" />
		{:else if f.expression === "wow"}
			<ellipse cy="5.6" rx="1.9" ry="2.3" fill="#3b1d2e" stroke={INK} stroke-width="0.8" />
		{:else}
			<path
				d="M-2.6 4.4q2.6 2.8 5.2 0"
				fill="none"
				stroke={INK}
				stroke-width="1.4"
				stroke-linecap="round"
			/>
		{/if}
	</g>
{/snippet}

{#snippet arm(x: number, y: number, side: number, angle: number)}
	<ellipse
		cx={x + side * 2.4}
		cy={y + 1}
		rx="4.8"
		ry="3.4"
		transform="rotate({angle} {x} {y})"
		fill="white"
		stroke={INK}
		stroke-width="1.6"
	/>
{/snippet}

<div
	class="pointer-events-none absolute inset-0 overflow-hidden"
	aria-hidden="true"
	bind:clientWidth={width}
	bind:clientHeight={height}
>
	<svg class="absolute inset-0 size-full">
		<defs>
			<filter id="{uid}-glow" x="-50%" y="-50%" width="200%" height="200%">
				<feDropShadow
					dx="0"
					dy="0"
					stdDeviation="3"
					flood-color="white"
					flood-opacity="0.25"
				/>
			</filter>
			<radialGradient id="{uid}-light">
				<stop offset="0" stop-color="#fde68a" stop-opacity="0.85" />
				<stop offset="1" stop-color="#fde68a" stop-opacity="0" />
			</radialGradient>
		</defs>

		{#each poses as p (p.kind)}
			{#each p.sparkles as s, i (i)}
				<path
					d="M0-3.2L0.9-0.9L3.2 0L0.9 0.9L0 3.2L-0.9 0.9L-3.2 0L-0.9-0.9Z"
					transform="translate({s.x} {s.y}) scale({s.size})"
					fill="#fef3c7"
					opacity={s.opacity}
				/>
			{/each}
		{/each}

		{#each poses as p (p.kind)}
			<g transform={p.transform} filter="url(#{uid}-glow)">
				<clipPath id="{uid}-{p.kind}"><path d={p.d} /></clipPath>

				<!-- Behind the sheet, so only their outer half shows -->
				{#if p.arms}
					{@render arm(p.left.x, p.left.y, -1, p.arms.left)}
					{@render arm(
						p.right.x,
						p.right.y,
						1,
						p.kind === "lumen" ? 20 + p.arms.right * 0.1 : p.arms.right,
					)}
				{/if}

				<path d={p.d} fill={p.shade} />
				<g clip-path="url(#{uid}-{p.kind})">
					<path d={p.d} fill="white" transform="translate({p.shadeX} -5)" />
					{#if p.kind === "lumen"}
						<circle
							cx={p.right.x + 4}
							cy={p.right.y + 12}
							r="20"
							fill="url(#{uid}-light)"
							opacity={p.flicker * 0.55}
						/>
					{/if}
				</g>
				<path d={p.d} fill="none" stroke={INK} stroke-width="1.8" stroke-linejoin="round" />

				{@render face(p.face)}

				{#if p.kind === "boo"}
					<g
						transform="translate(-8 -11) rotate({-28 + p.wiggle})"
						stroke={INK}
						stroke-width="1.4"
						stroke-linejoin="round"
					>
						<path d="M0 0C-3-6-9-7-9-1C-9 4-3 4 0 0Z" fill="#f49ae8" />
						<path d="M0 0C3-6 9-7 9-1C9 4 3 4 0 0Z" fill="#f49ae8" />
						<ellipse rx="2.2" ry="2.6" fill="#ff7ae0" />
					</g>
				{:else if p.kind === "zip"}
					<!-- A floppy little witch hat, its tip flung back by the speed -->
					<g
						transform="translate(1 -10) rotate({p.wiggle * 0.6 - p.shadeX * 3})"
						stroke={INK}
						stroke-width="1.3"
						stroke-linejoin="round"
					>
						<path
							d="M-6 0C-4-5-2-11 1-15C3-17 6-16 7-14C4-13 3-10 3-7C3-4 4-2 6 0Z"
							fill="#7c3aed"
						/>
						<path
							d="M-5.4-3.2L5.2-3.2L5.6-0.6L-5.8-0.6Z"
							fill="#f59e0b"
							stroke="none"
						/>
						<ellipse cy="0.6" rx="10" ry="2.2" fill="#6d28d9" />
					</g>
				{:else if p.kind === "baby"}
					<path
						d="M0-8.6c-1.2-4 2.4-6.6 4.2-3.8c1 1.8-1.2 3-2.4 1.6"
						transform="rotate({p.wiggle * 1.4} 0 -8.6)"
						fill="none"
						stroke={INK}
						stroke-width="1.3"
						stroke-linecap="round"
					/>
				{:else if p.kind === "lumen"}
					<g
						transform="translate({p.right.x + 6} {p.right.y + 3}) rotate({p.hang})"
						stroke={INK}
						stroke-width="0.9"
					>
						<circle
							cy="11"
							r="12"
							fill="url(#{uid}-light)"
							stroke="none"
							opacity={p.flicker}
						/>
						<path d="M-2.5 3a2.5 3 0 0 1 5 0" fill="none" stroke-width="1.1" />
						<rect x="-4" y="3" width="8" height="2.2" rx="1" fill="#5b4636" />
						<rect x="-3.5" y="5.2" width="7" height="9" rx="1.5" fill="#fde68a" />
						<path
							d="M0 7.2c1.8 2 1.8 4.2 0 5.6c-1.8-1.4-1.8-3.6 0-5.6z"
							fill="#f59e0b"
							stroke="none"
							opacity={p.flicker}
						/>
						<rect x="-4.5" y="14.2" width="9" height="2.4" rx="1" fill="#5b4636" />
					</g>
				{/if}
			</g>
		{/each}
	</svg>
</div>
