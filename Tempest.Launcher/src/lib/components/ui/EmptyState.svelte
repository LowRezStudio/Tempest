<script lang="ts">
	import type { Snippet } from "svelte";

	interface Props {
		title: string;
		description?: string;
		icon: Snippet;
		actions?: Snippet;
		class?: string;
	}

	let { title, description, icon, actions, class: className = "" }: Props = $props();
</script>

<div class="flex h-80 flex-col items-center justify-center gap-4 text-center {className}">
	<div class="opacity-30">
		{@render icon()}
	</div>
	<div>
		<p class="text-base-content/70 text-lg font-semibold">{title}</p>
		{#if description}
			<p class="text-base-content/50 mt-1 max-w-md text-sm">
				{description}
			</p>
		{/if}
	</div>
	{#if actions}
		{@render actions()}
	{/if}
</div>
