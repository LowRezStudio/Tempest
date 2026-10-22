<script lang="ts">
	interface Props {
		/** Where the eyes look, -1 to 1 on each axis */
		look: { x: number; y: number };
	}

	let { look }: Props = $props();
</script>

<svg class="snowman" viewBox="0 0 100 120" aria-hidden="true">
	<ellipse cx="50" cy="116" rx="30" ry="4" fill="black" opacity="0.15" />
	<g stroke="#7c4a1e" stroke-width="2.5" stroke-linecap="round" fill="none">
		<path d="M27 84L12 74M16 77l-4 -6" />
		<g class="wave">
			<path d="M73 84l15-10M84 77l5-6" />
			<circle cx="89" cy="73" r="3.5" fill="#ef4444" stroke="none" />
		</g>
	</g>
	<circle cx="12" cy="74" r="3.5" fill="#ef4444" />
	<g fill="white" stroke="#cbd5e1" stroke-width="1">
		<circle cx="50" cy="90" r="25" />
		<circle cx="50" cy="50" r="22" />
	</g>
	<path d="M28 95a25 25 0 0 0 44 0 27 27 0 0 1-44 0z" fill="#e2e8f0" />
	<path d="M30 56a22 22 0 0 0 40 0 24 24 0 0 1-40 0z" fill="#e2e8f0" />

	<path d="M28 41a22 22 0 0 1 44 0z" fill="#ef4444" />
	<rect x="26" y="37" width="48" height="8" rx="4" fill="#f8fafc" />
	<circle cx="50" cy="17" r="6" fill="#f8fafc" />

	<g transform="translate({look.x * 1.8} {look.y * 1.4})">
		<g class="eyes" fill="#1e293b">
			<ellipse cx="42" cy="53" rx="3.2" ry="4" />
			<ellipse cx="58" cy="53" rx="3.2" ry="4" />
			<circle cx="43.3" cy="51.4" r="1.3" fill="white" />
			<circle cx="59.3" cy="51.4" r="1.3" fill="white" />
		</g>
	</g>
	<ellipse cx="35" cy="60" rx="3.6" ry="2" fill="#fda4af" opacity="0.75" />
	<ellipse cx="65" cy="60" rx="3.6" ry="2" fill="#fda4af" opacity="0.75" />
	<path d="M50 57l8 2-8 2.4z" fill="#fb923c" />
	<path
		d="M46 63q4 3 8 0"
		fill="none"
		stroke="#1e293b"
		stroke-width="1.6"
		stroke-linecap="round"
	/>

	<path d="M31 68q19 8 38 0v6q-19 8-38 0z" fill="#16a34a" />
	<path d="M55 73h8l2 16h-8z" fill="#15803d" />
	<path d="M56 79h8M57 84h8" stroke="#f8fafc" stroke-width="1.5" />
	<g fill="#1e293b">
		<circle cx="50" cy="86" r="2.2" />
		<circle cx="50" cy="96" r="2.2" />
	</g>
</svg>

<style>
	.snowman {
		display: block;
		filter: drop-shadow(0 2px 3px rgb(0 0 0 / 0.2));
	}

	.wave {
		transform-origin: 73px 84px;
		transform-box: view-box;
		animation: wave 0.9s ease-in-out infinite alternate;
	}

	@keyframes wave {
		from {
			transform: rotate(-10deg);
		}
		to {
			transform: rotate(16deg);
		}
	}

	.eyes {
		transform-origin: 0 53px;
		transform-box: view-box;
		animation: blink 4.5s infinite;
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
		.wave,
		.eyes {
			animation: none;
		}
	}
</style>
