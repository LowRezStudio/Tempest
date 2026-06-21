<script lang="ts">
	import {
		EllipsisVertical,
		FolderOpen,
		PackageOpen,
		RefreshCw,
		RotateCcw,
		Trash2,
	} from "@lucide/svelte";
	import { useQueryClient } from "@tanstack/svelte-query";
	import { open as openDialog } from "@tauri-apps/plugin-dialog";
	import { remove } from "@tauri-apps/plugin-fs";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { installMod } from "$lib/core/mods";
	import { confirmReplaceMod } from "$lib/mods/ui";
	import { m } from "$lib/paraglide/messages";
	import { createSetupInstanceMutation } from "$lib/queries/instance";
	import {
		isPre20Version,
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { removeInstance, updateInstance } from "$lib/stores/instance";
	import { addToast } from "$lib/stores/ui";
	import type { Instance } from "$lib/types/instance";
	import type { Snippet } from "svelte";

	interface Props {
		instance: Instance;
		trigger?: Snippet;
	}

	let { instance, trigger }: Props = $props();

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
		{#if isReady}
			<PopoverMenuItem onclick={handleInstallMod} disabled={isSettingUp}>
				<PackageOpen size={16} />
				{m.instancemenu_install_mod()}
			</PopoverMenuItem>
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
		<PopoverMenuItem onclick={openFolder} disabled={!instance?.path}>
			<FolderOpen size={16} />
			{m.instancemenu_browse_folder()}
		</PopoverMenuItem>
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
