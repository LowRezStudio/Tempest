<script lang="ts">
	import { Library, Plus, Search, SearchX, X } from "@lucide/svelte";
	import InstanceRow from "$lib/components/library/InstanceRow.svelte";
	import LibraryArrangeMenu from "$lib/components/library/LibraryArrangeMenu.svelte";
	import NewInstanceOption from "$lib/components/library/NewInstanceOption.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createModsQueries } from "$lib/queries/mods";
	import { orderedInstances } from "$lib/stores/instance.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { openInstanceWizard } from "$lib/stores/ui.svelte";
	import { compareVersions } from "$lib/utils/version";
	import { patchOf } from "$lib/utils/versions";

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

<div class="bg-base-100 flex h-full flex-col">
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
					· {m.library_downloading({ count: downloadingCount })}
				{/if}
			</span>
		{/snippet}
	</Header>

	<div class="@container min-h-0 flex-1 scrollbar-gutter-stable overflow-y-auto">
		{#if instances.length === 0}
			<!-- Like the mod website's first-run dashboard: the two ways in, nothing else. -->
			<div class="px-4 py-12">
				<div
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
				<section class="min-w-0">
					<form
						class="mb-3 flex items-center gap-2"
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
						<!-- The aside has these when there is room beside the list -->
						<button
							type="button"
							class="btn btn-primary btn-sm @3xl:hidden"
							onclick={() => openInstanceWizard()}
						>
							<Plus size={14} />
							{m.library_new_instance()}
						</button>
					</form>

					<div class="mb-2 flex min-h-6 flex-wrap items-center gap-2 text-sm">
						<h2 class="font-semibold">{m.library_heading()}</h2>
						<span class="text-base-content/60 tabular-nums">{shown.length}</span>
						{#if query.trim()}
							<button
								class="badge badge-primary badge-sm cursor-pointer gap-1"
								onclick={() => (query = "")}
							>
								“{query.trim()}”
								<X size={10} />
							</button>
						{/if}
					</div>

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
						<ul
							class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border"
						>
							{#each shown as instance (instance.id)}
								<InstanceRow {instance} mods={modLists.byPath[instance.path]} />
							{/each}
						</ul>
					{/if}
				</section>

				<aside class="flex flex-col gap-4 @3xl:sticky @3xl:top-6">
					<div
						class="bg-base-200 border-base-300 rounded-box flex flex-col gap-2 border p-4 @max-3xl:hidden"
					>
						<h2 class="text-sm font-semibold">{m.library_new_instance()}</h2>
						<p class="text-base-content/60 mb-1 text-xs">
							{m.library_about_instances()}
						</p>
						<NewInstanceOption tab="download" />
						<NewInstanceOption tab="folder" />
					</div>
					<dl
						class="bg-base-200 border-base-300 rounded-box grid grid-cols-2 gap-3 border p-4 text-sm"
					>
						<div>
							<dt class="text-base-content/60 text-xs">
								{m.library_stat_instances()}
							</dt>
							<dd class="font-semibold tabular-nums">{instances.length}</dd>
						</div>
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
		{/if}
	</div>
</div>
