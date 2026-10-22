<script lang="ts">
	import { ArrowRight } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import type { Snippet } from "svelte";

	interface Props {
		title: string;
		/** Shows a "See all" link on the right */
		onSeeAll?: () => void;
		linkLabel?: string;
		children: Snippet;
		class?: string;
	}

	let { title, onSeeAll, linkLabel, children, class: className = "" }: Props = $props();
</script>

<!-- A titled list with a link to the rest of it, after the mod website's Section -->
<section class={["min-w-0", className]}>
	<div class="mb-2 flex min-h-6 items-center justify-between gap-2">
		<h2 class="font-semibold">{title}</h2>
		{#if onSeeAll}
			<button class="btn btn-ghost btn-xs text-base-content/70" onclick={onSeeAll}>
				{linkLabel ?? m.common_see_all()}
				<ArrowRight size={12} />
			</button>
		{/if}
	</div>
	{@render children()}
</section>
