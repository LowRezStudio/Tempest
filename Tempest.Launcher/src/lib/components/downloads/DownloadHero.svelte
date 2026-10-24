<script lang="ts">
	import { Files, Gauge, HardDrive, Pause, Play, X } from "@lucide/svelte";
	import { Progress } from "bits-ui";
	import { Spring, prefersReducedMotion } from "svelte/motion";
	import PatchBanner from "$lib/components/library/PatchBanner.svelte";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { folderName, formatBytes, formatDuration, formatSpeed } from "$lib/rigby/format";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { speedSamples } from "$lib/rigby/stores.svelte";
	import { getInstanceColor } from "$lib/utils/color";
	import { SMOOTH_SPRING } from "$lib/utils/motion";
	import { patchBannerOf, patchSummary } from "$lib/utils/versions";
	import SpeedGraph from "./SpeedGraph.svelte";
	import type { QueueItem } from "$lib/rigby/stores.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		/** The running download, the one the stopped queue takes next, or the last one finished */
		item: QueueItem;
		/** The instance it downloads into, unless it was removed from the library */
		instance?: Instance;
		oncancel: (item: QueueItem) => void;
		/** Puts a finished download away */
		onclose: (item: QueueItem) => void;
	}

	let { item, instance, oncancel, onclose }: Props = $props();

	const running = $derived(item.status === "running");
	const complete = $derived(item.status === "complete");
	// A finished download still sets the instance up before it can be played.
	const settingUp = $derived(complete && instance?.state.type === "setup");
	const progress = $derived(item.progress);
	const percent = $derived(complete ? 100 : (progress?.percent ?? 0));
	// Updates land ten times a second; the number and the bar glide between them.
	const percentSpring = Spring.of(() => percent, SMOOTH_SPRING);
	const shown = $derived(prefersReducedMotion.current ? percent : percentSpring.current);
	const banner = $derived(patchBannerOf(instance?.version));
	const color = $derived(getInstanceColor(instance));

	const status = $derived.by(() => {
		if (settingUp) return m.library_badge_setup();
		if (complete) return m.downloads_finished();
		if (item.status === "paused") return m.common_paused();
		if (!running) return m.downloads_queue_paused();
		return item.repair ? m.downloads_now_verifying() : m.downloads_now_downloading();
	});
	const tone = $derived(
		running || settingUp ? "text-primary" : complete ? "text-success" : "text-warning",
	);
	const timeLeft = $derived.by(() => {
		if (complete) {
			return item.finishedAt ? relativeDate(new Date(item.finishedAt).toISOString()) : "";
		}
		if (!running) return "";
		if (progress && progress.etaSeconds > 0) {
			return m.downloads_eta({ time: formatDuration(progress.etaSeconds) });
		}
		return m.common_calculating();
	});
	const toggleLabel = $derived(
		running
			? m.common_pause()
			: item.status === "paused"
				? m.library_resume()
				: m.common_start(),
	);

	let pausing = $state(false);
	async function toggle() {
		if (!running) {
			restoreQueue.start();
			return;
		}
		pausing = true;
		try {
			await restoreQueue.pause();
		} finally {
			pausing = false;
		}
	}

	const count = (n: number) => n.toLocaleString();

	const liveChips = $derived([
		{ icon: Gauge, value: running && progress ? formatSpeed(progress.bytesPerSecond) : "–" },
		{
			icon: Files,
			value: progress
				? m.downloads_of({
						done: count(progress.completedFiles),
						total: count(progress.totalFiles),
					})
				: "–",
		},
		// Only a check writes less than it reads, so only there is it news.
		...(item.repair && progress
			? [{ icon: HardDrive, value: formatBytes(progress.diskWriteBytes) }]
			: []),
	]);
	const chips = $derived(
		complete && item.result
			? [
					{ icon: Files, value: m.downloads_file_count({ count: item.result.files }) },
					{
						icon: HardDrive,
						value: m.downloads_written_size({
							size: formatBytes(item.result.diskWriteBytes),
						}),
					},
				]
			: liveChips,
	);

	// Tints on the glass rather than more glass: a translucent layer on another loses the text
	const tint = "border-transparent bg-base-content/10 hover:bg-base-content/15";
</script>

<!-- The download in progress on frosted glass made of the patch's art: the sharp banner and name,
     then the controls, the progress and the speed -->
<section class="glass rounded-box @container" aria-busy={running || settingUp ? "true" : undefined}>
	<!-- What the glass frosts: the art, over the instance's color for patches without one -->
	<div
		class="absolute inset-0 -z-20"
		style="background: radial-gradient(circle at 15% 25%, color-mix(in oklab, {color} 75%, transparent), transparent 55%), radial-gradient(circle at 85% 85%, color-mix(in oklab, var(--color-primary) 60%, transparent), transparent 50%), var(--color-base-300);"
	></div>
	{#if banner}
		<!-- Overhangs the card by more than the blur reaches, so its faded edges stay out of view -->
		<div class="pointer-events-none absolute -inset-32 -z-20">
			<img
				src={banner}
				alt=""
				draggable="false"
				class="size-full object-cover blur-2xl saturate-150 select-none"
			/>
		</div>
	{/if}

	<div class="flex items-center gap-6 p-6">
		<PatchBanner
			version={instance?.version}
			{color}
			class="rounded-field hidden w-56 shrink-0 shadow-lg @lg:block"
		/>
		<div class="min-w-0 flex-1">
			<p
				class={["text-xs font-semibold tracking-wider uppercase transition-colors", tone]}
				aria-live="polite"
			>
				{status}
			</p>
			<h2 class="mt-1 truncate text-2xl leading-tight font-bold tracking-tight">
				{instance?.label ?? folderName(item.outDir)}
			</h2>
			<p class="text-base-content/80 truncate text-sm" title={item.outDir}>
				{instance ? patchSummary(instance) : item.outDir}
			</p>
			<ul class="mt-4 flex flex-wrap gap-2 text-xs font-medium">
				{#each chips as chip, i (i)}
					<li
						class="bg-base-content/8 flex items-center gap-1.5 rounded-full px-2.5 py-1 tabular-nums"
					>
						<chip.icon size={12} class="opacity-70" />
						{chip.value}
					</li>
				{/each}
			</ul>
		</div>

		<!-- The toggle never moves: Pause and Resume share its place, with Cancel always beside it;
		     once finished, Close takes over -->

		<div class="flex shrink-0 items-center gap-1.5 self-start">
			{#if complete}
				<button class="btn btn-sm {tint}" onclick={() => onclose(item)}>
					<X size={14} />
					{m.common_close()}
				</button>
			{:else}
				<button
					class={["btn btn-sm min-w-28", running ? tint : "btn-primary"]}
					disabled={pausing}
					onclick={toggle}
				>
					{#if pausing}
						<span class="loading loading-spinner loading-xs"></span>
					{:else if running}
						<Pause size={14} />
					{:else}
						<Play size={14} />
					{/if}
					{toggleLabel}
				</button>
				<button
					class="btn btn-sm hover:text-error {tint}"
					aria-label={m.downloads_cancel()}
					onclick={() => oncancel(item)}
				>
					<X size={14} />
					{m.common_cancel()}
				</button>
			{/if}
		</div>
	</div>

	<div class="px-6">
		<div class="mb-2 flex items-baseline gap-3">
			<span class="text-3xl leading-none font-bold tracking-tight tabular-nums">
				{shown.toFixed(shown > 0 && shown < 99.95 ? 1 : 0)}%
			</span>
			<span class="text-base-content/70 min-w-0 flex-1 truncate text-sm">{timeLeft}</span>
			<span class="text-base-content/70 shrink-0 text-sm tabular-nums">
				{#if complete && item.result}
					{formatBytes(item.result.diskWriteBytes + item.result.reusedBytes)}
					<!-- A download writes every file, so only a check has repairs worth pointing out -->
					{#if item.repair && item.result.repairedFiles > 0}
						· <span class="text-warning">
							{m.downloads_repaired_count({
								count: count(item.result.repairedFiles),
							})}
						</span>
					{/if}
				{:else if progress}
					{m.downloads_of({
						done: formatBytes(progress.completedBytes),
						total: formatBytes(progress.totalBytes),
					})}
					{#if item.repair && progress.repairedFiles > 0}
						· <span class="text-warning">
							{m.downloads_repaired_count({
								count: count(progress.repairedFiles),
							})}
						</span>
					{/if}
				{:else if running}
					{m.downloads_starting()}
				{/if}
			</span>
		</div>
		<Progress.Root
			value={percent}
			max={100}
			class="bg-base-content/10 h-2.5 w-full overflow-hidden rounded-full"
			aria-label={status}
		>
			<!-- Slid rather than resized: a transform animates off the main thread -->
			<div
				class={[
					"h-full w-full rounded-full transition-colors duration-500",
					running || settingUp
						? "bg-primary loading-shimmer"
						: complete
							? "bg-success"
							: "bg-warning",
				]}
				style="transform: translateX({shown - 100}%);"
			></div>
		</Progress.Root>
	</div>

	<!-- Kept through a pause, dimmed so it reads as history rather than live. Without a history
	     (a download finished before the app last started) it would only be empty space. -->
	{#if speedSamples.value.length > 1}
		<SpeedGraph
			samples={speedSamples.value}
			live={running}
			class={["mt-4 h-20 transition-opacity duration-300", !running && "opacity-50"]}
		/>
	{:else}
		<div class="h-6"></div>
	{/if}
</section>

<style>
	.glass {
		position: relative;
		isolation: isolate;
		overflow: hidden;
		/* No border or edge highlight: against the frost they read as stray lines */
		box-shadow: 0 12px 32px -12px rgb(0 0 0 / 0.5);
	}

	/* The milky layer of the glass over the blurred art */
	.glass::before,
	.glass::after {
		content: "";
		position: absolute;
		inset: 0;
		z-index: -1;
		pointer-events: none;
	}
	.glass::before {
		/* With a faint sheen down from the top, where light would hit it */
		background:
			linear-gradient(to bottom, color-mix(in oklab, white 8%, transparent), transparent 45%),
			color-mix(in oklab, var(--color-base-100) 55%, transparent);
	}

	/* The frost: a fine grain, so the blur reads as glass rather than a smudge */
	.glass::after {
		opacity: 0.08;
		mix-blend-mode: overlay;
		background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E");
	}

	/* No prefers-reduced-transparency fallback: Windows reports it whenever "Transparency effects"
	   is off, which is common, and nothing here is see-through to the page anyway. */
	@media (prefers-contrast: more) {
		.glass {
			border: 1px solid var(--color-base-content);
		}
		.glass::before {
			background: var(--color-base-100);
		}
	}
</style>
