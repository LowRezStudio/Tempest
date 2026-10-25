<script lang="ts">
	import { fade } from "svelte/transition";
	import CursorFollower from "./CursorFollower.svelte";
</script>

<!-- The pumpkin only; the corner webs hang behind the pages (HalloweenWebs) -->
<div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	<CursorFollower
		gait="hop"
		width={64}
		throwable
		goo="#f97316"
		debris={["#ea580c", "#f97316", "#7c5a2e", "#84cc16"]}
	>
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
				<g class="mood snap" class:shown={look.face === "calm" || look.face === "wide"}>
					<g transform="translate({look.x * 1.6} {look.y * 1.4 + (look.away ? 1.5 : 0)})">
						<g class="pupils" class:wide={look.face === "wide"}>
							<g class="eyes" fill="#3b2414">
								<circle cx="25" cy="37" r="3.4" />
								<circle cx="39" cy="37" r="3.4" />
								<circle cx="26.2" cy="35.8" r="1.1" fill="white" />
								<circle cx="40.2" cy="35.8" r="1.1" fill="white" />
							</g>
						</g>
					</g>
				</g>
				<g class="mood" class:shown={look.face === "calm" && look.away}>
					<path
						d="M21 31.5l5.5-2M43 31.5l-5.5-2"
						fill="none"
						stroke="#3b2414"
						stroke-width="1.4"
						stroke-linecap="round"
					/>
				</g>
				<g fill="none" stroke="#3b2414" stroke-width="1.6" stroke-linecap="round">
					<path
						class="mood"
						class:shown={look.face === "calm" && !look.away}
						d="M29 43q3 3 6 0"
					/>
					<path
						class="mood"
						class:shown={look.face === "calm" && look.away}
						d="M29 45.5q3-2.5 6 0"
					/>
					<path
						class="mood snap"
						class:shown={look.face === "ouch"}
						d="M22.5 34.5l4 2.5-4 2.5M41.5 34.5l-4 2.5 4 2.5"
					/>
					<path
						class="mood snap"
						class:shown={look.face === "ouch" || look.face === "dizzy"}
						d="M28 45q1-1.5 2 0t2 0 2 0 2 0"
					/>
					<path
						class="mood snap"
						class:shown={look.face === "happy"}
						d="M22 38.5q3-4 6 0M36 38.5q3-4 6 0"
					/>
					<g class="mood snap" class:shown={look.face === "dizzy"} stroke-width="1.2">
						<path
							class="spiral"
							style="transform-origin: 25px 37px"
							d="M25 37a.75.75 0 0 1 1.5 0a1.5 1.5 0 0 1-3 0a2.25 2.25 0 0 1 4.5 0a3 3 0 0 1-6 0"
						/>
						<path
							class="spiral"
							style="transform-origin: 39px 37px"
							d="M39 37a.75.75 0 0 1 1.5 0a1.5 1.5 0 0 1-3 0a2.25 2.25 0 0 1 4.5 0a3 3 0 0 1-6 0"
						/>
					</g>
				</g>
				<ellipse
					class="mood snap"
					class:shown={look.face === "wide"}
					cx="32"
					cy="45.5"
					rx="2.2"
					ry="2.8"
					fill="#3b2414"
				/>
				<path
					class="mood snap"
					class:shown={look.face === "happy"}
					d="M27.5 42.5q4.5 5.5 9 0z"
					fill="#3b2414"
					stroke="#3b2414"
					stroke-width="1.2"
					stroke-linejoin="round"
				/>
				<ellipse cx="19.5" cy="43" rx="3.2" ry="1.8" fill="#fb7185" opacity="0.55" />
				<ellipse cx="44.5" cy="43" rx="3.2" ry="1.8" fill="#fb7185" opacity="0.55" />
				<!-- Blushing when petted or patted -->
				<g class="mood" class:shown={look.face === "happy"} fill="#fb7185" opacity="0.5">
					<ellipse cx="19.5" cy="42" rx="4" ry="2.4" />
					<ellipse cx="44.5" cy="42" rx="4" ry="2.4" />
				</g>
				{#if look.face === "dizzy"}
					<g
						transform="translate(32 5)"
						fill="#fde047"
						transition:fade={{ duration: 250 }}
					>
						{#each [0, -0.4, -0.8] as begin (begin)}
							<path d="M0-3l.9 2.1 2.1.9-2.1.9-.9 2.1-.9-2.1-2.1-.9 2.1-.9z">
								<animateMotion
									dur="1.2s"
									begin="{begin}s"
									repeatCount="indefinite"
									path="M-17 0a17 4.5 0 1 1 34 0a17 4.5 0 1 1-34 0"
								/>
							</path>
						{/each}
					</g>
				{/if}
			</svg>
		{/snippet}
	</CursorFollower>
</div>

<style>
	.pumpkin {
		display: block;
		overflow: visible;
		filter: drop-shadow(0 2px 3px rgb(0 0 0 / 0.25));
	}

	.mood {
		opacity: 0;
		transition: opacity 0.4s ease;
	}

	.mood.snap {
		transition-duration: 0.1s;
	}

	.mood.shown {
		opacity: 1;
	}

	.pupils {
		transform-origin: 32px 37px;
		transform-box: view-box;
		transition: transform 0.15s ease;
	}

	.pupils.wide {
		transform: scale(1.3);
	}

	.eyes {
		transform-origin: 0 37px;
		transform-box: view-box;
		animation: blink 5s infinite;
	}

	.spiral {
		transform-box: view-box;
		animation: spin 0.9s linear infinite;
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

	@keyframes spin {
		to {
			transform: rotate(360deg);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.eyes,
		.spiral {
			animation: none;
		}
	}
</style>
