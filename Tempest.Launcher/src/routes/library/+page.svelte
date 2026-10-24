<script lang="ts">
	import { Library, Plus, Search, SearchX, X } from "@lucide/svelte";
	import InstanceRow from "$lib/components/library/InstanceRow.svelte";
	import LibraryArrangeMenu from "$lib/components/library/LibraryArrangeMenu.svelte";
	import NewInstanceOption from "$lib/components/library/NewInstanceOption.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createModsQueries } from "$lib/queries/mods";
	import { orderedInstances, setInstanceOrder } from "$lib/stores/instance.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { openInstanceWizard } from "$lib/stores/ui.svelte";
	import { createReorderable } from "$lib/utils/reorder.svelte";
	import { compareVersions } from "$lib/utils/version";
	import { patchOf } from "$lib/utils/versions";
	import type { Instance } from "$lib/types/instance";

	let query = $state("");
	let searchInput = $state<HTMLInputElement | null>(null);

	const instances = $derived(orderedInstances.value);
	const shown = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (!q) return instances;
		return instances.filter((instance) =>
			[instance.label, instance.version, patchOf(instance.version)?.name].some((text) =>
				text?.toLowerCase().includes(q),
			),
		);
	});

	const modLists = createModsQueries(() =>
		instances.filter((i) => i.state.type === "prepared").map((i) => i.path),
	);

	const runningCount = $derived(
		instances.filter((i) => processesList.value.some((p) => p.instance.id === i.id)).length,
	);
	const downloadingCount = $derived(
		instances.filter((i) => i.state.type === "downloading" || i.state.type === "paused").length,
	);
	const modCount = $derived(
		Object.values(modLists.byPath).reduce((n, mods) => n + (mods?.length ?? 0), 0),
	);
	const patchSpan = $derived.by(() => {
		const known = instances
			.map((i) => i.version)
			.filter((v): v is string => !!v)
			.sort(compareVersions);
		if (known.length === 0) return "–";
		const [oldest, newest] = [known[0]!, known.at(-1)!];
		return oldest === newest ? oldest : `${oldest} – ${newest}`;
	});

	// Drag to reorder writes the same custom order the arrange menu sets.
	let listEl: HTMLDivElement | undefined = $state();
	const reorder = createReorderable<Instance>({
		ids: () => shown.map((i) => i.id),
		container: () => listEl,
		onReorder: setInstanceOrder,
	});
	// Reordering a filtered list would drop the hidden instances out of place.
	const canDrag = $derived(query.trim() === "");

	function onWindowKey(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
			e.preventDefault();
			searchInput?.focus();
			searchInput?.select();
		}
	}

	function onSearchKey(e: KeyboardEvent) {
		if (e.key === "Escape" && query) {
			e.preventDefault();
			query = "";
		}
	}
</script>

<svelte:window onkeydown={onWindowKey} />

<div class="flex h-full flex-col">
	<Header title={m.library_title()}>
		{#snippet icon()}
			<Library size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span>
				{instances.length}
				{m.library_instances({ count: instances.length })}
				{#if runningCount > 0}
					· <span class="text-success">{m.library_running({ count: runningCount })}</span>
				{/if}
				{#if downloadingCount > 0}
					{#if runningCount > 0}
						·
					{/if}{m.library_downloading({ count: downloadingCount })}
				{/if}
			</span>
		{/snippet}
	</Header>

	<div class="@container min-h-0 flex-1 scrollbar-gutter-stable overflow-y-auto">
		{#if instances.length === 0}
			<!-- Like the mod website's first-run dashboard: the two ways in, nothing else. -->
			<div class="px-4 py-12">
				<div
					data-panel
					class="bg-base-200 border-base-300 rounded-box mx-auto flex max-w-xl flex-col gap-4 border p-8"
				>
					<div>
						<h2 class="mb-1 text-lg font-semibold">{m.library_empty_title()}</h2>
						<p class="text-base-content/60 text-sm">{m.library_about_instances()}</p>
					</div>
					<div class="grid gap-2 @lg:grid-cols-2">
						<NewInstanceOption tab="download" />
						<NewInstanceOption tab="folder" />
					</div>
				</div>
			</div>
		{:else}
			<div
				class="mx-auto grid w-full max-w-6xl items-start gap-x-6 gap-y-4 px-4 py-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]"
			>
				<form
					class="flex items-center gap-2 @3xl:col-span-2"
					role="search"
					onsubmit={(e) => e.preventDefault()}
				>
					<label class="input input-sm min-w-0 flex-1">
						<Search size={14} class="opacity-50" />
						<input
							type="search"
							class="grow"
							placeholder={m.library_search_placeholder()}
							aria-label={m.common_search()}
							bind:this={searchInput}
							bind:value={query}
							onkeydown={onSearchKey}
						/>
					</label>
					<LibraryArrangeMenu />
					<button
						type="button"
						class="btn btn-primary btn-sm @3xl:hidden"
						onclick={() => openInstanceWizard()}
					>
						<Plus size={14} />
						{m.library_new_instance()}
					</button>
				</form>
				<section class="min-w-0">
					{#if query.trim()}
						<div class="mb-2 flex min-h-6 flex-wrap items-center gap-2 text-sm">
							<button
								class="badge badge-primary badge-sm cursor-pointer gap-1"
								onclick={() => (query = "")}
							>
								“{query.trim()}”
								<X size={10} />
							</button>
						</div>
					{/if}
					{#if shown.length === 0}
						<div
							class="text-base-content/60 flex flex-col items-center gap-2 py-20 text-center"
						>
							<SearchX size={40} />
							<p>{m.library_no_results({ query: query.trim() })}</p>
							<button class="link" onclick={() => (query = "")}>
								{m.library_clear_search()}
							</button>
						</div>
					{:else}
						<div
							bind:this={listEl}
							data-panel
							class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border"
							class:dragging={!!reorder.drag}
						>
							{#each shown as instance, i (instance.id)}
								<div
									data-id={instance.id}
									class="instance-slot"
									style:transform={reorder.shiftFor(i)}
									class:is-ghost={reorder.drag?.id === instance.id}
								>
									<InstanceRow
										{instance}
										mods={modLists.byPath[instance.path]}
										onpointerdown={canDrag
											? reorder.pointerdown(instance.id, i, instance)
											: undefined}
									/>
								</div>
							{/each}
						</div>
					{/if}
				</section>

				<aside class="flex flex-col gap-4 @3xl:sticky @3xl:top-6">
					<div
						data-panel
						class="bg-base-200 border-base-300 rounded-box flex flex-col gap-2 border p-4 @max-3xl:hidden"
					>
						<h2 class="text-sm font-semibold">{m.library_new_instance()}</h2>
						<NewInstanceOption tab="download" />
						<NewInstanceOption tab="folder" />
					</div>
					<dl
						data-panel
						class="bg-base-200 border-base-300 rounded-box grid grid-cols-2 gap-3 border p-4 text-sm"
					>
						<div>
							<dt class="text-base-content/60 text-xs">{m.library_stat_mods()}</dt>
							<dd class="font-semibold tabular-nums">{modCount}</dd>
						</div>
						<div>
							<dt class="text-base-content/60 text-xs">{m.library_stat_patches()}</dt>
							<dd class="font-semibold tabular-nums">{patchSpan}</dd>
						</div>
						<div>
							<dt class="text-base-content/60 text-xs">{m.library_stat_running()}</dt>
							<dd
								class="font-semibold tabular-nums"
								class:text-success={runningCount > 0}
							>
								{runningCount}
							</dd>
						</div>
					</dl>
				</aside>
			</div>

			{#if reorder.drag && canDrag}
				<div
					class="drag-clone pointer-events-none fixed z-[100]"
					style:top={`${reorder.pointerY - reorder.drag.offsetY}px`}
					style:left={`${reorder.pointerX - reorder.drag.offsetX}px`}
					style:width={`${reorder.drag.width}px`}
					aria-hidden="true"
					inert
				>
					<InstanceRow instance={reorder.drag.item} />
				</div>
			{/if}
		{/if}
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
