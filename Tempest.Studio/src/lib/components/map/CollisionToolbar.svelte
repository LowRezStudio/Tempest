<!-- The map toolbar's Collision switch: draw what pawns and shots hit
	(`collision.svelte.ts`) over the render geometry or instead of it, with
	the colour legend. -->
<script lang="ts">
	import { BLOCK_COLORS, BLOCK_LABELS, collision } from "$lib/studio/collision.svelte";
</script>

<button
	class="btn btn-xs ml-2 {collision.enabled ? 'btn-active btn-warning' : ''}"
	onclick={() => void collision.toggle()}
	title="Draw collision: volumes' convex hulls and meshes' simple collision (or their triangles when they use complex collision)"
	>Collision{#if collision.loading}&nbsp;<span class="loading loading-spinner loading-xs"
		></span>{/if}</button
>
{#if collision.enabled}
	<label class="ml-1 flex items-center gap-1" title="Hide the render geometry">
		<input type="checkbox" class="checkbox checkbox-xs" bind:checked={collision.only} />
		Only
	</label>
	<label
		class="ml-1 flex items-center gap-1"
		title="Leave out meshes and volumes that collide with nothing (triggers stay)"
	>
		<input
			type="checkbox"
			class="checkbox checkbox-xs"
			checked={collision.hideNone}
			onchange={(e) => {
				collision.hideNone = e.currentTarget.checked;
				collision.rebuild();
			}}
		/>
		Blocking only
	</label>
	<span class="ml-1 flex items-center gap-1">
		{#each Object.entries(BLOCK_COLORS) as [k, c] (k)}
			<span
				class="h-2.5 w-2.5 rounded-sm"
				style="background:{c}"
				title={BLOCK_LABELS[k as keyof typeof BLOCK_LABELS]}
			></span>
		{/each}
	</span>
{/if}
