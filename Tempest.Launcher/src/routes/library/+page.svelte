<script lang="ts">
	import { ArrowUpNarrowWide, Boxes, Library, Plus, Search } from "@lucide/svelte";
	import InstanceCard from "$lib/components/library/InstanceCard.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import {
		orderedInstances,
		sortInstancesByVersion,
		setInstanceOrder,
	} from "$lib/stores/instance.svelte";
	import { instanceWizardOpen } from "$lib/stores/ui.svelte";
	import { createReorderable } from "$lib/utils/reorder.svelte";
	import type { Instance } from "$lib/types/instance";

	let searchQuery = $state("");

	const sortedInstances = $derived(
		orderedInstances.value.filter(
			(instance) =>
				instance.label.toLowerCase().includes(searchQuery.toLowerCase()) ||
				instance.version?.toLowerCase().includes(searchQuery.toLowerCase()),
		),
	);

	let gridEl: HTMLDivElement | undefined = $state();
	const reorder = createReorderable<Instance>({
		ids: () => sortedInstances.map((i) => i.id),
		container: () => gridEl,
		onReorder: setInstanceOrder,
		grid: true,
	});

	const canDrag = $derived(searchQuery.trim() === "");
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
