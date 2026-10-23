<script lang="ts">
	import type { Settings } from "@lucide/svelte";
	import type { Snippet } from "svelte";

	interface Props {
		icon: typeof Settings;
		title: string;
		hint?: string;
		/** The button that does it */
		children: Snippet;
	}

	let { icon: Icon, title, hint, children }: Props = $props();
</script>

<!-- A setting that is done rather than switched, laid out like SwitchRow with a button for the toggle -->
<div class="bg-base-100 border-base-300 rounded-field flex items-center gap-3 border p-3">
	<span
		class="bg-base-300 text-base-content/60 flex size-8 shrink-0 items-center justify-center rounded-full"
		><Icon size={16} /></span
	>
	<span class="min-w-0 flex-1">
		<span class="block text-sm font-medium">{title}</span>
		{#if hint}<span class="text-base-content/60 block text-xs">{hint}</span>{/if}
	</span>
	{@render children()}
</div>
