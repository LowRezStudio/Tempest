<script lang="ts">
	import {
		CodeXml,
		Cpu,
		FileBox,
		Layers,
		RefreshCw,
		Search,
		SearchX,
		SlidersHorizontal,
		X,
	} from "@lucide/svelte";
	import {
		browseFilter as f,
		clearBrowseFilters,
		compatibleWith,
		kinds,
		matches,
	} from "$lib/mods/browse.svelte";
	import { kindHint, kindLabel } from "$lib/mods/listing";
	import { pendingInstalls } from "$lib/mods/pendingInstalls.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { createRemoveModMutation } from "$lib/queries/mods";
	import { createRepoCatalogQuery, createRepoTagsQuery } from "$lib/queries/repo";
	import { addToast } from "$lib/stores/ui.svelte";
	import RepoModRow from "./RepoModRow.svelte";
	import RepoSortMenu from "./RepoSortMenu.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { BrowseFilter } from "$lib/mods/browse.svelte";
	import type { RepoMod } from "$lib/queries/repo";

	interface Props {
		gamePath: string;
		patch: string | undefined;
		installed: ModRecord[];
		isRunning: boolean;
		onOpenDetails: (modId: string) => void;
	}

	let { gamePath, patch, installed, isRunning, onOpenDetails }: Props = $props();

	const catalogQuery = createRepoCatalogQuery(() => f.sort);
	const tagsQuery = createRepoTagsQuery();

	let filtersOpen = $state(false);
	let searchInput = $state<HTMLInputElement | null>(null);

	const catalog = $derived(catalogQuery.data ?? []);
	const installedById = $derived(new Map(installed.map((mod) => [mod.Id, mod])));
	const ctx = $derived({ patch, installed: new Set(installedById.keys()) });
	const shown = $derived(catalog.filter((mod) => matches(mod, f, ctx)));
	const count = (change: Partial<BrowseFilter>) =>
		catalog.filter((mod) => matches(mod, { ...f, ...change }, ctx)).length;
	// Shown again by turning the patch filter off; the hint below the list says so.
	const otherPatches = $derived(
		f.compatibleOnly && patch
			? catalog.filter(
					(mod) =>
						!compatibleWith(mod, patch) &&
						matches(mod, { ...f, compatibleOnly: false }, ctx),
				).length
			: 0,
	);

	const tags = $derived.by(() => {
		const top = (tagsQuery.data ?? []).slice(0, 16).map((t) => t.tag);
		return f.tag && !top.includes(f.tag) ? [f.tag, ...top] : top;
	});
	const active = $derived(
		[
			f.q.trim() && { label: `“${f.q.trim()}”`, clear: () => (f.q = "") },
			f.kind && { label: kindLabel(f.kind), clear: () => (f.kind = null) },
			f.tag && { label: `#${f.tag}`, clear: () => (f.tag = null) },
			f.hideInstalled && {
				label: m.repo_hide_installed(),
				clear: () => (f.hideInstalled = false),
			},
		].filter((a) => !!a),
	);

	const pendingByMod = $derived(
		new Map(
			pendingInstalls.value
				.filter((p) => p.gamePath === gamePath && p.modId)
				.map((p) => [p.modId!, p] as const),
		),
	);

	function install(mod: RepoMod) {
		const latest = mod.latest;
		if (!latest || pendingByMod.has(mod.id)) return;
		void installFromRepo(gamePath, mod.id, latest.version, mod.name, {
			replace: installedById.has(mod.id),
			sha256: latest.sha256,
		});
	}

	const removeModMutation = createRemoveModMutation();
	function uninstall(mod: RepoMod) {
		const record = installedById.get(mod.id);
		if (!record) return;
		removeModMutation.mutate(
			{ gamePath, modName: record.Name },
			{
				onError: (error) =>
					addToast({ message: `${mod.name}: ${error.message}`, tone: "error" }),
			},
		);
	}

	const kindIcon = { assets: FileBox, scripts: CodeXml, native: Cpu } as const;

	function onWindowKey(e: KeyboardEvent) {
		if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
			e.preventDefault();
			searchInput?.focus();
			searchInput?.select();
		}
	}

	function onSearchKey(e: KeyboardEvent) {
		if (e.key === "Escape" && f.q) {
			e.preventDefault();
			f.q = "";
		}
	}
</script>

<svelte:window onkeydown={onWindowKey} />

<!-- Plain buttons rather than daisyUI's menu, whose press flash is the theme's near-black neutral. -->
{#snippet kindItem(
	kind: BrowseFilter["kind"],
	Icon: typeof Layers,
	label: string,
	hint: string | undefined,
)}
	{@const active = f.kind === kind}
	<button
		class="rounded-field flex cursor-pointer items-center gap-2 px-2 py-1.5 text-left text-sm transition-colors {active
			? 'bg-base-content/10 font-semibold'
			: 'hover:bg-base-content/5'}"
		aria-pressed={active}
		title={hint}
		onclick={() => (f.kind = kind)}
	>
		<Icon size={14} class="shrink-0" />
		{label}
		<span class="ml-auto font-normal tabular-nums opacity-60">{count({ kind })}</span>
	</button>
{/snippet}

<div class="@container">
	<div class="grid gap-x-6 gap-y-3 @3xl:grid-cols-[200px_minmax(0,1fr)]">
		<form
			class="flex items-center gap-2 @3xl:col-start-2"
			role="search"
			onsubmit={(e) => e.preventDefault()}
		>
			<button
				type="button"
				class="btn btn-sm btn-square relative @3xl:hidden"
				class:btn-active={filtersOpen}
				aria-label={m.repo_filters()}
				title={m.repo_filters()}
				aria-expanded={filtersOpen}
				onclick={() => (filtersOpen = !filtersOpen)}
			>
				<SlidersHorizontal size={14} />
				{#if active.length > 0}
					<span class="badge badge-primary badge-xs absolute -top-1.5 -right-1.5"
						>{active.length}</span
					>
				{/if}
			</button>
			<label class="input input-sm min-w-0 flex-1">
				<Search size={14} class="opacity-50" />
				<input
					type="search"
					class="grow"
					placeholder={m.repo_search_placeholder()}
					aria-label={m.common_search()}
					bind:this={searchInput}
					bind:value={f.q}
					onkeydown={onSearchKey}
				/>
			</label>
			<RepoSortMenu bind:value={f.sort} />
		</form>

		<!-- Beside the list when there is room, behind the Filters button otherwise -->
		<aside
			class={[
				"flex flex-col gap-5 @3xl:sticky @3xl:top-6 @3xl:col-start-1 @3xl:row-span-2 @3xl:row-start-1 @3xl:self-start",
				"@max-3xl:bg-base-200 @max-3xl:border-base-300 @max-3xl:rounded-box @max-3xl:border @max-3xl:p-3",
				!filtersOpen && "@max-3xl:hidden",
			]}
		>
			<div class="flex flex-col gap-0.5" role="group" aria-label={m.repo_kind()}>
				<span class="text-base-content/60 px-2 pb-1 text-xs font-semibold"
					>{m.instance_browse()}</span
				>
				{@render kindItem(null, Layers, m.repo_all_mods(), undefined)}
				{#each kinds as kind (kind)}
					{@render kindItem(kind, kindIcon[kind], kindLabel(kind), kindHint(kind))}
				{/each}
			</div>

			<div class="flex flex-col gap-2 px-2">
				<span class="text-base-content/60 text-xs font-semibold">{m.repo_show()}</span>
				{#if patch}
					<label class="flex cursor-pointer items-center justify-between gap-2 text-sm">
						{m.repo_only_compatible({ patch })}
						<input
							type="checkbox"
							class="toggle toggle-xs toggle-primary"
							bind:checked={f.compatibleOnly}
						/>
					</label>
				{/if}
				<label class="flex cursor-pointer items-center justify-between gap-2 text-sm">
					{m.repo_hide_installed()}
					<input
						type="checkbox"
						class="toggle toggle-xs toggle-primary"
						bind:checked={f.hideInstalled}
					/>
				</label>
			</div>

			{#if tags.length > 0}
				<div class="flex flex-col gap-1.5">
					<span class="text-base-content/60 px-2 text-xs font-semibold"
						>{m.repo_tags()}</span
					>
					<div class="flex flex-wrap gap-1">
						{#each tags as tag (tag)}
							<button
								class="badge badge-sm cursor-pointer {f.tag === tag
									? 'badge-primary'
									: 'badge-ghost hover:bg-base-300'}"
								aria-pressed={f.tag === tag}
								onclick={() => (f.tag = f.tag === tag ? null : tag)}>{tag}</button
							>
						{/each}
					</div>
				</div>
			{/if}
		</aside>

		<div class="min-w-0 @3xl:col-start-2">
			<div class="mb-3 flex min-h-6 flex-wrap items-center gap-2 text-sm">
				<h2 class="font-semibold">{f.kind ? kindLabel(f.kind) : m.repo_all_mods()}</h2>
				{#if catalogQuery.data}
					<span class="text-base-content/60 tabular-nums">{shown.length}</span>
				{/if}
				{#each active as a (a.label)}
					<button
						class="badge badge-primary badge-sm cursor-pointer gap-1"
						onclick={a.clear}
					>
						{a.label}
						<X size={10} />
					</button>
				{/each}
				{#if catalogQuery.isFetching && catalogQuery.data}
					<span class="loading loading-spinner loading-xs ml-auto opacity-50"></span>
				{/if}
			</div>

			{#if catalogQuery.isError && !catalogQuery.data}
				<div
					class="text-base-content/60 flex flex-col items-center gap-3 py-20 text-center"
				>
					<SearchX size={40} />
					<p>{m.repo_unreachable()}</p>
					<button class="btn btn-sm" onclick={() => catalogQuery.refetch()}>
						<RefreshCw size={14} />
						{m.common_retry()}
					</button>
				</div>
			{:else if !catalogQuery.data}
				<ul
					class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border"
					aria-busy="true"
				>
					{#each { length: 6 } as _, i (i)}
						<li class="flex items-center gap-4 p-3">
							<div
								class="bg-base-300 rounded-field loading-shimmer aspect-video w-36 shrink-0"
							></div>
							<div class="flex flex-1 flex-col gap-2">
								<div class="bg-base-300 loading-shimmer h-4 w-48 rounded"></div>
								<div class="bg-base-300 loading-shimmer h-3 w-3/4 rounded"></div>
								<div class="bg-base-300 loading-shimmer h-3 w-40 rounded"></div>
							</div>
						</li>
					{/each}
				</ul>
			{:else if shown.length === 0}
				<div
					class="text-base-content/60 flex flex-col items-center gap-2 py-20 text-center"
				>
					<SearchX size={40} />
					<p>{m.repo_no_results()}</p>
					{#if otherPatches > 0}
						<button class="link" onclick={() => (f.compatibleOnly = false)}>
							{m.repo_other_patches_show({ count: otherPatches })}
						</button>
					{:else if active.length > 0}
						<button class="link" onclick={clearBrowseFilters}>
							{m.repo_clear_filters()}
						</button>
					{/if}
				</div>
			{:else}
				<ul class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border">
					{#each shown as mod (mod.id)}
						<RepoModRow
							{mod}
							installedVersion={installedById.get(mod.id)?.Version}
							{patch}
							pending={pendingByMod.get(mod.id)}
							{isRunning}
							onInstall={install}
							onUninstall={uninstall}
							onOpen={onOpenDetails}
						/>
					{/each}
				</ul>
				{#if otherPatches > 0}
					<p class="text-base-content/60 mt-3 text-center text-xs">
						{m.repo_other_patches_hidden({ count: otherPatches, patch: patch ?? "" })}
						<button class="link" onclick={() => (f.compatibleOnly = false)}>
							{m.repo_show_them()}
						</button>
					</p>
				{/if}
			{/if}
		</div>
	</div>
</div>
