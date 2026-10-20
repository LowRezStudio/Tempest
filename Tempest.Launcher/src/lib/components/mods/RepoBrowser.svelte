<script lang="ts">
	import {
		BadgeCheck,
		Code2,
		Cpu,
		Download,
		Info,
		FileBox,
		Search,
		ShieldAlert,
	} from "@lucide/svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import { createRepoBrowseQuery } from "$lib/queries/repo";
	import { compareVersions } from "$lib/utils/version";
	import type { ModRecord } from "$lib/core/mods";
	import type { BrowseParams, RepoMod } from "$lib/queries/repo";

	interface Props {
		gamePath: string;
		patch: string | undefined;
		installed: ModRecord[];
		isRunning: boolean;
		onOpenDetails: (modId: string) => void;
	}

	let { gamePath, patch, installed, isRunning, onOpenDetails }: Props = $props();

	let search = $state("");
	let kind = $state<BrowseParams["kind"]>("");
	let sort = $state<BrowseParams["sort"]>("downloads");
	let page = $state(1);
	let onlyCompatible = $state(true);
	let installing = $state<string | null>(null);

	const query = createRepoBrowseQuery(() => ({
		q: search.trim() || undefined,
		kind: kind || undefined,
		patch: onlyCompatible ? patch : undefined,
		sort,
		page,
	}));

	const installedById = $derived(new Map(installed.map((mod) => [mod.Id, mod])));
	const pages = $derived(
		query.data ? Math.max(1, Math.ceil(query.data.total / query.data.perPage)) : 1,
	);

	function stateOf(mod: RepoMod): "install" | "installed" | "update" {
		const local = installedById.get(mod.id);
		if (!local || !mod.latest) return "install";
		return compareVersions(mod.latest.version, local.Version) > 0 ? "update" : "installed";
	}

	async function install(mod: RepoMod) {
		if (!mod.latest || installing) return;
		installing = mod.id;
		try {
			await installFromRepo(gamePath, mod.id, mod.latest.version, mod.name);
		} finally {
			installing = null;
		}
	}

	const kindIcon = { assets: FileBox, scripts: Code2, native: Cpu } as const;
</script>

<div class="flex flex-col gap-4">
	<div class="flex flex-wrap items-center gap-2">
		<label class="input input-sm w-64">
			<Search size={14} class="opacity-50" />
			<input
				type="search"
				placeholder={m.repo_search_placeholder()}
				class="grow"
				bind:value={search}
				oninput={() => (page = 1)}
			/>
		</label>
		<select class="select select-sm" bind:value={kind} onchange={() => (page = 1)}>
			<option value="">{m.repo_kind_any()}</option>
			<option value="assets">{m.repo_kind_assets()}</option>
			<option value="scripts">{m.repo_kind_scripts()}</option>
			<option value="native">{m.repo_kind_native()}</option>
		</select>
		<select class="select select-sm" bind:value={sort} onchange={() => (page = 1)}>
			<option value="downloads">{m.repo_sort_downloads()}</option>
			<option value="updated">{m.repo_sort_updated()}</option>
			<option value="created">{m.repo_sort_created()}</option>
			<option value="name">{m.repo_sort_name()}</option>
		</select>
		{#if patch}
			<label class="label cursor-pointer gap-2 text-sm">
				<input
					type="checkbox"
					class="toggle toggle-sm"
					bind:checked={onlyCompatible}
					onchange={() => (page = 1)}
				/>
				{m.repo_only_compatible({ patch })}
			</label>
		{/if}
		{#if query.isFetching}
			<span class="loading loading-spinner loading-xs opacity-50"></span>
		{/if}
	</div>

	{#if query.isError}
		<div role="alert" class="alert alert-error text-sm">
			{m.repo_unreachable()}
			<button class="btn btn-sm" onclick={() => query.refetch()}>{m.common_retry()}</button>
		</div>
	{:else if query.isPending}
		<div class="flex h-60 items-center justify-center">
			<span class="loading loading-spinner loading-lg opacity-30"></span>
		</div>
	{:else if query.data.items.length === 0}
		<p class="text-base-content/60 py-10 text-center text-sm">{m.repo_no_results()}</p>
	{:else}
		<div class="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
			{#each query.data.items as mod (mod.id)}
				{@const state = stateOf(mod)}
				{@const Icon = kindIcon[mod.kind]}
				<div class="card card-sm bg-base-200 border-base-300 border">
					<div class="card-body gap-2">
						<div class="flex items-start gap-3">
							{#if mod.icon}
								<img
									src={mod.icon}
									alt=""
									class="h-10 w-10 shrink-0 rounded-lg object-cover"
								/>
							{:else}
								<div
									class="bg-base-300 text-primary flex h-10 w-10 shrink-0 items-center justify-center rounded-lg"
								>
									<Icon size={20} />
								</div>
							{/if}
							<div class="min-w-0 flex-1">
								<p class="truncate font-semibold">{mod.name}</p>
								<p class="text-base-content/60 truncate text-xs">
									{mod.owner.displayName}{mod.latest
										? ` · v${mod.latest.version}`
										: ""}
								</p>
							</div>
						</div>
						<p class="text-base-content/80 line-clamp-2 text-sm">{mod.summary}</p>
						<div class="flex flex-wrap items-center gap-1 text-xs">
							{#if mod.latest?.signer?.trust === "first-party"}
								<span class="badge badge-primary badge-xs gap-1"
									><BadgeCheck size={10} /> {m.repo_first_party()}</span
								>
							{:else if mod.latest?.signer?.trust === "unknown"}
								<span class="badge badge-warning badge-xs gap-1"
									><ShieldAlert size={10} /> {m.repo_unknown_signer()}</span
								>
							{/if}
							<span class="text-base-content/60 flex items-center gap-1"
								><Download size={10} /> {mod.downloads}</span
							>
						</div>
						<div class="card-actions mt-auto items-center justify-between">
							<button
								class="btn btn-ghost btn-xs"
								onclick={() => onOpenDetails(mod.id)}
							>
								<Info size={12} />
								{m.repo_details()}
							</button>
							{#if state === "installed"}
								<span class="badge badge-success badge-sm"
									>{m.repo_installed()}</span
								>
							{:else}
								<button
									class="btn btn-sm {state === 'update'
										? 'btn-primary'
										: 'btn-primary'}"
									disabled={!mod.latest || isRunning || installing !== null}
									onclick={() => install(mod)}
								>
									{#if installing === mod.id}
										<span class="loading loading-spinner loading-xs"></span>
									{:else}
										<Download size={14} />
									{/if}
									{state === "update" ? m.repo_update() : m.repo_install()}
								</button>
							{/if}
						</div>
					</div>
				</div>
			{/each}
		</div>
		{#if pages > 1}
			<div class="join self-center">
				<button
					class="btn btn-sm join-item"
					disabled={page <= 1}
					onclick={() => (page -= 1)}>«</button
				>
				<span class="btn btn-sm join-item pointer-events-none">{page} / {pages}</span>
				<button
					class="btn btn-sm join-item"
					disabled={page >= pages}
					onclick={() => (page += 1)}>»</button
				>
			</div>
		{/if}
	{/if}
</div>
