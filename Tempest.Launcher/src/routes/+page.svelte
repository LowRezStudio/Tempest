<script lang="ts">
	const backgrounds = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map((img) =>
		img.slice("/static".length),
	);

	const background = backgrounds[Math.floor(Math.random() * backgrounds.length)];
</script>

<div class="grid grid-cols-2 gap-2 md:grid-cols-4 lg:grid-cols-9">
	<button class="btn">Default</button>
	<button class="btn btn-neutral">Neutral</button>
	<button class="btn btn-primary">Primary</button>
	<button class="btn btn-secondary">Secondary</button>
	<button class="btn btn-accent">Accent</button>
	<button class="btn btn-info">Info</button>
	<button class="btn btn-success">Success</button>
	<button class="btn btn-warning">Warning</button>
	<button class="btn btn-error">Error</button>
</div>
