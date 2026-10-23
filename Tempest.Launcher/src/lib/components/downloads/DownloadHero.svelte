<script lang="ts">
	import { CloudDownload, Files, Gauge, Hourglass, Pause, ShieldCheck } from "@lucide/svelte";
	import { Progress } from "bits-ui";
	import { m } from "$lib/paraglide/messages";
	import { folderName, formatBytes, formatDuration, formatSpeed } from "$lib/rigby/format";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { speedSamples } from "$lib/rigby/stores.svelte";
	import { getInstanceColor } from "$lib/utils/color";
	import { patchBannerOf, patchSummary } from "$lib/utils/versions";
	import SpeedGraph from "./SpeedGraph.svelte";
	import type { QueueItem } from "$lib/rigby/stores.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		/** The running download */
		item: QueueItem;
		/** The instance it downloads into, unless it was removed from the library */
		instance?: Instance;
	}

	let { item, instance }: Props = $props();

	const progress = $derived(item.progress);
	const percent = $derived(progress?.percent ?? 0);
	const peak = $derived(Math.max(0, ...speedSamples.value.map((s) => s.bytesPerSecond)));
	const color = $derived(getInstanceColor(instance));

	const banner = $derived(patchBannerOf(instance?.version));
	let failedBanner = $state<string>();

	let pausing = $state(false);
	async function pause() {
		pausing = true;
		try {
			await restoreQueue.pause();
		} finally {
			pausing = false;
		}
	}

	const count = (n: number) => n.toLocaleString();
	// The website's secondary hero button, solid enough to read over the art
	const secondary = "btn btn-sm border-base-content/10 bg-base-100/60 hover:bg-base-100";
</script>

<!-- The download in progress, after the home hero: the patch's banner behind its name, then the
     numbers and the speed of the last two minutes -->
<section
	class="rounded-box border-base-300 bg-base-200 @container relative isolate overflow-hidden border"
	aria-busy="true"
>
	{#if banner && failedBanner !== banner}
		<div
			class="pointer-events-none absolute top-0 right-0 -z-10 h-44 w-3/5 select-none @2xl:w-2/3"
		>
			<img
				src={banner}
				alt=""
				draggable="false"
				class="size-full object-cover"
				onerror={() => (failedBanner = banner)}
			/>
			<!-- Keeps the text readable over any banner, in any theme -->
			<div
				class="from-base-200 via-base-200/70 absolute inset-0 bg-linear-to-r via-40% to-transparent"
			></div>
			<div class="from-base-200 absolute inset-0 bg-linear-to-t to-transparent to-70%"></div>
		</div>
	{:else}
		<div
			class="pointer-events-none absolute inset-0 -z-10"
			style="background: radial-gradient(ellipse at top right, color-mix(in oklab, {color} 22%, transparent), transparent 60%);"
		></div>
	{/if}

	<div class="flex items-start gap-3 px-5 pt-5">
		<div class="min-w-0 flex-1">
			<p class="text-primary flex items-center gap-1.5 text-sm font-medium">
				{#if item.repair}
					<ShieldCheck size={14} />
					{m.downloads_now_verifying()}
				{:else}
					<CloudDownload size={14} />
					{m.downloads_now_downloading()}
				{/if}
			</p>
			<h2 class="mt-1 truncate text-2xl font-bold tracking-tight">
				{instance?.label ?? folderName(item.outDir)}
			</h2>
			<p class="text-base-content/70 truncate text-sm" title={item.outDir}>
				{instance ? patchSummary(instance) : item.outDir}
			</p>
		</div>
		<button class={secondary} disabled={pausing} onclick={pause}>
			{#if pausing}
				<span class="loading loading-spinner loading-xs"></span>
			{:else}
				<Pause size={14} />
			{/if}
			{m.common_pause()}
		</button>
	</div>

	<div class="px-5 pt-8">
		<div class="flex items-end justify-between gap-3">
			<p class="text-4xl leading-none font-bold tracking-tight tabular-nums">
				{percent.toFixed(percent > 0 && percent < 100 ? 1 : 0)}<span
					class="text-base-content/50 ml-0.5 text-xl font-semibold">%</span
				>
			</p>
			<p class="text-base-content/70 text-sm tabular-nums">
				{progress
					? m.downloads_of({
							done: formatBytes(progress.completedBytes),
							total: formatBytes(progress.totalBytes),
						})
					: m.downloads_starting()}
			</p>
		</div>
		<Progress.Root
			value={percent}
			max={100}
			class="bg-base-300 mt-3 h-2 w-full rounded-full"
			aria-label={item.repair ? m.downloads_now_verifying() : m.downloads_now_downloading()}
		>
			<div
				class="bg-primary loading-shimmer h-full rounded-full transition-[width] duration-500 ease-out"
				style="width: {percent}%; box-shadow: 0 0 12px color-mix(in oklab, var(--color-primary) 45%, transparent);"
			></div>
		</Progress.Root>

		<dl class="mt-5 grid grid-cols-3 gap-3 text-sm">
			<div class="min-w-0">
				<dt class="text-base-content/60 flex items-center gap-1 text-xs">
					<Gauge size={12} />
					{m.downloads_speed()}
				</dt>
				<dd class="font-semibold tabular-nums">
					{progress ? formatSpeed(progress.bytesPerSecond) : "–"}
				</dd>
				{#if peak > 0}
					<dd class="text-base-content/60 text-xs tabular-nums">
						{m.downloads_peak({ speed: formatSpeed(peak) })}
					</dd>
				{/if}
			</div>
			<div class="min-w-0">
				<dt class="text-base-content/60 flex items-center gap-1 text-xs">
					<Hourglass size={12} />
					{m.downloads_time_left()}
				</dt>
				<dd class="font-semibold tabular-nums">
					{progress && progress.etaSeconds > 0
						? formatDuration(progress.etaSeconds)
						: m.common_calculating()}
				</dd>
			</div>
			<div class="min-w-0">
				<dt class="text-base-content/60 flex items-center gap-1 text-xs">
					<Files size={12} />
					{m.downloads_files_heading()}
				</dt>
				<dd class="font-semibold tabular-nums">
					{progress
						? m.downloads_of({
								done: count(progress.completedFiles),
								total: count(progress.totalFiles),
							})
						: "–"}
				</dd>
				{#if progress && progress.repairedFiles > 0}
					<dd class="text-warning text-xs tabular-nums">
						{m.downloads_repaired_count({ count: count(progress.repairedFiles) })}
					</dd>
				{/if}
			</div>
		</dl>
	</div>

	<SpeedGraph samples={speedSamples.value} class="mt-4 h-20" />
</section>
