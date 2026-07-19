<script lang="ts">
	import { EllipsisVertical, FolderOpen, Pencil } from "@lucide/svelte";
	import { path } from "@tauri-apps/api";
	import { stat } from "@tauri-apps/plugin-fs";
	import { openPath } from "@tauri-apps/plugin-opener";
	import Modal from "$lib/components/ui/Modal.svelte";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { m } from "$lib/paraglide/messages";
	import { createRenameModMutation } from "$lib/queries/mods";
	import type { ModRecord } from "$lib/core/mods";

	interface Props {
		mod: ModRecord;
		gamePath: string;
	}

	let { mod, gamePath }: Props = $props();

	let targetPath = $derived(mod.InstalledFiles[0] || mod.OriginalPath || "");

	// ponytail: extract filename from path for display as placeholder when cleared
	let actualFilename = $derived(
		targetPath ?
			targetPath.substring(
				Math.max(targetPath.lastIndexOf("/"), targetPath.lastIndexOf("\\")) + 1,
			)
		:	"",
	);

	async function showInExplorer() {
		if (mod.Kind === "V2") {
			if (!gamePath) return;
			const modFolder = await path.join(gamePath, ".tempest", "v2", "mods", mod.Id);
			await openPath(modFolder);
			return;
		}
		if (!targetPath) return;

		let resolvedFolder = targetPath;
		try {
			const info = await stat(targetPath);
			if (info.isFile) {
				resolvedFolder = await path.dirname(targetPath);
			}
		} catch (error) {
			console.error("Failed to stat mod targetPath:", error);
			// Fallback: if it ends with an extension or seems like a file, try directory
			if (
				targetPath.includes(".") &&
				!targetPath.endsWith("/") &&
				!targetPath.endsWith("\\")
			) {
				resolvedFolder = await path.dirname(targetPath);
			}
		}
		await openPath(resolvedFolder);
	}

	let isRenameModalOpen = $state(false);
	let editName = $state("");

	const renameMutation = createRenameModMutation();

	function openRename() {
		editName = mod.Name;
		isRenameModalOpen = true;
	}

	async function handleRename() {
		const finalName = editName.trim() || actualFilename;

		if (!finalName || finalName === mod.Name) {
			isRenameModalOpen = false;
			return;
		}

		await renameMutation.mutateAsync({
			gamePath,
			oldName: mod.Name,
			newName: finalName,
		});

		isRenameModalOpen = false;
	}
</script>

<PopoverMenu>
	{#snippet trigger()}
		<button class="btn btn-sm btn-square">
			<EllipsisVertical size={14} />
		</button>
	{/snippet}
	{#snippet children()}
		<PopoverMenuItem
			onclick={showInExplorer}
			disabled={mod.Kind === "V2" ? !gamePath : !targetPath}
		>
			<FolderOpen size={16} />
			{m.mod_show_in_explorer()}
		</PopoverMenuItem>
		<PopoverMenuItem onclick={openRename}>
			<Pencil size={16} />
			{m.mod_rename()}
		</PopoverMenuItem>
	{/snippet}
</PopoverMenu>

<Modal
	bind:open={isRenameModalOpen}
	title={m.mod_rename_dialog_title()}
	class="max-w-md"
	onsubmit={handleRename}
>
	<div class="space-y-4 pt-2">
		<div class="form-control">
			<label for="mod-name" class="label py-0.5">
				<span class="label-text text-sm">{m.mod_rename_dialog_label()}</span>
			</label>
			<input
				id="mod-name"
				type="text"
				class="input input-bordered w-full"
				placeholder={actualFilename}
				bind:value={editName}
			/>
		</div>
	</div>
	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={() => (isRenameModalOpen = false)}>
			{m.common_cancel()}
		</button>
		<button class="btn btn-accent" type="submit" disabled={renameMutation.isPending}>
			{#if renameMutation.isPending}
				<span class="loading loading-spinner loading-xs"></span>
			{/if}
			{m.mod_rename_dialog_button()}
		</button>
	{/snippet}
</Modal>
