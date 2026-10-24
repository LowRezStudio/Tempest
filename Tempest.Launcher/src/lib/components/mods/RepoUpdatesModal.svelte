<script lang="ts">
	import { ArrowRight, Download } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { m } from "$lib/paraglide/messages";
	import type { ModRecord } from "$lib/core/mods";
	import type { RepoUpdate } from "$lib/queries/repo";

	interface Props {
		open?: boolean;
		gamePath: string;
		mods: ModRecord[];
		updates: Map<string, RepoUpdate>;
		isRunning: boolean;
	}

	let { open = $bindable(false), gamePath, mods, updates, isRunning }: Props = $props();

	const pending = $derived(
		mods
			.map((mod) => ({ mod, update: updates.get(mod.Id) }))
			.filter(
				(x): x is { mod: ModRecord; update: RepoUpdate } =>
					!!x.update?.updateAvailable && !!x.update.latest,
			),
	);

	let busy = $state<string | null>(null);
	let done = $state<string[]>([]);
	const remaining = $derived(pending.filter(({ mod }) => !done.includes(mod.Id)));

	$effect(() => {
		if (open && !busy && remaining.length === 0) open = false;
	});

	async function updateOne(mod: ModRecord, update: RepoUpdate) {
		if (busy || !update.latest) return;
		busy = mod.Id;
		try {
			const outcome = await installFromRepo(
				gamePath,
				mod.Id,
				update.latest.version,
				mod.Name,
				{ replace: true },
			);
			if (outcome.status === "installed") done = [...done, mod.Id];
		} finally {
			busy = null;
		}
	}

	async function updateAll() {
		for (const { mod, update } of remaining) {
			if (isRunning) break;
			await updateOne(mod, update);
		}
	}
</script>

<Modal bind:open title={m.repo_updates_title({ count: pending.length })}>
	<p class="text-base-content/70 mb-3 text-sm">{m.repo_updates_description()}</p>
	<ul class="flex flex-col gap-2">
		{#each pending as { mod, update } (mod.Id)}
			<li class="bg-base-200 rounded-lg p-3">
				<div class="flex flex-wrap items-center gap-2">
					<span class="font-semibold">{mod.Name}</span>
					<span class="font-mono text-xs">{mod.Version}</span>
					<ArrowRight size={12} class="opacity-50" />
					<span class="font-mono text-xs font-semibold">{update.latest?.version}</span>
					{#if done.includes(mod.Id)}
						<span class="badge badge-success badge-xs ml-auto"
							>{m.repo_installed()}</span
						>
					{:else}
						<button
							class="btn btn-primary btn-xs ml-auto"
							disabled={isRunning || busy !== null}
							onclick={() => updateOne(mod, update)}
						>
							{#if busy === mod.Id}<span class="loading loading-spinner loading-xs"
								></span>{:else}<Download size={12} />{/if}
							{m.repo_update()}
						</button>
					{/if}
				</div>
				{#if update.latest?.changelog}
					<p class="text-base-content/70 mt-1 text-xs whitespace-pre-line">
						{update.latest.changelog}
					</p>
				{/if}
			</li>
		{/each}
	</ul>
	{#snippet actions()}
		<button class="btn btn-ghost btn-sm" onclick={() => (open = false)}
			>{m.repo_updates_later()}</button
		>
		<button
			class="btn btn-primary btn-sm"
			disabled={isRunning || busy !== null || remaining.length === 0}
			onclick={updateAll}
		>
			<Download size={14} />
			{m.repo_update_all()}
		</button>
	{/snippet}
</Modal>
