<script lang="ts">
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import { PackageOpen, Plus, Search } from "@lucide/svelte";
	import InstanceHeader from "$lib/components/library/InstanceHeader.svelte";
	import InstancePresetsTab from "$lib/components/library/InstancePresetsTab.svelte";
	import InstanceSettings from "$lib/components/library/InstanceSettings.svelte";
	import InstanceModTable from "$lib/components/mods/InstanceModTable.svelte";
	import ModDetailsModal from "$lib/components/mods/ModDetailsModal.svelte";
	import RepoBrowser from "$lib/components/mods/RepoBrowser.svelte";
	import RepoModModal from "$lib/components/mods/RepoModModal.svelte";
	import RepoUpdatesModal from "$lib/components/mods/RepoUpdatesModal.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import { pendingInstalls } from "$lib/mods/pendingInstalls.svelte";
	import { useInstallMods } from "$lib/mods/useInstallMods";
	import { m } from "$lib/paraglide/messages";
	import { createModsQuery, createRemoveModMutation } from "$lib/queries/mods";
	import { createRepoUpdatesQuery } from "$lib/queries/repo";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { modDeveloper } from "$lib/stores/settings.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import type { ModRecord } from "$lib/core/mods";

	type Tab = "content" | "browse" | "presets" | "settings";
	let activeTab = $state<Tab>("content");

	// Deep-link support: /instance/<id>?tab=settings (or browse/presets) opens directly there.
	$effect(() => {
		const tab = page.url.searchParams.get("tab");
		activeTab =
			tab === "settings" || tab === "browse" || (tab === "presets" && modDeveloper.value)
				? tab
				: "content";
	});

	const instance = $derived(instanceMap.value[page.params.id!]);
	let isReady = $derived(instance?.state?.type === "prepared");

	const { installMods: handleInstallMod } = useInstallMods(() => instance?.path);

	const modsQuery = createModsQuery(() => instance?.path ?? "");
	let modsList = $derived(modsQuery.data ?? []);
	let isInitialLoading = $derived(modsQuery.isPending);
	// Only a manual refresh spins the button; background resyncs after edits stay silent.
	let isRefreshingMods = $state(false);

	const removeModMutation = createRemoveModMutation();

	// Repository update checks for the installed V2 mods (the repository keys on manifest ids).
	const updatesQuery = createRepoUpdatesQuery(() => ({
		patch: instance?.version ?? "",
		mods: modsList
			.filter((mod) => mod.Kind === "V2")
			.map((mod) => ({ id: mod.Id, version: mod.Version })),
	}));
	const updates = $derived(new Map((updatesQuery.data ?? []).map((u) => [u.id, u])));
	const updateCount = $derived([...updates.values()].filter((u) => u.updateAvailable).length);

	// Mod details from the repository (readme, versions, downgrade), opened from the table or the Browse tab.
	let repoModId = $state<string | null>(null);
	let repoModOpen = $state(false);
	function openRepoMod(modId: string) {
		repoModId = modId;
		repoModOpen = true;
	}

	// One popup per instance per session when updates are found, from the first check after
	// opening the instance only: rechecks after installs or downgrades made here stay silent.
	let updatesOpen = $state(false);
	let updatesCheckedFor: string | undefined;
	$effect(() => {
		if (!instance || !updatesQuery.isSuccess || updatesCheckedFor === instance.id) return;
		updatesCheckedFor = instance.id;
		const available = [...updates.values()].filter((u) => u.updateAvailable);
		if (available.length === 0) return;
		const key = `repo-updates-shown:${instance.id}:${available
			.map((u) => `${u.id}@${u.latest?.version}`)
			.sort()
			.join(",")}`;
		try {
			if (sessionStorage.getItem(key)) return;
			sessionStorage.setItem(key, "1");
		} catch {
			return;
		}
		updatesOpen = true;
	});

	function handleRemoveMod(modName: string) {
		if (!instance) return;
		removeModMutation.mutate(
			{ gamePath: instance.path, modName },
			{
				onError: (error) =>
					addToast({ message: `${modName}: ${error.message}`, tone: "error" }),
			},
		);
	}

	async function handleRefreshMods() {
		isRefreshingMods = true;
		try {
			await modsQuery.refetch();
		} finally {
			isRefreshingMods = false;
		}
	}

	$effect(() => {
		if (!instance) {
			goto("/library");
		}
	});

	let isRunning = $derived(processesList.value.some((p) => p.instance?.id === instance?.id));

	let isFilesDialogOpen = $state(false);
	let selectedModForFiles = $state<ModRecord | null>(null);
	function handleOpenFiles(mod: ModRecord) {
		selectedModForFiles = mod;
		isFilesDialogOpen = true;
	}
</script>

<div class="flex h-full flex-col">
	{#if instance}
		<InstanceHeader
			{instance}
			tabs={[
				{ name: m.instance_content(), value: "content", count: modsQuery.data?.length },
				{ name: m.instance_browse(), value: "browse" },
				...(modDeveloper.value
					? [{ name: m.instance_presets(), value: "presets" as const }]
					: []),
				{ name: m.settings_title(), value: "settings" },
			]}
			bind:activeTab
			{updateCount}
			onShowUpdates={() => (updatesOpen = true)}
			onManagePresets={() => (activeTab = "presets")}
		/>
	{/if}

	<!-- Content Area -->
	<div class="flex flex-1 flex-col overflow-hidden">
		{#if activeTab === "content"}
			<div class="flex-1 [scrollbar-gutter:stable] overflow-y-auto">
				<!-- Less on top than the other tabs: the table's header row brings its own padding -->
				<div class="px-4 pt-2 pb-6">
					{#if isInitialLoading}
						<div
							class="flex h-80 flex-col items-center justify-center gap-4 text-center"
						>
							<span class="loading loading-spinner loading-lg opacity-30"></span>
							<p class="text-base-content/70 text-lg font-semibold">
								{m.common_loading()}
							</p>
						</div>
					{:else}
						<InstanceModTable
							mods={modsList}
							gamePath={instance?.path ?? ""}
							{isRunning}
							isLoading={isRefreshingMods}
							onRefresh={handleRefreshMods}
							onRemoveMod={handleRemoveMod}
							onOpenDetails={handleOpenFiles}
							{updates}
							patch={instance?.version}
							onOpenRepo={(mod) => openRepoMod(mod.Id)}
						/>
						{#if modsList.length === 0 && !pendingInstalls.value.some((p) => p.gamePath === instance?.path)}
							<EmptyState
								title={m.instance_no_content()}
								description={m.instance_drag_import_hint()}
							>
								{#snippet icon()}
									<PackageOpen size={48} />
								{/snippet}
								{#snippet actions()}
									<div class="mt-2 flex gap-2">
										<button
											class="btn btn-primary btn-sm"
											disabled={!isReady}
											onclick={() => (activeTab = "browse")}
										>
											<Search size={14} />
											{m.instance_find_mods()}
										</button>
										<button class="btn btn-sm" onclick={handleInstallMod}>
											<PackageOpen size={14} />
											{m.instancemenu_install_mod()}
										</button>
									</div>
								{/snippet}
							</EmptyState>
						{:else if isReady}
							<!-- The window handles drops itself; this is the hint and the file picker. -->
							<button
								class="border-base-content/10 hover:border-primary/40 text-base-content/40 hover:text-base-content/70 rounded-box mt-2 flex w-full cursor-pointer items-center justify-center gap-1.5 border border-dashed px-4 py-2.5 text-xs transition-colors"
								onclick={handleInstallMod}
							>
								<Plus size={14} />
								{m.instance_add_mod_tile()}
							</button>
						{/if}
					{/if}
				</div>
			</div>
		{:else if activeTab === "browse"}
			<div class="flex-1 [scrollbar-gutter:stable] overflow-y-auto">
				<div class="px-4 py-6">
					{#if isReady && instance}
						<RepoBrowser
							gamePath={instance.path}
							patch={instance.version}
							installed={modsList}
							{isRunning}
							onOpenDetails={openRepoMod}
						/>
					{:else}
						<p class="text-base-content/60 text-sm">{m.common_loading()}</p>
					{/if}
				</div>
			</div>
		{:else if activeTab === "presets" && modDeveloper.value}
			<div class="flex-1 overflow-y-auto">
				<div class="p-6">
					{#if instance}
						<InstancePresetsTab {instance} />
					{/if}
				</div>
			</div>
		{:else if activeTab === "settings" && instance}
			{#key instance.id}
				<InstanceSettings {instance} {isRunning} />
			{/key}
		{/if}
	</div>
</div>

<ModDetailsModal
	mod={selectedModForFiles}
	bind:open={isFilesDialogOpen}
	instancePath={instance?.path}
/>

{#if instance}
	<RepoModModal
		bind:open={repoModOpen}
		modId={repoModId}
		gamePath={instance.path}
		patch={instance.version}
		installedVersion={modsList.find((mod) => mod.Id === repoModId)?.Version}
		installedMod={modsList.find((mod) => mod.Id === repoModId)}
		{isRunning}
	/>
	<RepoUpdatesModal
		bind:open={updatesOpen}
		gamePath={instance.path}
		mods={modsList}
		{updates}
		{isRunning}
	/>
{/if}
