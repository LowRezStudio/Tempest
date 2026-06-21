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

<div class="flex flex-col items-center justify-center h-80 gap-4 text-center {className}">
	<div class="opacity-30">
		{@render icon()}
	</div>
	<div>
		<p class="text-lg font-semibold text-base-content/70">{title}</p>
		{#if description}
			<p class="text-sm text-base-content/50 max-w-md mt-1">
				{description}
			</p>
		{/if}
	</div>
	{#if actions}
		{@render actions()}
	{/if}
</div>
