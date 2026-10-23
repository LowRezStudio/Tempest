<script lang="ts">
	import { Code2, Map as MapIcon } from "@lucide/svelte";
	import { bytes } from "$lib/studio/format";
	import { studio } from "$lib/studio/state.svelte";

	let filter = $state("");
	let kind = $state<"all" | "content" | "maps" | "script">("all");

	const visible = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return studio.packages.filter((p) => {
			if (kind === "maps" && !p.map) return false;
			if (kind === "script" && !p.script) return false;
			if (kind === "content" && (p.script || p.map)) return false;
			return q === "" || p.name.toLowerCase().includes(q);
		});
	});
</script>

<div class="pane-title">
	Packages
	<span class="ml-auto font-mono normal-case opacity-60"
		>{visible.length}/{studio.packages.length}</span
	>
</div>
<div class="border-base-300 flex gap-1 border-b p-2">
	<input
		class="input input-sm flex-1 font-mono text-xs"
		placeholder="Filter…"
		bind:value={filter}
	/>
	<select class="select select-sm w-24 text-xs" bind:value={kind}>
		<option value="all">All</option>
		<option value="content">Content</option>
		<option value="maps">Maps</option>
		<option value="script">Script</option>
	</select>
</div>
<div class="min-h-0 flex-1 overflow-y-auto">
	{#each visible as p (p.path)}
		<button
			class="row-hover flex w-full items-center gap-2 px-3 py-1 text-left text-xs {studio.pkg
				?.path === p.path
				? 'row-selected'
				: ''}"
			onclick={() => studio.openPackage(p.path)}
			ondblclick={() => studio.openPackage(p.path)}
			title={p.path}
		>
			<span class="w-3 shrink-0 opacity-60">
				{#if p.script}<Code2 size={12} />{:else if p.map}<MapIcon size={12} />{/if}
			</span>
			<span class="truncate font-mono">{p.name}</span>
			{#if studio.dirty.includes(p.path)}
				<span class="text-warning">•</span>
			{/if}
			<span class="ml-auto shrink-0 font-mono text-[10px] opacity-50">{bytes(p.size)}</span>
		</button>
	{/each}
</div>
