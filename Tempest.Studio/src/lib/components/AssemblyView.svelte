<script lang="ts">
	import { Database } from "@lucide/svelte";
	import AssemblyGrid from "$lib/components/AssemblyGrid.svelte";
	import AssemblyRecord from "$lib/components/AssemblyRecord.svelte";
	import AssemblyTables from "$lib/components/AssemblyTables.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { studio } from "$lib/studio/state.svelte";

	$effect(() => {
		// Open the workspace's assembly the first time the tab shows.
		if (studio.workspace && !assembly.summary && !assembly.loading) void assembly.open();
	});
</script>

{#if assembly.summary}
	<div class="flex min-h-0 flex-1">
		<div class="border-base-300 flex w-52 shrink-0 flex-col border-r xl:w-64">
			<AssemblyTables />
		</div>
		<div class="flex min-w-0 flex-1 flex-col">
			<AssemblyGrid />
		</div>
		<div class="border-base-300 flex w-80 shrink-0 flex-col border-l xl:w-[26rem]">
			<AssemblyRecord />
		</div>
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
