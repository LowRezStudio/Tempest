<script lang="ts">
	import { modIconFallback } from "$lib/mods/listing";
	import type { RepoMod } from "$lib/queries/repo";

	interface Props {
		mod: Pick<RepoMod, "id" | "name" | "icon">;
		size?: number;
		class?: string;
	}

	let { mod, size = 40, class: className = "" }: Props = $props();

	const fallback = $derived(modIconFallback(mod.id, mod.name));
</script>

{#if mod.icon}
	<img
		src={mod.icon}
		alt=""
		width={size}
		height={size}
		class="shrink-0 rounded-lg object-cover {className}"
	/>
{:else}
	<div
		class="flex shrink-0 items-center justify-center rounded-lg font-bold text-white {className}"
		style="width:{size}px;height:{size}px;font-size:{size *
			0.36}px;background:{fallback.background}"
		aria-hidden="true"
	>
		{fallback.initials}
	</div>
{/if}
