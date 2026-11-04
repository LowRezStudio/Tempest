<script lang="ts">
	import RepoUpdatesModal from "$lib/components/mods/RepoUpdatesModal.svelte";
	import { isGameRunning } from "$lib/mods/installQueue.svelte";
	import { closeModUpdates, modUpdatePrompt } from "$lib/mods/updates.svelte";
	import { createModsQuery } from "$lib/queries/mods";
	import { createRepoUpdatesQuery, updatableMods } from "$lib/queries/repo";
	import { instanceMap } from "$lib/stores/instance.svelte";
	import { updaterStore } from "$lib/stores/updater.svelte";

	const gamePath = $derived(modUpdatePrompt.gamePath);
	const instance = $derived(
		gamePath ? Object.values(instanceMap.value).find((i) => i?.path === gamePath) : undefined,
	);
	const modsQuery = createModsQuery(() => gamePath ?? "");
	const mods = $derived(modsQuery.data ?? []);
	const updatesQuery = createRepoUpdatesQuery(() => ({
		patch: instance?.version ?? "",
		mods: updatableMods(mods),
	}));
	const updates = $derived(new Map((updatesQuery.data ?? []).map((u) => [u.id, u])));
</script>

<!-- Waits for the answer: with nothing to list, the popup closes itself. -->
{#if gamePath && instance && updatesQuery.data}
	{#key gamePath}
		<RepoUpdatesModal
			bind:open={
				() => !updaterStore.isOpen,
				(open) => {
					if (!open) closeModUpdates();
				}
			}
			{gamePath}
			instanceLabel={instance.label}
			{mods}
			{updates}
			isRunning={isGameRunning(gamePath)}
		/>
	{/key}
{/if}
