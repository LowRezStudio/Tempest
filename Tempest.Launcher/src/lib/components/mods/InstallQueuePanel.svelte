<script lang="ts">
	import { Clock, Download, Hourglass, RefreshCw, X } from "@lucide/svelte";
	import { slide } from "svelte/transition";
	import {
		installQueue,
		installQueuedNow,
		removeQueuedInstall,
		runningQueued,
	} from "$lib/mods/installQueue.svelte";
	import { pendingStatus } from "$lib/mods/pendingInstalls.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { QueuedInstall } from "$lib/mods/installQueue.svelte";
	import type { PendingInstall } from "$lib/mods/pendingInstalls.svelte";

	interface Props {
		gamePath: string;
		isRunning: boolean;
		/** What is downloading or installing right now */
		active: PendingInstall[];
	}

	let { gamePath, isRunning, active }: Props = $props();

	// An entry being installed moves from the waiting list to `active`.
	const waiting = $derived(
		installQueue.value.filter(
			(q) => q.gamePath === gamePath && !runningQueued.ids.includes(q.id),
		),
	);

	const detailOf = (item: QueuedInstall) =>
		[
			item.author && `by ${item.author}`,
			item.lockedFiles.length > 0 &&
				m.install_queue_in_use({ files: item.lockedFiles.join(", ") }),
		]
			.filter(Boolean)
			.join(" · ");
</script>

{#if active.length > 0}
	<ul
		data-panel
		class="bg-base-200 border-base-300 rounded-box divide-base-300 mb-3 divide-y overflow-hidden border"
		transition:slide={{ duration: 150 }}
	>
		{#each active as install (install.id)}
			<li
				class="pending-row flex items-center gap-3 px-4 py-2.5"
				class:installing={install.progress === null}
				aria-busy="true"
				style:--progress="{install.progress === null ? 100 : install.progress * 100}%"
			>
				<div
					class="bg-base-300 flex size-10 shrink-0 items-center justify-center rounded-lg"
				>
					<span class="loading loading-spinner loading-sm opacity-60"></span>
				</div>
				<div class="min-w-0 flex-1">
					<p class="flex min-w-0 items-baseline gap-2">
						<span class="truncate text-sm font-bold">{install.name}</span>
						{#if install.version}
							<span class="shrink-0 font-mono text-xs opacity-60">
								{install.version}
							</span>
						{/if}
					</p>
					<p class="truncate text-xs tabular-nums opacity-70">
						{install.author
							? `by ${install.author} · ${pendingStatus(install)}`
							: pendingStatus(install)}
					</p>
				</div>
			</li>
		{/each}
	</ul>
{/if}

<!-- Set apart from the mod list on purpose: nothing here is installed yet. -->
{#if waiting.length > 0}
	<section
		class="border-warning/40 bg-warning/5 rounded-box mb-3 border border-dashed"
		aria-labelledby="install-queue-title"
		transition:slide={{ duration: 150 }}
	>
		<header class="flex items-center gap-2 px-4 pt-3">
			<Hourglass size={15} class="text-warning shrink-0" />
			<h3 id="install-queue-title" class="text-sm font-semibold">
				{m.install_queue_title()}
			</h3>
			<span class="badge badge-warning badge-soft badge-sm tabular-nums">
				{waiting.length}
			</span>
		</header>
		<p class="text-base-content/60 px-4 pt-0.5 text-xs">
			{isRunning ? m.install_queue_hint_running() : m.install_queue_hint_idle()}
		</p>
		<ul class="divide-warning/20 mt-2 divide-y divide-dashed">
			{#each waiting as item (item.id)}
				{@const detail = detailOf(item)}
				<li
					class="flex items-center gap-3 px-4 py-2.5"
					transition:slide={{ duration: 150 }}
				>
					<div
						class="border-warning/40 text-warning/80 flex size-10 shrink-0 items-center justify-center rounded-lg border border-dashed"
					>
						<Clock size={18} />
					</div>
					<div class="min-w-0 flex-1">
						<p class="flex min-w-0 items-baseline gap-2">
							<span class="truncate text-sm font-bold">{item.name}</span>
							{#if item.version}
								<span class="shrink-0 font-mono text-xs opacity-60">
									{item.version}
								</span>
							{/if}
						</p>
						{#if detail}
							<p
								class="truncate text-xs opacity-60"
								title={item.lockedFiles.join("\n") || undefined}
							>
								{detail}
							</p>
						{/if}
					</div>
					<span class="badge badge-warning badge-outline badge-sm shrink-0">
						{m.install_queue_queued()}
					</span>
					<!-- Still refused (and kept queued) when the game holds a file it would replace. -->
					<button
						class="btn btn-ghost btn-sm shrink-0"
						onclick={() => void installQueuedNow(item.id)}
						title={isRunning
							? item.hotReloadable
								? m.repo_update_hot_reload_hint()
								: m.install_queue_install_anyway_hint()
							: undefined}
					>
						{#if isRunning && item.hotReloadable}
							<RefreshCw size={14} />
							{m.repo_update_hot_reload()}
						{:else}
							<Download size={14} />
							{isRunning
								? m.install_queue_install_anyway()
								: m.install_queue_install_now()}
						{/if}
					</button>
					<button
						class="btn btn-ghost btn-sm btn-square hover:text-error shrink-0"
						onclick={() => removeQueuedInstall(item.id)}
						title={m.install_queue_remove()}
						aria-label="{m.install_queue_remove()}: {item.name}"
					>
						<X size={14} />
					</button>
				</li>
			{/each}
		</ul>
	</section>
{/if}
