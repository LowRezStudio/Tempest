<script lang="ts" generics="T extends string">
	import {
		CircleArrowUp,
		Clock,
		CloudDownload,
		Cpu,
		FolderOpen,
		Play,
		Square,
		Terminal,
	} from "@lucide/svelte";
	import { Tabs } from "bits-ui";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { openPath } from "$lib/platform/api";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { processesList } from "$lib/stores/processes.svelte";
	import { addToast } from "$lib/stores/ui.svelte";
	import { getInstanceColor } from "$lib/utils/color";
	import { loadingScreensOf, patchSummary } from "$lib/utils/versions";
	import InstanceMenu from "./InstanceMenu.svelte";
	import InstanceSessionControls from "./InstanceSessionControls.svelte";
	import PatchBanner from "./PatchBanner.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props<Tab> {
		instance: Instance;
		tabs: { name: string; value: Tab; count?: number }[];
		activeTab: Tab;
		/** Mod updates the repository has for it */
		updateCount?: number;
		onShowUpdates?: () => void;
	}

	let {
		instance,
		tabs,
		activeTab = $bindable(),
		updateCount = 0,
		onShowUpdates,
	}: Props<T> = $props();

	const phase = $derived(instance.state.type);

	// One of the patch's loading screens, the same one on every visit, as the instance's banner
	const art = $derived.by(() => {
		const pool = loadingScreensOf(instance.version);
		let hash = 0;
		for (const char of instance.id) hash = (hash * 31 + char.charCodeAt(0)) | 0;
		return pool[Math.abs(hash) % pool.length];
	});

	const windows = $derived(processesList.value.filter((p) => p.instance.id === instance.id));
	const running = $derived(windows.length > 0);
	const launch = createLaunchGameMutation();
	const kill = createKillGameMutation();
	const busy = $derived(launch.isPending || kill.isPending);

	const playLabel = $derived.by(() => {
		if (phase === "setup") return m.common_setting_up();
		if (launch.isPending) return m.common_launching();
		if (kill.isPending) return m.common_stopping_label();
		if (running) return windows.length > 1 ? m.sessions_stop_all() : m.common_stop();
		return m.common_play();
	});

	function toggle() {
		const target = instance;
		const stopping = running;
		(stopping ? kill : launch).mutate(target, {
			onError: (error) =>
				addToast({
					title: stopping
						? m.library_stop_failed({ name: target.label })
						: m.library_launch_failed({ name: target.label }),
					message: error.message,
					tone: "error",
				}),
		});
	}

	const formatTime = (timestamp: number) =>
		new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

	const args = $derived(instance.launchOptions.args.join(" "));
	// The website's secondary hero button, solid enough to read over the art
	const secondary = "btn border-base-content/10 bg-base-100/60 hover:bg-base-100";
</script>

<!-- After the mod website's mod page: the instance on its banner, Play on the right, then the tabs -->
<header class="shrink-0 px-4 pt-4">
	<section
		class="rounded-box border-base-300 bg-base-200 @container relative isolate flex items-center gap-4 overflow-hidden border px-4 py-3"
	>
		{#if art}
			<img
				src={art}
				alt=""
				draggable="false"
				class="pointer-events-none absolute inset-0 -z-10 size-full object-cover select-none"
			/>
		{/if}
		<!-- Keeps the text and the buttons readable over any art, in any theme -->
		<div
			class="from-base-200 via-base-200/80 to-base-200/20 absolute inset-0 -z-10 bg-linear-to-r from-20% via-50%"
		></div>

		<PatchBanner
			version={instance.version}
			color={getInstanceColor(instance)}
			class="rounded-field hidden w-48 shrink-0 shadow-lg @3xl:block"
		/>
		<div class="min-w-0 flex-1">
			<h1 class="truncate text-3xl leading-tight font-bold tracking-tight">
				{instance.label}
			</h1>
			<p class="text-base-content/70 truncate text-sm">{patchSummary(instance)}</p>
			<div
				class="text-base-content/60 mt-1.5 flex items-center gap-x-3 overflow-hidden text-xs whitespace-nowrap"
			>
				{#if phase === "downloading" || phase === "paused"}
					<a
						href="/downloads"
						class="text-primary flex items-center gap-1 hover:underline"
					>
						<CloudDownload size={12} />
						{phase === "paused" ? m.common_paused() : m.common_downloading()}
					</a>
				{/if}
				<span class="flex items-center gap-1">
					<Cpu size={12} />
					{instance.launchOptions.platform ?? "Win64"}
				</span>
				{#if running}
					<span class="text-success flex items-center gap-1">
						<Clock size={12} />
						{m.library_playing_since({
							time: formatTime(Math.min(...windows.map((w) => w.startedAt))),
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
				{#if updateCount > 0}
					<button
						class="text-primary flex cursor-pointer items-center gap-1 hover:underline"
						onclick={onShowUpdates}
					>
						<CircleArrowUp size={12} />
						{m.home_updates({ count: updateCount })}
					</button>
				{/if}
				{#if args}
					<span class="flex min-w-0 items-center gap-1" title={args}>
						<Terminal size={12} class="shrink-0" />
						<span class="truncate font-mono">{args}</span>
					</span>
				{/if}
			</div>
		</div>
		<div class="flex shrink-0 items-center gap-2">
			<InstanceSessionControls {instance} {busy} variant="header" />
			<button
				class="btn min-w-32"
				class:btn-success={!running}
				class:btn-error={running}
				disabled={busy || (!running && phase !== "prepared")}
				aria-busy={busy || phase === "setup"}
				onclick={toggle}
			>
				{#if busy || phase === "setup"}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if running}
					<Square size={16} />
				{:else}
					<Play size={16} />
				{/if}
				{playLabel}
			</button>
			<button
				class="{secondary} btn-square"
				title={m.instancemenu_browse_folder()}
				aria-label={m.instancemenu_browse_folder()}
				disabled={!instance.path}
				onclick={() => void openPath(instance.path)}
			>
				<FolderOpen size={16} />
			</button>
			<InstanceMenu {instance} triggerClass="{secondary} btn-square" />
		</div>
	</section>

	<Tabs.Root value={activeTab} onValueChange={(value) => (activeTab = value as T)}>
		<Tabs.List class="tabs tabs-border mt-1">
			{#each tabs as tab (tab.value)}
				<Tabs.Trigger value={tab.value} class="tab data-[state=active]:tab-active gap-1.5">
					{tab.name}
					{#if tab.count !== undefined}
						<span class="text-base-content/50 text-xs tabular-nums">{tab.count}</span>
					{/if}
				</Tabs.Trigger>
			{/each}
		</Tabs.List>
	</Tabs.Root>
</header>
