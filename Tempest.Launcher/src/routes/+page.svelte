<script lang="ts">
	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map((img) =>
		img.slice("/static".length),
	);

	const background = backgrounds[Math.floor(Math.random() * backgrounds.length)];
</script>

<div class="pointer-events-none relative top-0 left-0 h-0">
	<img
		class="h-screen w-full object-cover object-center brightness-75"
		src={background}
		alt="background"
	/>
</div>

<div class="relative top-0 left-0 z-10 h-full p-2"></div>
