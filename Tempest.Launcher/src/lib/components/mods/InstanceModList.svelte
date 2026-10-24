<script lang="ts">
	import {
		CodeXml,
		Cpu,
		FileBox,
		FileDown,
		PackageOpen,
		RefreshCw,
		Search,
		SearchX,
	} from "@lucide/svelte";
	import { SvelteSet } from "svelte/reactivity";
	import {
		missingDependencies,
		offerMissingDependencies,
		turnedOff,
	} from "$lib/mods/dependencies";
	import { justInstalled, pendingInstalls } from "$lib/mods/pendingInstalls.svelte";
	import { m } from "$lib/paraglide/messages";
	import { invoke } from "$lib/platform/api";
	import {
		createDisableModMutation,
		createEnableModMutation,
		createInstalledIconsQuery,
		createRenameModMutation,
	} from "$lib/queries/mods";
	import { createRepoModsQuery, createRepoVersionCountsQuery } from "$lib/queries/repo";
	import { addToast } from "$lib/stores/ui.svelte";
	import InstalledModRow from "./InstalledModRow.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";
	import type { RepoUpdate } from "$lib/queries/repo";

	interface Props {
		mods: ModRecord[];
		gamePath: string;
		/** Search over name, author and id, typed into the tabs' row */
		query: string;
		/** The instance is set up and can take mods. */
		isReady: boolean;
		isRunning: boolean;
		isLoading: boolean;
		/** Repository update checks keyed by mod id */
		updates: Map<string, RepoUpdate>;
		onRefresh: () => void;
		onRemoveMod: (name: string) => void;
		onOpenFiles: (mod: ModRecord) => void;
		onOpenRepo: (modId: string, tab?: "readme" | "versions") => void;
		onAddMod: () => void;
		onDiscover: () => void;
	}

	let {
		mods,
		gamePath,
		query = $bindable(),
		isReady,
		isRunning,
		isLoading,
		updates,
		onRefresh,
		onRemoveMod,
		onOpenFiles,
		onOpenRepo,
		onAddMod,
		onDiscover,
	}: Props = $props();

	// Looked up per installed V2 mod (plain GETs; unknown ids come back empty), so the repo
	// features don't depend on the update check, which only adds the update info.
	const v2ModIds = () => mods.filter((mod) => mod.Kind === "V2").map((mod) => mod.Id);
	const repoMods = createRepoModsQuery(v2ModIds);
	// The package's own icon stands in when the repository has none (local or unpublished mods).
	const installedIcons = createInstalledIconsQuery(() => gamePath, v2ModIds);
	const versionCounts = createRepoVersionCountsQuery(() => [...(repoMods.data?.keys() ?? [])]);

	const shown = $derived.by(() => {
		const q = query.trim().toLowerCase();
		if (!q) return mods;
		return mods.filter((mod) =>
			[mod.Name, mod.Author, mod.Id].some((s) => s?.toLowerCase().includes(q)),
		);
	});

	// Dependencies no enabled mod of the list satisfies, per mod; the row offers to fetch them.
	const missingByMod = $derived(
		new Map(
			mods.flatMap((mod) => {
				const missing = missingDependencies(mod, mods);
				return missing.length > 0 ? [[mod.Id, missing] as const] : [];
			}),
		),
	);
	// Everything it misses is only turned off, so the row asks to turn it on, not to install.
	const onlyTurnedOff = (mod: ModRecord) =>
		missingByMod.get(mod.Id)?.every((dep) => turnedOff(mods, dep.id, [dep.requirement])) ??
		false;

	const pending = $derived(pendingInstalls.value.filter((p) => p.gamePath === gamePath));
	// Installs of a mod that is already listed (updates, version switches) play out in its own
	// row; the rest get a placeholder row.
	const pendingByMod = $derived(
		new Map(pending.flatMap((p) => (p.modId ? [[p.modId, p] as const] : []))),
	);
	const placeholders = $derived(
		pending.filter((p) => !p.modId || !mods.some((mod) => mod.Id === p.modId)),
	);

	const renameMutation = createRenameModMutation();
	const enableMutation = createEnableModMutation();
	const disableMutation = createDisableModMutation();
	const togglingModIds = new SvelteSet<string>();

	const errorMessage = (error: unknown) =>
		error instanceof Error ? error.message : String(error);

	// The list updates optimistically; a second click while the CLI still works on
	// the same mod is ignored so two enable/disable runs never race.
	async function toggle(mod: ModRecord, input: HTMLInputElement) {
		if (togglingModIds.has(mod.Id)) {
			input.checked = mod.Enabled;
			return;
		}
		togglingModIds.add(mod.Id);
		try {
			await (mod.Enabled ? disableMutation : enableMutation).mutateAsync({
				gamePath,
				modName: mod.Name,
			});
		} catch (error) {
			addToast({ message: `${mod.Name}: ${errorMessage(error)}`, tone: "error" });
		} finally {
			togglingModIds.delete(mod.Id);
		}
	}

	async function rename(mod: ModRecord, newName: string) {
		try {
			await renameMutation.mutateAsync({ gamePath, oldName: mod.Name, newName });
		} catch (error) {
			addToast({
				title: m.mod_rename_dialog_title(),
				message: errorMessage(error),
				tone: "error",
			});
		}
	}

	function fetchMissing(mod: ModRecord) {
		const missing = missingByMod.get(mod.Id);
		if (missing) void offerMissingDependencies(gamePath, mod.Name, missing);
	}

	async function showInExplorer(mod: ModRecord) {
		const targetPath = mod.InstalledFiles[0] || mod.OriginalPath || "";
		if (mod.Kind === "V2" ? !gamePath : !targetPath) return;
		// Host resolves the folder (V2 mod dir / V1 file's parent) and opens it.
		await invoke("mods:open-location", {
			gamePath,
			mod: mod.Kind === "V2" ? { kind: "V2", modId: mod.Id } : { kind: "V1", targetPath },
		});
	}

	function open(mod: ModRecord) {
		if (repoMods.data?.has(mod.Id)) onOpenRepo(mod.Id);
		else onOpenFiles(mod);
	}

	const statusOf = (install: PendingInstall) =>
		install.progress === null
			? m.mod_row_installing()
			: `${Math.round(install.progress * 100)}%`;

	const empty = $derived(mods.length === 0 && placeholders.length === 0);
	// One tile per kind of mod, fanned out above the empty state
	const emptyTiles = [
		{ Icon: FileBox, at: "left-0 top-2 -rotate-12" },
		{ Icon: Cpu, at: "right-0 top-2 rotate-12" },
		{ Icon: CodeXml, at: "left-1/2 top-0 -translate-x-1/2" },
	];
</script>

<div class="@container">
	{#if empty}
		<div class="flex flex-col items-center px-6 py-14 text-center">
			<div class="relative mb-6 h-16 w-36" aria-hidden="true">
				{#each emptyTiles as tile, i (i)}
					<div
						class="bg-base-200 border-base-content/10 text-base-content/50 absolute flex size-14 items-center justify-center rounded-2xl border shadow-lg shadow-black/20 {tile.at}"
					>
						<tile.Icon size={24} />
					</div>
				{/each}
			</div>
			<h2 class="text-xl font-bold">{m.mods_empty_title()}</h2>
			<p class="text-base-content/60 mt-1 max-w-md text-sm">{m.mods_empty_hint()}</p>
			<div class="mt-5 flex gap-2">
				<button class="btn btn-primary btn-sm" disabled={!isReady} onclick={onDiscover}>
					<Search size={14} />
					{m.instance_find_mods()}
				</button>
				<button class="btn btn-sm" onclick={onAddMod}>
					<PackageOpen size={14} />
					{m.instancemenu_install_mod()}
				</button>
			</div>
		</div>
	{:else}
		<!-- Fixed layout: the name column takes what is left and truncates instead of pushing the others -->
		<div data-panel class="bg-base-200 border-base-300 rounded-box overflow-hidden border">
			<table class="table table-fixed">
				<thead>
					<tr>
						<th>{m.common_name()}</th>
						<th class="w-[30%]">{m.common_version()}</th>
						<th class="w-60 text-right">
							<button
								class="hover:text-primary inline-flex max-w-full cursor-pointer items-center gap-1 text-sm font-semibold tabular-nums transition-colors disabled:opacity-40"
								onclick={onRefresh}
								disabled={isLoading}
								aria-busy={isLoading}
							>
								<span
									class="flex size-3.5 shrink-0 items-center justify-center"
									aria-hidden="true"
								>
									{#if isLoading}
										<span class="loading loading-spinner size-3.5"></span>
									{:else}
										<RefreshCw size={14} />
									{/if}
								</span>
								{m.common_refresh()}
							</button>
						</th>
					</tr>
				</thead>
				<tbody>
					{#each shown as mod (mod.Id)}
						{@const repo = repoMods.data?.get(mod.Id)}
						<!-- Waits for the repository so a listed mod's icon never swaps in after the package's. -->
						<InstalledModRow
							{mod}
							{repo}
							icon={mod.Kind === "V2" &&
							(repoMods.isPending || installedIcons.isPending)
								? undefined
								: (repo?.icon ?? installedIcons.data?.get(mod.Id) ?? null)}
							update={updates.get(mod.Id)}
							busy={pendingByMod.get(mod.Id)}
							missing={missingByMod.get(mod.Id)}
							missingTurnedOff={onlyTurnedOff(mod)}
							versionCount={versionCounts.data?.get(mod.Id) ?? 0}
							{isRunning}
							fresh={justInstalled.has(mod.Id)}
							onToggle={toggle}
							onOpen={open}
							onOpenVersions={(mod) => onOpenRepo(mod.Id, "versions")}
							onShowInExplorer={showInExplorer}
							onRename={rename}
							onRemove={(mod) => onRemoveMod(mod.Name)}
							onFetchMissing={fetchMissing}
						/>
					{/each}
					{#each placeholders as install (install.id)}
						<tr
							class="pending-row"
							class:installing={install.progress === null}
							aria-busy="true"
							style:--progress="{install.progress === null
								? 100
								: install.progress * 100}%"
						>
							<td>
								<div class="flex items-center gap-3">
									<div
										class="bg-base-300 flex size-10 shrink-0 items-center justify-center rounded-lg"
									>
										<span class="loading loading-spinner loading-sm opacity-60"
										></span>
									</div>
									<div class="min-w-0 flex-1">
										<p class="truncate text-sm font-bold">{install.name}</p>
										<p class="truncate text-xs tabular-nums opacity-70">
											{install.author
												? `by ${install.author} · ${statusOf(install)}`
												: statusOf(install)}
										</p>
									</div>
								</div>
							</td>
							<td>
								<p class="truncate text-sm font-semibold">
									{install.version ?? ""}
								</p>
							</td>
							<td></td>
						</tr>
					{/each}
					{#if shown.length === 0 && placeholders.length === 0}
						<tr>
							<td colspan="3">
								<div
									class="text-base-content/60 flex flex-col items-center gap-2 py-10 text-center"
								>
									<SearchX size={32} class="opacity-60" />
									<p class="text-sm">{m.mods_no_match()}</p>
									<button class="link text-sm" onclick={() => (query = "")}>
										{m.repo_clear_filters()}
									</button>
								</div>
							</td>
						</tr>
					{/if}
				</tbody>
			</table>
		</div>
		{#if isReady}
			<p class="text-base-content/40 mt-3 flex items-center justify-center gap-1.5 text-xs">
				<FileDown size={13} />
				{m.mods_drop_anywhere()}
			</p>
		{/if}
	{/if}
</div>
