<script lang="ts">
	import {
		ArrowRight,
		Box,
		CircleArrowUp,
		Clock,
		CloudDownload,
		Cpu,
		FolderOpen,
		MapPin,
		Package,
		Pin,
		Play,
		Search,
		Shuffle,
		Square,
	} from "@lucide/svelte";
	import { untrack } from "svelte";
	import { fade } from "svelte/transition";
	import InstanceSessionControls from "$lib/components/library/InstanceSessionControls.svelte";
	import NewInstanceOption from "$lib/components/library/NewInstanceOption.svelte";
	import maps from "$lib/data/maps.json";
	import { relativeDate } from "$lib/mods/listing";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { createModsQuery } from "$lib/queries/mods";
	import { createRepoUpdatesQuery } from "$lib/queries/repo";
	import { processesList } from "$lib/stores/processes.svelte";
	import { pinnedBackground } from "$lib/stores/settings.svelte";
	import { addToast, openInstanceWizard } from "$lib/stores/ui.svelte";
	import { currentWeather } from "$lib/theme/weather.svelte";
	import { getMapsForVersion, patchSummary } from "$lib/utils/versions";
	import HomeInstanceTile from "./HomeInstanceTile.svelte";
	import type { Instance } from "$lib/types/instance";

	interface Props {
		/** The instance to play, when one is ready */
		instance?: Instance;
		/** The other instances, in library order */
		others: Instance[];
	}

	let { instance, others }: Props = $props();

	const loadingScreens = Object.keys(import.meta.glob("/static/loading-screens/*.webp")).map(
		(path) => path.replace("/static", ""),
	);
	const winterScreens = loadingScreens.filter((path) => /Ice|Frost|NRIgloo|NRMines/.test(path));

	// The loading screens of the patch's own maps, so the art is what that game shows; the
	// snowy maps while it snows.
	function artFor(version: string | undefined): string[] {
		if (currentWeather.value?.kind === "snow" && winterScreens.length) return winterScreens;
		const own = version
			? getMapsForVersion(version)
					.map((map) => map.iconPath)
					.filter((path) => loadingScreens.includes(path))
			: [];
		return own.length > 0 ? [...new Set(own)] : loadingScreens;
	}

	function pickArt(current: string | undefined) {
		const pool = artFor(instance?.version);
		const choices = pool.length > 1 ? pool.filter((path) => path !== current) : pool;
		return choices[Math.floor(Math.random() * choices.length)];
	}

	let art = $state(untrack(() => pinnedBackground.value ?? pickArt(undefined)));
	const pinned = $derived(!!art && pinnedBackground.value === art);
	const mapName = $derived(maps.find((map) => map.iconPath === art)?.displayName);

	function shuffle() {
		pinnedBackground.value = undefined;
		art = pickArt(art);
	}

	function togglePin() {
		pinnedBackground.value = pinned ? undefined : art;
	}

	const windows = $derived(
		instance ? processesList.value.filter((p) => p.instance.id === instance.id) : [],
	);
	const running = $derived(windows.length > 0);
	const settingUp = $derived(instance?.state.type === "setup");
	const launch = createLaunchGameMutation();
	const kill = createKillGameMutation();
	const busy = $derived(launch.isPending || kill.isPending);

	const playLabel = $derived.by(() => {
		if (settingUp) return m.common_setting_up();
		if (launch.isPending) return m.common_launching();
		if (kill.isPending) return m.common_stopping_label();
		if (running) return windows.length > 1 ? m.sessions_stop_all() : m.common_stop();
		return m.common_play();
	});

	function toggle() {
		if (!instance) return;
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

	const modsQuery = createModsQuery(() =>
		instance?.state.type === "prepared" ? instance.path : "",
	);
	const mods = $derived(modsQuery.data);
	// Shares the instance page's check, whose popup lists the updates.
	const updatesQuery = createRepoUpdatesQuery(() => ({
		patch: instance?.version ?? "",
		mods: (mods ?? [])
			.filter((mod) => mod.Kind === "V2")
			.map((mod) => ({ id: mod.Id, version: mod.Version })),
	}));
	const updateCount = $derived(
		(updatesQuery.data ?? []).filter((update) => update.updateAvailable).length,
	);

	const eyebrow = $derived.by(() => {
		if (!instance) return undefined;
		if (running) return m.home_now_playing();
		if (!instance.lastPlayedAt) return m.home_ready_to_play();
		return m.home_last_played({
			when: relativeDate(new Date(instance.lastPlayedAt).toISOString()),
		});
	});

	const formatTime = (timestamp: number) =>
		new Date(timestamp).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

	const hasAside = $derived(!!instance || others.length > 0);
	// The website's secondary hero button, solid enough to read over the art
	const secondary = "btn border-base-content/10 bg-base-100/60 hover:bg-base-100";
</script>

<div class="grid gap-4 @4xl:grid-cols-[minmax(0,1fr)_20rem]">
	<section
		class={[
			"rounded-box border-base-300 bg-base-200 relative isolate flex min-h-72 flex-col overflow-hidden border",
			!hasAside && "@4xl:col-span-2",
		]}
	>
		{#key art}
			<img
				src={art}
				alt=""
				draggable="false"
				transition:fade={{ duration: 300 }}
				class="pointer-events-none absolute inset-0 -z-10 size-full object-cover select-none"
			/>
		{/key}
		<!-- Keeps the text readable over any art, in any theme -->
		<div
			class="from-base-200 via-base-200/80 absolute inset-0 -z-10 bg-linear-to-r from-20% via-50% to-transparent"
		></div>

		<div class="flex items-center justify-between gap-3 px-6 pt-5">
			<p class={["text-sm font-medium", running ? "text-success" : "text-base-content/70"]}>
				{eyebrow}
			</p>
			<div
				class="bg-base-100/60 rounded-field ml-auto flex items-center gap-0.5 p-0.5 text-xs backdrop-blur-sm"
			>
				{#if mapName}
					<span class="text-base-content/70 flex items-center gap-1 px-1.5">
						<MapPin size={12} />
						{mapName}
					</span>
				{/if}
				<button
					class="btn btn-ghost btn-xs btn-square"
					title={m.home_art_shuffle()}
					aria-label={m.home_art_shuffle()}
					onclick={shuffle}
				>
					<Shuffle size={12} />
				</button>
				<button
					class={["btn btn-ghost btn-xs btn-square", pinned && "text-primary"]}
					title={pinned ? m.home_art_unpin() : m.home_art_pin()}
					aria-label={m.home_art_pin()}
					aria-pressed={pinned}
					onclick={togglePin}
				>
					<Pin size={12} />
				</button>
			</div>
		</div>

		<div class="mt-auto px-6 pt-10 pb-6">
			{#if instance}
				<h2 class="text-3xl font-bold tracking-tight @5xl:text-4xl">
					<a href="/instance/{instance.id}" class="hover:underline">{instance.label}</a>
				</h2>
				<p class="text-base-content/70 mt-1 text-sm @5xl:text-base">
					{patchSummary(instance)}
				</p>
				<div
					class="text-base-content/60 mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"
				>
					<span class="flex items-center gap-1">
						<Cpu size={12} />
						{instance.launchOptions.platform ?? "Win64"}
					</span>
					{#if mods}
						<span
							class="flex items-center gap-1"
							title={m.library_mods_enabled({
								enabled: mods.filter((mod) => mod.Enabled).length,
								count: mods.length,
							})}
						>
							<Package size={12} />
							{m.library_mods({ count: mods.length })}
						</span>
					{/if}
					{#if updateCount > 0}
						<a
							href="/instance/{instance.id}"
							class="text-primary flex items-center gap-1 hover:underline"
						>
							<CircleArrowUp size={12} />
							{m.home_updates({ count: updateCount })}
						</a>
					{/if}
					{#if running}
						<span class="text-success flex items-center gap-1">
							<Clock size={12} />
							{m.library_playing_since({
								time: formatTime(Math.min(...windows.map((w) => w.startedAt))),
							})}
						</span>
					{/if}
				</div>
				<div class="mt-5 flex flex-wrap items-center gap-2">
					<button
						class="btn min-w-32"
						class:btn-success={!running}
						class:btn-error={running}
						disabled={busy || settingUp}
						aria-busy={busy || settingUp}
						onclick={toggle}
					>
						{#if busy || settingUp}
							<span class="loading loading-spinner loading-xs"></span>
						{:else if running}
							<Square size={16} />
						{:else}
							<Play size={16} />
						{/if}
						{playLabel}
					</button>
					<InstanceSessionControls {instance} {busy} variant="header" />
					<a href="/instance/{instance.id}?tab=browse" class={secondary}>
						<Search size={16} />
						{m.home_find_mods()}
					</a>
					<a href="/instance/{instance.id}" class={secondary}>
						<Box size={16} />
						{m.home_open_instance()}
					</a>
				</div>
			{:else}
				<h2 class="text-3xl font-bold tracking-tight @5xl:text-4xl">
					{m.home_empty_title()}
				</h2>
				<p class="text-base-content/70 mt-2 max-w-md text-sm @5xl:text-base">
					{m.library_about_instances()}
				</p>
				<div class="mt-5 flex flex-wrap gap-2">
					<button class="btn btn-primary" onclick={() => openInstanceWizard("download")}>
						<CloudDownload size={16} />
						{m.library_download_patch()}
					</button>
					<button class={secondary} onclick={() => openInstanceWizard("folder")}>
						<FolderOpen size={16} />
						{m.library_import_folder()}
					</button>
				</div>
			{/if}
		</div>
	</section>

	{#if hasAside}
		<aside class="bg-base-200 border-base-300 rounded-box flex flex-col gap-1 border p-3">
			{#if others.length > 0}
				<div class="mb-1 flex min-h-6 items-center justify-between gap-2 pl-1.5">
					<h2 class="text-sm font-semibold">
						{instance ? m.home_other_instances() : m.library_heading()}
					</h2>
					<a href="/library" class="btn btn-ghost btn-xs text-base-content/70">
						{m.common_see_all()}
						<ArrowRight size={12} />
					</a>
				</div>
				{#each others.slice(0, 4) as other (other.id)}
					<HomeInstanceTile instance={other} />
				{/each}
			{:else}
				<h2 class="px-1.5 text-sm font-semibold">{m.library_new_instance()}</h2>
				<p class="text-base-content/60 mb-1 px-1.5 text-xs">{m.home_another_patch()}</p>
				<div class="flex flex-col gap-2">
					<NewInstanceOption tab="download" />
					<NewInstanceOption tab="folder" />
				</div>
			{/if}
		</aside>
	{/if}
</div>
