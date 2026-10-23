<script lang="ts">
	import {
		Clock,
		Cpu,
		FolderOpen,
		Gauge,
		Package,
		Play,
		RotateCcw,
		Square,
	} from "@lucide/svelte";
	import PaladinsIcon from "$lib/components/ui/PaladinsIcon.svelte";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { restoreQueue } from "$lib/rigby/restore-queue";
	import { queueItems } from "$lib/rigby/stores.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import { getContrastColor, getInstanceColor } from "$lib/utils/color";
	import { patchSummary } from "$lib/utils/versions";
	import InstanceMenu from "./InstanceMenu.svelte";
	import InstanceSessionControls from "./InstanceSessionControls.svelte";
	import PatchBanner from "./PatchBanner.svelte";
	import type { ModRecord } from "$lib/core/mods";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		instance: Instance;
		/** Its mod list, once loaded */
		mods?: ModRecord[];
	}

	let { instance, mods }: Props = $props();

	const phase = $derived(instance.state.type);
	const playable = $derived(phase === "prepared" || phase === "setup");
	const color = $derived(getInstanceColor(instance));
	const contrast = $derived(getContrastColor(color));
	const summary = $derived(patchSummary(instance));

	const download = $derived(
		queueItems.value.find(
			(item) => item.outDir === instance.path && item.status !== "complete",
		),
	);
	const percent = $derived(Math.round(download?.progress?.percent ?? 0));

	const windows = $derived(processesList.value.filter((p) => p.instance.id === instance.id));
	const running = $derived(windows.length > 0);
	const launch = createLaunchGameMutation();
	const kill = createKillGameMutation();
	const busy = $derived(launch.isPending || kill.isPending);

	const badge = $derived.by(() => {
		if (running) return { label: m.library_badge_running(), tone: "badge-success" };
		if (phase === "setup") return { label: m.library_badge_setup(), tone: "badge-primary" };
		if (phase === "paused") return { label: m.common_paused(), tone: "badge-warning" };
		if (phase !== "downloading") return null;
		if (download?.status === "error") {
			return { label: m.library_badge_failed(), tone: "badge-error" };
		}
		if (download?.status === "running") {
			return { label: m.library_badge_downloading({ percent }), tone: "badge-primary" };
		}
		return { label: m.library_badge_queued(), tone: "badge-ghost" };
	});

	const enabledMods = $derived(mods?.filter((mod) => mod.Enabled).length ?? 0);
	const startedAt = $derived(Math.min(...windows.map((w) => w.startedAt)));

	function toggle() {
		const stopping = running;
		(stopping ? kill : launch).mutate(instance, {
			onError: (error) =>
				addToast({
					title: stopping
						? m.library_stop_failed({ name: instance.label })
						: m.library_launch_failed({ name: instance.label }),
					message: error.message,
					tone: "error",
				}),
		});
	}
</script>

<li
	class="hover:bg-base-300/40 @container relative flex items-center gap-4 p-3 transition-colors"
	class:pending-row={phase === "setup" || download?.status === "running"}
	class:installing={phase === "setup"}
	aria-busy={phase === "setup" || phase === "downloading" ? "true" : undefined}
	style={download?.status === "running" ? `--progress: ${percent}%` : undefined}
>
	<PatchBanner
		version={instance.version}
		{color}
		class={["hidden w-36 shrink-0 @xl:block", !playable && "opacity-60"]}
	/>
	<div
		class="rounded-field flex size-10 shrink-0 items-center justify-center text-[11px] font-bold tabular-nums @xl:hidden"
		class:opacity-60={!playable}
		style="background-color: {color}; color: {contrast};"
	>
		{#if instance.version}
			{instance.version}
		{:else}
			<PaladinsIcon size={28} color={contrast} />
		{/if}
	</div>

	<div class="min-w-0 flex-1">
		<div class="flex min-w-0 items-center gap-2">
			<!-- The whole row opens the instance (its download while there is one); the stretched
			     link keeps it one tab stop. -->
			<a
				href={playable ? `/instance/${instance.id}` : "/downloads"}
				class="truncate font-semibold after:absolute after:inset-0 hover:underline"
			>
				{instance.label}
			</a>
			{#if badge}
				<span class="badge badge-sm shrink-0 whitespace-nowrap tabular-nums {badge.tone}"
					>{badge.label}</span
				>
			{/if}
		</div>
		<p class="text-base-content/70 line-clamp-1 text-sm">{summary}</p>
		<div
			class="text-base-content/60 mt-1 flex items-center gap-x-3 overflow-hidden text-xs whitespace-nowrap"
		>
			{#if playable}
				<span class="flex items-center gap-1">
					<Cpu size={12} />
					{instance.launchOptions.platform ?? "Win64"}
				</span>
				{#if mods}
					<span
						class="flex items-center gap-1"
						title={m.library_mods_enabled({ enabled: enabledMods, count: mods.length })}
					>
						<Package size={12} />
						{m.library_mods({ count: mods.length })}
					</span>
				{/if}
				{#if running}
					<span class="text-success flex items-center gap-1">
						<Clock size={12} />
						{m.library_playing_since({
							time: new Date(startedAt).toLocaleTimeString([], {
								hour: "2-digit",
								minute: "2-digit",
							}),
						})}
					</span>
				{:else if instance.lastPlayedAt}
					<span
						class="flex items-center gap-1"
						title={new Date(instance.lastPlayedAt).toLocaleString()}
					>
						<Clock size={12} />
						{m.library_played({
							when: relativeDate(new Date(instance.lastPlayedAt).toISOString()),
						})}
					</span>
				{/if}
			{:else if download?.status === "running" && download.progress}
				<span class="flex items-center gap-1 tabular-nums">
					<Gauge size={12} />
					{Math.round(download.progress.bytesPerSecond / 1024 / 1024)} MB/s
				</span>
			{:else if download?.status === "pending"}
				<span>{m.common_waiting_in_queue()}</span>
			{:else if download?.status === "error" && download.error}
				<span class="text-error min-w-0 truncate" title={download.error}
					>{download.error}</span
				>
			{/if}
			<span class="hidden min-w-0 items-center gap-1 @2xl:flex" title={instance.path}>
				<FolderOpen size={12} class="shrink-0" />
				<span class="truncate">{instance.path}</span>
			</span>
		</div>
	</div>

	<!-- Above the stretched link, so these stay their own controls. -->
	<div class="relative z-10 flex shrink-0 items-center gap-1">
		{#if playable}
			<InstanceSessionControls {instance} {busy} />
			<button
				class="btn btn-sm min-w-24"
				class:btn-success={!running}
				class:btn-error={running}
				disabled={busy || phase === "setup"}
				aria-busy={busy}
				onclick={toggle}
			>
				{#if busy || phase === "setup"}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if running}
					<Square size={14} />
				{:else}
					<Play size={14} />
				{/if}
				{running
					? windows.length > 1
						? m.sessions_stop_all()
						: m.common_stop()
					: m.common_play()}
			</button>
		{:else if phase === "paused" && download}
			<button class="btn btn-sm min-w-24" onclick={() => restoreQueue.resume(download.id)}>
				<Play size={14} />
				{m.library_resume()}
			</button>
		{:else if download?.status === "error"}
			<button class="btn btn-sm min-w-24" onclick={() => restoreQueue.retry(download.id)}>
				<RotateCcw size={14} />
				{m.common_retry()}
			</button>
		{:else}
			<span class="btn btn-sm btn-disabled min-w-24 tabular-nums">
				<span class="loading loading-spinner loading-xs"></span>
				{download?.status === "running" ? `${percent}%` : m.library_badge_queued()}
			</span>
		{/if}
		<InstanceMenu {instance} />
	</div>
</li>
