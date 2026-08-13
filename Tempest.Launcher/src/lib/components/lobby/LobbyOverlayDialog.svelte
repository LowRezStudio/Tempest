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

<div class="pointer-events-none absolute inset-0 z-30 flex items-center justify-center bg-black/50">
	<div
		class="modal-box pointer-events-auto flex flex-col items-center gap-4 text-center opacity-100"
	>
		<p class="text-lg font-semibold">
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
