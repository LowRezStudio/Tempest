<script lang="ts">
	import { untrack } from "svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { cleanupInstances } from "$lib/core/uninstall.svelte";
	import { queueRunning } from "$lib/rigby/stores.svelte";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import type { Instance } from "$lib/types/instance";
	let { open = $bindable(false) }: { open?: boolean } = $props();
	let instances = $state<Instance[]>([]);
	let busy = $state(false);
	let cleanupError = $state("");
	let complete = $state(false);
	let deleteFiles = $state<Record<string, boolean>>({});
	const blocked = $derived(
		processesList.value.length > 0 ||
			queueRunning.value ||
			Object.values(instanceMap.value).some(
				(i) => i?.state.type === "setup" || i?.state.type === "downloading",
			),
	);
	$effect(() => {
		if (open) {
			const snapshot = untrack(() =>
				Object.values(instanceMap.value).filter((i): i is Instance => !!i),
			);
			instances = snapshot;
			deleteFiles = Object.fromEntries(
				snapshot.map((i) => [i.id, !!i.managedPath && i.managedPath === i.path]),
			);
			cleanupError = "";
			complete = false;
		}
	});
	async function clean() {
		if (blocked || busy) return;
		busy = true;
		cleanupError = "";
		try {
			await cleanupInstances(
				instances
					.filter((i) => instanceMap.value[i.id])
					.map((i) => ({
						...i,
						managedPath: deleteFiles[i.id] ? i.path : undefined,
						origin: deleteFiles[i.id] ? "download" : "import",
					})),
			);
			complete = true;
		} catch (error) {
			cleanupError = String(error);
		} finally {
			busy = false;
		}
	}
</script>

<Modal bind:open title="Prepare to uninstall Tempest" dismissible={!busy}>
	{#if complete}
		<p>
			All instance records and their Tempest mods have been removed. You can now uninstall the
			launcher through your operating system.
		</p>
	{:else}
		<p class="mb-4">
			Remove all instance records and their mods. Imported game folders are kept. Selected
			Tempest game folders will be permanently deleted.
		</p>
		<div class="space-y-3">
			{#each instances as instance (instance.id)}
				<label class="flex items-start gap-3">
					<input
						type="checkbox"
						class="checkbox"
						bind:checked={deleteFiles[instance.id]}
						disabled={busy || instance.origin === "import"}
					/>
					<span
						><span class="block font-semibold">{instance.label}</span><span
							class="block text-xs break-all opacity-70">{instance.path}</span
						><span class="text-sm"
							>{instance.origin === "import"
								? "Keep imported game files; remove mods"
								: "Delete game folder (select only for Tempest downloads)"}</span
						></span
					>
				</label>
			{/each}
		</div>
		{#if blocked}<p role="alert" class="text-warning mt-4">
				Close running games and wait for setup to finish. Pause downloads before continuing.
			</p>{/if}
		{#if cleanupError}<p role="alert" class="text-error mt-4">{cleanupError}</p>{/if}
	{/if}
	{#snippet actions()}
		<button class="btn btn-ghost" type="button" disabled={busy} onclick={() => (open = false)}
			>{complete ? "Done" : "Cancel"}</button
		>
		{#if !complete}<button
				class="btn btn-error"
				type="button"
				disabled={busy || blocked}
				onclick={clean}
				>{#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}Remove
				instances and mods</button
			>{/if}
	{/snippet}
</Modal>
