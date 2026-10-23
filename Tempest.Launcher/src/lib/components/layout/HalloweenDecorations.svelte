<script lang="ts">
	import CursorFollower from "./CursorFollower.svelte";

	// A corner web: spokes out of the corner and threads sagging toward it between them.
	const spokes = 7;
	const angle = (i: number) => (Math.PI / 2) * (1 + i / (spokes - 1));
	const at = (r: number, a: number) =>
		`${(100 + r * Math.cos(a)).toFixed(1)} ${(r * Math.sin(a)).toFixed(1)}`;
	let web = "";
	for (let i = 0; i < spokes; i++) web += `M100 0L${at(100, angle(i))}`;
	for (const r of [16, 30, 45, 60, 76, 92]) {
		for (let i = 0; i < spokes - 1; i++) {
			const sag = at(r * 0.82, (angle(i) + angle(i + 1)) / 2);
			web += `M${at(r, angle(i))}Q${sag} ${at(r, angle(i + 1))}`;
		}
	}
</script>

<div class="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true">
	<svg class="text-base-content/25 absolute top-0 right-0 size-32" viewBox="0 0 100 100">
		<path d={web} fill="none" stroke="currentColor" stroke-width="0.7" />
	</svg>
	<svg
		class="text-base-content/25 absolute right-0 bottom-0 size-40 -scale-y-100"
		viewBox="0 0 100 100"
	>
		<path d={web} fill="none" stroke="currentColor" stroke-width="0.6" />
	</svg>

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
				<g transform="translate({look.x * 1.6} {look.y * 1.4})">
					<g class="eyes" fill="#3b2414">
						<circle cx="25" cy="37" r="3.4" />
						<circle cx="39" cy="37" r="3.4" />
						<circle cx="26.2" cy="35.8" r="1.1" fill="white" />
						<circle cx="40.2" cy="35.8" r="1.1" fill="white" />
					</g>
				</g>
				<path
					d="M29 43q3 3 6 0"
					fill="none"
					stroke="#3b2414"
					stroke-width="1.6"
					stroke-linecap="round"
				/>
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
