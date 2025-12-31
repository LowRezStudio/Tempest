<script lang="ts">
	import HomeFloatingActions from "$lib/components/home/HomeFloatingActions.svelte";

	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map((img) =>
		img.replace("/static", ""),
	);

	const background = backgrounds[Math.floor(Math.random() * backgrounds.length)];
</script>

<div class="pointer-events-none fixed top-0 left-0 w-full h-full">
	<img
		class="h-screen w-full object-cover object-center brightness-75"
		src={background}
		alt="background"
	/>
</div>

<div class="relative top-0 left-0 z-10 h-full p-2"></div>

<HomeFloatingActions />
