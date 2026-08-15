<script lang="ts">
	import { Boxes, Library, Plus, Search, X } from "@lucide/svelte";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { instanceMap, instanceOrder, setInstanceOrder } from "$lib/stores/instance.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui.svelte";
	import { createReorderable } from "$lib/utils/reorder.svelte";
	import type { Instance } from "$lib/types/instance";

	let searchQuery = $state("");
	let sortBy = $state<"name" | "version" | "date">("date");
	let groupBy = $state<"none" | "version" | "group">("group");

	const orderedInstances = $derived.by(() => {
		const order = instanceOrder.value;
		const all = Object.values(instanceMap.value).filter((i): i is Instance => !!i);
		const byId = new Map(all.map((i) => [i.id, i]));
		const sorted: Instance[] = [];
		for (const id of order) {
			const inst = byId.get(id);
			if (inst) {
				sorted.push(inst);
				byId.delete(id);
			}
		}
		for (const inst of byId.values()) sorted.push(inst);
		return sorted;
	});

	const filteredInstances = $derived(
		orderedInstances.filter(
			(instance) =>
				instance.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
				instance.version?.toLowerCase().includes(searchQuery.toLowerCase()),
		),
	);

	const sortedInstances = $derived(
		sortBy === "date"
			? filteredInstances
			: [...filteredInstances].sort((a, b) => {
					if (sortBy === "name") return a.label.localeCompare(b.label);
					if (sortBy === "version") {
						return (a.version || "").localeCompare(b.version || "");
					}
					return 0;
				}),
	);

	let gridEl: HTMLDivElement | undefined = $state();
	const reorder = createReorderable<Instance>({
		ids: () => sortedInstances.map((i) => i.id),
		container: () => gridEl,
		onReorder: setInstanceOrder,
		grid: true,
	});

	const canDrag = $derived(sortBy === "date" && searchQuery.trim() === "");
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
			<button class="btn btn-accent" onclick={() => (instanceWizardOpen.value = true)}>
				<Plus size={16} />
				{m.library_new_instance()}
			</button>
		{/snippet}
		{#snippet subtitle()}
			<span
				>{orderedInstances.length}
				{m.library_instances({ count: orderedInstances.length })}</span
			>
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
						<EmptyState title={m.library_no_instances()}>
							{#snippet icon()}
								<Boxes size={48} />
							{/snippet}
							{#snippet actions()}
								<button
									class="btn btn-accent gap-2"
									onclick={() => (instanceWizardOpen.value = true)}
								>
									<Plus size={20} />
									{m.library_create_first()}
								</button>
							{/snippet}
						</EmptyState>
					{/if}
				{:else}
					<div
						bind:this={gridEl}
						class="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4"
						class:dragging={!!reorder.drag}
					>
						{#each sortedInstances as instance, i (instance.id)}
							<div
								data-id={instance.id}
								class="instance-slot"
								style:transform={reorder.shiftFor(i)}
								class:is-ghost={reorder.drag?.id === instance.id}
							>
								<InstanceCard
									{instance}
									onpointerdown={canDrag
										? reorder.pointerdown(instance.id, i, instance)
										: undefined}
								/>
							</div>
						{/each}
					</div>
				{/if}

				{#if reorder.drag && canDrag}
					<div
						class="drag-clone pointer-events-none fixed z-[100]"
						style:top={`${reorder.pointerY - reorder.drag.offsetY}px`}
						style:left={`${reorder.pointerX - reorder.drag.offsetX}px`}
						style:width={`${reorder.drag.width}px`}
						aria-hidden="true"
						inert
					>
						<InstanceCard instance={reorder.drag.item} />
					</div>
				{/if}
			</div>
		</div>
	</div>
</div>

<style>
	.instance-slot {
		touch-action: none;
		position: relative;
		z-index: 1;
	}
	/* Transitions only while dragging so the drop reorder is instant (no glitch). */
	.dragging .instance-slot {
		will-change: transform;
		transition: transform 200ms ease;
	}
	.instance-slot.is-ghost {
		z-index: 0;
		opacity: 0.3;
		border-radius: 0.5rem;
	}
	.drag-clone {
		filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.45));
		transform: scale(1.02);
		transform-origin: center center;
		opacity: 0.95;
	}
</style>
