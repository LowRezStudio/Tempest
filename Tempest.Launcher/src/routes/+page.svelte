<script lang="ts">
	import HomeFloatingActions from "$lib/components/home/HomeFloatingActions.svelte";
	import { pinnedBackground } from "$lib/stores/settings";
	import { addToast } from "$lib/stores/ui";
	import { fade } from "svelte/transition";

	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map((img) =>
		img.replace("/static", ""),
	);

	const getRandomBackground = () => backgrounds[Math.floor(Math.random() * backgrounds.length)];

	let currentBackground = $state($pinnedBackground || getRandomBackground());

	function changeBackground() {
		if ($pinnedBackground) return;
		if (backgrounds.length <= 1) return;
		let next = getRandomBackground();
		while (next === currentBackground) {
			next = getRandomBackground();
		}
		currentBackground = next;
	}

	function pinBackground(event: MouseEvent) {
		event.preventDefault();
		if ($pinnedBackground === currentBackground) {
			pinnedBackground.set(undefined);
			addToast({
				message: "Background unpinned!",
				tone: "info",
			});
		} else {
			pinnedBackground.set(currentBackground);
			addToast({
				message: "Background pinned as the default one!",
				tone: "success",
			});
		}
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
	class="absolute inset-0 z-0 select-none overflow-hidden no-drag-select"
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
			class="absolute inset-0 h-full w-full object-cover object-center brightness-75 pointer-events-none select-none no-drag-select"
			src={currentBackground}
			alt="background"
			draggable="false"
			ondragstart={(e) => e.preventDefault()}
			onselectstart={(e) => e.preventDefault()}
		/>
	{/key}
</div>

<div class="relative top-0 left-0 z-10 h-full p-2 pointer-events-none"></div>

<HomeFloatingActions />

<style>
	.no-drag-select {
		-webkit-user-select: none !important;
		-webkit-user-drag: none !important;
		user-select: none !important;
	}
</style>
