<script lang="ts">
	import { ArrowRight, Download } from "@lucide/svelte";
	import { untrack } from "svelte";
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

	type Item = { mod: ModRecord; update: RepoUpdate };

	const AUTO_CLOSE_DELAY_MS = 800;
	let { open = $bindable(false), gamePath, mods, updates, isRunning }: Props = $props();
	const pending = $derived(
		mods
			.map((mod) => ({ mod, update: updates.get(mod.Id) }))
			.filter((x): x is Item => !!x.update?.updateAvailable && !!x.update.latest),
	);
	let snapshot = $state.raw<Item[] | null>(null);
	let done = $state<string[]>([]);
	let errors = $state<Record<string, string>>({});
	let current = $state<string | null>(null);
	let working = $state(false);
	let runId = 0;
	const items = $derived(snapshot ?? pending);
	const remaining = $derived(items.filter(({ mod }) => !done.includes(mod.Id)));
	const canInteract = $derived(!!gamePath && !isRunning && !working);
	const nothingLeft = $derived(open && !working && remaining.length === 0);

	function errorMessage(e: unknown): string {
		if (e instanceof Error && e.message) return e.message;
		if (typeof e === "string" && e) return e;
		return "Unknown error";
	}

	function resetState() {
		runId++;
		snapshot = null;
		done = [];
		errors = {};
		current = null;
	}

	$effect(() => {
		if (!open) untrack(resetState);
	});

	$effect(() => {
		if (!nothingLeft) return;
		const timer = setTimeout(
			() => {
				open = false;
			},
			done.length > 0 ? AUTO_CLOSE_DELAY_MS : 0,
		);
		return () => clearTimeout(timer);
	});

	async function installOne({ mod, update }: Item, token: number): Promise<void> {
		const latest = update.latest;
		if (!latest) return;

		current = mod.Id;
		delete errors[mod.Id];

		try {
			const outcome = await installFromRepo(gamePath, mod.Id, latest.version, mod.Name, {
				replace: true,
			});
			if (token !== runId) return;

			if (outcome.status === "installed") {
				done = [...done, mod.Id];
			} else {
				errors[mod.Id] = String(outcome.status);
			}
		} catch (e) {
			console.error(`Failed to update mod "${mod.Id}"`, e);
			if (token === runId) errors[mod.Id] = errorMessage(e);
		} finally {
			if (token === runId) current = null;
		}
	}

	async function updateOne(item: Item) {
		if (!canInteract) return;
		working = true;
		const token = runId;
		snapshot ??= pending;
		try {
			await installOne(item, token);
		} finally {
			working = false;
		}
	}

	async function updateAll() {
		if (!canInteract) return;
		working = true;
		const token = runId;
		const queue = (snapshot ??= pending);
		try {
			for (const item of queue) {
				// Stop if the modal was closed/reset or the game was launched mid-run.
				if (token !== runId || !open || isRunning) break;
				if (done.includes(item.mod.Id)) continue;
				await installOne(item, token);
			}
		} finally {
			working = false;
		}
	}
</script>

<Modal bind:open title={m.repo_updates_title({ count: items.length })}>
	<p class="text-base-content/70 mb-3 text-sm">{m.repo_updates_description()}</p>
	<ul class="flex flex-col gap-2">
		{#each items as item (item.mod.Id)}
			{@const { mod, update } = item}
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
							disabled={!canInteract}
							onclick={() => updateOne(item)}
						>
							{#if current === mod.Id}<span class="loading loading-spinner loading-xs"
								></span>{:else}<Download size={12} />{/if}
							{m.repo_update()}
						</button>
					{/if}
				</div>
				{#if errors[mod.Id]}
					<p class="text-error mt-1 text-xs" role="alert">
						{m.repo_update_failed()}: {errors[mod.Id]}
					</p>
				{/if}
				{#if update.latest?.changelog}
					<p class="text-base-content/70 mt-1 text-xs whitespace-pre-line">
						{update.latest.changelog}
					</p>
				{/if}
			</li>
		{/each}
	</ul>
	{#snippet actions()}
		<button class="btn btn-ghost btn-sm" disabled={working} onclick={() => (open = false)}
			>{m.repo_updates_later()}</button
		>
		<button
			class="btn btn-primary btn-sm"
			disabled={!canInteract || remaining.length === 0}
			onclick={updateAll}
		>
			<Download size={14} />
			{m.repo_update_all()}
		</button>
	{/snippet}
</Modal>
