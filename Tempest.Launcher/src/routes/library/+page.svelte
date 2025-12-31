<script lang="ts">
	import { instanceMap } from "$lib/stores/instance";
	import { instanceWizardOpen } from "$lib/stores/ui";
	import type { Instance } from "$lib/types/instance";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import { Search, X, Plus, Library } from "@lucide/svelte";

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

<div class="flex flex-col h-full bg-base-100">
	<div>
		<div class="px-4 py-3">
			<div class="flex items-center justify-between">
				<div class="flex items-center gap-3">
					<div
						class="w-16 h-16 rounded-xl bg-base-200 flex items-center justify-center shrink-0"
					>
						<Library size={32} class="opacity-60" />
					</div>
					<div>
						<h1 class="text-2xl font-bold mb-1">Library</h1>
						<div class="flex items-center gap-3 text-sm text-base-content/70">
							<span
								>{instanceList.length}
								{instanceList.length === 1 ? "instance" : "instances"}</span
							>
						</div>
					</div>
				</div>

				<div class="flex items-center gap-2">
					<label class="input input-bordered">
						<Search size={16} class="opacity-50" />
						<input
							type="text"
							placeholder="Search"
							class="grow"
							bind:value={searchQuery}
						/>
					</label>
					<button class="btn btn-accent" onclick={() => instanceWizardOpen.set(true)}>
						<Plus size={16} />
						New Instance
					</button>
				</div>
			</div>
		</div>
	</div>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-200">
		<div class="flex-1 overflow-y-auto">
			<div class="px-4 py-6">
				{#if sortedInstances.length === 0}
					<div class="flex flex-col items-center justify-center h-full gap-4">
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
