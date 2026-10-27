<script lang="ts">
	import ClassIcon from "$lib/components/ClassIcon.svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { onUpkEvent, rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import { ui } from "$lib/studio/ui.svelte";
	import type { SearchHit } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let query = $state("");
	let className = $state("");
	let hits = $state<SearchHit[]>([]);
	let truncated = $state(false);
	let searching = $state(false);
	let progress = $state<{ done: number; total: number } | null>(null);
	let searched = $state(false);

	$effect(() => {
		if (!open) return;
		const stop = onUpkEvent((e) => {
			if (e.event === "search.progress") {
				progress = { done: e.done as number, total: e.total as number };
			}
		});
		return stop;
	});

	// Opened from the palette with a query: run it.
	$effect(() => {
		if (open && ui.searchQuery) {
			query = ui.searchQuery;
			ui.searchQuery = "";
			void search();
		}
	});

	async function search(event?: Event) {
		event?.preventDefault();
		if (query.trim().length < 2 && !className.trim()) return;
		searching = true;
		progress = null;
		try {
			const r = await rpc("workspace.search", {
				query: query.trim(),
				class: className.trim() || undefined,
				limit: 1000,
			});
			hits = r.results;
			truncated = r.truncated;
			searched = true;
		} catch (error) {
			toasts.error(error, "Search failed");
		} finally {
			searching = false;
			progress = null;
		}
	}

	// The result is shown in the Packages view, whichever view the search
	// was opened from.
	async function go(hit: SearchHit) {
		open = false;
		await studio.visit(hit.path, hit.index);
	}

	let input = $state<HTMLInputElement>();
	$effect(() => {
		if (open && input) {
			input.focus();
			input.select();
		}
	});
</script>

<Modal bind:open title="Search every package" width="max-w-4xl">
	<form class="flex gap-2" onsubmit={search}>
		<input
			class="input input-sm flex-1 font-mono"
			placeholder="object path contains… (e.g. androxus_arms)"
			bind:value={query}
			bind:this={input}
		/>
		<input
			class="input input-sm w-48 font-mono"
			placeholder="class (exact)"
			bind:value={className}
		/>
		<button class="btn btn-sm btn-primary" type="submit" disabled={searching}>
			{#if searching}<span class="loading loading-xs loading-spinner"></span>{/if}
			Search
		</button>
	</form>
	<p class="mt-1 text-[11px] opacity-60">
		The first search reads the export table of every package in the folder (a few seconds;
		cached afterwards).
		{#if progress}· indexing {progress.done}/{progress.total}{/if}
	</p>
	<div class="mt-3 max-h-[55vh] overflow-auto">
		{#if searched && hits.length === 0}
			<p class="text-sm opacity-60">No match.</p>
		{/if}
		{#each hits as hit, k (k)}
			<button
				class="row-hover flex w-full items-center gap-2 px-2 py-1 text-left font-mono text-xs"
				onclick={() => go(hit)}
			>
				<ClassIcon className={hit.class} />
				<span class="w-44 shrink-0 truncate opacity-70">{hit.package}</span>
				<span class="truncate">{hit.object}</span>
				<span class="ml-auto shrink-0 text-[10px] opacity-50">{hit.class}</span>
			</button>
		{/each}
		{#if truncated}
			<p class="mt-2 text-xs opacity-60">More matches exist; narrow the query.</p>
		{/if}
	</div>
</Modal>
