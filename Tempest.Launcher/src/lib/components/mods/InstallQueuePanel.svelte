<script lang="ts">
	import { Clock, Download, Hourglass, X } from "@lucide/svelte";
	import { slide } from "svelte/transition";
	import {
		installQueue,
		installQueuedNow,
		removeQueuedInstall,
		runningQueued,
	} from "$lib/mods/installQueue.svelte";
	import { m } from "$lib/paraglide/messages";
	import type { QueuedInstall } from "$lib/mods/installQueue.svelte";

	interface Props {
		gamePath: string;
		isRunning: boolean;
	}

	let { gamePath, isRunning }: Props = $props();

	// An entry being installed leaves the panel and shows as an installing row in the list.
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
						title={isRunning ? m.install_queue_install_anyway_hint() : undefined}
					>
						<Download size={14} />
						{isRunning
							? m.install_queue_install_anyway()
							: m.install_queue_install_now()}
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
