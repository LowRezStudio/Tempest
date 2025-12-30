<script lang="ts">
	import { instanceMap } from "$lib/stores/instance";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import type { Instance } from "$lib/types/instance";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import { Search, X, Plus } from "@lucide/svelte";

	let searchQuery = $state("");
	let sortBy = $state<"name" | "version" | "date">("name");
	let groupBy = $state<"none" | "version" | "group">("group");

	const instanceList = $derived(Object.values($instanceMap).filter(Boolean) as Instance[]);

	const filteredInstances = $derived(
		instanceList.filter(
			(instance) =>
				instance.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
				instance.version?.toLowerCase().includes(searchQuery.toLowerCase()),
		),
	);

	const sortedInstances = $derived(
		[...filteredInstances].sort((a, b) => {
			if (sortBy === "name") return a.label.localeCompare(b.label);
			if (sortBy === "version") return (a.version || "").localeCompare(b.version || "");
			return 0;
		}),
	);
</script>

<div class="flex flex-col h-full">
	<div class="flex items-center gap-4 p-4 bg-base-200">
		<label class="input input-bordered input-sm">
			<Search size={16} class="opacity-50" />
			<input type="text" placeholder="Search" class="grow" bind:value={searchQuery} />
		</label>

		<!-- <select class="select select-bordered select-sm bg-base-100" bind:value={sortBy}>
			<option value="name">Sort by: Name</option>
			<option value="version">Sort by: Version</option>
			<option value="date">Sort by: Date</option>
		</select>

		<select class="select select-bordered select-sm bg-base-100" bind:value={groupBy}>
			<option value="group">Group by: Group</option>
			<option value="version">Group by: Version</option>
			<option value="none">Group by: None</option>
		</select> -->
	</div>

	<div class="flex-1 overflow-y-auto p-4">
		{#if sortedInstances.length === 0}
			<div class="flex flex-col items-center justify-center h-full gap-4 opacity-60">
				{#if searchQuery}
					<p class="text-lg max-w-md truncate">
						No instances found matching "{searchQuery}"
					</p>
				{:else}
					<p class="text-lg">No instances yet</p>
					<button
						class="btn btn-accent gap-2"
						onclick={() => instanceWizardOpen.set(true)}
					>
						<Plus size={20} />
						Create Your First Instance
					</button>
				{/if}
			</div>
		{:else}
			<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-4">
				{#each sortedInstances as instance (instance.id)}
					<InstanceCard {instance} />
				{/each}
			</div>
		{/if}
	</div>
</div>
