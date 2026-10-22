<script lang="ts">
	import { Download, PackagePlus } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { DependencyToInstall } from "$lib/mods/dependencies";

	type Choice = "install" | "skip" | "cancel";

	interface Props {
		open: boolean;
		modName: string;
		items: DependencyToInstall[];
		onconfirm: (choice: Choice) => void;
		oncancel: () => void;
	}

	let { open = $bindable(false), modName, items, onconfirm, oncancel }: Props = $props();

	function choose(choice: Choice) {
		onconfirm(choice);
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal bind:open title={m.deps_dialog_title()} class="max-w-md" onclose={handleCancel}>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-primary mt-0.5 shrink-0">
				<PackagePlus size={24} />
			</div>
			<div>
				<h4 class="text-base font-bold">
					{m.deps_dialog_heading({ name: modName, count: items.length })}
				</h4>
				<p class="mt-1 text-sm opacity-70">
					{m.deps_dialog_message()}
				</p>
			</div>
		</div>
		<ul class="flex flex-col gap-1.5">
			{#each items as item (item.id)}
				<li class="bg-base-200 flex items-center gap-2 rounded-lg px-3 py-2 text-sm">
					<span class="font-semibold">{item.name}</span>
					<span class="font-mono text-xs opacity-70">v{item.version}</span>
					<span
						class="ml-auto truncate text-xs opacity-50"
						title={item.requiredBy.join(", ")}
					>
						{m.deps_dialog_required_by({ names: item.requiredBy.join(", ") })}
					</span>
				</li>
			{/each}
		</ul>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={() => choose("skip")}>
			{m.deps_dialog_skip()}
		</button>
		<button class="btn btn-primary" type="button" onclick={() => choose("install")}>
			<Download size={16} />
			{m.deps_dialog_install()}
		</button>
	{/snippet}
</Modal>
