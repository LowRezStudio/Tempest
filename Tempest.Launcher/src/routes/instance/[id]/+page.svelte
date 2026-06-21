<script lang="ts">
	import {
		Box,
		Gamepad2,
		PackageOpen,
		Play,
		RefreshCw,
		Settings,
		Square,
		Terminal,
		Trash2,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
	import InstanceMenu from "$lib/components/library/InstanceMenu.svelte";
	import ModMenu from "$lib/components/mods/ModMenu.svelte";
	import EmptyState from "$lib/components/ui/EmptyState.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { installMod } from "$lib/core/mods";
	import { confirmReplaceMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createModsQuery, createRemoveModMutation } from "$lib/queries/mods";
	import { instanceMap } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { addToast } from "$lib/stores/ui";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";

	let activeTab = $state<"content">("content");

	const instance = $derived($instanceMap[page.params.id!]);
	let isSettingUp = $derived(
		(instance?.state as { type?: string } | undefined)?.type === "setup",
	);

	const queryClient = useQueryClient();

	async function handleInstallMod() {
		if (!instance?.path) return;
		try {
			const result = await openDialog({
				directory: false,
				multiple: true,
				title: m.dialog_select_mod_files_title(),
				filters: [{ name: m.dialog_select_mod_files_filter(), extensions: ["upk", "pck"] }],
			});

			if (result) {
				const paths = Array.isArray(result) ? result : [result];
				let successCount = 0;
				let lastInstalledName = "";

				for (const filePath of paths) {
					let res = await installMod(instance.path, filePath, false);
					if (res.Conflict) {
						const modName = filePath.split(/[/\\]/).pop() || filePath;
						const confirmed = await confirmReplaceMod(modName, res.IsModConflict);
						if (confirmed) {
							res = await installMod(instance.path, filePath, true);
						} else {
							continue; // Skip this one on cancel
						}
					}

					if (res.Success) {
						successCount++;
						lastInstalledName =
							res.Mod?.Name ??
							filePath.split(/[/\\]/).pop() ??
							m.toast_mod_installed_fallback();
					} else {
						addToast({
							title: m.toast_installation_failed_title(),
							message: `${filePath.split(/[/\\]/).pop()}: ${res.Message || m.toast_installation_failed_unknown()}`,
							tone: "error",
						});
					}
				}

				if (successCount > 0) {
					if (successCount === 1) {
						addToast({
							title: m.toast_mod_installed_title(),
							message: m.toast_mod_installed_message({
								name: lastInstalledName,
							}),
							tone: "success",
						});
					} else {
						addToast({
							title: m.toast_mod_installed_title(),
							message: `Successfully installed ${successCount} mods`,
							tone: "success",
						});
					}
					queryClient.invalidateQueries({ queryKey: ["mods", instance.path] });
				}
			}
		} catch (error: any) {
			addToast({
				title: m.toast_installation_failed_title(),
				message: error.message || m.toast_installation_failed_internal(),
				tone: "error",
			});
		}
	}

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
	let isRunning = $derived($processesList.some((p) => p.instance?.id === instance?.id));
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
</script>

<div class="flex flex-col h-full bg-base-100">
	<!-- Header -->
	<Header
		title={instance?.label || m.common_loading()}
		tabs={[{ name: m.instance_content(), value: "content" }]}
		{activeTab}
		onSelectTab={(tab) => (activeTab = tab)}
		iconBg={getInstanceColor(instance)}
	>
		{#snippet icon()}
			{#if isSettingUp}
				<span
					class="loading loading-spinner loading-md"
					style="color: {getContrastColor(getInstanceColor(instance))};"
				></span>
			{:else}
				<Box size={32} style="color: {getContrastColor(getInstanceColor(instance))};" />
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
						<table class="table">
							<thead>
								<tr>
									<th>
										<button
											class="flex items-center gap-1 font-semibold text-sm"
										>
											<span>{m.common_name()}</span>
										</button>
									</th>
									<th class="w-48">{m.common_version()}</th>
									<th class="w-auto text-right">
										<button
											class="btn btn-ghost btn-sm"
											onclick={handleRefreshMods}
											disabled={isQueryLoading}
										>
											{#if isQueryLoading}
												<span class="loading loading-spinner loading-xs"
												></span>
											{:else}
												<RefreshCw size={14} />
											{/if}
											{m.common_refresh()}
										</button>
									</th>
								</tr>
							</thead>
							<tbody>
								{#each modsList as mod (mod.Id)}
									<tr class="hover">
										<td>
											<div class="flex items-center gap-3">
												<div
													class="w-10 h-10 rounded-lg bg-base-200 flex items-center justify-center shrink-0"
												>
													<Box size={20} class="opacity-60" />
												</div>
												<div class="flex-1 min-w-0">
													<h3 class="font-bold text-sm truncate">
														{mod.Name}
													</h3>
													<p class="text-xs opacity-70">
														by {mod.Author}
													</p>
												</div>
											</div>
										</td>
										<td>
											<p class="font-semibold text-sm">{mod.Version}</p>
										</td>
										<td>
											<div class="flex items-center justify-end gap-1">
												<button
													class="btn btn-error btn-sm btn-square"
													disabled={removeModMutation.isPending}
													onclick={() => handleRemoveMod(mod.Name)}
												>
													<Trash2 size={14} />
												</button>
												<ModMenu {mod} gamePath={instance?.path ?? ""} />
											</div>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				</div>
			</div>
		{/if}
	</div>
</div>
