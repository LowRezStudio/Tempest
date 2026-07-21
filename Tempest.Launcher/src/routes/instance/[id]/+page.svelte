<script lang="ts">
	import {
		FolderOpen,
		Gamepad2,
		PackageOpen,
		Play,
		Plus,
		Settings,
		Square,
		Terminal,
	} from "@lucide/svelte";
	import { openPath } from "@tauri-apps/plugin-opener";
	import CrystalIcon from "$lib/components/ui/CrystalIcon.svelte";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import InstanceMenu from "$lib/components/library/InstanceMenu.svelte";
	import InstanceModTable from "$lib/components/mods/InstanceModTable.svelte";
	import ModDetailsModal from "$lib/components/mods/ModDetailsModal.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { useInstallMods } from "$lib/mods/useInstallMods";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createModsQuery, createRemoveModMutation } from "$lib/queries/mods";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { getContrastColor, getInstanceColor, getMutedInstanceColor } from "$lib/utils/color";
	import type { ModRecord } from "$lib/core/mods";

	let activeTab = $state<"content">("content");

	const instance = $derived(instanceMap.value[page.params.id!]);
	let isSettingUp = $derived(
		(instance?.state as { type?: string } | undefined)?.type === "setup",
	);
	let isReady = $derived(instance?.state?.type === "prepared");

	const { installMods: handleInstallMod } = useInstallMods(() => instance?.path);

	const modsQuery = createModsQuery(() => instance?.path ?? "");
	let modsList = $derived(modsQuery.data ?? []);
	let isQueryLoading = $derived(modsQuery.isFetching);

	const removeModMutation = createRemoveModMutation();

	function handleRemoveMod(modName: string) {
		if (!instance) return;
		removeModMutation.mutate({ gamePath: instance.path, modName });
	}

	function handleRefreshMods() {
		modsQuery.refetch();
	}

	$effect(() => {
		if (!instance) {
			goto("/library");
		}
	});

	let isSettingsModalOpen = $state(false);

	// Check if this instance is currently running
	let isRunning = $derived(processesList.value.some((p) => p.instance?.id === instance?.id));
	const launchGameMutation = createLaunchGameMutation();
	const killGameMutation = createKillGameMutation();
	let isLaunching = $derived(launchGameMutation.isPending);
	let isKilling = $derived(killGameMutation.isPending);
	let launchError = $derived(launchGameMutation.error?.message ?? "");
	let killError = $derived(killGameMutation.error?.message ?? "");

	function handleLaunchToggle() {
		if (!instance) return;
		if (isRunning) {
			killGameMutation.mutate(instance);
			return;
		}
		launchGameMutation.mutate(instance);
	}

	function clearLaunchError() {
		launchGameMutation.reset();
	}

	function clearKillError() {
		killGameMutation.reset();
	}

	async function handleOpenInstanceFolder() {
		if (!instance?.path) return;
		await openPath(instance.path);
	}

	let isFilesDialogOpen = $state(false);
	let selectedModForFiles = $state<ModRecord | null>(null);

	function handleOpenFiles(mod: ModRecord) {
		selectedModForFiles = mod;
		isFilesDialogOpen = true;
	}
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<Header
		title={instance?.label || m.common_loading()}
		tabs={[{ name: m.instance_content(), value: "content" }]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
		iconBg={getMutedInstanceColor(instance)}
	>
		{#snippet icon()}
			{#if isSettingUp}
				<span
					class="loading loading-spinner loading-md"
					style="color: {getContrastColor(getInstanceColor(instance))};"
				></span>
			{:else}
				<CrystalIcon class="w-11 h-11" />
			{/if}
		{/snippet}
		{#snippet actions()}
			<button
				class="btn text-sm"
				class:btn-accent={!isRunning}
				class:btn-error={isRunning}
				disabled={isLaunching || isKilling || isSettingUp}
				aria-busy={isLaunching || isKilling || isSettingUp}
				onclick={handleLaunchToggle}
			>
				{#if isLaunching}
					<span class="loading loading-spinner loading-xs"></span>
					{m.common_launching()}
				{:else if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
					{m.common_stopping_label()}
				{:else if isRunning}
					<Square size={16} />
					{m.common_stop()}
				{:else}
					<Play size={16} />
					{m.common_play()}
				{/if}
			</button>
			<button class="btn btn-square" onclick={() => (isSettingsModalOpen = true)}>
				<Settings size={16} />
			</button>
			<button
				class="btn btn-square"
				disabled={!instance?.path}
				onclick={handleOpenInstanceFolder}
			>
				<FolderOpen size={16} />
			</button>
			{#if isReady}
				<button class="btn btn-square" onclick={handleInstallMod}>
					<Plus size={16} />
				</button>
			{/if}
			{#if instance}
				<InstanceMenu {instance} bind:openSettingsModal={isSettingsModalOpen} />
			{/if}
		{/snippet}
		{#snippet subtitle()}
			<div class="flex items-center gap-1.5">
				<Gamepad2 size={14} />
				<span>{instance?.version || m.common_unknown_version()}</span>
				{#if instance?.launchOptions?.args && instance.launchOptions.args.length > 0}
					<div
						class="flex items-center gap-1 min-w-0 ml-1.5"
						title={instance.launchOptions.args.join(" ")}
					>
						<Terminal size={14} class="shrink-0" />
						<span class="text-xs font-mono truncate max-w-[350px]">
							{instance.launchOptions.args.join(" ")}
						</span>
					</div>
				{/if}
			</div>
		{/snippet}
		{#snippet errors()}
			{#if launchError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{launchError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearLaunchError}>
							{m.common_dismiss()}
						</button>
					</div>
				</div>
			{/if}
			{#if killError}
				<div class="pt-3">
					<div class="alert alert-error">
						<span>{killError}</span>
						<button class="btn btn-ghost btn-sm" onclick={clearKillError}>
							{m.common_dismiss()}
						</button>
					</div>
				</div>
			{/if}
		{/snippet}
	</Header>

	<!-- Content Area -->
	<div class="flex-1 flex flex-col overflow-hidden bg-base-100">
		{#if activeTab === "content"}
			<div class="flex-1 overflow-y-auto">
				<div class="px-4 py-6">
					{#if modsList.length === 0}
						<EmptyState
							title={m.instance_no_content()}
							description={m.instance_drag_import_hint()}
						>
							{#snippet icon()}
								<PackageOpen size={48} />
							{/snippet}
							{#snippet actions()}
								<button
									class="btn btn-accent btn-sm mt-2"
									onclick={handleInstallMod}
								>
									<PackageOpen size={14} />
									{m.instancemenu_install_mod()}
								</button>
							{/snippet}
						</EmptyState>
					{:else}
						<InstanceModTable
							mods={modsList}
							gamePath={instance?.path ?? ""}
							{isRunning}
							isLoading={isQueryLoading}
							isRemoving={removeModMutation.isPending}
							onRefresh={handleRefreshMods}
							onRemoveMod={handleRemoveMod}
							onOpenDetails={handleOpenFiles}
						/>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</div>

<ModDetailsModal
	mod={selectedModForFiles}
	bind:open={isFilesDialogOpen}
	instancePath={instance?.path}
/>
