<script lang="ts">
	import {
		Box,
		EllipsisVertical,
		Folder,
		FolderOpen,
		PackageOpen,
		RefreshCw,
		RotateCcw,
		Settings,
		Trash2,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { remove } from "@tauri-apps/plugin-fs";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { installMod } from "$lib/core/mods";
	import { confirmReplaceMod, confirmUnverifiedMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import {
		createInstancePlatformsQuery,
		createSetupInstanceMutation,
	} from "$lib/queries/instance";
	import {
		isPre20Version,
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { removeInstance, updateInstance } from "$lib/stores/instance";
	import { addToast, removeToast } from "$lib/stores/ui";
	import { parseArgs } from "$lib/utils/args";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import type { Instance, InstancePlatform } from "$lib/types/instance";
	import type { Snippet } from "svelte";

	interface Props {
		instance: Instance;
		trigger?: Snippet;
		openSettingsModal?: boolean;
	}

	let { instance, trigger, openSettingsModal = $bindable(false) }: Props = $props();

	const queryClient = useQueryClient();

	async function handleInstallMod() {
		if (!instance?.path) return;
		try {
			const result = await openDialog({
				directory: false,
				multiple: true,
				title: m.dialog_select_mod_files_title(),
				filters: [
					{
						name: m.dialog_select_mod_files_filter(),
						extensions: ["upk", "pck", "zip", "tempest"],
					},
				],
			});

			if (result) {
				const paths = Array.isArray(result) ? result : [result];
				let successCount = 0;
				let lastInstalledName = "";

				for (const filePath of paths) {
					const modFileName = filePath.split(/[/\\]/).pop() || filePath;
					let installingToastId: string | undefined;
					try {
						installingToastId = addToast({
							title: m.toast_mod_installing_title(),
							message: m.toast_mod_installing_message({ name: modFileName }),
							tone: "info",
							duration: 0,
						});

						let allowedUnsigned = false;
						let res = await installMod(instance.path, filePath, false, false);
						if (res.Unverified) {
							const confirmed = await confirmUnverifiedMod(modFileName);
							if (confirmed) {
								allowedUnsigned = true;
								res = await installMod(instance.path, filePath, false, true);
							} else {
								if (installingToastId) removeToast(installingToastId);
								continue; // Skip this one on cancel
							}
						}

						if (res.Conflict) {
							const confirmed = await confirmReplaceMod(
								modFileName,
								res.IsModConflict,
							);
							if (confirmed) {
								res = await installMod(
									instance.path,
									filePath,
									true,
									allowedUnsigned,
								);
							} else {
								if (installingToastId) removeToast(installingToastId);
								continue; // Skip this one on cancel
							}
						}

						if (installingToastId) removeToast(installingToastId);

						if (res.Success) {
							successCount++;
							lastInstalledName =
								res.Mod?.Name ?? modFileName ?? m.toast_mod_installed_fallback();
						} else {
							addToast({
								title: m.toast_installation_failed_title(),
								message: `${modFileName}: ${res.Message || m.toast_installation_failed_unknown()}`,
								tone: "error",
							});
						}
					} catch (error: any) {
						if (installingToastId) removeToast(installingToastId);
						addToast({
							title: m.toast_installation_failed_title(),
							message: `${modFileName}: ${error.message || m.toast_installation_failed_internal()}`,
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

	let showDeleteConfirm = $state(false);

	let isSettingUp = $derived(instance.state.type === "setup");
	let isDownloading = $derived(instance.state.type === "downloading");
	let isPaused = $derived(instance.state.type === "paused");
	let isReady = $derived(instance.state.type === "prepared");
	let isActive = $derived(isDownloading || isPaused);
	let canRestore = $derived(
		instance?.version && instance?.path && isPre20Version(instance.version),
	);

	const setupInstanceMutation = createSetupInstanceMutation();

	async function openFolder() {
		if (!instance?.path) return;
		await revealItemInDir(instance.path);
	}

	async function handleDeleteConfirm(deleteData: boolean) {
		if (!instance) return;

		if (isActive && instance.path) {
			restoreQueue.cancel(instance.path);
		}

		if (deleteData && instance.path) {
			try {
				await remove(instance.path, { recursive: true });
			} catch (error) {
				console.error("Failed to delete instance data:", error);
			}
		}

		removeInstance(instance.id);
	}

	function handleRunSetup() {
		if (!instance || isSettingUp) return;
		void runSetup(instance);
	}

	async function runSetup(targetInstance: Instance) {
		updateInstance(targetInstance.id, {
			state: { type: "setup" },
		});
		try {
			await setupInstanceMutation.mutateAsync(targetInstance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(targetInstance.id, {
				state: { type: "prepared" },
			});
		}
	}

	function handleRestore() {
		if (!instance?.version || !instance?.path || !canRestore || isSettingUp) return;

		restoreQueue.add({
			manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{version}", instance.version)],
			outDir: instance.path,
			baseUrl: RIGBY_BASE_URL,
		});

		updateInstance(instance.id, {
			state: { type: "downloading" },
		});
	}

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

	$effect(() => {
		if (openSettingsModal) {
			openSettings();
			openSettingsModal = false;
		}
	});
</script>

<PopoverMenu>
	{#snippet trigger()}
		{#if trigger}
			{@render trigger()}
		{:else}
			<button class="btn btn-square">
				<EllipsisVertical size={16} />
			</button>
		{/if}
	{/snippet}
	{#snippet children()}
		<PopoverMenuItem onclick={openSettings} disabled={isSettingUp}>
			<Settings size={16} />
			{m.instance_instance_settings()}
		</PopoverMenuItem>

		{#if isReady}
			<PopoverMenuItem onclick={handleInstallMod} disabled={isSettingUp}>
				<PackageOpen size={16} />
				{m.instancemenu_install_mod()}
			</PopoverMenuItem>
		{/if}

		<PopoverMenuItem onclick={openFolder} disabled={!instance?.path}>
			<FolderOpen size={16} />
			{m.instancemenu_browse_folder()}
		</PopoverMenuItem>

		{#if isReady}
			<PopoverMenuItem onclick={handleRunSetup} disabled={isSettingUp}>
				<RefreshCw size={16} />
				{m.instancemenu_run_setup()}
			</PopoverMenuItem>
			{#if canRestore}
				<PopoverMenuItem onclick={handleRestore} disabled={isSettingUp}>
					<RotateCcw size={16} />
					{m.instancemenu_verify()}
				</PopoverMenuItem>
			{/if}
		{/if}

		<PopoverMenuItem onclick={() => (showDeleteConfirm = true)} class="text-error">
			<Trash2 size={16} />
			{m.instancemenu_delete_instance()}
		</PopoverMenuItem>
	{/snippet}
</PopoverMenu>

<DeleteInstanceDialog
	bind:open={showDeleteConfirm}
	instanceName={instance.label}
	onconfirm={handleDeleteConfirm}
/>

<Modal
	bind:open={isSettingsModalOpen}
	title={m.instance_instance_settings()}
	class="max-w-2xl"
	onsubmit={saveSettings}
>
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
				<button class="btn btn-accent join-item" type="button" onclick={handleBrowse}>
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
			<button
				class="btn btn-ghost"
				type="button"
				onclick={() => (isSettingsModalOpen = false)}
			>
				{m.common_cancel()}
			</button>
			<button class="btn btn-accent" type="submit">
				{m.common_save_changes()}
			</button>
		</div>
	{/snippet}
</Modal>
