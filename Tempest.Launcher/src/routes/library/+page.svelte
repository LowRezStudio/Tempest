<script lang="ts">
	import { ArrowUpNarrowWide, Boxes, Library, Plus, Search } from "@lucide/svelte";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { orderedInstances, sortInstancesByVersion } from "$lib/stores/instance.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui.svelte";

	let searchQuery = $state("");

	const sortedInstances = $derived(
		orderedInstances.value.filter(
			(instance) =>
				instance.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
				instance.version?.toLowerCase().includes(searchQuery.toLowerCase()),
		),
	);
</script>

<div class="bg-base-100 flex h-full flex-col">
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
		{/snippet}
		{#snippet subtitle()}
			<span
				>{orderedInstances.value.length}
				{m.library_instances({ count: orderedInstances.value.length })}</span
			>
			<button
				class="btn btn-ghost btn-square btn-xs"
				title={m.library_sort_version()}
				aria-label={m.library_sort_version()}
				onclick={sortInstancesByVersion}
			>
				<ArrowUpNarrowWide size={14} />
			</button>
		{/snippet}
	</Header>

	<!-- Content Area -->
	<div class="bg-base-100 flex flex-1 flex-col overflow-hidden">
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
						<div class="flex flex-col gap-6">
							<div
								class="grid w-full grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4"
							>
								<button
									type="button"
									onclick={() => (instanceWizardOpen.value = true)}
									class="group bg-base-200/50 hover:bg-base-200 border-base-300/25 hover:border-base-300/40 text-base-content/60 hover:text-base-content flex h-[84px] cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 p-4 opacity-80 transition-all duration-200 hover:opacity-100"
									aria-label={m.library_new_instance()}
								>
									<Plus
										size={22}
										class="opacity-70 transition-opacity group-hover:opacity-100"
									/>
									<span class="text-sm leading-none font-medium"
										>{m.library_new_instance()}</span
									>
								</button>
							</div>
							<div
								class="flex flex-col items-center justify-center gap-4 pt-24 opacity-40"
							>
								<div class="opacity-30">
									<Boxes size={64} />
								</div>
								<p class="text-base-content/60 text-sm font-medium">
									{m.library_no_instances()}
								</p>
							</div>
						</div>
					{/if}
				{:else}
					<div
						class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4"
					>
						{#each sortedInstances as instance (instance.id)}
							<InstanceCard {instance} />
						{/each}
						<button
							type="button"
							onclick={() => (instanceWizardOpen.value = true)}
							class="group bg-base-200/50 hover:bg-base-200 border-base-300/25 hover:border-base-300/40 text-base-content/60 hover:text-base-content flex h-[84px] cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 p-4 opacity-80 transition-all duration-200 hover:opacity-100"
							aria-label={m.library_new_instance()}
						>
							<Plus
								size={22}
								class="opacity-70 transition-opacity group-hover:opacity-100"
							/>
							<span class="text-sm leading-none font-medium"
								>{m.library_new_instance()}</span
							>
						</button>
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>
