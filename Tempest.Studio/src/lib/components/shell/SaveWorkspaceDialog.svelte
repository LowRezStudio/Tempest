<!-- Save the session as a .twrk: linked (small, needs this game folder) or
	portable (whole files, opens anywhere). -->
<script lang="ts">
	import { Link, Package } from "@lucide/svelte";
	import Modal from "$lib/components/Modal.svelte";
	import { assembly } from "$lib/studio/assembly.svelte";
	import { session } from "$lib/studio/session.svelte";
	import { studio } from "$lib/studio/state.svelte";
	import type { TwrkMode } from "../../../../electron/src/ipc/contract";

	let { open = $bindable(false) }: { open?: boolean } = $props();
	let mode = $state<TwrkMode>(session.defaultMode);

	$effect(() => {
		if (open) mode = session.file?.mode ?? session.defaultMode;
	});

	const modes = [
		{
			id: "linked" as const,
			icon: Link,
			title: "Linked",
			text: "Only the changes, as deltas against this game folder. Small (kilobytes to a few MB). Opens on a machine with the same game files; script packages and changes a delta cannot hold exactly are stored whole.",
		},
		{
			id: "portable" as const,
			icon: Package,
			title: "Portable",
			text: "Every changed file whole, the packages opened from outside the game folder, and the recorded bases (so Export mod still makes deltas). Bigger, but opens on any machine or game copy.",
		},
	];

	async function save() {
		open = false;
		await session.save(true, mode);
	}
</script>

<Modal bind:open title="Save workspace" width="max-w-xl">
	<div class="space-y-3 text-sm">
		<p class="text-xs opacity-70">
			A workspace file keeps every unsaved change ({studio.dirty.length} package{studio.dirty
				.length === 1
				? ""
				: "s"}, {assembly.dirtyDocs.length} .dat file{assembly.dirtyDocs.length === 1
				? ""
				: "s"}) and where you were (open package, selection, tables, tabs, history), whether
			or not the files themselves were saved. Open it later with Open workspace file or by
			double-clicking it.
		</p>
		<div class="grid gap-2 sm:grid-cols-2">
			{#each modes as m (m.id)}
				<label
					class="border-base-300 hover:border-primary/60 flex cursor-pointer flex-col gap-1 rounded-lg border p-3 {mode ===
					m.id
						? 'border-primary bg-primary/10'
						: ''}"
				>
					<span class="flex items-center gap-2 font-semibold">
						<input type="radio" class="radio radio-xs" bind:group={mode} value={m.id} />
						<m.icon size={14} />
						{m.title}
					</span>
					<span class="text-xs opacity-70">{m.text}</span>
				</label>
			{/each}
		</div>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" onclick={() => (open = false)}>Cancel</button>
			<button class="btn btn-primary btn-sm" onclick={save}>Choose file and save…</button>
		</div>
	</div>
</Modal>
