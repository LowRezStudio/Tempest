<script lang="ts">
	import {
		BookOpen,
		EllipsisVertical,
		FolderOpen,
		PackageOpen,
		RefreshCw,
		RotateCcw,
		Settings,
		Trash2,
	} from "@lucide/svelte";
	import { openPath, openUrl } from "@tauri-apps/plugin-opener";
	import { page } from "$app/state";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import InstanceSettingsModal from "$lib/components/library/InstanceSettingsModal.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { deleteInstance } from "$lib/core/instance-delete";
	import versions from "$lib/data/versions.json";
	import { useInstallMods } from "$lib/mods/useInstallMods";
	import { m } from "$lib/paraglide/messages";
	import { createSetupInstanceMutation } from "$lib/queries/instance";
	import {
		RIGBY_BASE_URL,
		RIGBY_MANIFEST_URL_TEMPLATE,
		WIKI_BASE_URL,
	} from "$lib/rigby/constants";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { updateInstance } from "$lib/stores/instance";
	import type { Instance } from "$lib/types/instance";
	import type { Snippet } from "svelte";

	interface Props {
		instance: Instance;
		trigger?: Snippet;
		openSettingsModal?: boolean;
	}

	let { instance, trigger, openSettingsModal = $bindable(false) }: Props = $props();

	const flatVersions = versions;
	const versionEntry = $derived(flatVersions.find((v) => v.version === instance.version));
	const wikiReference = $derived(versionEntry?.wikiReference);

	async function handleOpenWiki() {
		if (!wikiReference) return;
		try {
			await openUrl(`${WIKI_BASE_URL}${encodeURIComponent(wikiReference)}?fandom=allow`);
		} catch (error) {
			console.error("Failed to open wiki:", error);
		}
	}

	const { installMods: handleInstallMod } = useInstallMods(() => instance.path);

	let showDeleteConfirm = $state(false);

	let isSettingUp = $derived(instance.state.type === "setup");
	let isReady = $derived(instance.state.type === "prepared");
	let canRestore = $derived(!!((instance?.version || instance?.manifestId) && instance?.path));
	let isOnInstancePage = $derived(page.route.id === "/instance/[id]");

	const setupInstanceMutation = createSetupInstanceMutation();

	async function openFolder() {
		if (!instance?.path) return;
		await openPath(instance.path);
	}

	async function handleDeleteConfirm(deleteData: boolean) {
		if (!instance) return;
		await deleteInstance(instance, deleteData);
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
		if (!instance?.path || !canRestore || isSettingUp) return;

		const manifestId =
			instance.manifestId ?? versions.find((i) => i.version === instance.version)?.id;

		if (!manifestId) return;

		restoreQueue.add({
			manifests: [RIGBY_MANIFEST_URL_TEMPLATE.replace("{id}", manifestId)],
			outDir: instance.path,
			baseUrl: RIGBY_BASE_URL,
		});

		updateInstance(instance.id, {
			state: { type: "downloading" },
		});
	}

	let isSettingsModalOpen = $state(false);

	$effect(() => {
		if (openSettingsModal) {
			isSettingsModalOpen = true;
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
		{#if !isOnInstancePage}
			<PopoverMenuItem onclick={() => (isSettingsModalOpen = true)} disabled={isSettingUp}>
				<Settings size={16} />
				{m.instance_instance_settings()}
			</PopoverMenuItem>
		{/if}

		{#if isReady}
			<PopoverMenuItem onclick={handleInstallMod} disabled={isSettingUp}>
				<PackageOpen size={16} />
				{m.instancemenu_install_mod()}
			</PopoverMenuItem>
		{/if}

		{#if !isOnInstancePage}
			<PopoverMenuItem onclick={openFolder} disabled={!instance?.path}>
				<FolderOpen size={16} />
				{m.instancemenu_browse_folder()}
			</PopoverMenuItem>
		{/if}

		{#if wikiReference}
			<PopoverMenuItem onclick={handleOpenWiki}>
				<BookOpen size={16} />
				{m.instancemenu_open_wiki()}
			</PopoverMenuItem>
		{/if}

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

<InstanceSettingsModal {instance} bind:open={isSettingsModalOpen} />
