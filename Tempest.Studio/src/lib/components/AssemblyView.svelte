<!-- Tables | grid | row: the side panes resizable and collapsible (Ctrl+B,
	Ctrl+J); on narrow windows the row pane floats over the grid. -->
<script lang="ts">
	import { Database, X } from "@lucide/svelte";
	import AssemblyGrid from "$lib/components/AssemblyGrid.svelte";
	import AssemblyRecord from "$lib/components/AssemblyRecord.svelte";
	import AssemblyTables from "$lib/components/AssemblyTables.svelte";
	import Pane from "$lib/components/shell/Pane.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import { ui } from "$lib/studio/ui.svelte";

	$effect(() => {
		// Open the workspace's assembly the first time the view shows.
		if (studio.workspace && !assembly.summary && !assembly.loading) void assembly.open();
	});

	// Narrow windows: the floating row pane opens with a selection.
	let lastSelected: number | null = null;
	$effect(() => {
		const s = assembly.selected;
		if (s !== lastSelected) {
			lastSelected = s;
			if (ui.compact && s !== null) ui.toggle("details", false);
		}
	});
	const showRecord = $derived(
		!ui.isCollapsed("details") && (!ui.compact || assembly.selected !== null),
	);
</script>

{#if assembly.summary}
	<div class="relative flex min-h-0 flex-1">
		{#if !ui.isCollapsed("side")}
			<Pane
				id="asm.tables"
				side="left"
				initial={250}
				min={170}
				max={480}
				overlay={ui.compact}
			>
				<AssemblyTables />
			</Pane>
		{/if}
		<div class="flex min-w-0 flex-1 flex-col">
			<AssemblyGrid />
		</div>
		{#if showRecord}
			<Pane
				id="asm.record"
				side="right"
				initial={ui.compact ? 380 : 440}
				min={280}
				max={900}
				overlay={ui.compact}
			>
				{#if ui.compact}
					<button
						class="btn btn-ghost btn-xs btn-square absolute top-1.5 right-1.5 z-20"
						onclick={() => ui.toggle("details", true)}
						title="Close the row pane (Ctrl+J)"
						aria-label="Close the row pane"
					>
						<X size={13} />
					</button>
				{/if}
				<AssemblyRecord />
			</Pane>
		{/if}
	</div>
{:else}
	<div class="flex flex-1 items-center justify-center">
		<div class="flex flex-col items-center gap-3 text-sm opacity-70">
			<Database size={32} />
			{#if assembly.loading}
				<span class="loading loading-spinner"></span>
				Loading assembly.dat and the language files…
			{:else}
				<span>No assembly.dat next to this workspace's packages.</span>
				<button class="btn btn-sm" onclick={() => assembly.open()}>Retry</button>
			{/if}
		</div>
	</div>
{/if}
