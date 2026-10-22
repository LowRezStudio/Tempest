<script lang="ts">
	import { goto } from "$app/navigation";
	import { page } from "$app/state";
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
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { deleteInstance } from "$lib/core/instance-delete";
	import { manifestIdOf, runSetup, verifyFiles } from "$lib/core/instance-maintenance";
	import { useInstallMods } from "$lib/mods/useInstallMods";
	import { m } from "$lib/paraglide/messages";
	import { openPath, openUrl } from "$lib/platform/api";
	import { WIKI_BASE_URL } from "$lib/rigby/constants";
	import { patchOf } from "$lib/utils/versions";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		triggerClass?: string;
	}

	let { instance, triggerClass = "btn btn-sm btn-square btn-ghost" }: Props = $props();

	const wikiReference = $derived(patchOf(instance.version)?.wikiReference);

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
	let canRestore = $derived(!!(instance.path && manifestIdOf(instance)));
	let isOnInstancePage = $derived(page.route.id === "/instance/[id]");

	async function openFolder() {
		if (!instance?.path) return;
		await openPath(instance.path);
	}

	async function handleDeleteConfirm(mode: "library" | "library_mods" | "delete") {
		if (!instance) return;
		await deleteInstance(instance, mode);
	}

	function handleRunSetup() {
		if (!instance || isSettingUp) return;
		void runSetup(instance);
	}

	function handleRestore() {
		if (isSettingUp) return;
		if (verifyFiles(instance) && isOnInstancePage) {
			goto("/downloads");
		}
	}
</script>

<PopoverMenu {triggerClass} triggerLabel={m.instancemenu_more()}>
	{#snippet trigger()}
		<EllipsisVertical size={16} />
	{/snippet}
	{#snippet children()}
		{#if !isOnInstancePage}
			<PopoverMenuItem
				onclick={() => goto(`/instance/${instance.id}?tab=settings`)}
				disabled={isSettingUp}
			>
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
