<script lang="ts">
	import { untrack } from "svelte";
	import type { Weather } from "$lib/theme/weather.svelte";

	interface Props {
		kind: Weather;
	}

	let { kind }: Props = $props();

	const rand = (min: number, max: number) => min + Math.random() * (max - min);
	const oneOf = (items: string[]) => items[Math.floor(Math.random() * items.length)]!;

	const leafShapes = {
		maple: "M12 2l1.6 3.6 3.4-1.2-1 3.8 4-.6-2.4 3.2 3.4 1.6-3.8 1.4.8 3.2-3.6-1.4-.4 3.8h-.2V22h-1.6v-2.6h-.2l-.4-3.8-3.6 1.4.8-3.2-3.8-1.4 3.4-1.6L3 7.6l4 .6-1-3.8 3.4 1.2z",
		oval: "M12 2C7 6 4.5 10 4.5 14a7.5 7.5 0 0 0 15 0C19.5 10 17 6 12 2z",
	};
	const colors = {
		leaves: ["#c2410c", "#d97706", "#b91c1c", "#ca8a04", "#9a3412"],
		petals: ["#f9a8d4", "#fbcfe8", "#f472b6"],
	};

	// Built once, so toggling other state never reshuffles the sky.
	const particles = untrack(() => {
		const count = { snow: 50, leaves: 12, petals: 40, fireflies: 30 }[kind];
		const sizes = { snow: [2, 6], leaves: [14, 24], petals: [10, 16], fireflies: [3, 5] }[kind];
		return Array.from({ length: count }, () => ({
			left: rand(0, 100),
			top: rand(5, 95),
			size: rand(sizes[0]!, sizes[1]!),
			duration: kind === "fireflies" ? rand(4, 9) : rand(12, 24),
			// Negative, so the sky is already full when it appears
			delay: -rand(0, 24),
			drift: rand(-100, 100) * (kind === "snow" ? 1 : 1.6),
			spin: rand(2, 5),
			shape: kind === "leaves" ? oneOf([leafShapes.maple, leafShapes.oval]) : leafShapes.oval,
			color: kind === "leaves" || kind === "petals" ? oneOf(colors[kind]) : undefined,
			opacity: rand(0.55, 0.95),
		}));
	});
</script>

<div class="weather pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	{#each particles as p, i (i)}
		{#if kind === "fireflies"}
			<span
				class="firefly"
				style="left: {p.left}%; top: {p.top}%; width: {p.size}px; height: {p.size}px; --wander: {p.duration}s; --blink: {p.spin}s; --drift: {p.drift}px; animation-delay: {p.delay}s;"
			></span>
		{:else}
			<span
				class="faller"
				style="left: {p.left}%; opacity: {p.opacity}; animation-duration: {p.duration}s; animation-delay: {p.delay}s; --drift: {p.drift}px;"
			>
				{#if kind === "snow"}
					<span class="flake" style="width: {p.size}px; height: {p.size}px;"></span>
				{:else}
					<svg
						class="leaf"
						viewBox="0 0 24 24"
						style="width: {p.size}px; height: {p.size}px; color: {p.color}; animation-duration: {p.spin}s;"
					>
						<path fill="currentColor" d={p.shape} />
					</svg>
				{/if}
			</span>
		{/if}
	{/each}
</div>

<style>
	.weather {
		container-type: size;
	}

	.faller {
		position: absolute;
		top: -24px;
		animation: fall linear infinite;
	}

	@keyframes fall {
		to {
			transform: translate(var(--drift), calc(100cqh + 48px));
		}
	}

	.flake {
		display: block;
		border-radius: 9999px;
		background: white;
		/* Keeps the flakes visible over light themes too */
		box-shadow: 0 0 2px rgb(0 0 0 / 0.3);
	}

	.leaf {
		display: block;
		filter: drop-shadow(0 1px 1px rgb(0 0 0 / 0.25));
		animation: tumble linear infinite;
	}

	@keyframes tumble {
		to {
			transform: rotate(360deg) rotateX(360deg);
		}
	}

	.firefly {
		position: absolute;
		border-radius: 9999px;
		background: #fde68a;
		box-shadow: 0 0 6px 2px rgb(253 224 71 / 0.7);
		animation:
			wander var(--wander) ease-in-out infinite alternate,
			blink var(--blink) ease-in-out infinite;
	}

	@keyframes wander {
		to {
			transform: translate(var(--drift), calc(var(--drift) * -0.6));
		}
	}

	@keyframes blink {
		0%,
		100% {
			opacity: 0.2;
		}
		50% {
			opacity: 1;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.weather {
			display: none;
		}
	}
</style>
