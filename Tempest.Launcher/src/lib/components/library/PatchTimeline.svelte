<script lang="ts">
	import type { Patch } from "$lib/utils/versions";

	interface Props {
		/** Oldest first, as in versions.json */
		patches: Patch[];
		/** The picked patch's id */
		value: string;
		/** Accessible name */
		label: string;
		/** A patch's id whose tick stands out while not picked */
		highlight?: string;
	}

	let { patches, value = $bindable(), label, highlight }: Props = $props();

	/** Every patch by major version (0.x is the beta era), oldest first */
	const eras = $derived.by(() => {
		const byMajor = new Map<string, { patch: Patch; i: number }[]>();
		for (const [i, patch] of patches.entries()) {
			const major = patch.version.split(".")[0]!;
			byMajor.set(major, [...(byMajor.get(major) ?? []), { patch, i }]);
		}
		return [...byMajor];
	});
	const index = $derived(patches.findIndex((p) => p.id === value));

	let root = $state<HTMLElement>();
	let width = $state(0);
	let tipWidth = $state(0);
	let hovered = $state<number | null>(null);
	let dragging = $state(false);

	// Where each era and tick sits, worked out the way flex lays them out: eras 6px apart, ticks
	// 1px apart, every tick equally wide.
	const layout = $derived.by(() => {
		const gaps = 6 * (eras.length - 1) + (patches.length - eras.length);
		const tick = Math.max(0, (width - gaps) / patches.length);
		const lefts: number[] = [];
		const centers: number[] = [];
		let x = 0;
		for (const [, list] of eras) {
			lefts.push(x);
			for (let j = 0; j < list.length; j++) centers.push(x + j * (tick + 1) + tick / 2);
			x += list.length * tick + list.length - 1 + 6;
		}
		return { lefts, centers };
	});

	/**
	 * Which era labels ("7.x") show, as on the mod website: each sits at its era's left (the last
	 * at the right end) and is left out where it would run into the next one shown.
	 */
	const LABEL = 18 + 4; // three 10px mono characters and a little air
	const eraLabels = $derived.by(() => {
		const shown = eras.map(() => false);
		let next = width - LABEL;
		shown[eras.length - 1] = true;
		for (let i = eras.length - 2; i >= 0; i--) {
			if (layout.lefts[i]! + LABEL <= next) {
				shown[i] = true;
				next = layout.lefts[i]!;
			}
		}
		return shown;
	});

	const tipLeft = $derived(
		hovered === null
			? 0
			: Math.max(0, Math.min(layout.centers[hovered]! - tipWidth / 2, width - tipWidth)),
	);

	// The tick nearest the pointer, so the gaps and overshooting the ends still land on one.
	function tickAt(clientX: number): number {
		const x = clientX - root!.getBoundingClientRect().left;
		let closest = 0;
		for (const [i, center] of layout.centers.entries()) {
			if (Math.abs(x - center) < Math.abs(x - layout.centers[closest]!)) closest = i;
		}
		return closest;
	}

	function pick(i: number) {
		value = patches[i]!.id;
	}

	// Pressing picks the nearest patch and dragging scrubs through them, like a slider.
	function onpointerdown(e: PointerEvent) {
		if (e.button !== 0) return;
		e.preventDefault();
		root!.focus({ preventScroll: true });
		root!.setPointerCapture(e.pointerId);
		dragging = true;
		pick(tickAt(e.clientX));
		hovered = null;
	}

	function onpointermove(e: PointerEvent) {
		if (dragging) pick(tickAt(e.clientX));
		else hovered = tickAt(e.clientX);
	}

	function onkeydown(e: KeyboardEvent) {
		const last = patches.length - 1;
		const at = index < 0 ? last : index;
		const moves: Record<string, number> = {
			ArrowLeft: at - 1,
			ArrowDown: at - 1,
			ArrowRight: at + 1,
			ArrowUp: at + 1,
			PageDown: at - 10,
			PageUp: at + 10,
			Home: 0,
			End: last,
		};
		if (!(e.key in moves)) return;
		e.preventDefault();
		pick(Math.max(0, Math.min(last, moves[e.key]!)));
	}
</script>

<!-- After the mod website's patch timeline: every patch as a tick, the picked one filled. The
     hovered one's name overhangs what sits above the timeline rather than taking room of its own. -->
<div
	bind:this={root}
	bind:clientWidth={width}
	role="slider"
	tabindex="0"
	aria-label={label}
	aria-valuemin={0}
	aria-valuemax={patches.length - 1}
	aria-valuenow={Math.max(index, 0)}
	aria-valuetext={index >= 0 ? `${patches[index]!.version} ${patches[index]!.name}` : undefined}
	class="relative flex cursor-pointer touch-none gap-1.5 rounded outline-offset-4 select-none focus-visible:outline-2"
	{onpointerdown}
	{onpointermove}
	onpointerup={() => (dragging = false)}
	onpointercancel={() => (dragging = false)}
	onpointerleave={() => (hovered = null)}
	{onkeydown}
>
	{#if hovered !== null && width > 0}
		{@const patch = patches[hovered]!}
		<div
			class={[
				"pointer-events-none absolute bottom-full z-10 mb-1.5 rounded px-2 py-0.5 text-xs whitespace-nowrap shadow",
				hovered === index
					? "bg-primary text-primary-content"
					: "bg-neutral text-neutral-content",
			]}
			style:left={`${tipLeft}px`}
			bind:offsetWidth={tipWidth}
		>
			<span class="font-mono font-semibold">{patch.version}</span>
			<span class="opacity-70">· {patch.name}</span>
		</div>
	{/if}
	<!-- Every tick is as wide as the others: an era's label doesn't widen it, it overhangs
	     (leftwards for the last era, so it stays inside the timeline). -->
	{#each eras as [major, list], i (major)}
		{@const picked = list.some((t) => t.i === index)}
		<div
			class="flex min-w-0 flex-col gap-1"
			style:flex={`${list.length} 1 ${list.length - 1}px`}
		>
			<div class="flex h-6 gap-px">
				{#each list as t (t.patch.id)}
					<span
						class={[
							"min-w-0 flex-1 rounded-xs",
							!dragging && "transition-colors",
							t.i === index
								? "bg-primary ring-primary/30 ring-2"
								: t.patch.id === highlight
									? "bg-warning"
									: "bg-base-300",
							hovered === t.i && t.i !== index && "ring-primary ring-1",
						]}
					></span>
				{/each}
			</div>
			<span
				class={[
					"font-mono text-[10px] whitespace-nowrap",
					picked ? "text-primary font-semibold" : "text-base-content/50",
					i === eras.length - 1 && "self-end",
					!eraLabels[i] && "invisible",
				]}>{major}.x</span
			>
		</div>
	{/each}
</div>
