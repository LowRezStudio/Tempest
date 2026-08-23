<script lang="ts">
	import { MousePointerClick, Pin } from "@lucide/svelte";
	import { fade } from "svelte/transition";
	import HomeFloatingActions from "$lib/components/home/HomeFloatingActions.svelte";
	import { m } from "$lib/paraglide/messages";
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

<div class="pointer-events-none fixed top-6 right-6 z-40 flex flex-col items-end gap-1.5">
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

<HomeFloatingActions />

<style>
	.no-drag-select {
		-webkit-user-select: none !important;
		-webkit-user-drag: none !important;
		user-select: none !important;
	}
</style>
