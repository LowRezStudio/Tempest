<script lang="ts">
	import { assembly } from "$lib/studio/assembly.svelte";
	import { basename } from "$lib/studio/format";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	const ago = (t: number) => {
		const s = Math.round((Date.now() - t) / 1000);
		return s < 60
			? "just now"
			: s < 3600
				? `${Math.round(s / 60)} min ago`
				: `${Math.round(s / 3600)} h ago`;
	};
	let now = $state(Date.now());
	$effect(() => {
		const t = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(t);
	});
</script>

<footer
	class="border-base-300 bg-base-200 flex h-6 shrink-0 items-center gap-3 overflow-hidden border-t px-3 font-mono text-[11px] whitespace-nowrap"
>
	{#if studio.busy}
		<span class="loading loading-xs loading-spinner"></span>
		<span class="truncate">{studio.busy}</span>
	{:else if studio.backendError}
		<span class="text-error truncate">{studio.backendError}</span>
	{:else if studio.mode === "assembly" && assembly.doc}
		<span class="truncate opacity-80" title={assembly.doc.path}>{assembly.doc.path}</span>
		{#if assembly.page}
			<span class="opacity-50 max-md:hidden">{assembly.page.total} rows</span>
		{/if}
	{:else if studio.pkg}
		<span class="truncate opacity-80" title={studio.pkg.path}>{studio.pkg.path}</span>
		<span class="opacity-50 max-lg:hidden">
			{studio.pkg.summary.exports} exports · {studio.pkg.summary.imports} imports · {studio
				.pkg.summary.names} names · {studio.pkg.summary.compressed
				? "compressed"
				: "uncompressed"}{studio.pkg.summary.obscured ? " + obscured" : ""}
		</span>
	{:else}
		<span class="truncate opacity-60">{studio.workspace?.dir}</span>
	{/if}
	<div class="flex-1"></div>
	{#if session.file}
		<span class="opacity-60 max-md:hidden" title={session.file.path}>
			{basename(session.file.path)} ({session.file.mode}){session.savedAt
				? ` · saved ${(void now, ago(session.savedAt))}`
				: ""}
		</span>
	{/if}
	{#if ui.zoom !== 1}
		<button
			class="opacity-60 hover:opacity-100"
			onclick={() => ui.setZoom(1)}
			title="Reset zoom (Ctrl+0)"
		>
			{Math.round(ui.zoom * 100)}%
		</button>
	{/if}
	{#if studio.workspace}
		<span class="opacity-50 max-xl:hidden">
			{studio.workspace.textureIndex ? "texture index ok" : "no MergedFileIndexCache.bin"}
		</span>
	{/if}
</footer>
