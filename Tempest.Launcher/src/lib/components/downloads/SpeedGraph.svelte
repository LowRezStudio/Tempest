<script lang="ts">
	import { m } from "$lib/paraglide/messages";
	import { getLocale } from "$lib/paraglide/runtime";
	import { formatSpeed } from "$lib/rigby/format";
	import { SPEED_WINDOW_MS } from "$lib/rigby/stores.svelte";
	import type { SpeedSample } from "$lib/rigby/stores.svelte";

	interface Props {
		samples: SpeedSample[];
		class?: string;
	}

	let { samples, class: className = "" }: Props = $props();

	const id = $props.id();
	let width = $state(0);
	let height = $state(0);
	let pointerX = $state<number>();

	// Room for the end dot and its ring, so neither is clipped at the top or right edge.
	const INSET = 6;

	const peak = $derived(Math.max(0, ...samples.map((sample) => sample.bytesPerSecond)));
	// A floor of 1 MB/s, so a slow start doesn't fill the height.
	const ceiling = $derived(Math.max(peak * 1.15, 1024 * 1024));

	// The newest sample sits at the right edge and older ones scroll off the left: a fixed window
	// like Steam's graph, not a line stretched over however much history there is.
	const points = $derived.by(() => {
		const latest = samples.at(-1);
		if (!latest || width === 0) return [];
		const plotWidth = width - INSET;
		const plotHeight = height - INSET - 1;
		return samples.map((sample) => ({
			x: plotWidth - ((latest.at - sample.at) / SPEED_WINDOW_MS) * plotWidth,
			y: height - 1 - (sample.bytesPerSecond / ceiling) * plotHeight,
			sample,
		}));
	});

	const line = $derived(points.map((p, i) => `${i ? "L" : "M"}${p.x},${p.y}`).join(""));
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
			-Math.round((samples.at(-1)!.at - sample.at) / 1000),
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
					<stop offset="0" stop-color="var(--color-primary)" stop-opacity="0.2" />
					<stop offset="1" stop-color="var(--color-primary)" stop-opacity="0.02" />
				</linearGradient>
			</defs>
			<path d={area} fill="url(#{id}-wash)" />
			<path
				d={line}
				fill="none"
				class="stroke-primary"
				stroke-width="2"
				stroke-linejoin="round"
				stroke-linecap="round"
			/>
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
