<script lang="ts">
	import Modal from "$lib/components/Modal.svelte";
	import { rpc } from "$lib/studio/rpc";
	import { studio } from "$lib/studio/state.svelte";
	import { toasts } from "$lib/studio/toast.svelte";
	import type { ExportRow } from "../../../electron/src/ipc/contract";

	let { open = $bindable(false), row }: { open?: boolean; row: ExportRow } = $props();
	let name = $state("");

	$effect(() => {
		if (open) name = row.name;
	});

	async function submit(event: Event) {
		event.preventDefault();
		if (!studio.pkg || !name.trim()) return;
		try {
			await rpc("object.rename", { path: studio.pkg.path, index: row.i, name: name.trim() });
			studio.markDirty();
			await studio.reloadTables();
			await studio.select(row.i);
			open = false;
		} catch (error) {
			toasts.error(error, "Rename failed");
		}
	}
</script>

<Modal bind:open title="Rename {row.name}" width="max-w-md">
	<form onsubmit={submit} class="space-y-3">
		<!-- svelte-ignore a11y_autofocus -->
		<input class="input w-full font-mono" bind:value={name} autofocus spellcheck="false" />
		<p class="text-xs opacity-60">
			Only the object's own name changes; references keep pointing at it. Content packages use
			lowercase names.
		</p>
		<div class="flex justify-end gap-2">
			<button class="btn btn-ghost btn-sm" type="button" onclick={() => (open = false)}
				>Cancel</button
			>
			<button class="btn btn-sm btn-primary" type="submit">Rename</button>
		</div>
	</form>
</Modal>
