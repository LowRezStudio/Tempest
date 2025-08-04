<script lang="ts">
	import type { HTMLInputAttributes } from "svelte/elements";

	interface Props extends HTMLInputAttributes {
		value?: string;
	}

	let { value = $bindable(), ...props }: Props = $props();
</script>

<input bind:value {...props}>

<style>
	input {
		background: linear-gradient(#0f92cecc, #1f72a4cc, #137da9cc);
		box-shadow:
			rgba(0, 0, 0, 0.6) 0px 3px 6px 0px,
			rgba(0, 0, 0, 0.3) 0px 3px 6px 0px;
		color: inherit;
		border: 2px solid var(--color-primary);
		border-radius: var(--border-radius);
		padding: 0.5rem 1rem;
		font-size: 15px;
		font-weight: 600;
	}

	input:hover {
		background: linear-gradient(#1cc6fbcc, #0194d4cc, #00a2dacc);
		box-shadow: #0f92ce 0px 0px 1rem 0.1rem;
		border-color: #1cc6fb;
	}

	input:active {
		background: linear-gradient(#0f92cecc, #1f72a4cc, #137da9cc);
		box-shadow: #0f92ce 0px 0px 1rem 0.1rem;
		border-color: #0f92ce;
	}

	input:focus {
		outline: 2px solid #1cc6fb;
		outline-offset: -2px;
	}

	input:disabled {
		opacity: 0.5;
		cursor: not-allowed;
		background: linear-gradient(#666666cc, #555555cc, #444444cc);
		border-color: #666666;
		box-shadow: none;
	}

	input:disabled:hover {
		background: linear-gradient(#666666cc, #555555cc, #444444cc);
		border-color: #666666;
		box-shadow: none;
	}
</style>
