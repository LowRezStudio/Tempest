<script lang="ts">
	import {
		Box,
		Folder,
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
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installMod } from "$lib/core/mods";
	import { confirmReplaceMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createInstancePlatformsQuery } from "$lib/queries/instance";
	import { createModsQuery, createRemoveModMutation } from "$lib/queries/mods";
	import { instanceMap, updateInstance } from "$lib/stores/instance";
	import { processesList } from "$lib/stores/processes";
	import { addToast } from "$lib/stores/ui";
	import { parseArgs } from "$lib/utils/args";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import type { InstancePlatform } from "$lib/types/instance";

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
	const colorPresets = [
		"#ef4444",
		"#f97316",
		"#f59e0b",
		"#10b981",
		"#06b6d4",
		"#3b82f6",
		"#6366f1",
		"#8b5cf6",
		"#ec4899",
		"#6b7280",
	];
	let editName = $state("");
	let editVersion = $state("");
	let editPath = $state("");
	let editPlatform = $state<InstancePlatform>("Win64");
	let editArgs = $state<string[]>([]);
	let editColor = $state("");
	let argsInput = $state("");

	function addArgs() {
		if (!argsInput.trim()) return;
		const newArgs = parseArgs(argsInput);
		editArgs = [...editArgs, ...newArgs];
		argsInput = "";
	}

	function removeArg(index: number) {
		editArgs = editArgs.filter((_, i) => i !== index);
	}

	function moveArg(index: number, direction: -1 | 1) {
		const newIndex = index + direction;
		if (newIndex < 0 || newIndex >= editArgs.length) return;
		const newArgs = [...editArgs];
		[newArgs[index], newArgs[newIndex]] = [newArgs[newIndex], newArgs[index]];
		editArgs = newArgs;
	}

	function handleArgsKeydown(e: KeyboardEvent) {
		if (e.key === "Enter") {
			e.preventDefault();
			addArgs();
		}
	}

	function openSettings() {
		if (!instance) return;
		editName = instance.label;
		editVersion = instance.version || "";
		editPath = instance.path;
		editPlatform = instance.launchOptions?.platform ?? "Win64";
		editArgs = instance.launchOptions?.args ?? [];
		editColor = getInstanceColor(instance);
		argsInput = "";
		isSettingsModalOpen = true;
	}

	function saveSettings() {
		if (!instance) return;
		updateInstance(instance.id, {
			label: editName,
			version: editVersion,
			path: editPath,
			color: editColor,
			launchOptions: {
				...instance.launchOptions,
				platform: editPlatform,
				args: editArgs,
			},
		});
		isSettingsModalOpen = false;
	}

	async function handleBrowse() {
		const result = await openDialog({
			directory: true,
			multiple: false,
			title: m.settings_select_instance_folder(),
		});
		if (result) {
			editPath = result;
		}
	}

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

	const platformsQuery = createInstancePlatformsQuery(() => editPath);

	let availablePlatforms = $derived(
		(editPath ? platformsQuery.data : undefined) ?? ([] as InstancePlatform[]),
	);
	let isDetectingPlatforms = $derived(platformsQuery.isFetching);

	$effect(() => {
		if (!availablePlatforms.length) return;
		if (!availablePlatforms.includes(editPlatform)) {
			editPlatform = availablePlatforms[0] ?? "Win64";
		}
	});
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
			<button class="btn btn-square" onclick={openSettings}>
				<Settings size={16} />
			</button>
			{#if instance}
				<InstanceMenu {instance} />
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

<Modal bind:open={isSettingsModalOpen} title={m.instance_instance_settings()} class="max-w-2xl">
	<div class="space-y-4">
		<div class="form-control">
			<label for="instance-name" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_name()}</span>
			</label>
			<input
				id="instance-name"
				type="text"
				placeholder={m.instance_name()}
				class="input input-bordered w-full"
				bind:value={editName}
			/>
		</div>

		<div class="form-control">
			<label for="instance-version" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_version()}</span>
			</label>
			<input
				id="instance-version"
				type="text"
				placeholder="1.0.0"
				class="input input-bordered w-full"
				bind:value={editVersion}
			/>
		</div>

		<div class="form-control">
			<label for="instance-color" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_color()}</span>
			</label>
			<div class="flex items-center gap-6 w-full">
				<div
					class="w-20 h-20 rounded-xl flex items-center justify-center shrink-0 overflow-hidden shadow-sm border border-base-content/10"
					style="background-color: {editColor};"
				>
					<Box size={40} style="color: {getContrastColor(editColor)};" />
				</div>

				<div class="flex-1 h-20 flex flex-col justify-between min-w-0">
					<div class="flex flex-wrap justify-between w-full">
						{#each colorPresets as preset}
							<button
								type="button"
								class="w-7 h-7 rounded-full border border-base-content/10 cursor-pointer transition-transform hover:scale-110 active:scale-95 flex items-center justify-center shrink-0"
								style="background-color: {preset};"
								onclick={() => (editColor = preset)}
								title={preset}
							>
								{#if editColor.toLowerCase() === preset.toLowerCase()}
									<span class="text-white text-[10px] font-bold">✓</span>
								{/if}
							</button>
						{/each}
					</div>

					<div class="flex items-center gap-2 w-full">
						<input
							id="instance-color"
							type="color"
							class="w-16 h-10 p-0.5 rounded border border-base-300 cursor-pointer bg-base-100 shrink-0"
							bind:value={editColor}
						/>
						<button
							type="button"
							class="btn btn-sm flex-1"
							onclick={() => {
								editColor = getInstanceColor({
									...instance,
									label: editName,
									color: undefined,
								});
							}}
						>
							{m.instance_reset_color()}
						</button>
					</div>
				</div>
			</div>
		</div>

		<div class="form-control">
			<label for="instance-path" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_installation_path()}</span>
			</label>
			<div class="join w-full">
				<input
					id="instance-path"
					type="text"
					placeholder="/path/to/instance"
					class="input input-bordered join-item flex-1 font-mono"
					bind:value={editPath}
				/>
				<button class="btn btn-accent join-item" onclick={handleBrowse}>
					<Folder size={16} />
					{m.common_browse()}
				</button>
			</div>
		</div>

		<div class="form-control">
			<label for="instance-args" class="label py-0.5">
				<span class="label-text text-sm">{m.instance_launch_arguments()}</span>
			</label>
			<div class="space-y-2">
				<div class="join w-full">
					<input
						id="instance-args"
						type="text"
						placeholder=""
						class="input input-bordered join-item flex-1 font-mono text-sm"
						bind:value={argsInput}
						onkeydown={handleArgsKeydown}
					/>
					<button class="btn btn-accent join-item" onclick={addArgs} type="button">
						{m.common_add()}
					</button>
				</div>
				{#if editArgs.length > 0}
					<div class="flex flex-wrap gap-1.5">
						{#each editArgs as arg, i (i)}
							<span class="badge badge-ghost badge-neutral gap-1">
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => moveArg(i, -1)}
									disabled={i === 0}
								>
									&#8592;
								</button>
								<span class="font-mono text-xs">{arg}</span>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => moveArg(i, 1)}
									disabled={i === editArgs.length - 1}
								>
									&#8594;
								</button>
								<button
									type="button"
									class="btn btn-ghost btn-xs btn-square p-0 h-4 w-4 min-h-0 text-base-content/60 hover:text-base-content"
									onclick={() => removeArg(i)}
								>
									&times;
								</button>
							</span>
						{/each}
					</div>
				{/if}
				<p class="text-xs opacity-60">{m.instance_space_separated()}</p>
			</div>
		</div>

		{#if availablePlatforms.length > 1}
			<div class="form-control">
				<label for="instance-platform" class="label py-0.5">
					<span class="label-text text-sm">{m.instance_platform()}</span>
				</label>
				<select
					id="instance-platform"
					class="select select-bordered w-full"
					disabled={isDetectingPlatforms}
					bind:value={editPlatform}
				>
					{#each availablePlatforms as platform}
						<option value={platform}>{platform}</option>
					{/each}
				</select>
			</div>
		{/if}
	</div>

	{#snippet actions()}
		<div class="flex justify-end gap-2 w-full">
			<button class="btn btn-ghost" onclick={() => (isSettingsModalOpen = false)}>
				{m.common_cancel()}
			</button>
			<button class="btn btn-accent" onclick={saveSettings}>
				{m.common_save_changes()}
			</button>
		</div>
	{/snippet}
</Modal>
