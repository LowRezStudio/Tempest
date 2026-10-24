<script lang="ts">
	import CursorFollower from "./CursorFollower.svelte";
</script>

<!-- The pumpkin only; the corner webs hang behind the pages (HalloweenWebs) -->
<div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	<CursorFollower gait="hop" width={64}>
		{#snippet children(look)}
			<svg class="pumpkin" viewBox="0 0 64 60">
				<path
					d="M31 18q-3-6 2-11"
					fill="none"
					stroke="#7c5a2e"
					stroke-width="4"
					stroke-linecap="round"
				/>
				<path d="M34 12q7-8 15-2-7 7-15 2z" fill="#84cc16" />
				<path
					d="M29 16q-7-5-10 0t3 4"
					fill="none"
					stroke="#65a30d"
					stroke-width="1.5"
					stroke-linecap="round"
				/>
				<ellipse cx="20" cy="38" rx="15" ry="17" fill="#ea580c" />
				<ellipse cx="44" cy="38" rx="15" ry="17" fill="#ea580c" />
				<ellipse cx="32" cy="37" rx="16" ry="19" fill="#f97316" />
				<ellipse cx="26" cy="27" rx="5" ry="2.5" fill="#fdba74" opacity="0.6" />
				<!-- With the cursor out of the window it looks down, brows and mouth drooping -->
				<g transform="translate({look.x * 1.6} {look.y * 1.4 + (look.away ? 1.5 : 0)})">
					<g class="eyes" fill="#3b2414">
						<circle cx="25" cy="37" r="3.4" />
						<circle cx="39" cy="37" r="3.4" />
						<circle cx="26.2" cy="35.8" r="1.1" fill="white" />
						<circle cx="40.2" cy="35.8" r="1.1" fill="white" />
					</g>
				</g>
				<g class="mood" class:shown={look.away}>
					<path
						d="M21 31.5l5.5-2M43 31.5l-5.5-2"
						fill="none"
						stroke="#3b2414"
						stroke-width="1.4"
						stroke-linecap="round"
					/>
				</g>
				<g fill="none" stroke="#3b2414" stroke-width="1.6" stroke-linecap="round">
					<path class="mood" class:shown={!look.away} d="M29 43q3 3 6 0" />
					<path class="mood" class:shown={look.away} d="M29 45.5q3-2.5 6 0" />
				</g>
				<ellipse cx="19.5" cy="43" rx="3.2" ry="1.8" fill="#fb7185" opacity="0.55" />
				<ellipse cx="44.5" cy="43" rx="3.2" ry="1.8" fill="#fb7185" opacity="0.55" />
			</svg>
		{/snippet}
	</CursorFollower>
</div>

<style>
	.pumpkin {
		display: block;
		filter: drop-shadow(0 2px 3px rgb(0 0 0 / 0.25));
	}

	.mood {
		opacity: 0;
		transition: opacity 0.4s ease;
	}

	.mood.shown {
		opacity: 1;
	}

	.eyes {
		transform-origin: 0 37px;
		transform-box: view-box;
		animation: blink 5s infinite;
	}

	@keyframes blink {
		0%,
		94%,
		100% {
			transform: scaleY(1);
		}
		97% {
			transform: scaleY(0.1);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.eyes {
			animation: none;
		}
	}
</style>
