<script lang="ts">
	import { Spring, prefersReducedMotion } from "svelte/motion";
	import { m } from "$lib/paraglide/messages";
	import { getLocale } from "$lib/paraglide/runtime";
	import { formatSpeed } from "$lib/rigby/format";
	import { SPEED_WINDOW_MS } from "$lib/rigby/stores.svelte";
	import { SMOOTH_SPRING } from "$lib/utils/motion";
	import type { SpeedSample } from "$lib/rigby/stores.svelte";
	import type { ClassValue } from "svelte/elements";

	interface Props {
		samples: SpeedSample[];
		/** Scrolls with the clock between samples; off, the graph rests on its last sample */
		live?: boolean;
		class?: ClassValue;
	}

	let { samples, live = false, class: className = "" }: Props = $props();

	const id = $props.id();
	let width = $state(0);
	let height = $state(0);
	let pointerX = $state<number>();

	// Room for the end dot and its ring, so neither is clipped at the top or right edge.
	const INSET = 6;

	const animate = $derived(!prefersReducedMotion.current);
	const latest = $derived(samples.at(-1));

	let now = $state(Date.now());
	$effect(() => {
		if (!live || !animate) return;
		let frame = requestAnimationFrame(function tick() {
			now = Date.now();
			frame = requestAnimationFrame(tick);
		});
		return () => cancelAnimationFrame(frame);
	});
	const head = $derived(live && animate ? Math.max(now, latest?.at ?? 0) : (latest?.at ?? 0));

	const peak = $derived(Math.max(0, ...samples.map((sample) => sample.bytesPerSecond)));
	// A floor of 1 MB/s, so a slow start doesn't fill the height.
	const ceilingTarget = $derived(Math.max(peak * 1.15, 1024 * 1024));
	// The scale and the newest value glide to new values instead of jumping with each sample.
	const ceilingSpring = Spring.of(() => ceilingTarget, SMOOTH_SPRING);
	const headSpring = Spring.of(() => latest?.bytesPerSecond ?? 0, SMOOTH_SPRING);
	const ceiling = $derived(animate ? ceilingSpring.current : ceilingTarget);

	// The clock sits at the right edge and the samples scroll off the left: a fixed window like
	// Steam's graph, not a line stretched over however much history there is.
	const points = $derived.by(() => {
		if (!latest || width === 0) return [];
		const plotWidth = width - INSET;
		const plotHeight = height - INSET - 1;
		const yOf = (bytesPerSecond: number) =>
			height - 1 - (bytesPerSecond / ceiling) * plotHeight;
		const placed = samples.map((sample) => ({
			x: plotWidth - ((head - sample.at) / SPEED_WINDOW_MS) * plotWidth,
			y: yOf(sample.bytesPerSecond),
			sample,
		}));
		// The line's tip, at the right edge with the newest speed sprung into place
		if (live && animate) {
			placed.push({ x: plotWidth, y: yOf(headSpring.current), sample: latest });
		}
		return placed;
	});

	/**
	 * Through the midpoints with each sample as a control point, so the line has no corners.
	 * @param pts - The points in order.
	 * @returns An SVG path.
	 */
	function curve(pts: { x: number; y: number }[]) {
		if (pts.length < 3) return pts.map((p, i) => `${i ? "L" : "M"}${p.x},${p.y}`).join("");
		let path = `M${pts[0]!.x},${pts[0]!.y}`;
		for (let i = 1; i < pts.length - 1; i++) {
			const p = pts[i]!;
			const next = pts[i + 1]!;
			path += `Q${p.x},${p.y} ${(p.x + next.x) / 2},${(p.y + next.y) / 2}`;
		}
		const last = pts.at(-1)!;
		return `${path}L${last.x},${last.y}`;
	}

	const line = $derived(curve(points));
	const area = $derived(
		points.length > 1 ? `${line}L${points.at(-1)!.x},${height}L${points[0]!.x},${height}Z` : "",
	);

	const hovered = $derived.by(() => {
		const x = pointerX;
		if (x === undefined) return undefined;
		let nearest = points[0];
		for (const p of points) {
			if (Math.abs(p.x - x) < Math.abs(nearest!.x - x)) nearest = p;
		}
		return nearest;
	});
	const dot = $derived(hovered ?? points.at(-1));

	function track(event: PointerEvent) {
		pointerX = event.clientX - (event.currentTarget as Element).getBoundingClientRect().left;
	}

	const ago = (sample: SpeedSample) =>
		new Intl.RelativeTimeFormat(getLocale(), { numeric: "auto" }).format(
			-Math.round((head - sample.at) / 1000),
			"second",
		);
</script>

<div
	class={["relative", className]}
	role="img"
	aria-label={m.downloads_graph_label({ speed: formatSpeed(peak) })}
	bind:clientWidth={width}
	bind:clientHeight={height}
>
	{#if points.length > 1 && dot}
		<svg
			{width}
			{height}
			class="absolute inset-0 overflow-visible"
			onpointermove={track}
			onpointerleave={() => (pointerX = undefined)}
			aria-hidden="true"
		>
			<defs>
				<linearGradient id="{id}-wash" x1="0" y1="0" x2="0" y2="1">
					<stop offset="0" stop-color="var(--color-primary)" stop-opacity="0.45" />
					<stop offset="1" stop-color="var(--color-primary)" stop-opacity="0.04" />
				</linearGradient>
				<!-- The oldest end fades in instead of starting on a hard edge -->
				<linearGradient
					id="{id}-start"
					gradientUnits="userSpaceOnUse"
					x1={points[0]!.x}
					x2={points[0]!.x + 48}
					y1="0"
					y2="0"
				>
					<stop offset="0" stop-color="black" />
					<stop offset="1" stop-color="white" />
				</linearGradient>
				<mask
					id="{id}-fade"
					maskUnits="userSpaceOnUse"
					x="0"
					y="-8"
					{width}
					height={height + 16}
				>
					<rect x="0" y="-8" {width} height={height + 16} fill="url(#{id}-start)" />
				</mask>
			</defs>
			<g mask="url(#{id}-fade)">
				<path d={area} fill="url(#{id}-wash)" />
				<path
					d={line}
					fill="none"
					class="stroke-primary"
					stroke-width="2"
					stroke-linejoin="round"
					stroke-linecap="round"
				/>
			</g>
			{#if hovered}
				<line
					x1={hovered.x}
					x2={hovered.x}
					y1="0"
					y2={height}
					class="stroke-base-content/25"
					stroke-width="1"
				/>
			{/if}
			<circle
				cx={dot.x}
				cy={dot.y}
				r="4"
				class="fill-primary stroke-base-200"
				stroke-width="2"
			/>
			<!-- The whole band answers the pointer, not just the line -->
			<rect {width} {height} fill="transparent" />
		</svg>
		{#if hovered}
			<div
				class="bg-base-100 border-base-300 rounded-field pointer-events-none absolute bottom-full z-10 mb-1 -translate-x-1/2 border px-2 py-1 text-xs whitespace-nowrap shadow-sm"
				style="left: clamp(6rem, {hovered.x}px, calc(100% - 6rem));"
			>
				<span class="font-semibold tabular-nums"
					>{formatSpeed(hovered.sample.bytesPerSecond)}</span
				>
				<span class="text-base-content/60">· {ago(hovered.sample)}</span>
			</div>
		{/if}
	{/if}
</div>
