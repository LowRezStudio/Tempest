<script lang="ts">
	import { Box, FolderOpen, Pencil, RefreshCw, RotateCw, Trash2 } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { reloadModDll } from "$lib/core/mods";
	import type { ModRecord } from "$lib/core/mods";
	import { exists, stat } from "@tauri-apps/plugin-fs";
	import { path } from "@tauri-apps/api";
	import { openPath } from "@tauri-apps/plugin-opener";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { createRenameModMutation } from "$lib/queries/mods";

	interface Props {
		mods: ModRecord[];
		gamePath: string;
		isRunning: boolean;
		isLoading: boolean;
		isRemoving?: boolean;
		onRefresh: () => void;
		onRemoveMod: (name: string) => void;
		onOpenDetails: (mod: ModRecord) => void;
	}

	let {
		mods,
		gamePath,
		isRunning,
		isLoading,
		isRemoving = false,
		onRefresh,
		onRemoveMod,
		onOpenDetails,
	}: Props = $props();

	let reloadingModId = $state<string | null>(null);

	async function showInExplorer(mod: ModRecord) {
		if (mod.Kind === "V2") {
			if (!gamePath) return;
			const modFolder = await path.join(gamePath, ".tempest", "v2", "mods", mod.Id);
			await openPath(modFolder);
			return;
		}
		const targetPath = mod.InstalledFiles[0] || mod.OriginalPath || "";
		if (!targetPath) return;
		let resolvedFolder = targetPath;
		try {
			const info = await stat(targetPath);
			if (info.isFile) resolvedFolder = await path.dirname(targetPath);
		} catch {
			if (targetPath.includes(".") && !targetPath.endsWith("/") && !targetPath.endsWith("\\"))
				resolvedFolder = await path.dirname(targetPath);
		}
		await openPath(resolvedFolder);
	}

	let editingMod = $state<{ name: string } | null>(null);
	let isRenameModalOpen = $state(false);
	const renameMutation = createRenameModMutation();
	let editName = $state("");

	function openRename(mod: ModRecord) {
		editingMod = { name: mod.Name };
		editName = mod.Name;
		isRenameModalOpen = true;
	}

	function closeRename() { isRenameModalOpen = false; editingMod = null; }

	$effect(() => { if (!isRenameModalOpen && editingMod) editingMod = null; });

	async function handleRename() {
		if (!editingMod) return;
		const finalName = editName.trim();
		if (!finalName || finalName === editingMod.name) { closeRename(); return; }
		await renameMutation.mutateAsync({ gamePath, oldName: editingMod.name, newName: finalName });
		closeRename();
	}

	let hasDllMap = $state<Map<string, boolean>>(new Map());

	$effect(() => {
		if (!gamePath) return;
		const next = new Map<string, boolean>();
		for (const mod of mods) {
			if (mod.Kind?.toLowerCase() !== "v2") continue;
			const id = mod.Id;
			exists(`${gamePath}/.tempest/v2/mods/${id}/dlls`).then(r => { next.set(id, r); hasDllMap = new Map(next); }).catch(() => { next.set(id, false); hasDllMap = new Map(next); });
		}
	});

	const handleReload = async (mod: ModRecord) => {
		reloadingModId = mod.Id;
		try { await reloadModDll(gamePath, mod.Id); } finally { reloadingModId = null; }
	};
</script>

<table class="table">
	<thead>
		<tr>
			<th>
				<button class="flex items-center gap-1 font-semibold text-sm">
					<span>{m.common_name()}</span>
				</button>
			</th>
			<th class="w-48">{m.common_version()}</th>
			<th class="w-auto text-right">
				<button class="btn btn-ghost btn-sm" onclick={onRefresh} disabled={isLoading}>
					{#if isLoading}
						<span class="loading loading-spinner loading-xs"></span>
					{:else}
						<RefreshCw size={14} />
					{/if}
					{m.common_refresh()}
				</button>
			</th>
		</tr>
	</thead>
	<tbody>
		{#each mods as mod (mod.Id)}
			<tr class="hover">
				<td>
					<div class="flex items-center gap-3">
						<button
							class="w-10 h-10 rounded-lg bg-base-200 hover:bg-base-300 flex items-center justify-center shrink-0 transition-all text-primary hover:scale-105 active:scale-95 cursor-pointer"
							onclick={() => onOpenDetails(mod)}
							title={m.mod_updated_files()}
						>
							<Box size={20} class="opacity-75" />
						</button>
						<div class="flex-1 min-w-0">
							<div class="flex items-center gap-2">
								<button
									class="font-bold text-sm truncate text-left hover:text-primary transition-colors cursor-pointer"
									onclick={() => onOpenDetails(mod)}
									title={m.mod_updated_files()}
								>
									{mod.Name}
								</button>
							</div>
							<p class="text-xs opacity-70">by {mod.Author}</p>
						</div>
					</div>
				</td>
				<td>
					<p class="font-semibold text-sm">{mod.Version || "Unknown"}</p>
				</td>
				<td>
					<div class="flex items-center justify-end gap-1">
						{#if hasDllMap.get(mod.Id)}
							<button
								class="btn btn-sm btn-square btn-ghost {isRunning ? 'text-primary' : 'text-base-content/60'}"
								disabled={!isRunning || reloadingModId === mod.Id}
								onclick={() => handleReload(mod)}
								title="Reload DLLs"
							>
								{#if reloadingModId === mod.Id}
									<span class="loading loading-spinner loading-xs"></span>
								{:else}
									<RotateCw size={14} />
								{/if}
							</button>
						{/if}
						<button
							class="btn btn-sm btn-square btn-ghost hover:text-error"
							disabled={isRemoving}
							onclick={() => onRemoveMod(mod.Name)}
							title="Delete mod"
						>
							<Trash2 size={14} />
						</button>
						<button
							class="btn btn-sm btn-square btn-ghost"
							onclick={() => showInExplorer(mod)}
							title={m.mod_show_in_explorer()}
						>
							<FolderOpen size={14} />
						</button>
						<button
							class="btn btn-sm btn-square btn-ghost"
							onclick={() => openRename(mod)}
							title={m.mod_rename()}
						>
							<Pencil size={14} />
						</button>
					</div>
				</td>
			</tr>
		{/each}
	</tbody>
</table>

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
			<input id="mod-name" type="text" class="input input-bordered w-full" bind:value={editName} />
		</div>
	</div>
	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={closeRename}>{m.common_cancel()}</button>
		<button class="btn btn-accent" type="submit" disabled={renameMutation.isPending}>
			{#if renameMutation.isPending}<span class="loading loading-spinner loading-xs"></span>{/if}
			{m.mod_rename_dialog_button()}
		</button>
	{/snippet}
</Modal>
