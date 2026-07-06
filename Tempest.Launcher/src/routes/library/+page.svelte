<script lang="ts">
	import { Boxes, Library, Plus, Search, X } from "@lucide/svelte";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui.svelte";
	import type { Instance } from "$lib/types/instance";

	let searchQuery = $state("");
	let sortBy = $state<"name" | "version" | "date">("name");
	let groupBy = $state<"none" | "version" | "group">("group");

	const instanceList = $derived(Object.values(instanceMap.value).filter(Boolean) as Instance[]);

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

<div class="flex flex-col h-full bg-base-100">
	<Header title={m.library_title()}>
		{#snippet icon()}
			<Library size={32} class="opacity-60" />
		{/snippet}
		{#snippet actions()}
			<label class="input input-bordered">
				<Search size={16} class="opacity-50" />
				<input
					type="text"
					placeholder={m.library_search_placeholder()}
					class="grow"
					bind:value={searchQuery}
				/>
			</label>
			<button class="btn btn-accent" onclick={() => instanceWizardOpen.value = true}>
				<Plus size={16} />
				{m.library_new_instance()}
			</button>
		{/snippet}
		{#snippet subtitle()}
			<span
				>{instanceList.length}
				{m.library_instances({ count: instanceList.length })}</span
			>
		{/snippet}
	</Header>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				{#if sortedInstances.length === 0}
					{#if searchQuery}
						<EmptyState
							title={m.library_no_results({ query: searchQuery })}
							description={m.library_try_different()}
						>
							{#snippet icon()}
								<Search size={48} />
							{/snippet}
						</EmptyState>
					{:else}
						<EmptyState title={m.library_no_instances()}>
							{#snippet icon()}
								<Boxes size={48} />
							{/snippet}
							{#snippet actions()}
							<button
								class="btn btn-accent gap-2"
								onclick={() => instanceWizardOpen.value = true}
							>
									<Plus size={20} />
									{m.library_create_first()}
								</button>
							{/snippet}
						</EmptyState>
					{/if}
				{:else}
					<div
						class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4 gap-4"
					>
						{#each sortedInstances as instance (instance.id)}
							<InstanceCard {instance} />
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>
