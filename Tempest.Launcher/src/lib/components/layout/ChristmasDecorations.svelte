<script lang="ts">
	const baubles = [
		{ x: 30, y: 97, color: "#ef4444" },
		{ x: 58, y: 105, color: "#3b82f6" },
		{ x: 73, y: 93, color: "#f59e0b" },
		{ x: 39, y: 77, color: "#f59e0b" },
		{ x: 62, y: 72, color: "#ef4444" },
		{ x: 46, y: 55, color: "#3b82f6" },
	];
	const lights = [
		[22, 108],
		[40, 111],
		[66, 110],
		[81, 105],
		[27, 84],
		[51, 90],
		[71, 82],
		[37, 62],
		[61, 61],
		[52, 44],
	].map(([x, y], i) => ({
		x,
		y,
		color: ["#fde047", "#f472b6", "#67e8f9", "#fb923c"][i % 4],
		delay: (i * 0.37) % 1.5,
	}));
</script>

{#snippet gift(size: number, color: string, ribbon: string)}
	<svg viewBox="0 0 40 40" class="gift" style="width: {size}px;">
		<rect x="5" y="16" width="30" height="22" rx="2" fill={color} />
		<rect x="5" y="17" width="30" height="3" fill="black" opacity="0.15" />
		<rect x="3" y="11" width="34" height="7" rx="2" fill={color} />
		<rect x="17.5" y="11" width="5" height="27" fill={ribbon} />
		<path
			d="M20 11c-4-7-12-6-9-1 1.5 2 5 1.5 9 1zM20 11c4-7 12-6 9-1-1.5 2-5 1.5-9 1z"
			fill={ribbon}
		/>
	</svg>
{/snippet}

<div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	<div class="absolute bottom-0 left-2 flex items-end">
		<svg viewBox="0 0 100 130" class="tree w-24">
			<rect x="44" y="108" width="12" height="16" rx="2" fill="#7c4a1e" />
			<path d="M10 112L50 62L90 112Q50 120 10 112z" fill="#15803d" />
			<path d="M18 86L50 40L82 86Q50 94 18 86z" fill="#16a34a" />
			<path d="M27 60L50 20L73 60Q50 67 27 60z" fill="#22c55e" />
			<g fill="none" stroke="#facc15" stroke-width="1.8" stroke-linecap="round">
				<path d="M34 52Q50 60 66 50" />
				<path d="M25 77Q50 89 76 74" />
				<path d="M17 102Q52 116 86 100" />
			</g>
			{#each baubles as bauble (bauble.x)}
				<circle cx={bauble.x} cy={bauble.y} r="3.6" fill={bauble.color} />
				<circle cx={bauble.x - 1.1} cy={bauble.y - 1.1} r="1" fill="white" opacity="0.7" />
			{/each}
			{#each lights as light (light.x)}
				<circle
					class="light"
					cx={light.x}
					cy={light.y}
					r="1.9"
					fill={light.color}
					style="animation-delay: {light.delay}s;"
				/>
			{/each}
			<path
				class="star"
				d="M50 9L52.35 14.76L58.56 15.22L53.8 19.24L55.29 25.28L50 22L44.71 25.28L46.2 19.24L41.44 15.22L47.65 14.76Z"
				fill="#fde047"
			/>
		</svg>
		<span class="-ml-5">{@render gift(34, "#dc2626", "#facc15")}</span>
		<span class="-ml-1">{@render gift(26, "#2563eb", "#f8fafc")}</span>
	</div>

	<div class="absolute right-3 bottom-0 flex items-end">
		<span class="relative">
			<span class="absolute -top-3 left-3">{@render gift(24, "#16a34a", "#ef4444")}</span>
			{@render gift(40, "#9333ea", "#fde047")}
		</span>
		<span class="-ml-1">{@render gift(28, "#f59e0b", "#dc2626")}</span>
	</div>
</div>

<style>
	.tree,
	.gift {
		display: block;
		filter: drop-shadow(0 2px 3px rgb(0 0 0 / 0.25));
	}

	.light {
		animation: twinkle 1.5s ease-in-out infinite alternate;
	}

	@keyframes twinkle {
		from {
			opacity: 1;
		}
		to {
			opacity: 0.3;
		}
	}

	.star {
		transform-origin: 50px 17px;
		transform-box: view-box;
		filter: drop-shadow(0 0 3px rgb(253 224 71 / 0.9));
		animation: shine 2.4s ease-in-out infinite alternate;
	}

	@keyframes shine {
		to {
			transform: scale(1.15);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.light,
		.star {
			animation: none;
		}
	}
</style>
