<script lang="ts">
	import { CloudDownload, FolderOpen } from "@lucide/svelte";
	import { m } from "$lib/paraglide/messages";
	import { openInstanceWizard } from "$lib/stores/ui.svelte";

	interface Props {
		/** The instance wizard's tab it opens */
		tab: "download" | "folder";
	}

	let { tab }: Props = $props();

	const options = {
		download: {
			Icon: CloudDownload,
			title: m.library_download_patch,
			hint: m.library_download_patch_hint,
		},
		folder: {
			Icon: FolderOpen,
			title: m.library_import_folder,
			hint: m.library_import_folder_hint,
		},
	};
	const { Icon, title, hint } = $derived(options[tab]);
</script>

<!-- One way to add an instance, like the mod website's kind tiles -->
<button
	type="button"
	class="bg-base-100/50 border-base-300 hover:border-primary/60 rounded-field flex cursor-pointer items-start gap-3 border p-3 text-left text-sm transition-colors"
	onclick={() => openInstanceWizard(tab)}
>
	<Icon size={16} class="text-primary mt-0.5 shrink-0" />
	<span class="min-w-0">
		<span class="block font-semibold">{title()}</span>
		<span class="text-base-content/60 block text-xs">{hint()}</span>
	</span>
</button>
