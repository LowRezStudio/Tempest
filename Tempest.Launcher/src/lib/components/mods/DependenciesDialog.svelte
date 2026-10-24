<script lang="ts">
	import { Download, PackagePlus, Power } from "@lucide/svelte";
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

	// Turned-off dependencies are switched back on rather than downloaded again.
	const enabling = $derived(items.filter((item) => item.enableName).length);
	const mix = $derived(
		enabling === 0 ? "install" : enabling === items.length ? "enable" : "both",
	);

	function choose(choice: Choice) {
		onconfirm(choice);
		open = false;
	}

	function handleCancel() {
		oncancel();
		open = false;
	}
</script>

<Modal bind:open title={m.deps_dialog_title()} class="max-w-lg" onclose={handleCancel}>
	<div class="space-y-4">
		<div class="flex items-start gap-3">
			<div class="text-primary mt-0.5 shrink-0">
				<PackagePlus size={24} />
			</div>
			<div class="min-w-0">
				<h4 class="text-base font-bold wrap-break-word">
					{m.deps_dialog_heading({ name: modName, count: items.length })}
				</h4>
				<p class="mt-1 text-sm opacity-70">
					{mix === "enable"
						? m.deps_dialog_enable_message({ count: items.length })
						: mix === "both"
							? m.deps_dialog_both_message()
							: m.deps_dialog_message()}
				</p>
			</div>
		</div>
		<!-- Everything wraps rather than truncates: the dialog is the one place these names are read -->
		<ul class="flex flex-col gap-1.5">
			{#each items as item (item.id)}
				<li class="bg-base-200 flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm">
					<div class="min-w-0 flex-1">
						<p class="wrap-break-word">
							<span class="font-semibold">{item.name}</span>
							<span class="ml-1 font-mono text-xs whitespace-nowrap opacity-70"
								>v{item.version}</span
							>
						</p>
						<p class="text-base-content/60 text-xs wrap-break-word">
							{m.deps_dialog_required_by({ names: item.requiredBy.join(", ") })}
						</p>
					</div>
					{#if item.enableName}
						<span
							class="text-base-content/70 bg-base-300 flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-xs whitespace-nowrap"
						>
							<Power size={12} />
							{m.deps_dialog_turned_off()}
						</span>
					{/if}
				</li>
			{/each}
		</ul>
	</div>

	{#snippet actions()}
		<button class="btn btn-ghost" type="button" onclick={() => choose("skip")}>
			{mix === "enable" ? m.deps_dialog_keep_off() : m.deps_dialog_skip()}
		</button>
		<button class="btn btn-primary" type="button" onclick={() => choose("install")}>
			{#if mix === "enable"}
				<Power size={16} />
				{m.deps_dialog_enable()}
			{:else}
				<Download size={16} />
				{mix === "both" ? m.deps_dialog_enable_install() : m.deps_dialog_install()}
			{/if}
		</button>
	{/snippet}
</Modal>
