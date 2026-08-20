<script lang="ts">
	import { goto } from "$app/navigation";
	import {
		Box,
		ChevronDown,
		ChevronUp,
		Library,
		Play,
		ScrollText,
		Square,
		Terminal,
	} from "@lucide/svelte";
	import { onMount } from "svelte";
	import { m } from "$lib/paraglide/messages";
	import { createKillGameMutation, createLaunchGameMutation } from "$lib/queries/core";
	import { cachedReleaseNotes, fetchLatestRelease } from "$lib/queries/release";
	import { lastLaunchedInstance, instanceMap } from "$lib/stores/instance.svelte";
	import { persistedState } from "$lib/stores/persisted.svelte";
	import { processesList } from "$lib/stores/processes.svelte";
	import { commandsPageOpen } from "$lib/stores/ui.svelte";

	type TileId = "commands" | "releasenotes";

	const commandsMinimized = persistedState("home_commands_minimized", true);
	const releasenotesMinimized = persistedState("home_releasenotes_minimized", true);

	let minimized = $state<Record<TileId, boolean>>({
		commands: commandsMinimized.value,
		releasenotes: releasenotesMinimized.value,
	});

	function toggleMinimize(tile: TileId) {
		const next = !minimized[tile];
		minimized = { ...minimized, [tile]: next };
		if (tile === "commands") commandsMinimized.value = next;
		else releasenotesMinimized.value = next;
	}

	let currentInstance = $derived(
		lastLaunchedInstance.value ?? Object.values(instanceMap.value).find(Boolean),
	);

	let isRunning = $derived(
		currentInstance
			? processesList.value.some((p) => p.instance.id === currentInstance.id)
			: false,
	);

	let loading = $state(false);

	onMount(() => {
		loading = true;
		fetchLatestRelease().finally(() => {
			loading = false;
		});
	});

	const launchGameMutation = createLaunchGameMutation();
	const killGameMutation = createKillGameMutation();
	let isLaunching = $derived(launchGameMutation.isPending);
	let isKilling = $derived(killGameMutation.isPending);
	let actionError = $derived(
		launchGameMutation.error?.message || killGameMutation.error?.message || "",
	);

	function handleLaunchToggle() {
		if (!currentInstance) return;
		if (isRunning) {
			killGameMutation.mutate(currentInstance);
			return;
		}
		launchGameMutation.mutate(currentInstance);
	}

	function clearActionError() {
		if (launchGameMutation.error) {
			launchGameMutation.reset();
		}
		if (killGameMutation.error) {
			killGameMutation.reset();
		}
	}
</script>

<div class="fixed right-6 bottom-6 left-6 z-50 flex items-end justify-between gap-4">
	<div class="flex w-[380px] flex-col gap-2">
		<div
			class="card bg-base-200/95 shadow-xl backdrop-blur-sm transition-all duration-150 hover:brightness-90"
		>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class="card-body cursor-pointer p-3 select-none"
				onclick={() => toggleMinimize("commands")}
				role="button"
				tabindex="0"
				onkeydown={(e) => {
					if (e.key === "Enter" || e.key === " ") toggleMinimize("commands");
				}}
			>
				<div class="mb-2 flex items-center gap-2">
					<div
						class="bg-base-300 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
					>
						<Terminal size={16} class="opacity-60" />
					</div>
					<div class="flex-1">
						<h3 class="text-xs font-bold">{m.home_quick_commands()}</h3>
						<p class="text-[10px] opacity-60">{m.home_commands_subtitle()}</p>
					</div>
					<button
						class="btn btn-ghost btn-xs btn-square shrink-0"
						onclick={(e) => {
							e.stopPropagation();
							toggleMinimize("commands");
						}}
						aria-label={minimized.commands
							? m.home_show_section()
							: m.home_hide_section()}
					>
						{#if minimized.commands}
							<ChevronUp size={12} />
						{:else}
							<ChevronDown size={12} />
						{/if}
					</button>
				</div>
				{#if !minimized.commands}
					<p class="mb-3 text-[11px] leading-relaxed opacity-60">
						{m.home_commands_intro()}
					</p>
					<div class="flex flex-wrap gap-x-2 gap-y-1 font-mono text-xs">
						{#each ["Disconnect", "Changetaskforce", "God", "Cooldown", "Fillenergy", "Switchclass", "Spawnbot", "Ghost", "Walk", "EDBN", "TEDBN", "Pushscene", "Popscene", "Set3p", "Set1p", "Freezeai", "Allowmount"] as cmd}
							<code
								class="bg-base-300/60 text-base-content/80 rounded px-1.5 py-0.5 text-[11px]"
								>{cmd}</code
							>
						{/each}
					</div>
					<button
						class="btn btn-ghost btn-xs mt-3 gap-1.5 text-xs"
						onclick={(e) => {
							e.stopPropagation();
							commandsPageOpen.value = true;
							goto("/commands");
						}}
					>
						<Terminal size={12} />
						{m.home_open_full_commands()}
					</button>
				{/if}
			</div>
		</div>
		<div
			class="card bg-base-200/95 shadow-xl backdrop-blur-sm transition-all duration-150 hover:brightness-90"
		>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div
				class="card-body cursor-pointer p-3 select-none"
				onclick={() => toggleMinimize("releasenotes")}
				role="button"
				tabindex="0"
				onkeydown={(e) => {
					if (e.key === "Enter" || e.key === " ") toggleMinimize("releasenotes");
				}}
			>
				<div class="mb-2 flex items-center gap-2">
					<div
						class="bg-base-300 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
					>
						<ScrollText size={16} class="opacity-60" />
					</div>
					<div class="flex-1">
						<h3 class="text-xs font-bold">{m.home_releasenotes_title()}</h3>
						<p class="text-[10px] opacity-60">
							{cachedReleaseNotes.value ? `v${cachedReleaseNotes.value.version}` : ""}
						</p>
					</div>
					<button
						class="btn btn-ghost btn-xs btn-square shrink-0"
						onclick={(e) => {
							e.stopPropagation();
							toggleMinimize("releasenotes");
						}}
						aria-label={minimized.releasenotes
							? m.home_show_section()
							: m.home_hide_section()}
					>
						{#if minimized.releasenotes}
							<ChevronUp size={12} />
						{:else}
							<ChevronDown size={12} />
						{/if}
					</button>
				</div>
				{#if !minimized.releasenotes}
					{#if loading}
						<p class="text-xs opacity-60">{m.home_releasenotes_loading()}</p>
					{:else if cachedReleaseNotes.value}
						<div class="mb-2 flex items-center gap-2">
							<span class="badge badge-accent badge-sm"
								>{cachedReleaseNotes.value.version}</span
							>
						</div>
						<div
							class="max-h-40 overflow-y-auto text-xs leading-relaxed whitespace-pre-wrap opacity-90"
						>
							{cachedReleaseNotes.value.body}
						</div>
					{:else}
						<p class="text-xs opacity-60">{m.home_releasenotes_unavailable()}</p>
					{/if}
				{/if}
			</div>
		</div>
	</div>

	{#if currentInstance}
		<div class="join shadow-lg">
			<button
				class="btn btn-lg join-item min-h-14 gap-2"
				class:btn-accent={!isRunning}
				class:btn-error={isRunning}
				disabled={isLaunching || isKilling}
				aria-busy={isLaunching || isKilling}
				onclick={handleLaunchToggle}
				aria-label={isRunning ? m.home_stop_game() : m.home_run_game()}
			>
				{#if isLaunching}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if isKilling}
					<span class="loading loading-spinner loading-xs"></span>
				{:else if isRunning}
					<Square size={20} />
				{:else}
					<Play size={20} />
				{/if}
				<div class="flex flex-col items-start">
					<span class="text-sm font-semibold">
						{isLaunching
							? m.common_launching()
							: isKilling
								? m.common_stopping_label()
								: isRunning
									? m.home_stop_game()
									: m.home_run_game()}
					</span>
					<span class="text-xs opacity-80">{currentInstance.label}</span>
				</div>
			</button>

			<a href={`/instance/${currentInstance.id}`} class="btn btn-lg join-item min-h-14">
				<Box size={20} />
			</a>
		</div>
		{#if actionError}
			<div class="pt-2">
				<div class="alert alert-error">
					<span>{actionError}</span>
					<button class="btn btn-ghost btn-sm" onclick={clearActionError}
						>{m.common_dismiss()}</button
					>
				</div>
			</div>
		{/if}
	{:else}
		<div class="join shadow-lg">
			<a href="/library" class="btn btn-lg btn-accent join-item min-h-14 gap-2">
				<Library size={20} />
				<div class="flex flex-col items-start">
					<span class="text-sm font-semibold">{m.home_get_started()}</span>
					<span class="text-xs opacity-80">{m.home_get_started_subtitle()}</span>
				</div>
			</a>
		</div>
	{/if}
</div>
