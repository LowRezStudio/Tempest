<script lang="ts">
	import { EllipsisVertical, FolderOpen, RefreshCw, RotateCcw, Trash2 } from "@lucide/svelte";
	import { remove } from "@tauri-apps/plugin-fs";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { createSetupInstanceMutation } from "$lib/queries/instance";
	import {
		isPre20Version,
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { removeInstance, updateInstance } from "$lib/stores/instance";
	import type { Instance } from "$lib/types/instance";
	import type { Snippet } from "svelte";

	interface Props {
		instance: Instance;
		trigger?: Snippet;
	}

	let { instance, trigger }: Props = $props();

	let showDeleteConfirm = $state(false);

	let isSettingUp = $derived((instance.state as { type: string }).type === "setup");
	let isDownloading = $derived((instance.state as { type: string }).type === "downloading");
	let isReady = $derived((instance.state as { type: string }).type === "prepared");
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

		if (isDownloading && instance.path) {
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
			state: { type: "setup" } as unknown as Instance["state"],
		});
		try {
			await setupInstanceMutation.mutateAsync(targetInstance);
		} catch (error) {
			console.error("Instance setup failed:", error);
		} finally {
			updateInstance(targetInstance.id, {
				state: { type: "prepared" } as unknown as Instance["state"],
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
			state: { type: "downloading" } as unknown as Instance["state"],
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
			<PopoverMenuItem onclick={handleRunSetup} disabled={isSettingUp}>
				<RefreshCw size={16} />
				Run Setup
			</PopoverMenuItem>
			{#if canRestore}
				<PopoverMenuItem onclick={handleRestore} disabled={isSettingUp}>
					<RotateCcw size={16} />
					Verify
				</PopoverMenuItem>
			{/if}
		{/if}
		<PopoverMenuItem onclick={openFolder} disabled={!instance?.path}>
			<FolderOpen size={16} />
			Browse Folder
		</PopoverMenuItem>
		<PopoverMenuItem onclick={() => (showDeleteConfirm = true)} class="text-error">
			<Trash2 size={16} />
			Delete Instance
		</PopoverMenuItem>
	{/snippet}
</PopoverMenu>

<DeleteInstanceDialog
	bind:open={showDeleteConfirm}
	instanceName={instance.label}
	onconfirm={handleDeleteConfirm}
/>
