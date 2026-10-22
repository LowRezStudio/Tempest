<script lang="ts">
	import { studio } from "$lib/studio/state.svelte";
</script>

<div
	class="border-base-300 bg-base-200 flex h-6 shrink-0 items-center gap-3 border-t px-3 font-mono text-[11px] opacity-80"
>
	{#if studio.busy}
		<span class="loading loading-xs loading-spinner"></span>
		<span>{studio.busy}</span>
	{:else if studio.backendError}
		<span class="text-error">{studio.backendError}</span>
	{:else if studio.pkg}
		<span>{studio.pkg.path}</span>
		<span class="opacity-60">
			{studio.pkg.summary.exports} exports · {studio.pkg.summary.imports} imports · {studio
				.pkg.summary.names} names · {studio.pkg.summary.compressed
				? "compressed"
				: "uncompressed"}{studio.pkg.summary.obscured ? " + obscured" : ""}
		</span>
	{:else}
		<span class="opacity-60">{studio.workspace?.dir}</span>
	{/if}
	<div class="flex-1"></div>
	{#if studio.workspace}
		<span class="opacity-60">
			{studio.workspace.textureIndex ? "texture index ok" : "no MergedFileIndexCache.bin"}
		</span>
	{/if}
</div>
