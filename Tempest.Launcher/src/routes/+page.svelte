<script lang="ts">
	import { MousePointerClick, Pin, GripHorizontal, RotateCcw } from "@lucide/svelte";
	import { fade } from "svelte/transition";
	import HomeFloatingActions from "$lib/components/home/HomeFloatingActions.svelte";
	import { m } from "$lib/paraglide/messages";
	import {
		hintPosStore,
		resetAllHomePositions,
		clampPos,
	} from "$lib/stores/homePositions.svelte";
	import { persistedState } from "$lib/stores/persisted.svelte";
	import { pinnedBackground } from "$lib/stores/settings.svelte";

	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map((img) =>
		img.replace("/static", ""),
	);

	const getRandomBackground = () => backgrounds[Math.floor(Math.random() * backgrounds.length)];

	let currentBackground = $state(pinnedBackground.value || getRandomBackground());

	// True while the home page shows exactly the background persisted as pinned.
	let isPinned = $derived(pinnedBackground.value === currentBackground);

	// Flashes the left-click hint icon blue briefly after each background cycle.
	let clickFlash = $state(false);
	let clickFlashTimer: ReturnType<typeof setTimeout> | undefined;

	function changeBackground() {
		if (pinnedBackground.value) return;
		if (backgrounds.length <= 1) return;
		let next = getRandomBackground();
		while (next === currentBackground) {
			next = getRandomBackground();
		}
		currentBackground = next;

		clearTimeout(clickFlashTimer);
		clickFlash = true;
		clickFlashTimer = setTimeout(() => (clickFlash = false), 200);
	}

	function pinBackground(event: MouseEvent) {
		event.preventDefault();
		pinnedBackground.value = isPinned ? undefined : currentBackground;
	}

	// Hint tiles drag state (right-anchored, top-anchored)
	type HintPos = { x: number; y: number };
	const HINT_MARGIN = 8;
	let hintPos = $state<HintPos | null>(hintPosStore.value ?? null);
	let hintTilesEl: HTMLDivElement | undefined = $state();
	let hintDragging = $state(false);

	$effect(() => {
		hintPos = hintPosStore.value;
	});
	let hintDragStartPos: HintPos | null = { x: 0, y: 0 };
	let hintDragStartPointer: HintPos = { x: 0, y: 0 };

	function startHintDrag(event: PointerEvent) {
		if (!hintTilesEl) return;
		if (!hintPos) {
			const rect = hintTilesEl.getBoundingClientRect();
			hintPos = clampPos(
				{
					// Right-anchored: distance from right edge
					x: window.innerWidth - rect.right,
					// Top-anchored: distance from top edge
					y: rect.top,
				},
				hintTilesEl,
				HINT_MARGIN,
			);
		}
		hintDragStartPos = hintPos;
		hintDragStartPointer = { x: event.clientX, y: event.clientY };
		hintDragging = true;
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
	}

	function moveHintDrag(event: PointerEvent) {
		if (!hintDragging) return;
		// Right-anchored: mouse right -> element left (decrease right distance)
		// Top-anchored: mouse down -> element down (increase top distance)
		const start = hintDragStartPos!;
		hintPos = clampPos(
			{
				x: start.x - (event.clientX - hintDragStartPointer.x),
				y: start.y + (event.clientY - hintDragStartPointer.y),
			},
			hintTilesEl!,
			HINT_MARGIN,
		);
	}

	function endHintDrag() {
		if (!hintDragging) return;
		hintDragging = false;
		hintPosStore.value = hintPos;
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
	class="no-drag-select absolute inset-0 z-0 overflow-hidden select-none"
	role="button"
	tabindex="-1"
	onclick={changeBackground}
	oncontextmenu={pinBackground}
	ondragstart={(e) => e.preventDefault()}
	onselectstart={(e) => e.preventDefault()}
>
	{#key currentBackground}
		<img
			transition:fade={{ duration: 300 }}
			class="no-drag-select pointer-events-none absolute inset-0 h-full w-full object-cover object-center brightness-75 select-none"
			src={currentBackground}
			alt="background"
			draggable="false"
			ondragstart={(e) => e.preventDefault()}
			onselectstart={(e) => e.preventDefault()}
		/>
	{/key}
</div>

<div class="pointer-events-none relative top-0 left-0 z-10 h-full p-2"></div>

<div
	bind:this={hintTilesEl}
	class="fixed z-40 flex flex-col items-end gap-1.5 select-none"
	class:top-8={hintPos === null}
	class:right-8={hintPos === null}
	style:top={hintPos ? `${hintPos.y}px` : undefined}
	style:right={hintPos ? `${hintPos.x}px` : undefined}
>
	<button
		type="button"
		class="flex h-5 w-full cursor-grab touch-none justify-center rounded-b-lg border-0 bg-transparent p-0 opacity-60 focus-visible:ring-0 active:cursor-grabbing"
		aria-label="Move hint tiles"
		onpointerdown={startHintDrag}
		onpointermove={moveHintDrag}
		onpointerup={endHintDrag}
		onpointercancel={endHintDrag}
	>
		<GripHorizontal size={14} />
	</button>
	<div
		class="bg-base-200/30 text-base-content/60 flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs backdrop-blur-sm"
	>
		<MousePointerClick
			size={14}
			class={`shrink-0 opacity-60 ${clickFlash ? "text-info" : ""}`}
		/>
		<span>{m.home_background_change()}</span>
	</div>
	<div
		class="bg-base-200/30 text-base-content/60 flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs backdrop-blur-sm"
	>
		<Pin size={14} class={`shrink-0 opacity-60 ${isPinned ? "text-info" : ""}`} />
		<span>{m.home_background_pin()}</span>
	</div>
</div>

<!-- Global reset button -->
<div class="fixed top-8 left-8 z-50">
	<button
		type="button"
		class="btn btn-ghost btn-sm rounded-lg opacity-40 hover:opacity-80"
		aria-label="Reset all positions"
		onclick={resetAllHomePositions}
	>
		<RotateCcw size={16} />
	</button>
</div>

<HomeFloatingActions />

<style>
	.no-drag-select {
		-webkit-user-select: none !important;
		-webkit-user-drag: none !important;
		user-select: none !important;
	}
</style>
