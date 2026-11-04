<script lang="ts">
	import { ArrowRight, Download, RefreshCw } from "@lucide/svelte";
	import Modal from "$lib/components/ui/Modal.svelte";
	import { installQueuedNow } from "$lib/mods/installQueue.svelte";
	import { installFromRepo } from "$lib/mods/repoInstall";
	import { checkModUpdates, queuedUpdateOf } from "$lib/mods/updates.svelte";
	import { m } from "$lib/paraglide/messages";
	import { autoInstallModUpdates } from "$lib/stores/settings.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { RepoUpdate } from "$lib/queries/repo";

	interface Props {
		open?: boolean;
		gamePath: string;
		instanceLabel: string;
		mods: ModRecord[];
		updates: Map<string, RepoUpdate>;
		isRunning: boolean;
	}

	let {
		open = $bindable(false),
		gamePath,
		instanceLabel,
		mods,
		updates,
		isRunning,
	}: Props = $props();

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

	const queuedFor = (mod: ModRecord, update: RepoUpdate) =>
		update.latest ? queuedUpdateOf(gamePath, mod.Id, update.latest.version) : undefined;
	// What a click can still do: a queued update waits for the game unless it can reload live.
	const actionable = $derived(
		remaining.filter(({ mod, update }) => {
			const queued = queuedFor(mod, update);
			return !queued || (queued.hotReloadable && isRunning);
		}),
	);

	$effect(() => {
		if (open && !busy && remaining.length === 0) open = false;
	});

	$effect(() => {
		if (!open) done = [];
	});

	async function updateOne(mod: ModRecord, update: RepoUpdate) {
		if (busy || !update.latest) return;
		busy = mod.Id;
		try {
			const queued = queuedFor(mod, update);
			const status = queued
				? await installQueuedNow(queued.id)
				: (
						await installFromRepo(gamePath, mod.Id, update.latest.version, mod.Name, {
							replace: true,
						})
					).status;
			if (status === "installed") done = [...done, mod.Id];
		} finally {
			busy = null;
		}
	}

	async function updateAll() {
		for (const { mod, update } of actionable) {
			await updateOne(mod, update);
		}
	}

	function turnedOnAutoInstall(checked: boolean) {
		if (!checked) return;
		open = false;
		void checkModUpdates();
	}
</script>

<Modal bind:open title={m.repo_updates_title({ count: pending.length })}>
	<p class="text-base-content/70 mb-3 text-sm">
		<span class="font-semibold">{instanceLabel}.</span>
		{m.repo_updates_description()}
	</p>
	<ul class="flex flex-col gap-2">
		{#each pending as { mod, update } (mod.Id)}
			{@const queued = queuedFor(mod, update)}
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
					{:else if queued?.hotReloadable && isRunning}
						<button
							class="btn btn-primary btn-xs ml-auto"
							disabled={busy !== null}
							title={m.repo_update_hot_reload_hint()}
							onclick={() => updateOne(mod, update)}
						>
							{#if busy === mod.Id}<span class="loading loading-spinner loading-xs"
								></span>{:else}<RefreshCw size={12} />{/if}
							{m.repo_update_hot_reload()}
						</button>
					{:else if queued}
						<span class="badge badge-warning badge-outline badge-xs ml-auto"
							>{m.install_queue_queued()}</span
						>
					{:else}
						<button
							class="btn btn-primary btn-xs ml-auto"
							disabled={busy !== null}
							title={isRunning ? m.repo_install_queued_hint() : undefined}
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
	<label class="mt-4 flex cursor-pointer items-start gap-2">
		<input
			type="checkbox"
			class="checkbox checkbox-sm checkbox-primary mt-0.5"
			bind:checked={autoInstallModUpdates.value}
			onchange={(event) => turnedOnAutoInstall(event.currentTarget.checked)}
		/>
		<span>
			<span class="block text-sm font-medium">{m.settings_auto_install_mod_updates()}</span>
			<span class="text-base-content/60 block text-xs">
				{m.settings_auto_install_mod_updates_hint()}
			</span>
		</span>
	</label>
	{#snippet actions()}
		<button class="btn btn-ghost btn-sm" onclick={() => (open = false)}
			>{m.repo_updates_later()}</button
		>
		<button
			class="btn btn-primary btn-sm"
			disabled={busy !== null || actionable.length === 0}
			onclick={updateAll}
		>
			<Download size={14} />
			{m.repo_update_all()}
		</button>
	{/snippet}
</Modal>
