<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		title: string;
		subtitle?: string;
		loading?: boolean;
		children?: Snippet;
	}

	let { title, subtitle, loading, children }: Props = $props();
</script>

<div class="absolute inset-0 flex items-center justify-center z-30 bg-black/50 pointer-events-none">
	<div
		class="modal-box flex flex-col items-center gap-4 opacity-100 pointer-events-auto text-center"
	>
		<p class="font-semibold text-lg">
			{title}
		</p>

		{#if subtitle}
			{subtitle}
		{/if}
		{#if loading}
			<span class="loading loading-spinner loading-md"></span>
		{/if}
		{@render children?.()}
	</div>
</div>
