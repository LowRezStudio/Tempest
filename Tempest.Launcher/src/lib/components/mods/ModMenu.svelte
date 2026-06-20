<script lang="ts">
	import { EllipsisVertical, FolderOpen } from "@lucide/svelte";
	import { revealItemInDir } from "@tauri-apps/plugin-opener";
	import PopoverMenu from "$lib/components/ui/PopoverMenu.svelte";
	import PopoverMenuItem from "$lib/components/ui/PopoverMenuItem.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { ModRecord } from "$lib/core/mods";

	interface Props {
		mod: ModRecord;
	}

	let { mod }: Props = $props();

	let targetPath = $derived(mod.InstalledFiles[0] || mod.OriginalPath || "");

	async function showInExplorer() {
		if (!targetPath) return;
		await revealItemInDir(targetPath);
	}
</script>

<PopoverMenu>
	{#snippet trigger()}
		<button class="btn btn-sm btn-square">
			<EllipsisVertical size={14} />
		</button>
	{/snippet}
	{#snippet children()}
		<PopoverMenuItem onclick={showInExplorer} disabled={!targetPath}>
			<FolderOpen size={16} />
			{m.mod_show_in_explorer()}
		</PopoverMenuItem>
	{/snippet}
</PopoverMenu>
