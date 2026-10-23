<script lang="ts">
	import { Download } from "@lucide/svelte";
	import DownloadHero from "$lib/components/downloads/DownloadHero.svelte";
	import DownloadRow from "$lib/components/downloads/DownloadRow.svelte";
	import DeleteInstanceDialog from "$lib/components/library/DeleteInstanceDialog.svelte";
	import NewInstanceOption from "$lib/components/library/NewInstanceOption.svelte";
	import Header from "$lib/components/ui/Header.svelte";
	import { deleteInstance, type DeleteMode } from "$lib/core/instance-delete";
	import { m } from "$lib/paraglide/messages";
	import { formatBytes } from "$lib/rigby/format";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { queueItems, queueRunning } from "$lib/rigby/stores.svelte";
	import { instanceMap, updateInstance } from "$lib/stores/instance.svelte";
	import { persistedState } from "$lib/stores/persisted.svelte";
	import type { QueueItem } from "$lib/rigby/stores.svelte";
	import type { Instance } from "$lib/types/instance";

	const items = $derived(queueItems.value);
	// Queue order is array order: the queue takes the first pending or paused item next.
	const current = $derived(items.find((item) => item.status === "running"));
	const waiting = $derived(
		items.filter((item) => item.status === "pending" || item.status === "paused"),
	);
	const failed = $derived(items.filter((item) => item.status === "error"));
	const finished = $derived(items.filter((item) => item.status === "complete").reverse());
	const written = $derived(
		finished.reduce((total, item) => total + (item.result?.diskWriteBytes ?? 0), 0),
	);
	// The last finished download stays in the hero until it's closed.
	const closedHero = persistedState<string | null>("downloads:closedHero", null);
	const lastFinished = $derived(finished[0]?.id === closedHero.value ? undefined : finished[0]);
	// With the queue stopped, the item it takes next stays in the hero instead of the list.
	const featured = $derived(
		current ??
			(waiting.length > 0 ? (queueRunning.value ? undefined : waiting[0]) : lastFinished),
	);
	const upNext = $derived(waiting.filter((item) => item !== featured));
	const finishedList = $derived(finished.filter((item) => item !== featured));
	const currentPercent = $derived(Math.round(current?.progress?.percent ?? 0));

	const instanceAt = (path: string) =>
		Object.values(instanceMap.value).find((instance) => instance?.path === path);

	let cancelling = $state<{ item: QueueItem; instance: Instance }>();
	let confirmOpen = $state(false);

	function cancel(item: QueueItem) {
		const instance = instanceAt(item.outDir);
		// A new install is no use half downloaded, so cancelling it removes the instance; ask how.
		if (instance && !item.repair && !item.noDownload) {
			cancelling = { item, instance };
			confirmOpen = true;
			return;
		}
		if (item.status === "running") {
			// Stops the child too, and puts the instance back to prepared.
			void restoreQueue.cancel(item.outDir);
			return;
		}
		restoreQueue.remove(item.id);
		if (instance) updateInstance(instance.id, { state: { type: "prepared" } });
	}

	async function confirmCancel(mode: DeleteMode) {
		if (!cancelling) return;
		await deleteInstance(cancelling.instance, mode);
		// A failed download already left the instance "prepared", which deleteInstance doesn't cancel.
		restoreQueue.remove(cancelling.item.id);
		cancelling = undefined;
	}
</script>

{#snippet list(title: string, entries: QueueItem[], clearable = false)}
	<section class="min-w-0">
		<div class="mb-2 flex min-h-6 items-center gap-2 text-sm">
			<h2 class="font-semibold">{title}</h2>
			<span class="text-base-content/60 tabular-nums">{entries.length}</span>
			{#if clearable}
				<button
					class="btn btn-ghost btn-xs text-base-content/70 ml-auto"
					onclick={() => restoreQueue.clearCompleted()}
				>
					{m.common_clear()}
				</button>
			{/if}
		</div>
		<ul class="bg-base-200 border-base-300 rounded-box divide-base-300 divide-y border">
			{#each entries as item (item.id)}
				<DownloadRow {item} instance={instanceAt(item.outDir)} oncancel={cancel} />
			{/each}
		</ul>
	</section>
{/snippet}

<div class="flex h-full flex-col">
	<Header title={m.downloads_title()}>
		{#snippet icon()}
			<Download size={32} class="opacity-60" />
		{/snippet}
		{#snippet subtitle()}
			<span class="tabular-nums">
				{#if items.length === 0}
					{m.downloads_idle()}
				{:else}
					{@const parts = [
						current && {
							text: current.repair
								? m.downloads_summary_verifying({ percent: currentPercent })
								: m.library_badge_downloading({ percent: currentPercent }),
							tone: "text-primary",
						},
						waiting.length > 0 && {
							text: m.downloads_summary_queued({ count: waiting.length }),
						},
						failed.length > 0 && {
							text: m.downloads_summary_failed({ count: failed.length }),
							tone: "text-error",
						},
						finished.length > 0 && {
							text: m.downloads_summary_finished({ count: finished.length }),
						},
					].filter((part) => !!part)}
					{#each parts as part, i (part.text)}
						{#if i > 0}{" · "}{/if}
						<span class={part.tone}>{part.text}</span>
					{/each}
				{/if}
			</span>
		{/snippet}
	</Header>

	<div class="@container min-h-0 flex-1 scrollbar-gutter-stable overflow-y-auto">
		{#if items.length === 0}
			<div class="px-4 py-12">
				<div
					class="bg-base-200 border-base-300 rounded-box mx-auto flex max-w-xl flex-col gap-4 border p-8"
				>
					<div>
						<h2 class="mb-1 text-lg font-semibold">{m.downloads_idle()}</h2>
						<p class="text-base-content/60 text-sm">{m.downloads_empty_hint()}</p>
					</div>
					<NewInstanceOption tab="download" />
				</div>
			</div>
		{:else}
			<div class="mx-auto flex w-full max-w-6xl flex-col gap-6 px-4 py-6">
				{#if featured}
					<DownloadHero
						item={featured}
						instance={instanceAt(featured.outDir)}
						oncancel={cancel}
						onclose={(item) => (closedHero.value = item.id)}
					/>
				{/if}

				<div class="grid items-start gap-6 @3xl:grid-cols-[minmax(0,1fr)_18rem]">
					<div class="flex min-w-0 flex-col gap-6">
						{#if failed.length > 0}
							{@render list(m.downloads_failed_heading(), failed)}
						{/if}
						{#if upNext.length > 0}
							{@render list(m.downloads_up_next(), upNext)}
						{/if}
						{#if finishedList.length > 0}
							{@render list(m.downloads_finished(), finishedList, true)}
						{/if}
					</div>

					<aside class="flex flex-col gap-6 @3xl:sticky @3xl:top-6">
						<!-- Headed like the lists, so it lines up with the first of them -->
						<section>
							<div class="mb-2 flex min-h-6 items-center text-sm">
								<h2 class="font-semibold">{m.downloads_add_heading()}</h2>
							</div>
							<div
								class="bg-base-200 border-base-300 rounded-box overflow-hidden border"
							>
								<NewInstanceOption tab="download" look="row" />
							</div>
						</section>
						<dl
							class="bg-base-200 border-base-300 rounded-box grid grid-cols-2 gap-3 border p-4 text-sm"
						>
							<div>
								<dt class="text-base-content/60 text-xs">
									{m.downloads_stat_queued()}
								</dt>
								<dd class="font-semibold tabular-nums">{waiting.length}</dd>
							</div>
							<div>
								<dt class="text-base-content/60 text-xs">
									{m.downloads_stat_finished()}
								</dt>
								<dd class="font-semibold tabular-nums">{finished.length}</dd>
							</div>
							<div>
								<dt class="text-base-content/60 text-xs">
									{m.downloads_stat_failed()}
								</dt>
								<dd
									class="font-semibold tabular-nums"
									class:text-error={failed.length > 0}
								>
									{failed.length}
								</dd>
							</div>
							<div>
								<dt class="text-base-content/60 text-xs">
									{m.downloads_stat_written()}
								</dt>
								<dd class="font-semibold tabular-nums">{formatBytes(written)}</dd>
							</div>
						</dl>
					</aside>
				</div>
			</div>
		{/if}
	</div>
</div>

<DeleteInstanceDialog
	bind:open={confirmOpen}
	instanceName={cancelling?.instance.label ?? ""}
	onconfirm={confirmCancel}
/>
