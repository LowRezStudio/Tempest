<script lang="ts">
	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp"))
		.map(img => img.slice("/static".length));

	const background = backgrounds[Math.floor(Math.random() * backgrounds.length)];
</script>

<div class="relative left-0 top-0 h-0 pointer-events-none">
	<img
		class="brightness-75 h-screen w-full object-cover object-center"
		src={background}
		alt="background"
	/>
</div>

<div class="relative left-0 top-0 p-2 h-full z-10"></div>
