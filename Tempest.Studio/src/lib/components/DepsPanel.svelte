<script lang="ts">
	import ClassIcon from "$lib/components/ClassIcon.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import type { Deps, ObjRef } from "../../../electron/src/ipc/contract";

	interface Props {
		path: string;
		index: number;
	}
	const { path, index }: Props = $props();

	let deps = $state<Deps | null>(null);
	let error = $state<string | null>(null);

	$effect(() => {
		void (async () => {
			try {
				deps = await rpc("object.deps", { path, index });
			} catch (error_) {
				error = error_ instanceof Error ? error_.message : String(error_);
			}
		})();
	});

	function go(r: ObjRef) {
		if (r.ref > 0) void studio.select(r.ref - 1);
	}
</script>

{#snippet ref(r: ObjRef, label: string)}
	<tr>
		<td class="w-24 opacity-60">{label}</td>
		<td>
			{#if r.ref > 0}
				<button
					class="link link-primary inline-flex items-center gap-1"
					onclick={() => go(r)}><ClassIcon className={r.class} /> {r.path}</button
				>
			{:else if r.ref < 0}
				<span class="inline-flex items-center gap-1 italic"
					><ClassIcon className={r.class} />
					{r.path} <span class="text-[10px] opacity-50">import</span></span
				>
			{:else}
				<span class="opacity-50">None</span>
			{/if}
			<span class="ml-1 text-[10px] opacity-40">{r.class}</span>
		</td>
	</tr>
{/snippet}

<div class="p-3 font-mono text-xs">
	{#if error}
		<p class="text-error">{error}</p>
	{:else if !deps}
		<span class="loading loading-sm loading-spinner"></span>
	{:else}
		<table class="table-xs table">
			<tbody>
				{@render ref(deps.class, "class")}
				{@render ref(deps.outer, "outer")}
				{@render ref(deps.archetype, "archetype")}
				{@render ref(deps.super, "super")}
			</tbody>
		</table>
		<h4 class="mt-3 mb-1 font-semibold opacity-70">References ({deps.references.length})</h4>
		<table class="table-xs table">
			<tbody>
				{#each deps.references as r, k (k)}
					{@render ref(r, "")}
				{/each}
			</tbody>
		</table>
		<h4 class="mt-3 mb-1 font-semibold opacity-70">Referenced by ({deps.referrers.length})</h4>
		<table class="table-xs table">
			<tbody>
				{#each deps.referrers as r, k (k)}
					{@render ref(r, "")}
				{/each}
			</tbody>
		</table>
	{/if}
</div>
